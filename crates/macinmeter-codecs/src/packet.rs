//! Indexed packet results and their bounded in-order commit buffer.
//!
//! ADR-0014 fixes one commit rule for every decode route: packets may complete
//! out of order, but PCM, frame counts, integrity state and errors are only
//! ever committed in input packet order. This module owns that rule so the
//! serial route and any future route-specific packet workers share a single
//! implementation instead of two that can drift.
//!
//! The serial route is the differential oracle. It runs on
//! [`DecodeReservation::serial`], which retains no out-of-order PCM. A result
//! at the commit head is returned directly from `accept`; only later indices
//! enter the bounded pending map.

use macinmeter_domain::{AnalysisError, AnalysisStage, DecodeReservation, ErrorCode, PcmBlock};
use std::collections::BTreeMap;

/// Everything one packet contributes to the stream, produced together.
///
/// Integrity bytes ride with the PCM because they are per-packet work that a
/// worker can do, while consuming them is reserved for the in-order commit
/// point. Keeping them in one payload means a packet can never be committed
/// for its audio but skipped for its signature.
#[derive(Debug)]
pub(crate) struct DecodedPacket {
    /// Finite interleaved `f64` frames decoded from this packet.
    pub(crate) block: PcmBlock,
    /// Bytes this packet contributes to the FLAC stream signature, if the
    /// stream declares one.
    pub(crate) integrity: Option<Vec<u8>>,
}

/// The result of decoding one packet that belongs to the selected track.
///
/// A failure is a first-class outcome: a worker may never signal failure with
/// empty PCM, and a packet is never skipped.
#[derive(Debug)]
pub(crate) enum PacketOutcome {
    /// The packet decoded into frames.
    Decoded(DecodedPacket),
    /// The packet decoded successfully but carried no frames.
    Empty,
    /// The packet failed to decode.
    Failed(AnalysisError),
}

impl PacketOutcome {
    /// Bytes a stalled result holds until its turn to commit.
    ///
    /// Integrity bytes are retained alongside the PCM, so they are charged to
    /// the same in-flight permit rather than growing outside it.
    fn retained_bytes(&self) -> u64 {
        match self {
            Self::Decoded(packet) => {
                let samples = u64::try_from(packet.block.samples().len()).unwrap_or(u64::MAX);
                let pcm = samples.saturating_mul(size_of::<f64>() as u64);
                let integrity = packet
                    .integrity
                    .as_ref()
                    .map_or(0, |bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX));
                pcm.saturating_add(integrity)
            }
            Self::Empty | Self::Failed(_) => 0,
        }
    }
}

/// A bounded buffer that commits indexed packet outcomes in input order.
///
/// Ordering is the whole point, so the earliest failing index always wins: a
/// later failure that completes first still waits behind every earlier index.
/// Once a failure is committed the buffer is terminal, and results that arrive
/// afterwards are discarded rather than reopening a settled stream.
#[derive(Debug)]
pub(crate) struct PacketReorderBuffer {
    reservation: DecodeReservation,
    next_index: u64,
    pending: BTreeMap<u64, PacketOutcome>,
    stalled_retained_bytes: u64,
    terminal: bool,
    /// Results that had to wait for an earlier index.
    ///
    /// A parallel test whose packets all happened to finish in order would
    /// prove nothing about reordering, so tests assert this actually rose.
    #[cfg(test)]
    stalled_accepts: usize,
    /// The most entries and stalled bytes ever held at once.
    ///
    /// The permit bounds retention structurally, but a bound that is merely
    /// never exceeded is not the same claim as one that does not grow with the
    /// stream. Recording the high-water mark lets a test compare streams of
    /// very different lengths directly.
    #[cfg(test)]
    peak_pending: usize,
    #[cfg(test)]
    peak_stalled_retained_bytes: u64,
}

impl PacketReorderBuffer {
    pub(crate) fn new(reservation: DecodeReservation) -> Self {
        Self {
            reservation,
            next_index: 0,
            pending: BTreeMap::new(),
            stalled_retained_bytes: 0,
            terminal: false,
            #[cfg(test)]
            stalled_accepts: 0,
            #[cfg(test)]
            peak_pending: 0,
            #[cfg(test)]
            peak_stalled_retained_bytes: 0,
        }
    }

    /// Results so far that had to wait for an earlier index.
    #[cfg(test)]
    pub(crate) const fn stalled_accepts(&self) -> usize {
        self.stalled_accepts
    }

    /// The most entries ever held at once.
    #[cfg(test)]
    pub(crate) const fn peak_pending(&self) -> usize {
        self.peak_pending
    }

