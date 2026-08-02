//! Bounded decode reservations handed down by the owning application layer.
//!
//! ADR-0014 makes one application-owned plan the single source of every
//! internal worker and memory allocation. [`DecodeReservation`] carries a
//! validated upper bound downwards; its fields are immutable, so code receiving
//! one cannot widen that particular allocation. The type has no application
//! dependency, so `codecs` consumes it without knowing how the owning plan sized
//! it. Its cross-crate constructor is hidden from supported public docs and is
//! used by first-party production code only in the application plan.

use crate::{AnalysisError, AnalysisStage, ErrorCode};
use std::num::NonZeroUsize;

/// The absolute product ceiling on decoder workers inside one opened source.
pub const MAX_DECODE_WORKERS: usize = 8;

/// The absolute product ceiling on queued packets inside one opened source.
pub const MAX_DECODE_QUEUE_CAPACITY: usize = 64;

/// The absolute product ceiling on decoded PCM bytes awaiting in-order commit
/// inside one opened source.
pub const MAX_IN_FLIGHT_PCM_BYTES: u64 = 64 * 1024 * 1024;

/// A bounded decode allocation granted before any decode sub-task is scheduled.
///
/// The serial reservation is the product default. Its zero in-flight PCM budget
/// is the literal serial contract: a serial route decodes and commits one block
/// at a time, so no decoded block may ever wait on an earlier index.
///
/// This type crosses the `macinmeter`/`macinmeter-codecs` crate boundary, so its
/// validated constructor is technically public. It is not an authority token
/// or supported tuning API: first-party production code obtains it only from
/// the application-owned concurrency plan, and direct decoder callers use
/// [`DecodeReservation::serial`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeReservation {
    workers: NonZeroUsize,
    queue_capacity: NonZeroUsize,
    max_in_flight_pcm_bytes: u64,
}

impl DecodeReservation {
    /// One worker, one queued packet, no out-of-order PCM retention.
    pub const fn serial() -> Self {
        Self {
            workers: NonZeroUsize::MIN,
            queue_capacity: NonZeroUsize::MIN,
            max_in_flight_pcm_bytes: 0,
        }
    }

    /// Build a reservation, rejecting anything past the fixed product ceilings.
    ///
    /// The queue must hold at least one packet per worker, and any reservation
    /// that can decode out of order must budget the PCM those workers retain
    /// while waiting for earlier indices.
    #[doc(hidden)]
    pub fn new(
        workers: NonZeroUsize,
        queue_capacity: NonZeroUsize,
        max_in_flight_pcm_bytes: u64,
    ) -> Result<Self, AnalysisError> {
        let invalid = |message: &str, details: String| {
            AnalysisError::new(
                ErrorCode::InvalidRequest,
                AnalysisStage::Validation,
                message,
            )
            .with_details(details)
        };

        if workers.get() > MAX_DECODE_WORKERS {
            return Err(invalid(
                "decode reservation exceeds the fixed worker ceiling",
                format!(
                    "workers={}; max_workers={MAX_DECODE_WORKERS}",
                    workers.get()
                ),
            ));
        }
        if queue_capacity.get() > MAX_DECODE_QUEUE_CAPACITY {
            return Err(invalid(
                "decode reservation exceeds the fixed queue ceiling",
                format!(
                    "queue_capacity={}; max_queue_capacity={MAX_DECODE_QUEUE_CAPACITY}",
                    queue_capacity.get()
                ),
            ));
        }
        if queue_capacity < workers {
            return Err(invalid(
                "decode reservation queues fewer packets than it has workers",
                format!(
                    "workers={}; queue_capacity={}",
                    workers.get(),
                    queue_capacity.get()
                ),
            ));
        }
        if max_in_flight_pcm_bytes > MAX_IN_FLIGHT_PCM_BYTES {
            return Err(invalid(
                "decode reservation exceeds the fixed in-flight PCM ceiling",
                format!(
                    "max_in_flight_pcm_bytes={max_in_flight_pcm_bytes}; \
                     ceiling={MAX_IN_FLIGHT_PCM_BYTES}"
                ),
            ));
        }
        if workers.get() > 1 && max_in_flight_pcm_bytes == 0 {
            return Err(invalid(
                "a multi-worker decode reservation must budget in-flight PCM",
                format!("workers={}; max_in_flight_pcm_bytes=0", workers.get()),
            ));
        }

        Ok(Self {
            workers,
            queue_capacity,
            max_in_flight_pcm_bytes,
        })
    }

    pub const fn workers(self) -> NonZeroUsize {
        self.workers
    }

    pub const fn queue_capacity(self) -> NonZeroUsize {
        self.queue_capacity
    }

    pub const fn max_in_flight_pcm_bytes(self) -> u64 {
        self.max_in_flight_pcm_bytes
    }

    /// Whether this reservation permits exactly one packet in flight at a time.
    pub const fn is_serial(self) -> bool {
        self.workers.get() == 1 && self.queue_capacity.get() == 1
    }
}

impl Default for DecodeReservation {
    fn default() -> Self {
        Self::serial()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nonzero(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).unwrap()
    }

    #[test]
    fn the_serial_reservation_retains_no_out_of_order_pcm() {
        let reservation = DecodeReservation::serial();
        assert!(reservation.is_serial());
        assert_eq!(reservation.workers().get(), 1);
        assert_eq!(reservation.queue_capacity().get(), 1);
        assert_eq!(reservation.max_in_flight_pcm_bytes(), 0);
        assert_eq!(DecodeReservation::default(), reservation);
    }

    #[test]
    fn reservations_are_capped_by_the_fixed_product_ceilings() {
        let workers = nonzero(MAX_DECODE_WORKERS + 1);
        let error = DecodeReservation::new(workers, nonzero(MAX_DECODE_QUEUE_CAPACITY), 1)
            .expect_err("the worker ceiling is fixed in code");
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(error.stage, AnalysisStage::Validation);

        let error = DecodeReservation::new(nonzero(2), nonzero(MAX_DECODE_QUEUE_CAPACITY + 1), 1)
            .expect_err("the queue ceiling is fixed in code");
        assert_eq!(error.code, ErrorCode::InvalidRequest);

        let error = DecodeReservation::new(nonzero(2), nonzero(8), MAX_IN_FLIGHT_PCM_BYTES + 1)
            .expect_err("the in-flight PCM ceiling is fixed in code");
        assert_eq!(error.code, ErrorCode::InvalidRequest);
    }

    #[test]
    fn a_reservation_must_queue_at_least_one_packet_per_worker() {
        let error = DecodeReservation::new(nonzero(4), nonzero(3), 1024)
            .expect_err("a starved worker must not be reservable");
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        let details = error.details.expect("details name both bounds");
        assert!(details.contains("workers=4"), "{details}");
        assert!(details.contains("queue_capacity=3"), "{details}");
    }

    #[test]
    fn out_of_order_decoding_must_budget_the_pcm_it_retains() {
        let error = DecodeReservation::new(nonzero(2), nonzero(8), 0)
            .expect_err("multiple workers retain PCM while waiting for earlier indices");
        assert_eq!(error.code, ErrorCode::InvalidRequest);

        let reservation = DecodeReservation::new(nonzero(2), nonzero(8), 1024).unwrap();
        assert!(!reservation.is_serial());
        assert_eq!(reservation.max_in_flight_pcm_bytes(), 1024);
    }
}
