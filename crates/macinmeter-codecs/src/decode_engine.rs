//! Where indexed packet outcomes come from.
//!
//! The serial route and the ADR-0013 ALAC worker pool decode through the same
//! [`decode_packet`], so geometry validation, error classification and `f64`
//! conversion cannot drift between them. Only the scheduling around it differs,
//! and both hand their results to the same in-order commit buffer.

use crate::{
    error::{BACKEND, analysis_error, decoder_creation_error, runtime_error},
    flac_integrity::FlacIntegrityPlan,
    packet::{DecodedPacket, PacketOutcome},
};
use macinmeter_domain::{
    AnalysisError, AnalysisStage, ChannelCount, DecodeReservation, ErrorCode, PcmBlock,
};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    thread::{self, JoinHandle},
};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{CodecParameters, Decoder, DecoderOptions},
    errors::Error as SymphoniaError,
    formats::{FormatReader, Packet},
};

/// Maximum packets a worker may hold in its own inbox, beyond the one it is
/// decoding.
///
/// For workers other than the demux-owned slot zero, dispatch order is a fixed
/// `index % workers`, so which decoder sees which packet never depends on
/// scheduling. A smaller reservation shrinks this depth, including to a
/// zero-capacity rendezvous.
const MAX_DISPATCH_DEPTH: usize = 2;

pub(crate) fn dispatch_depth(reservation: DecodeReservation) -> usize {
    reservation
        .queue_capacity()
        .get()
        .checked_div(reservation.workers().get())
        .unwrap_or(1)
        .saturating_sub(1)
        .min(MAX_DISPATCH_DEPTH)
}

/// Test-only failure points for thread construction.
#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum SpawnFault {
    #[default]
    None,
    Worker(usize),
    Demux,
}

/// Internal options for one worker pool.
///
/// Production always uses the default. Tests pass instance-owned injection
/// state instead of mutating process-global scheduling knobs.
#[derive(Debug, Clone, Default)]
pub(crate) struct PoolOptions {
    _private: (),
    #[cfg(test)]
    force_first_result_after_later: bool,
    #[cfg(test)]
    spawn_fault: SpawnFault,
}

#[cfg(test)]
impl PoolOptions {
    pub(crate) fn force_first_result_after_later() -> Self {
        Self {
            force_first_result_after_later: true,
            ..Self::default()
        }
    }

    pub(crate) fn fail_worker_spawn(worker: usize) -> Self {
        Self {
            spawn_fault: SpawnFault::Worker(worker),
            ..Self::default()
        }
    }

    pub(crate) fn fail_demux_spawn() -> Self {
        Self {
            spawn_fault: SpawnFault::Demux,
            ..Self::default()
        }
    }
}

#[cfg(test)]
thread_local! {
    /// Worker pools started on this thread.
    ///
    /// An equivalence test that silently fell back to the serial route would
    /// pass while proving nothing, so tests assert against this counter that
    /// the pool they meant to exercise actually ran. Pools are started on the
    /// thread that opens the source, so a thread-local count stays exact while
    /// the test harness runs cases in parallel.
    pub(crate) static STARTED_WORKER_POOLS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };

    /// Threads joined while unwinding a failed pool construction on this thread.
    pub(crate) static FAILED_START_JOINED_THREADS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

/// The immutable geometry every decoded packet is validated against.
///
/// Workers only read this, so the values a packet is checked against cannot
/// depend on how far decoding has progressed.
#[derive(Debug)]
pub(crate) struct PacketDecodeContext {
    path: PathBuf,
    sample_rate: u32,
    channels: ChannelCount,
    integrity: Option<FlacIntegrityPlan>,
}