    /// The most stalled retained bytes ever held at once.
    #[cfg(test)]
    pub(crate) const fn peak_stalled_retained_bytes(&self) -> u64 {
        self.peak_stalled_retained_bytes
    }

    /// The next packet index the buffer will commit.
    #[cfg(any(test, feature = "performance-probes"))]
    pub(crate) const fn next_index(&self) -> u64 {
        self.next_index
    }

    #[cfg(feature = "performance-probes")]
    pub(crate) fn pending_geometry(&self) -> (usize, u64) {
        (self.pending.len(), self.stalled_retained_bytes)
    }

    /// Accept one completed packet result.
    ///
    /// The commit head is returned immediately and never consumes a reorder
    /// slot. This is also a liveness rule: a full pending map must still accept
    /// the one earlier result that unlocks it. Results arriving after a
    /// committed failure are discarded. Duplicate indices, indices behind the
    /// commit point, and stalled results that exceed the granted queue or
    /// in-flight PCM permit are scheduler contract breaches and become
    /// structured errors rather than silent truncation.
    pub(crate) fn accept(
        &mut self,
        index: u64,
        outcome: PacketOutcome,
    ) -> Result<Option<PacketOutcome>, AnalysisError> {
        if self.terminal {
            return Ok(None);
        }
        if index < self.next_index {
            return Err(self.contract_error(
                "a packet result arrived behind the committed packet index",
                format!("index={index}; next_index={}", self.next_index),
            ));
        }
        if self.pending.contains_key(&index) {
            return Err(self.contract_error(
                "a packet index completed more than once",
                format!("index={index}"),
            ));
        }

        if index == self.next_index {
            return Ok(Some(self.advance(outcome)));
        }

        if self.pending.len() >= self.reservation.queue_capacity().get() {
            return Err(self.capacity_error(
                "reordered packet results exceeded the granted queue permit",
                format!(
                    "pending={}; queue_capacity={}",
                    self.pending.len(),
                    self.reservation.queue_capacity().get()
                ),
            ));
        }

        // Only results that wait for an earlier index reach this point. A
        // serial route therefore uses neither the pending map nor its zero-byte
        // in-flight permit.
        let bytes = outcome.retained_bytes();
        let stalled = self.stalled_retained_bytes.saturating_add(bytes);
        if stalled > self.reservation.max_in_flight_pcm_bytes() {
            return Err(self.capacity_error(
                "reordered packet payload exceeded the granted in-flight permit",
                format!(
                    "stalled_retained_bytes={stalled}; max_in_flight_pcm_bytes={}",
                    self.reservation.max_in_flight_pcm_bytes()
                ),
            ));
        }
        self.stalled_retained_bytes = stalled;

        #[cfg(test)]
        {
            self.stalled_accepts += 1;
            self.peak_pending = self.peak_pending.max(self.pending.len() + 1);
            self.peak_stalled_retained_bytes = self.peak_stalled_retained_bytes.max(stalled);
        }
        self.pending.insert(index, outcome);
        Ok(None)
    }

    /// Take the next outcome in input order, if it has completed.
    ///
    /// Returns `None` while an earlier index is still outstanding, which is
    /// what keeps a later failure from overtaking an earlier one.
    pub(crate) fn take_ready(&mut self) -> Option<PacketOutcome> {
        if self.terminal {
            return None;
        }
        let outcome = self.pending.remove(&self.next_index)?;
        Some(self.advance(outcome))
    }

    fn advance(&mut self, outcome: PacketOutcome) -> PacketOutcome {
        self.next_index += 1;
        if let Some(bytes) = self
            .pending
            .get(&self.next_index)
            .map(PacketOutcome::retained_bytes)
        {
            self.stalled_retained_bytes = self.stalled_retained_bytes.saturating_sub(bytes);
        }
        if matches!(&outcome, PacketOutcome::Failed(_)) {
            self.enter_terminal();
        }
        outcome
    }

    /// Assert that every accepted index was committed before end of stream.
    ///
    /// A surviving entry means the producer left a gap in the index space, so
    /// the stream would otherwise end on silently dropped audio.
    pub(crate) fn finish(&mut self) -> Result<(), AnalysisError> {
        if self.terminal || self.pending.is_empty() {
            return Ok(());
        }
        let lowest = self
            .pending
            .keys()
            .next()
            .copied()
            .unwrap_or(self.next_index);
        Err(self.contract_error(
            "the packet index space ended with an uncommitted gap",
            format!(
                "next_index={}; pending={}; lowest_pending={lowest}",
                self.next_index,
                self.pending.len()
            ),
        ))
    }

