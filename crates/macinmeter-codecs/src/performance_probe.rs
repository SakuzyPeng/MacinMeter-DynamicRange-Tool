//! Source-owned counters for local ADR-0014 attribution.
//!
//! This module only exists behind the non-default `performance-probes` feature.
//! It observes the selected product topology without adding a scheduler, a
//! process-global switch, or a public tuning surface.

use macinmeter_domain::MAX_DECODE_WORKERS;
use std::{
    array,
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
    time::Instant,
};

const ORDERING: Ordering = Ordering::Relaxed;

#[derive(Debug, Default)]
struct WorkerCounters {
    packets: AtomicU64,
    backend_decode_ns: AtomicU64,
    integrity_conversion_ns: AtomicU64,
    pcm_conversion_ns: AtomicU64,
    inbox_wait_ns: AtomicU64,
    result_send_wait_ns: AtomicU64,
    lifetime_ns: AtomicU64,
}

/// Thread-local totals published once when a decoder thread stops.
#[derive(Debug, Default)]
pub(crate) struct WorkerProbeTotals {
    pub(crate) packets: u64,
    pub(crate) backend_decode_ns: u64,
    pub(crate) integrity_conversion_ns: u64,
    pub(crate) pcm_conversion_ns: u64,
    pub(crate) inbox_wait_ns: u64,
    pub(crate) result_send_wait_ns: u64,
    pub(crate) lifetime_ns: u64,
}

/// One decoder slot's accumulated timing.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketWorkerProbeSnapshot {
    pub slot: usize,
    pub packets: u64,
    pub backend_decode_ns: u64,
    pub integrity_conversion_ns: u64,
    pub pcm_conversion_ns: u64,
    pub inbox_wait_ns: u64,
    pub result_send_wait_ns: u64,
    pub lifetime_ns: u64,
}

/// A completed source's timing counters.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketPipelineProbeSnapshot {
    pub decoder_workers: usize,
    pub file_identify_ns: u64,
    pub container_inspection_ns: u64,
    pub backend_probe_ns: u64,
    pub route_setup_ns: u64,
    pub demux_packet_read_ns: u64,
    pub demux_dispatch_wait_ns: u64,
    pub caller_result_wait_ns: u64,
    pub caller_commit_ns: u64,
    pub caller_finish_ns: u64,
    pub reorder_stalls: u64,
    pub peak_reorder_packets: usize,
    pub peak_reorder_bytes: u64,
    pub hasher_packets: u64,
    pub hasher_receive_wait_ns: u64,
    pub hasher_active_ns: u64,
    pub hasher_send_wait_ns: u64,
    pub hasher_lifetime_ns: u64,
    pub workers: Vec<PacketWorkerProbeSnapshot>,
}

/// One source-owned collector.
///
/// Atomics are only updated in explicit probe builds. Hot decoder loops retain
/// thread-local totals and publish once at thread exit, so observation does not
/// turn every packet into a shared-counter contention point.
#[doc(hidden)]
#[derive(Debug)]
pub struct PacketPipelineProbe {
    decoder_workers: AtomicUsize,
    file_identify_ns: AtomicU64,
    container_inspection_ns: AtomicU64,
    backend_probe_ns: AtomicU64,
    route_setup_ns: AtomicU64,
    demux_packet_read_ns: AtomicU64,
    demux_dispatch_wait_ns: AtomicU64,
    caller_result_wait_ns: AtomicU64,
    caller_commit_ns: AtomicU64,
    caller_finish_ns: AtomicU64,
    reorder_stalls: AtomicU64,
    peak_reorder_packets: AtomicUsize,
    peak_reorder_bytes: AtomicU64,
    hasher_packets: AtomicU64,
    hasher_receive_wait_ns: AtomicU64,
    hasher_active_ns: AtomicU64,
    hasher_send_wait_ns: AtomicU64,
    hasher_lifetime_ns: AtomicU64,
    workers: [WorkerCounters; MAX_DECODE_WORKERS],
}

impl Default for PacketPipelineProbe {
    fn default() -> Self {
        Self {
            decoder_workers: AtomicUsize::new(0),
            file_identify_ns: AtomicU64::new(0),
            container_inspection_ns: AtomicU64::new(0),
            backend_probe_ns: AtomicU64::new(0),
            route_setup_ns: AtomicU64::new(0),
            demux_packet_read_ns: AtomicU64::new(0),
            demux_dispatch_wait_ns: AtomicU64::new(0),
            caller_result_wait_ns: AtomicU64::new(0),
            caller_commit_ns: AtomicU64::new(0),
            caller_finish_ns: AtomicU64::new(0),
            reorder_stalls: AtomicU64::new(0),
            peak_reorder_packets: AtomicUsize::new(0),
            peak_reorder_bytes: AtomicU64::new(0),
            hasher_packets: AtomicU64::new(0),
            hasher_receive_wait_ns: AtomicU64::new(0),
            hasher_active_ns: AtomicU64::new(0),
            hasher_send_wait_ns: AtomicU64::new(0),
            hasher_lifetime_ns: AtomicU64::new(0),
            workers: array::from_fn(|_| WorkerCounters::default()),
        }
    }
}

impl PacketPipelineProbe {
    pub(crate) fn set_decoder_workers(&self, workers: usize) {
        debug_assert!((1..=MAX_DECODE_WORKERS).contains(&workers));
        self.decoder_workers.store(workers, ORDERING);
    }

    pub(crate) fn add_file_identify(&self, started: Instant) {
        add_elapsed(&self.file_identify_ns, started);
    }