impl PacketDecodeContext {
    pub(crate) fn new(
        path: &Path,
        sample_rate: u32,
        channels: ChannelCount,
        integrity: Option<FlacIntegrityPlan>,
    ) -> Self {
        Self {
            path: path.to_path_buf(),
            sample_rate,
            channels,
            integrity,
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the backend decoder should still run its own integrity check.
    ///
    /// A check the product owns is never also delegated to the decoder: for
    /// FLAC that would hash every sample twice, and under packet workers each
    /// decoder would only ever see its own subsequence.
    pub(crate) const fn backend_verification(&self) -> bool {
        self.integrity.is_none()
    }
}

/// Decode one packet into an outcome.
///
/// Failure is returned as [`PacketOutcome::Failed`] rather than made sticky
/// here: terminal state belongs to the commit step, so the earliest input index
/// always decides which error escapes.
pub(crate) fn decode_packet(
    context: &PacketDecodeContext,
    decoder: &mut dyn Decoder,
    packet: &Packet,
) -> PacketOutcome {
    let decoded = match decoder.decode(packet) {
        Ok(decoded) => decoded,
        Err(error) => {
            return PacketOutcome::Failed(runtime_error(
                &context.path,
                "failed to decode an audio packet",
                error,
            ));
        }
    };
    if decoded.frames() == 0 {
        return PacketOutcome::Empty;
    }

    let decoded_rate = decoded.spec().rate;
    let decoded_channels = decoded.spec().channels.count();
    if decoded_rate != context.sample_rate || decoded_channels != context.channels.as_usize() {
        let details = format!(
            "opened as {} Hz/{} channels, decoder produced {decoded_rate} Hz/{decoded_channels} channels",
            context.sample_rate,
            context.channels.get(),
        );
        return PacketOutcome::Failed(analysis_error(
            &context.path,
            ErrorCode::DecodeFailed,
            AnalysisStage::Decode,
            "PCM stream parameters changed after opening",
            Some(details),
        ));
    }

    let duration = match u64::try_from(decoded.capacity()) {
        Ok(duration) => duration,
        Err(_) => {
            return PacketOutcome::Failed(analysis_error(
                &context.path,
                ErrorCode::ResourceExhausted,
                AnalysisStage::Decode,
                "decoded audio buffer is too large",
                None,
            ));
        }
    };
    // The signature bytes are taken from the decoder's own buffer, before the
    // `f64` conversion, and travel with the PCM to the in-order commit point.
    let integrity = match context.integrity {
        Some(plan) => match plan.packet_bytes(&context.path, &decoded) {
            Ok(bytes) => Some(bytes),
            Err(error) => return PacketOutcome::Failed(error),
        },
        None => None,
    };

    let mut sample_buffer = SampleBuffer::<f64>::new(duration, *decoded.spec());
    sample_buffer.copy_interleaved_ref(decoded);
    match PcmBlock::new(sample_buffer.samples().to_vec(), context.channels) {
        Ok(block) => PacketOutcome::Decoded(DecodedPacket { block, integrity }),
        Err(error) => PacketOutcome::Failed(
            error
                .with_display_path(context.path.display().to_string())
                .with_backend(BACKEND),
        ),
    }
}

/// What an engine produced for one turn.
pub(crate) enum EngineOutcome {
    /// One packet completed under its stable input index.
    Indexed { index: u64, outcome: PacketOutcome },
    /// Every packet of the selected track has been produced.
    Exhausted,
}

/// A demux and decode strategy behind the sequential commit buffer.
pub(crate) enum PacketEngine {
    /// Demux and decode on the calling thread. This is the differential oracle.
    Serial(SerialEngine),
    /// ADR-0013 stable ALAC route only: bounded workers behind sequential demux.
    PacketWorkers(PacketWorkerPool),
}

impl PacketEngine {
    /// Produce the next completed packet, in whatever order it finished.
    pub(crate) fn next(&mut self) -> Result<EngineOutcome, AnalysisError> {
        match self {
            Self::Serial(engine) => engine.next(),
            Self::PacketWorkers(pool) => pool.next(),
        }
    }

    /// Close the engine and report its integrity verdict.
    ///
    /// This runs after the commit buffer has confirmed the index space is
    /// complete, so a verdict can never mask a dropped packet.
    pub(crate) fn finish(&mut self) -> Result<(), AnalysisError> {
        match self {
            Self::Serial(engine) => engine.finish(),
            Self::PacketWorkers(pool) => pool.finish(),
        }
    }
}

/// Sequential demux and decode on the calling thread.
pub(crate) struct SerialEngine {
    context: PacketDecodeContext,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    next_index: u64,
}

impl SerialEngine {
    pub(crate) fn new(
        context: PacketDecodeContext,
        format: Box<dyn FormatReader>,
        decoder: Box<dyn Decoder>,
        track_id: u32,
    ) -> Self {
        Self {
            context,
            format,
            decoder,
            track_id,
            next_index: 0,
        }
    }

    fn next(&mut self) -> Result<EngineOutcome, AnalysisError> {
        let packet = match next_track_packet(&mut self.format, self.track_id) {
            DemuxStep::Packet(packet) => packet,
            DemuxStep::Exhausted => return Ok(EngineOutcome::Exhausted),
            DemuxStep::Failed(error) => {
                return Err(runtime_error(
                    &self.context.path,
                    "failed to read an audio packet",
                    error,
                ));
            }
        };

        // Packets are numbered at demux time, before any decode work.
        let index = self.next_index;
        self.next_index += 1;
        let outcome = decode_packet(&self.context, self.decoder.as_mut(), &packet);
        Ok(EngineOutcome::Indexed { index, outcome })
    }

    fn finish(&mut self) -> Result<(), AnalysisError> {
        verify_verdict(&self.context, self.decoder.finalize().verify_ok)
    }
}

/// One indexed unit of demuxed work.
struct IndexedPacket {
    index: u64,
    packet: Packet,
}

/// What one worker reported when it stopped.
struct WorkerVerdict {
    verify_ok: Option<bool>,
}

/// A route that has graduated to packet-level workers.
///
/// ADR-0014 §2/§3 requires the decision to name a route, never an extension or
/// a generic codec descriptor. Adding a variant here is the deliberate act of
/// graduating one, and each is graduated on its own evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParallelRoute {
    /// The ADR-0013 constrained MP4/M4A route.
    Alac,
    /// FLAC, whose stream signature the product verifies in commit order.
    Flac,
}

impl ParallelRoute {
    const fn name(self) -> &'static str {
        match self {
            Self::Alac => "ALAC",
            Self::Flac => "FLAC",
        }
    }