    fn enter_terminal(&mut self) {
        self.terminal = true;
        self.pending.clear();
        self.stalled_retained_bytes = 0;
    }

    fn contract_error(&mut self, message: &str, details: String) -> AnalysisError {
        self.enter_terminal();
        AnalysisError::new(ErrorCode::Internal, AnalysisStage::Internal, message)
            .with_details(details)
    }

    fn capacity_error(&mut self, message: &str, details: String) -> AnalysisError {
        self.enter_terminal();
        AnalysisError::new(ErrorCode::ResourceExhausted, AnalysisStage::Decode, message)
            .with_details(details)
    }
}

/// Deterministic completion orders for fault and reordering injection.
///
/// Packet workers must not be tested against a wall-clock race. These fixed
/// permutations let a test force a specific out-of-order completion and stay
/// reproducible on every host.
#[cfg(test)]
pub(crate) mod fault {
    /// Every injected completion order for a run of `len` packets.
    ///
    /// Each entry is a permutation of `0..len`, covering in-order, fully
    /// reversed, pairwise-swapped, rotated and last-first completion.
    pub(crate) fn completion_orders(len: u64) -> Vec<Vec<u64>> {
        if len == 0 {
            return vec![Vec::new()];
        }
        let in_order: Vec<u64> = (0..len).collect();

        let reversed: Vec<u64> = (0..len).rev().collect();

        let mut swapped = in_order.clone();
        for pair in swapped.chunks_mut(2) {
            pair.reverse();
        }

        let mut rotated = in_order.clone();
        rotated.rotate_left(1);

        let mut last_first = vec![len - 1];
        last_first.extend(0..len - 1);

        vec![in_order, reversed, swapped, rotated, last_first]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use macinmeter_domain::ChannelCount;
    use std::num::NonZeroUsize;

    fn nonzero(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).unwrap()
    }

    fn block(value: f64) -> DecodedPacket {
        DecodedPacket {
            block: PcmBlock::new(vec![value], ChannelCount::new(1).unwrap()).unwrap(),
            integrity: None,
        }
    }

    fn parallel_reservation() -> DecodeReservation {
        DecodeReservation::new(nonzero(4), nonzero(8), 64 * 1024).unwrap()
    }

    fn decoded_value(outcome: Option<PacketOutcome>) -> f64 {
        match outcome.expect("an outcome must be ready") {
            PacketOutcome::Decoded(packet) => packet.block.samples()[0],
            other => panic!("expected decoded PCM, got {other:?}"),
        }
    }

    fn push_decoded(committed: &mut Vec<u64>, outcome: PacketOutcome) {
        committed.push(match outcome {
            PacketOutcome::Decoded(packet) => packet.block.samples()[0] as u64,
            other => panic!("expected decoded PCM, got {other:?}"),
        });
    }

    #[test]
    fn the_serial_reservation_commits_every_packet_it_accepts() {
        let mut buffer = PacketReorderBuffer::new(DecodeReservation::serial());
        for index in 0..4 {
            let ready = buffer
                .accept(index, PacketOutcome::Decoded(block(index as f64)))
                .unwrap();
            assert_eq!(decoded_value(ready), index as f64);
            assert!(buffer.take_ready().is_none());
        }
        assert_eq!(buffer.next_index(), 4);
        buffer.finish().unwrap();
    }

    #[test]
    fn every_injected_completion_order_commits_the_same_input_sequence() {
        for order in fault::completion_orders(5) {
            let mut buffer = PacketReorderBuffer::new(parallel_reservation());
            let mut committed = Vec::new();
            for index in order.iter().copied() {
                if let Some(outcome) = buffer
                    .accept(index, PacketOutcome::Decoded(block(index as f64)))
                    .unwrap()
                {
                    push_decoded(&mut committed, outcome);
                }
                while let Some(outcome) = buffer.take_ready() {
                    push_decoded(&mut committed, outcome);
                }
            }
            buffer.finish().unwrap();
            assert_eq!(committed, vec![0, 1, 2, 3, 4], "completion order {order:?}");
        }
    }