    pub(crate) fn add_container_inspection(&self, started: Instant) {
        add_elapsed(&self.container_inspection_ns, started);
    }

    pub(crate) fn add_backend_probe(&self, started: Instant) {
        add_elapsed(&self.backend_probe_ns, started);
    }

    pub(crate) fn add_route_setup(&self, started: Instant) {
        add_elapsed(&self.route_setup_ns, started);
    }

    pub(crate) fn add_demux_packet_read(&self, started: Instant) {
        add_elapsed(&self.demux_packet_read_ns, started);
    }

    pub(crate) fn add_demux_dispatch_wait(&self, started: Instant) {
        add_elapsed(&self.demux_dispatch_wait_ns, started);
    }

    pub(crate) fn add_caller_result_wait(&self, started: Instant) {
        add_elapsed(&self.caller_result_wait_ns, started);
    }

    pub(crate) fn add_caller_commit(&self, started: Instant) {
        add_elapsed(&self.caller_commit_ns, started);
    }

    pub(crate) fn add_caller_finish(&self, started: Instant) {
        add_elapsed(&self.caller_finish_ns, started);
    }

    pub(crate) fn observe_reorder(&self, stalled: bool, packets: usize, bytes: u64) {
        if stalled {
            self.reorder_stalls.fetch_add(1, ORDERING);
        }
        self.peak_reorder_packets.fetch_max(packets, ORDERING);
        self.peak_reorder_bytes.fetch_max(bytes, ORDERING);
    }

    pub(crate) fn add_hasher_send_wait(&self, started: Instant) {
        add_elapsed(&self.hasher_send_wait_ns, started);
    }

    pub(crate) fn record_hasher(
        &self,
        packets: u64,
        receive_wait_ns: u64,
        active_ns: u64,
        lifetime_ns: u64,
    ) {
        self.hasher_packets.fetch_add(packets, ORDERING);
        self.hasher_receive_wait_ns
            .fetch_add(receive_wait_ns, ORDERING);
        self.hasher_active_ns.fetch_add(active_ns, ORDERING);
        self.hasher_lifetime_ns.fetch_add(lifetime_ns, ORDERING);
    }

    pub(crate) fn record_worker(&self, slot: usize, totals: &WorkerProbeTotals) {
        let counters = &self.workers[slot];
        counters.packets.fetch_add(totals.packets, ORDERING);
        counters
            .backend_decode_ns
            .fetch_add(totals.backend_decode_ns, ORDERING);
        counters
            .integrity_conversion_ns
            .fetch_add(totals.integrity_conversion_ns, ORDERING);
        counters
            .pcm_conversion_ns
            .fetch_add(totals.pcm_conversion_ns, ORDERING);
        counters
            .inbox_wait_ns
            .fetch_add(totals.inbox_wait_ns, ORDERING);
        counters
            .result_send_wait_ns
            .fetch_add(totals.result_send_wait_ns, ORDERING);
        counters.lifetime_ns.fetch_add(totals.lifetime_ns, ORDERING);
    }

    /// Take a stable post-EOF view. Callers snapshot only after the source has
    /// joined every owned thread.
    pub fn snapshot(&self) -> PacketPipelineProbeSnapshot {
        let decoder_workers = self.decoder_workers.load(ORDERING);
        let workers = self.workers[..decoder_workers]
            .iter()
            .enumerate()
            .map(|(slot, counters)| PacketWorkerProbeSnapshot {
                slot,
                packets: counters.packets.load(ORDERING),
                backend_decode_ns: counters.backend_decode_ns.load(ORDERING),
                integrity_conversion_ns: counters.integrity_conversion_ns.load(ORDERING),
                pcm_conversion_ns: counters.pcm_conversion_ns.load(ORDERING),
                inbox_wait_ns: counters.inbox_wait_ns.load(ORDERING),
                result_send_wait_ns: counters.result_send_wait_ns.load(ORDERING),
                lifetime_ns: counters.lifetime_ns.load(ORDERING),
            })
            .collect();
        PacketPipelineProbeSnapshot {
            decoder_workers,
            file_identify_ns: self.file_identify_ns.load(ORDERING),
            container_inspection_ns: self.container_inspection_ns.load(ORDERING),
            backend_probe_ns: self.backend_probe_ns.load(ORDERING),
            route_setup_ns: self.route_setup_ns.load(ORDERING),
            demux_packet_read_ns: self.demux_packet_read_ns.load(ORDERING),
            demux_dispatch_wait_ns: self.demux_dispatch_wait_ns.load(ORDERING),
            caller_result_wait_ns: self.caller_result_wait_ns.load(ORDERING),
            caller_commit_ns: self.caller_commit_ns.load(ORDERING),
            caller_finish_ns: self.caller_finish_ns.load(ORDERING),
            reorder_stalls: self.reorder_stalls.load(ORDERING),
            peak_reorder_packets: self.peak_reorder_packets.load(ORDERING),
            peak_reorder_bytes: self.peak_reorder_bytes.load(ORDERING),
            hasher_packets: self.hasher_packets.load(ORDERING),
            hasher_receive_wait_ns: self.hasher_receive_wait_ns.load(ORDERING),
            hasher_active_ns: self.hasher_active_ns.load(ORDERING),
            hasher_send_wait_ns: self.hasher_send_wait_ns.load(ORDERING),
            hasher_lifetime_ns: self.hasher_lifetime_ns.load(ORDERING),
            workers,
        }
    }
}

pub(crate) fn elapsed_ns(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

fn add_elapsed(counter: &AtomicU64, started: Instant) {
    counter.fetch_add(elapsed_ns(started), ORDERING);
}