    const fn thread_prefix(self) -> &'static str {
        match self {
            Self::Alac => "macinmeter-alac",
            Self::Flac => "macinmeter-flac",
        }
    }
}

/// Bounded packet workers behind a sequential demux thread.
///
/// Demux, packet numbering and dispatch stay sequential; only decoding is
/// concurrent. Every thread is owned by this pool and joined before it is
/// dropped, so no decoding ever outlives the source that started it.
pub(crate) struct PacketWorkerPool {
    route: ParallelRoute,
    context: Arc<PacketDecodeContext>,
    demux: Option<JoinHandle<Result<WorkerVerdict, AnalysisError>>>,
    workers: Vec<JoinHandle<WorkerVerdict>>,
    results: Option<Receiver<(u64, PacketOutcome)>>,
    #[cfg(test)]
    force_first_result_after_later: bool,
    #[cfg(test)]
    held_first_result: Option<(u64, PacketOutcome)>,
    #[cfg(test)]
    later_result_emitted: bool,
    #[cfg(test)]
    first_result_emitted: bool,
}

impl PacketWorkerPool {
    /// Start the pool inside an already-granted reservation.
    ///
    /// Every decoder is built here, on the calling thread, so a decoder that
    /// cannot be created fails the open rather than a worker.
    pub(crate) fn new(
        route: ParallelRoute,
        context: PacketDecodeContext,
        format: Box<dyn FormatReader>,
        codec_params: &CodecParameters,
        track_id: u32,
        reservation: DecodeReservation,
        _options: PoolOptions,
    ) -> Result<Self, AnalysisError> {
        let worker_count = reservation.workers().get();
        let dispatch_depth = dispatch_depth(reservation);
        debug_assert!(
            worker_count.saturating_mul(dispatch_depth + 1) <= reservation.queue_capacity().get(),
            "dispatched packets must fit the granted reorder permit"
        );

        let decoder_options = DecoderOptions {
            verify: context.backend_verification(),
        };
        let mut decoders = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            decoders.push(
                symphonia::default::get_codecs()
                    .make(codec_params, &decoder_options)
                    .map_err(|error| decoder_creation_error(context.path(), error))?,
            );
        }