    #[test]
    fn the_earliest_failing_index_wins_even_when_a_later_one_completes_first() {
        let mut buffer = PacketReorderBuffer::new(parallel_reservation());
        let late = AnalysisError::new(ErrorCode::DecodeFailed, AnalysisStage::Decode, "late");
        let early = AnalysisError::new(ErrorCode::MalformedMedia, AnalysisStage::Decode, "early");

        assert!(
            buffer
                .accept(2, PacketOutcome::Failed(late))
                .unwrap()
                .is_none()
        );
        assert!(
            buffer.take_ready().is_none(),
            "a later failure must not overtake outstanding earlier indices"
        );
        assert!(
            buffer
                .accept(1, PacketOutcome::Failed(early))
                .unwrap()
                .is_none()
        );
        assert!(buffer.take_ready().is_none());
        assert!(matches!(
            buffer.accept(0, PacketOutcome::Empty).unwrap(),
            Some(PacketOutcome::Empty)
        ));
        let failure = match buffer.take_ready() {
            Some(PacketOutcome::Failed(error)) => error,
            other => panic!("expected the earliest failure, got {other:?}"),
        };
        assert_eq!(failure.message, "early");
        assert!(
            buffer.take_ready().is_none(),
            "a committed failure is terminal"
        );
    }

    #[test]
    fn results_arriving_after_a_committed_failure_are_discarded() {
        let mut buffer = PacketReorderBuffer::new(parallel_reservation());
        let error = AnalysisError::new(ErrorCode::DecodeFailed, AnalysisStage::Decode, "failed");
        assert!(matches!(
            buffer.accept(0, PacketOutcome::Failed(error)).unwrap(),
            Some(PacketOutcome::Failed(_))
        ));

        assert!(
            buffer
                .accept(1, PacketOutcome::Decoded(block(1.0)))
                .expect("a late worker result must not become a second error")
                .is_none()
        );
        assert!(buffer.take_ready().is_none());
        buffer.finish().unwrap();
    }

    #[test]
    fn duplicate_and_already_committed_indices_are_contract_errors() {
        let mut buffer = PacketReorderBuffer::new(parallel_reservation());
        assert!(buffer.accept(1, PacketOutcome::Empty).unwrap().is_none());
        let error = buffer
            .accept(1, PacketOutcome::Empty)
            .expect_err("one index may complete only once");
        assert_eq!(error.code, ErrorCode::Internal);
        assert_eq!(error.stage, AnalysisStage::Internal);

        let mut buffer = PacketReorderBuffer::new(parallel_reservation());
        assert!(matches!(
            buffer.accept(0, PacketOutcome::Empty).unwrap(),
            Some(PacketOutcome::Empty)
        ));
        let error = buffer
            .accept(0, PacketOutcome::Empty)
            .expect_err("a committed index may not be reopened");
        assert_eq!(error.code, ErrorCode::Internal);
    }

    #[test]
    fn an_uncommitted_index_gap_fails_instead_of_dropping_audio() {
        let mut buffer = PacketReorderBuffer::new(parallel_reservation());
        assert!(
            buffer
                .accept(1, PacketOutcome::Decoded(block(1.0)))
                .unwrap()
                .is_none()
        );
        let error = buffer
            .finish()
            .expect_err("packet index 0 never completed, so the stream is incomplete");
        assert_eq!(error.code, ErrorCode::Internal);
        let details = error.details.expect("details name the gap");
        assert!(details.contains("next_index=0"), "{details}");
        assert!(details.contains("lowest_pending=1"), "{details}");
    }

    #[test]
    fn stalled_results_may_not_exceed_the_granted_permits() {
        let reservation = DecodeReservation::new(nonzero(2), nonzero(2), 64 * 1024).unwrap();
        let mut buffer = PacketReorderBuffer::new(reservation);
        assert!(buffer.accept(1, PacketOutcome::Empty).unwrap().is_none());
        assert!(buffer.accept(2, PacketOutcome::Empty).unwrap().is_none());
        let error = buffer
            .accept(3, PacketOutcome::Empty)
            .expect_err("the queue permit is a hard bound");
        assert_eq!(error.code, ErrorCode::ResourceExhausted);
        assert_eq!(error.stage, AnalysisStage::Decode);

        let reservation = DecodeReservation::new(nonzero(2), nonzero(8), 8).unwrap();
        let mut buffer = PacketReorderBuffer::new(reservation);
        assert!(
            buffer
                .accept(1, PacketOutcome::Decoded(block(1.0)))
                .expect("one stalled f64 sample fits the permit exactly")
                .is_none()
        );
        let error = buffer
            .accept(2, PacketOutcome::Decoded(block(2.0)))
            .expect_err("the in-flight PCM permit is a hard bound");
        assert_eq!(error.code, ErrorCode::ResourceExhausted);
    }

