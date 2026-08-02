//! Indexed packet results and their bounded in-order commit buffer.
//!
//! ADR-0014 fixes one commit rule for every decode route: packets may complete
//! out of order, but PCM, frame counts, integrity state and errors are only
//! ever committed in input packet order. This module owns that rule so the
//! serial route and any future route-specific packet workers share a single
//! implementation instead of two that can drift.
//!
//! The serial route is the differential oracle. It runs on
//! [`DecodeReservation::serial`], which retains no out-of-order PCM, so every
//! `accept` is immediately followed by a matching `take_ready`.

use macinmeter_domain::{AnalysisError, AnalysisStage, DecodeReservation, ErrorCode, PcmBlock};
use std::collections::BTreeMap;

/// The result of decoding one packet that belongs to the selected track.
///
/// A failure is a first-class outcome: a worker may never signal failure with
/// empty PCM, and a packet is never skipped.
#[derive(Debug)]
pub(crate) enum PacketOutcome {
    /// Finite interleaved `f64` frames decoded from this packet.
    Decoded(PcmBlock),
    /// The packet decoded successfully but carried no frames.
    Empty,
    /// The packet failed to decode.
    Failed(AnalysisError),
}

impl PacketOutcome {
    fn pcm_bytes(&self) -> u64 {
        match self {
            Self::Decoded(block) => {
                let samples = u64::try_from(block.samples().len()).unwrap_or(u64::MAX);
                samples.saturating_mul(size_of::<f64>() as u64)
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
    stalled_pcm_bytes: u64,
    terminal: bool,
}

impl PacketReorderBuffer {
    pub(crate) fn new(reservation: DecodeReservation) -> Self {
        Self {
            reservation,
            next_index: 0,
            pending: BTreeMap::new(),
            stalled_pcm_bytes: 0,
            terminal: false,
        }
    }

    /// The next packet index the buffer will commit.
    #[cfg(test)]
    pub(crate) const fn next_index(&self) -> u64 {
        self.next_index
    }

    /// Accept one completed packet result.
    ///
    /// Results arriving after a committed failure are discarded. Duplicate
    /// indices, indices behind the commit point, and results that exceed the
    /// granted queue or in-flight PCM permit are scheduler contract breaches
    /// and become structured errors rather than silent truncation.
    pub(crate) fn accept(
        &mut self,
        index: u64,
        outcome: PacketOutcome,
    ) -> Result<(), AnalysisError> {
        if self.terminal {
            return Ok(());
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

        // Only a result that has to wait for an earlier index occupies the
        // in-flight PCM permit. A serial route always accepts the head, which
        // is why its zero budget is satisfiable rather than vacuous.
        let stalls = index != self.next_index;
        let bytes = outcome.pcm_bytes();
        if stalls {
            let stalled = self.stalled_pcm_bytes.saturating_add(bytes);
            if stalled > self.reservation.max_in_flight_pcm_bytes() {
                return Err(self.capacity_error(
                    "reordered packet PCM exceeded the granted in-flight permit",
                    format!(
                        "stalled_pcm_bytes={stalled}; max_in_flight_pcm_bytes={}",
                        self.reservation.max_in_flight_pcm_bytes()
                    ),
                ));
            }
            self.stalled_pcm_bytes = stalled;
        }

        self.pending.insert(index, outcome);
        Ok(())
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
        self.next_index += 1;
        if let Some(bytes) = self
            .pending
            .get(&self.next_index)
            .map(PacketOutcome::pcm_bytes)
        {
            self.stalled_pcm_bytes = self.stalled_pcm_bytes.saturating_sub(bytes);
        }
        if matches!(outcome, PacketOutcome::Failed(_)) {
            self.enter_terminal();
        }
        Some(outcome)
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
        self.stalled_pcm_bytes = 0;
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

    fn block(value: f64) -> PcmBlock {
        PcmBlock::new(vec![value], ChannelCount::new(1).unwrap()).unwrap()
    }

    fn parallel_reservation() -> DecodeReservation {
        DecodeReservation::new(nonzero(4), nonzero(8), 64 * 1024).unwrap()
    }

    fn decoded_value(outcome: Option<PacketOutcome>) -> f64 {
        match outcome.expect("an outcome must be ready") {
            PacketOutcome::Decoded(block) => block.samples()[0],
            other => panic!("expected decoded PCM, got {other:?}"),
        }
    }

    #[test]
    fn the_serial_reservation_commits_every_packet_it_accepts() {
        let mut buffer = PacketReorderBuffer::new(DecodeReservation::serial());
        for index in 0..4 {
            buffer
                .accept(index, PacketOutcome::Decoded(block(index as f64)))
                .unwrap();
            assert_eq!(decoded_value(buffer.take_ready()), index as f64);
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
                buffer
                    .accept(index, PacketOutcome::Decoded(block(index as f64)))
                    .unwrap();
                while let Some(outcome) = buffer.take_ready() {
                    committed.push(match outcome {
                        PacketOutcome::Decoded(block) => block.samples()[0] as u64,
                        other => panic!("expected decoded PCM, got {other:?}"),
                    });
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

        buffer.accept(2, PacketOutcome::Failed(late)).unwrap();
        assert!(
            buffer.take_ready().is_none(),
            "a later failure must not overtake outstanding earlier indices"
        );
        buffer.accept(1, PacketOutcome::Failed(early)).unwrap();
        assert!(buffer.take_ready().is_none());
        buffer.accept(0, PacketOutcome::Empty).unwrap();

        assert!(matches!(buffer.take_ready(), Some(PacketOutcome::Empty)));
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
        buffer.accept(0, PacketOutcome::Failed(error)).unwrap();
        assert!(matches!(
            buffer.take_ready(),
            Some(PacketOutcome::Failed(_))
        ));

        buffer
            .accept(1, PacketOutcome::Decoded(block(1.0)))
            .expect("a late worker result must not become a second error");
        assert!(buffer.take_ready().is_none());
        buffer.finish().unwrap();
    }

    #[test]
    fn duplicate_and_already_committed_indices_are_contract_errors() {
        let mut buffer = PacketReorderBuffer::new(parallel_reservation());
        buffer.accept(1, PacketOutcome::Empty).unwrap();
        let error = buffer
            .accept(1, PacketOutcome::Empty)
            .expect_err("one index may complete only once");
        assert_eq!(error.code, ErrorCode::Internal);
        assert_eq!(error.stage, AnalysisStage::Internal);

        let mut buffer = PacketReorderBuffer::new(parallel_reservation());
        buffer.accept(0, PacketOutcome::Empty).unwrap();
        buffer.take_ready().unwrap();
        let error = buffer
            .accept(0, PacketOutcome::Empty)
            .expect_err("a committed index may not be reopened");
        assert_eq!(error.code, ErrorCode::Internal);
    }

    #[test]
    fn an_uncommitted_index_gap_fails_instead_of_dropping_audio() {
        let mut buffer = PacketReorderBuffer::new(parallel_reservation());
        buffer
            .accept(1, PacketOutcome::Decoded(block(1.0)))
            .unwrap();
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
        buffer.accept(1, PacketOutcome::Empty).unwrap();
        buffer.accept(2, PacketOutcome::Empty).unwrap();
        let error = buffer
            .accept(3, PacketOutcome::Empty)
            .expect_err("the queue permit is a hard bound");
        assert_eq!(error.code, ErrorCode::ResourceExhausted);
        assert_eq!(error.stage, AnalysisStage::Decode);

        let reservation = DecodeReservation::new(nonzero(2), nonzero(8), 8).unwrap();
        let mut buffer = PacketReorderBuffer::new(reservation);
        buffer
            .accept(1, PacketOutcome::Decoded(block(1.0)))
            .expect("one stalled f64 sample fits the permit exactly");
        let error = buffer
            .accept(2, PacketOutcome::Decoded(block(2.0)))
            .expect_err("the in-flight PCM permit is a hard bound");
        assert_eq!(error.code, ErrorCode::ResourceExhausted);
    }

    #[test]
    fn committing_the_head_releases_the_in_flight_permit_it_held() {
        let reservation = DecodeReservation::new(nonzero(2), nonzero(8), 8).unwrap();
        let mut buffer = PacketReorderBuffer::new(reservation);
        // Index 1 stalls and occupies the whole in-flight permit.
        buffer
            .accept(1, PacketOutcome::Decoded(block(1.0)))
            .unwrap();
        buffer
            .accept(0, PacketOutcome::Decoded(block(0.0)))
            .unwrap();
        assert_eq!(decoded_value(buffer.take_ready()), 0.0);
        assert_eq!(decoded_value(buffer.take_ready()), 1.0);

        // Index 1 stopped stalling when it became the head, so the permit is
        // free for the next out-of-order result.
        buffer
            .accept(3, PacketOutcome::Decoded(block(3.0)))
            .expect("the released permit must be reusable");
        buffer
            .accept(2, PacketOutcome::Decoded(block(2.0)))
            .unwrap();
        assert_eq!(decoded_value(buffer.take_ready()), 2.0);
        assert_eq!(decoded_value(buffer.take_ready()), 3.0);
        buffer.finish().unwrap();
    }
}