        let context = Arc::new(context);
        let (result_tx, results) = sync_channel::<(u64, PacketOutcome)>(worker_count);
        let mut decoders = decoders.into_iter();
        let demux_decoder = decoders.next().ok_or_else(|| {
            analysis_error(
                context.path(),
                ErrorCode::Internal,
                AnalysisStage::Internal,
                format!(
                    "a {} worker reservation contained no decoder slot",
                    route.name()
                ),
                None,
            )
        })?;
        let mut inboxes = Vec::with_capacity(worker_count.saturating_sub(1));
        let mut workers = Vec::with_capacity(worker_count.saturating_sub(1));

        for (worker, decoder) in (1..worker_count).zip(decoders) {
            let (packet_tx, packet_rx) = sync_channel::<IndexedPacket>(dispatch_depth);
            inboxes.push(packet_tx);
            let worker_context = Arc::clone(&context);
            let result_tx = result_tx.clone();

            #[cfg(test)]
            if _options.spawn_fault == SpawnFault::Worker(worker) {
                let error = worker_spawn_error(
                    route,
                    context.path(),
                    io::Error::other("injected worker spawn failure"),
                );
                cleanup_failed_construction(inboxes, results, workers);
                return Err(error);
            }

            let handle = match thread::Builder::new()
                .name(format!("{}-{worker}", route.thread_prefix()))
                .spawn(move || run_worker(&worker_context, decoder, &packet_rx, &result_tx))
            {
                Ok(handle) => handle,
                Err(error) => {
                    let error = worker_spawn_error(route, context.path(), error);
                    cleanup_failed_construction(inboxes, results, workers);
                    return Err(error);
                }
            };
            workers.push(handle);
        }

        let demux_context = Arc::clone(&context);
        let demux_inboxes = inboxes.clone();
        let demux_result_tx = result_tx.clone();

        #[cfg(test)]
        if _options.spawn_fault == SpawnFault::Demux {
            let error = demux_spawn_error(
                route,
                context.path(),
                io::Error::other("injected demux spawn failure"),
            );
            drop(demux_inboxes);
            cleanup_failed_construction(inboxes, results, workers);
            return Err(error);
        }

        let demux = match thread::Builder::new()
            .name(format!("{}-demux", route.thread_prefix()))
            .spawn(move || {
                run_demux(
                    &demux_context,
                    format,
                    demux_decoder,
                    track_id,
                    worker_count,
                    &demux_inboxes,
                    &demux_result_tx,
                )
            }) {
            Ok(handle) => handle,
            Err(error) => {
                let error = demux_spawn_error(route, context.path(), error);
                cleanup_failed_construction(inboxes, results, workers);
                return Err(error);
            }
        };
        debug_assert_eq!(
            workers.len() + 1,
            worker_count,
            "the demux decoder and worker handles must exactly spend the reservation"
        );
        // Only the decoder threads may keep channels open after construction.
        drop(inboxes);
        drop(result_tx);

        #[cfg(test)]
        STARTED_WORKER_POOLS.with(|started| started.set(started.get() + 1));