    /// Drive `packets` through the tightest permit under the worst completion
    /// order a bounded scheduler can produce, returning the retention peaks.
    ///
    /// The producer keeps the commit head outstanding for as long as the permit
    /// allows and only releases it when nothing else can be accepted, which is
    /// the deepest the buffer can ever be driven.
    fn worst_case_retention(packets: u64, workers: usize) -> (usize, u64, u64) {
        // The minimum legal reservation: one queued packet per worker.
        let reservation =
            DecodeReservation::new(nonzero(workers), nonzero(workers), 64 * 1024).unwrap();
        let mut buffer = PacketReorderBuffer::new(reservation);
        let mut committed = 0_u64;
        let mut next_to_produce = 0_u64;

        while committed < packets {
            let head = next_to_produce;
            // Withhold the head and fill every remaining slot behind it.
            next_to_produce += 1;
            while next_to_produce < packets && buffer.pending.len() < workers {
                buffer
                    .accept(next_to_produce, PacketOutcome::Decoded(block(1.0)))
                    .unwrap_or_else(|error| panic!("index {next_to_produce}: {error}"));
                next_to_produce += 1;
            }
            // Only now release the head, draining everything it was blocking.
            buffer
                .accept(head, PacketOutcome::Decoded(block(0.0)))
                .unwrap_or_else(|error| panic!("head {head}: {error}"));
            committed += 1;
            while buffer.take_ready().is_some() {
                committed += 1;
            }
        }

        buffer.finish().unwrap();
        (
            buffer.peak_pending(),
            buffer.peak_stalled_retained_bytes(),
            committed,
        )
    }

    #[test]
    fn worst_case_retention_does_not_grow_with_stream_length() {
        // A permit that is merely never exceeded is a weaker claim than one
        // that does not grow with the stream, so compare lengths two orders of
        // magnitude apart under identical worst-case reordering.
        for workers in [2, 4, 8] {
            let (short_pending, short_bytes, short_committed) =
                worst_case_retention(1_000, workers);
            let (long_pending, long_bytes, long_committed) = worst_case_retention(100_000, workers);

            assert_eq!(short_committed, 1_000);
            assert_eq!(long_committed, 100_000);
            assert_eq!(
                short_pending, long_pending,
                "{workers} workers retained more entries on the longer stream"
            );
            assert_eq!(
                short_bytes, long_bytes,
                "{workers} workers retained more PCM on the longer stream"
            );
            assert!(
                long_pending <= workers,
                "{workers} workers exceeded the granted queue permit: {long_pending}"
            );
            assert!(
                long_pending > 1,
                "{workers} workers never reordered, so this proved nothing"
            );
        }
    }

    #[test]
    fn committing_the_head_releases_the_in_flight_permit_it_held() {
        let reservation = DecodeReservation::new(nonzero(2), nonzero(8), 8).unwrap();
        let mut buffer = PacketReorderBuffer::new(reservation);
        // Index 1 stalls and occupies the whole in-flight permit.
        assert!(
            buffer
                .accept(1, PacketOutcome::Decoded(block(1.0)))
                .unwrap()
                .is_none()
        );
        let head = buffer
            .accept(0, PacketOutcome::Decoded(block(0.0)))
            .unwrap();
        assert_eq!(decoded_value(head), 0.0);
        assert_eq!(decoded_value(buffer.take_ready()), 1.0);

        // Index 1 stopped stalling when it became the head, so the permit is
        // free for the next out-of-order result.
        assert!(
            buffer
                .accept(3, PacketOutcome::Decoded(block(3.0)))
                .expect("the released permit must be reusable")
                .is_none()
        );
        let head = buffer
            .accept(2, PacketOutcome::Decoded(block(2.0)))
            .unwrap();
        assert_eq!(decoded_value(head), 2.0);
        assert_eq!(decoded_value(buffer.take_ready()), 3.0);
        buffer.finish().unwrap();
    }

    #[test]
    fn a_full_reorder_queue_still_accepts_the_head_that_unlocks_it() {
        let reservation = DecodeReservation::new(nonzero(2), nonzero(2), 64 * 1024).unwrap();
        let mut buffer = PacketReorderBuffer::new(reservation);
        for index in [1, 2] {
            assert!(
                buffer
                    .accept(index, PacketOutcome::Decoded(block(index as f64)))
                    .unwrap()
                    .is_none()
            );
        }

        let head = buffer
            .accept(0, PacketOutcome::Decoded(block(0.0)))
            .expect("the commit head does not consume a reorder slot");
        assert_eq!(decoded_value(head), 0.0);
        assert_eq!(decoded_value(buffer.take_ready()), 1.0);
        assert_eq!(decoded_value(buffer.take_ready()), 2.0);
        buffer.finish().unwrap();
    }
}
