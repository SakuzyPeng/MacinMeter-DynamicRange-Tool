//! Where indexed packet outcomes come from.
//!
//! The serial route and the ADR-0013 ALAC worker pool decode through the same
//! [`decode_packet`], so geometry validation, error classification and `f64`
//! conversion cannot drift between them. Only the scheduling around it differs,
//! and both hand their results to the same in-order commit buffer.

use crate::{
    error::{BACKEND, analysis_error, decoder_creation_error, runtime_error},
    packet::PacketOutcome,
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

/// Packets a worker may hold in its own inbox, beyond the one it is decoding.
///
/// With one inbox per worker the dispatch order is a fixed `index % workers`,
/// so which worker sees which packet never depends on scheduling.
const DISPATCH_DEPTH: usize = 2;

/// Microseconds the first worker sleeps before each decode, for tests.
///
/// Out-of-order completion must not be left to the scheduler's mood. Stalling
/// exactly one worker forces every packet it owns — starting with index 0 — to
/// finish last, which is the worst reordering the permits have to survive. A
/// zero value, the default, is a plain load with no timing effect.
#[cfg(test)]
pub(crate) static STALL_FIRST_WORKER_MICROS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

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
}

impl PacketDecodeContext {
    pub(crate) fn new(path: &Path, sample_rate: u32, channels: ChannelCount) -> Self {
        Self {
            path: path.to_path_buf(),
            sample_rate,
            channels,
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
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
    let mut sample_buffer = SampleBuffer::<f64>::new(duration, *decoded.spec());
    sample_buffer.copy_interleaved_ref(decoded);
    match PcmBlock::new(sample_buffer.samples().to_vec(), context.channels) {
        Ok(block) => PacketOutcome::Decoded(block),
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
    AlacWorkers(AlacWorkerPool),
}

impl PacketEngine {
    /// Produce the next completed packet, in whatever order it finished.
    pub(crate) fn next(&mut self) -> Result<EngineOutcome, AnalysisError> {
        match self {
            Self::Serial(engine) => engine.next(),
            Self::AlacWorkers(pool) => pool.next(),
        }
    }

    /// Close the engine and report its integrity verdict.
    ///
    /// This runs after the commit buffer has confirmed the index space is
    /// complete, so a verdict can never mask a dropped packet.
    pub(crate) fn finish(&mut self) -> Result<(), AnalysisError> {
        match self {
            Self::Serial(engine) => engine.finish(),
            Self::AlacWorkers(pool) => pool.finish(),
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

/// Bounded ALAC packet workers behind a sequential demux thread.
///
/// Demux, packet numbering and dispatch stay sequential; only decoding is
/// concurrent. Every thread is owned by this pool and joined before it is
/// dropped, so no decoding ever outlives the source that started it.
pub(crate) struct AlacWorkerPool {
    context: Arc<PacketDecodeContext>,
    demux: Option<JoinHandle<Result<(), AnalysisError>>>,
    workers: Vec<JoinHandle<WorkerVerdict>>,
    results: Option<Receiver<(u64, PacketOutcome)>>,
}

impl AlacWorkerPool {
    /// Start the pool inside an already-granted reservation.
    ///
    /// Every decoder is built here, on the calling thread, so a decoder that
    /// cannot be created fails the open rather than a worker.
    pub(crate) fn new(
        context: PacketDecodeContext,
        format: Box<dyn FormatReader>,
        codec_params: &CodecParameters,
        track_id: u32,
        reservation: DecodeReservation,
    ) -> Result<Self, AnalysisError> {
        let worker_count = reservation.workers().get();
        debug_assert!(
            worker_count.saturating_mul(DISPATCH_DEPTH + 1) <= reservation.queue_capacity().get(),
            "dispatched packets must fit the granted reorder permit"
        );

        let mut decoders = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            decoders.push(
                symphonia::default::get_codecs()
                    .make(codec_params, &DecoderOptions { verify: true })
                    .map_err(|error| decoder_creation_error(context.path(), error))?,
            );
        }

        let context = Arc::new(context);
        let (result_tx, results) = sync_channel::<(u64, PacketOutcome)>(worker_count);
        let mut inboxes = Vec::with_capacity(worker_count);
        let mut workers = Vec::with_capacity(worker_count);

        for (worker, decoder) in decoders.into_iter().enumerate() {
            let (packet_tx, packet_rx) = sync_channel::<IndexedPacket>(DISPATCH_DEPTH);
            inboxes.push(packet_tx);
            let worker_context = Arc::clone(&context);
            let result_tx = result_tx.clone();
            workers.push(
                thread::Builder::new()
                    .name(format!("macinmeter-alac-{worker}"))
                    .spawn(move || {
                        run_worker(worker, &worker_context, decoder, &packet_rx, &result_tx)
                    })
                    .map_err(|error| {
                        analysis_error(
                            context.path(),
                            ErrorCode::ResourceExhausted,
                            AnalysisStage::Decode,
                            "failed to start an ALAC decode worker",
                            Some(error.to_string()),
                        )
                    })?,
            );
        }
        // Only the workers may keep the result channel open, so the reader can
        // recognise "every worker finished" as a plain disconnect.
        drop(result_tx);

        let demux_context = Arc::clone(&context);
        let demux = thread::Builder::new()
            .name("macinmeter-alac-demux".to_owned())
            .spawn(move || run_demux(&demux_context, format, track_id, &inboxes))
            .map_err(|error| {
                analysis_error(
                    context.path(),
                    ErrorCode::ResourceExhausted,
                    AnalysisStage::Decode,
                    "failed to start the ALAC demux thread",
                    Some(error.to_string()),
                )
            })?;

        #[cfg(test)]
        STARTED_WORKER_POOLS.with(|started| started.set(started.get() + 1));

        Ok(Self {
            context,
            demux: Some(demux),
            workers,
            results: Some(results),
        })
    }

    fn next(&mut self) -> Result<EngineOutcome, AnalysisError> {
        let Some(results) = self.results.as_ref() else {
            return Ok(EngineOutcome::Exhausted);
        };
        match results.recv() {
            Ok((index, outcome)) => Ok(EngineOutcome::Indexed { index, outcome }),
            // Every worker dropped its sender, so every dispatched packet has
            // already been reported.
            Err(_) => Ok(EngineOutcome::Exhausted),
        }
    }

    fn finish(&mut self) -> Result<(), AnalysisError> {
        let (demux_result, verdicts) = self.shutdown();
        demux_result?;
        for verdict in verdicts {
            // ADR-0014 §4: a worker only ever saw its own subset of packets, so
            // its `finalize()` cannot stand in for a stream-level integrity
            // signature. This route is graduated on a decoder that reports no
            // such verdict at all; if one ever appears, the parallel path must
            // fail rather than quietly accept a per-subset check.
            if let Some(verify_ok) = verdict?.verify_ok {
                return Err(analysis_error(
                    self.context.path(),
                    ErrorCode::Internal,
                    AnalysisStage::Internal,
                    "the ALAC route decoder reported a stream-level integrity verdict that \
                     worker-local finalization cannot reproduce",
                    Some(format!("worker_verify_ok={verify_ok}")),
                ));
            }
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
        Result<(), AnalysisError>,
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
                        "an ALAC decode worker panicked",
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
                    "the ALAC demux thread panicked",
                    None,
                ))
            }),
            None => Ok(()),
        };

        (demux_result, verdicts)
    }
}

impl Drop for AlacWorkerPool {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn run_worker(
    worker: usize,
    context: &PacketDecodeContext,
    mut decoder: Box<dyn Decoder>,
    packets: &Receiver<IndexedPacket>,
    results: &SyncSender<(u64, PacketOutcome)>,
) -> WorkerVerdict {
    while let Ok(indexed) = packets.recv() {
        #[cfg(test)]
        if worker == 0 {
            let micros = STALL_FIRST_WORKER_MICROS.load(std::sync::atomic::Ordering::Relaxed);
            if micros > 0 {
                thread::sleep(std::time::Duration::from_micros(micros));
            }
        }
        #[cfg(not(test))]
        let _ = worker;

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
    track_id: u32,
    inboxes: &[SyncSender<IndexedPacket>],
) -> Result<(), AnalysisError> {
    let mut next_index = 0_u64;
    loop {
        let packet = match next_track_packet(&mut format, track_id) {
            DemuxStep::Packet(packet) => packet,
            DemuxStep::Exhausted => return Ok(()),
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
        // happens to be free.
        let inbox = &inboxes[(index % inboxes.len() as u64) as usize];
        if inbox.send(IndexedPacket { index, packet }).is_err() {
            // The worker stopped, so the reader is already shutting down.
            return Ok(());
        }
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