        Ok(Self {
            route,
            context,
            demux: Some(demux),
            workers,
            results: Some(results),
            #[cfg(test)]
            force_first_result_after_later: _options.force_first_result_after_later,
            #[cfg(test)]
            held_first_result: None,
            #[cfg(test)]
            later_result_emitted: false,
            #[cfg(test)]
            first_result_emitted: false,
        })
    }

    fn next(&mut self) -> Result<EngineOutcome, AnalysisError> {
        #[cfg(test)]
        if self.force_first_result_after_later {
            return Ok(self.next_with_forced_reordering());
        }

        Ok(self.receive())
    }

    fn receive(&self) -> EngineOutcome {
        let Some(results) = self.results.as_ref() else {
            return EngineOutcome::Exhausted;
        };
        match results.recv() {
            Ok((index, outcome)) => EngineOutcome::Indexed { index, outcome },
            // Every worker dropped its sender, so every dispatched packet has
            // already been reported.
            Err(_) => EngineOutcome::Exhausted,
        }
    }

    /// Deterministically publish a later result before packet zero.
    ///
    /// This is an instance-owned test seam at the engine boundary. It exercises
    /// the exact production reorder/commit path without process-global state or
    /// a wall-clock race between worker threads.
    #[cfg(test)]
    fn next_with_forced_reordering(&mut self) -> EngineOutcome {
        if self.later_result_emitted
            && let Some((index, outcome)) = self.held_first_result.take()
        {
            self.first_result_emitted = true;
            return EngineOutcome::Indexed { index, outcome };
        }

        loop {
            match self.receive() {
                EngineOutcome::Indexed { index: 0, outcome }
                    if !self.first_result_emitted && !self.later_result_emitted =>
                {
                    self.held_first_result = Some((0, outcome));
                }
                EngineOutcome::Indexed { index: 0, outcome } => {
                    self.first_result_emitted = true;
                    return EngineOutcome::Indexed { index: 0, outcome };
                }
                EngineOutcome::Indexed { index, outcome } if !self.first_result_emitted => {
                    self.later_result_emitted = true;
                    return EngineOutcome::Indexed { index, outcome };
                }
                EngineOutcome::Indexed { index, outcome } => {
                    return EngineOutcome::Indexed { index, outcome };
                }
                EngineOutcome::Exhausted => {
                    if let Some((index, outcome)) = self.held_first_result.take() {
                        self.first_result_emitted = true;
                        return EngineOutcome::Indexed { index, outcome };
                    }
                    return EngineOutcome::Exhausted;
                }
            }
        }
    }

    fn finish(&mut self) -> Result<(), AnalysisError> {
        let (demux_result, verdicts) = self.shutdown();
        reject_worker_verdict(&self.context, demux_result?)?;
        for verdict in verdicts {
            reject_worker_verdict(&self.context, verdict?)?;
        }
        Ok(())
    }

    /// Stop dispatching, drain both ends and join every thread.
    ///
    /// Dropping the receiver first unblocks any worker parked on `send`, which
    /// in turn disconnects the demux thread. Nothing is left detached.
    fn shutdown(
        &mut self,
    ) -> (
        Result<WorkerVerdict, AnalysisError>,
        Vec<Result<WorkerVerdict, AnalysisError>>,
    ) {
        self.results = None;

        let verdicts = self
            .workers
            .drain(..)
            .map(|worker| {
                worker.join().map_err(|_| {
                    analysis_error(
                        self.context.path(),
                        ErrorCode::Internal,
                        AnalysisStage::Internal,
                        format!(
                            "a packet decode worker on the {} route panicked",
                            self.route.name()
                        ),
                        None,
                    )
                })
            })
            .collect();

        let demux_result = match self.demux.take() {
            Some(demux) => demux.join().unwrap_or_else(|_| {
                Err(analysis_error(
                    self.context.path(),
                    ErrorCode::Internal,
                    AnalysisStage::Internal,
                    format!(
                        "the demux thread on the {} route panicked",
                        self.route.name()
                    ),
                    None,
                ))
            }),
            None => Ok(WorkerVerdict { verify_ok: None }),
        };

        (demux_result, verdicts)
    }
}

