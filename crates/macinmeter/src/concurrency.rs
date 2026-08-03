//! The one bounded worker and memory plan every internal concurrency axis
//! draws from.
//!
//! ADR-0014 lifts the blanket ban on packet-, file- and window-level
//! parallelism, but only under a single application-owned budget. File lanes,
//! packet workers and window workers all spend permits from the same plan, so
//! they can never multiply into `lanes × packets × windows` concurrency, and a
//! worker never waits on a second pool for a second permit.

use crate::{AnalysisError, AnalysisStage, ErrorCode};
use macinmeter_domain::{DecodeReservation, MAX_DECODE_QUEUE_CAPACITY, MAX_DECODE_WORKERS};
use std::num::NonZeroUsize;

/// Packets queued per granted decoder worker.
const QUEUE_DEPTH_PER_WORKER: usize = 4;

/// Decoded PCM budgeted per granted decoder worker while it waits for earlier
/// packet indices.
const IN_FLIGHT_PCM_BYTES_PER_WORKER: u64 = 4 * 1024 * 1024;

/// The bounded internal resource plan owned by one active application job.
///
/// The plan may shrink below what a caller asked for — to the fixed product
/// ceiling or to the host's reported parallelism — but it never grows, and it
/// never derives its size from media declarations, batch length or recursion
/// depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConcurrencyPlan {
    total_workers: NonZeroUsize,
}

impl ConcurrencyPlan {
    /// The product default: one worker for the whole job.
    pub(crate) const fn serial() -> Self {
        Self {
            total_workers: NonZeroUsize::MIN,
        }
    }

    /// A plan of at most `requested` workers, capped by the product ceiling and
    /// the host.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "ADR-0014 keeps non-serial production plans dormant until a route graduates"
        )
    )]
    pub(crate) fn bounded(requested: NonZeroUsize) -> Self {
        let host = std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
        Self::bounded_for_parallelism(requested, host)
    }

    fn bounded_for_parallelism(
        requested: NonZeroUsize,
        available_parallelism: NonZeroUsize,
    ) -> Self {
        let workers = requested
            .get()
            .min(MAX_DECODE_WORKERS)
            .min(available_parallelism.get());
        Self {
            total_workers: NonZeroUsize::new(workers).unwrap_or(NonZeroUsize::MIN),
        }
    }

    /// Exercise the production bound derivation against a deterministic host
    /// ceiling instead of depending on the test runner's CPU allocation.
    #[cfg(test)]
    pub(crate) fn bounded_for_test(
        requested: NonZeroUsize,
        available_parallelism: NonZeroUsize,
    ) -> Self {
        Self::bounded_for_parallelism(requested, available_parallelism)
    }

    pub(crate) const fn total_workers(self) -> NonZeroUsize {
        self.total_workers
    }

    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "the product plan is unconditionally serial, so nothing branches on it yet"
        )
    )]
    pub(crate) const fn is_serial(self) -> bool {
        self.total_workers.get() == 1
    }

    /// Split the whole plan across file lanes in one shot.
    ///
    /// This is the only place permits are handed out. Lanes and their packet
    /// workers come out of the same total, so a batch that widens its lanes
    /// necessarily narrows each lane's decoder instead of stacking pools.
    pub(crate) fn allocate(
        self,
        requested_file_lanes: NonZeroUsize,
    ) -> Result<PlanAllocation, AnalysisError> {
        let total = self.total_workers().get();
        let lanes = requested_file_lanes.get().min(total);
        let workers_per_lane = total / lanes;
        debug_assert!(
            lanes.saturating_mul(workers_per_lane) <= total,
            "a plan allocation may never exceed its own total"
        );

        let file_lanes = NonZeroUsize::new(lanes).unwrap_or(NonZeroUsize::MIN);
        let workers = NonZeroUsize::new(workers_per_lane).unwrap_or(NonZeroUsize::MIN);
        let decode = if workers.get() == 1 {
            DecodeReservation::serial()
        } else {
            let queue = workers
                .get()
                .saturating_mul(QUEUE_DEPTH_PER_WORKER)
                .min(MAX_DECODE_QUEUE_CAPACITY);
            let queue_capacity = NonZeroUsize::new(queue).ok_or_else(internal_plan_error)?;
            let in_flight_pcm_bytes =
                IN_FLIGHT_PCM_BYTES_PER_WORKER.saturating_mul(workers.get() as u64);
            DecodeReservation::new(workers, queue_capacity, in_flight_pcm_bytes)?
        };

        Ok(PlanAllocation { file_lanes, decode })
    }
}

impl Default for ConcurrencyPlan {
    fn default() -> Self {
        Self::serial()
    }
}

/// One non-recursive split of a [`ConcurrencyPlan`] across concurrent work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlanAllocation {
    file_lanes: NonZeroUsize,
    decode: DecodeReservation,
}

impl PlanAllocation {
    pub(crate) const fn file_lanes(self) -> NonZeroUsize {
        self.file_lanes
    }

    /// The decode permit granted to each file lane.
    pub(crate) const fn decode(self) -> DecodeReservation {
        self.decode
    }
}

fn internal_plan_error() -> AnalysisError {
    AnalysisError::new(
        ErrorCode::Internal,
        AnalysisStage::Internal,
        "the application concurrency plan produced an empty permit",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use macinmeter_domain::MAX_IN_FLIGHT_PCM_BYTES;

    fn nonzero(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).unwrap()
    }

    fn plan_of(total_workers: usize) -> ConcurrencyPlan {
        ConcurrencyPlan {
            total_workers: nonzero(total_workers),
        }
    }

    #[test]
    fn the_default_plan_is_serial_and_grants_the_serial_reservation() {
        let plan = ConcurrencyPlan::default();
        assert_eq!(plan, ConcurrencyPlan::serial());
        assert!(plan.is_serial());

        let allocation = plan.allocate(nonzero(1)).unwrap();
        assert_eq!(allocation.file_lanes().get(), 1);
        assert!(allocation.decode().is_serial());
    }

    #[test]
    fn a_plan_never_grows_past_the_product_ceiling_or_the_host() {
        let host = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
        let plan = ConcurrencyPlan::bounded(nonzero(MAX_DECODE_WORKERS * 4));
        let workers = plan.total_workers().get();
        assert!(workers <= MAX_DECODE_WORKERS, "workers={workers}");
        assert!(workers <= host, "workers={workers}; host={host}");
        assert_eq!(workers, MAX_DECODE_WORKERS.min(host));
    }

    #[test]
    fn lanes_and_their_workers_never_multiply_past_the_plan_total() {
        for total in 1..=MAX_DECODE_WORKERS {
            let plan = plan_of(total);
            for requested_lanes in 1..=(MAX_DECODE_WORKERS * 2) {
                let allocation = plan.allocate(nonzero(requested_lanes)).unwrap();
                let lanes = allocation.file_lanes().get();
                let per_lane = allocation.decode().workers().get();
                assert!(
                    lanes * per_lane <= total,
                    "total={total}; requested_lanes={requested_lanes}; \
                     lanes={lanes}; per_lane={per_lane}"
                );
                assert!(lanes <= total, "a lane may not exist without a permit");
            }
        }
    }

    #[test]
    fn widening_lanes_narrows_each_lane_instead_of_stacking_pools() {
        let plan = plan_of(8);
        assert_eq!(
            plan.allocate(nonzero(1)).unwrap().decode().workers().get(),
            8
        );
        assert_eq!(
            plan.allocate(nonzero(2)).unwrap().decode().workers().get(),
            4
        );
        assert_eq!(
            plan.allocate(nonzero(4)).unwrap().decode().workers().get(),
            2
        );

        let saturated = plan.allocate(nonzero(8)).unwrap();
        assert_eq!(saturated.file_lanes().get(), 8);
        assert!(
            saturated.decode().is_serial(),
            "a fully lane-saturated plan decodes each file serially"
        );
    }

    #[test]
    fn every_granted_reservation_stays_inside_the_domain_ceilings() {
        for total in 1..=MAX_DECODE_WORKERS {
            for requested_lanes in 1..=MAX_DECODE_WORKERS {
                let allocation = plan_of(total).allocate(nonzero(requested_lanes)).unwrap();
                let decode = allocation.decode();
                assert!(decode.workers().get() <= MAX_DECODE_WORKERS);
                assert!(decode.queue_capacity().get() <= MAX_DECODE_QUEUE_CAPACITY);
                assert!(decode.queue_capacity() >= decode.workers());
                assert!(decode.max_in_flight_pcm_bytes() <= MAX_IN_FLIGHT_PCM_BYTES);
                assert_eq!(
                    decode.max_in_flight_pcm_bytes() == 0,
                    decode.workers().get() == 1,
                    "only a single-worker lane may retain no out-of-order PCM"
                );
            }
        }
    }
}