impl Drop for PacketWorkerPool {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn run_worker(
    context: &PacketDecodeContext,
    mut decoder: Box<dyn Decoder>,
    packets: &Receiver<IndexedPacket>,
    results: &SyncSender<(u64, PacketOutcome)>,
) -> WorkerVerdict {
    while let Ok(indexed) = packets.recv() {
        let outcome = decode_packet(context, decoder.as_mut(), &indexed.packet);
        if results.send((indexed.index, outcome)).is_err() {
            // The reader is gone; stop rather than decode into a closed channel.
            break;
        }
    }
    WorkerVerdict {
        verify_ok: decoder.finalize().verify_ok,
    }
}

fn run_demux(
    context: &PacketDecodeContext,
    mut format: Box<dyn FormatReader>,
    mut decoder: Box<dyn Decoder>,
    track_id: u32,
    worker_count: usize,
    inboxes: &[SyncSender<IndexedPacket>],
    results: &SyncSender<(u64, PacketOutcome)>,
) -> Result<WorkerVerdict, AnalysisError> {
    let mut next_index = 0_u64;
    loop {
        let packet = match next_track_packet(&mut format, track_id) {
            DemuxStep::Packet(packet) => packet,
            DemuxStep::Exhausted => {
                return Ok(WorkerVerdict {
                    verify_ok: decoder.finalize().verify_ok,
                });
            }
            DemuxStep::Failed(error) => {
                return Err(runtime_error(
                    context.path(),
                    "failed to read an audio packet",
                    error,
                ));
            }
        };

        let index = next_index;
        next_index += 1;
        // Dispatch is a fixed function of the index, never of which worker
        // happens to be free. Slot zero shares this thread with sequential
        // demux, so the reservation's N permits create exactly N threads.
        let worker = (index % worker_count as u64) as usize;
        if worker == 0 {
            let outcome = decode_packet(context, decoder.as_mut(), &packet);
            if results.send((index, outcome)).is_err() {
                return Ok(WorkerVerdict {
                    verify_ok: decoder.finalize().verify_ok,
                });
            }
        } else {
            let inbox = &inboxes[worker - 1];
            if inbox.send(IndexedPacket { index, packet }).is_err() {
                // A worker stopped, so the reader is already shutting down or
                // its panic will be reported by the owning join handle.
                return Ok(WorkerVerdict {
                    verify_ok: decoder.finalize().verify_ok,
                });
            }
        }
    }
}

fn reject_worker_verdict(
    context: &PacketDecodeContext,
    verdict: WorkerVerdict,
) -> Result<(), AnalysisError> {
    // ADR-0014 §4: each decoder only sees its own subset of packets, so its
    // `finalize()` cannot stand in for a stream-level integrity signature. A
    // graduated route either has no such verdict to report (ALAC) or has had it
    // relocated to the product's own in-order verifier (FLAC); either way a
    // worker reporting one means the route reached a worker it never graduated
    // for.
    if let Some(verify_ok) = verdict.verify_ok {
        return Err(analysis_error(
            context.path(),
            ErrorCode::Internal,
            AnalysisStage::Internal,
            "a packet worker decoder reported a stream-level integrity verdict that \
             worker-local finalization cannot reproduce",
            Some(format!("worker_verify_ok={verify_ok}")),
        ));
    }
    Ok(())
}

fn worker_spawn_error(route: ParallelRoute, path: &Path, error: io::Error) -> AnalysisError {
    analysis_error(
        path,
        ErrorCode::ResourceExhausted,
        AnalysisStage::Decode,
        format!(
            "failed to start a packet decode worker on the {} route",
            route.name()
        ),
        Some(error.to_string()),
    )
}

fn demux_spawn_error(route: ParallelRoute, path: &Path, error: io::Error) -> AnalysisError {
    analysis_error(
        path,
        ErrorCode::ResourceExhausted,
        AnalysisStage::Decode,
        format!(
            "failed to start the demux thread on the {} route",
            route.name()
        ),
        Some(error.to_string()),
    )
}

/// Disconnect every not-yet-owned inbox and join all threads that did start.
///
/// This guard path runs before `AlacWorkerPool` itself exists, so it cannot rely
/// on the pool's `Drop` implementation.
fn cleanup_failed_construction(
    inboxes: Vec<SyncSender<IndexedPacket>>,
    results: Receiver<(u64, PacketOutcome)>,
    workers: Vec<JoinHandle<WorkerVerdict>>,
) {
    drop(results);
    drop(inboxes);
    for worker in workers {
        let _ = worker.join();
        #[cfg(test)]
        FAILED_START_JOINED_THREADS.with(|joined| joined.set(joined.get() + 1));
    }
}

enum DemuxStep {
    Packet(Packet),
    Exhausted,
    Failed(SymphoniaError),
}

/// Pull the next packet belonging to the selected track.
fn next_track_packet(format: &mut Box<dyn FormatReader>, track_id: u32) -> DemuxStep {
    loop {
        match format.next_packet() {
            Ok(packet) if packet.track_id() == track_id => return DemuxStep::Packet(packet),
            Ok(_) => continue,
            Err(SymphoniaError::IoError(error)) if error.kind() == io::ErrorKind::UnexpectedEof => {
                return DemuxStep::Exhausted;
            }
            Err(error) => return DemuxStep::Failed(error),
        }
    }
}

fn verify_verdict(
    context: &PacketDecodeContext,
    verify_ok: Option<bool>,
) -> Result<(), AnalysisError> {
    if verify_ok == Some(false) {
        return Err(analysis_error(
            context.path(),
            ErrorCode::DecodeFailed,
            AnalysisStage::Decode,
            "decoder integrity verification failed",
            None,
        ));
    }
    Ok(())
}
