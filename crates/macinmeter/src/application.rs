use crate::{
    AnalysisError, AnalysisEvent, AnalysisReport, AnalysisStage, ErrorCode, ExecutionControl,
};
use macinmeter_analysis::AnalyzerSession;
#[cfg(feature = "performance-probes")]
use macinmeter_codecs::{DecodeEngineKind, DecodeExecution};
use macinmeter_codecs::{DecoderFactory, OpenedAudio, ReadOutcome};
use macinmeter_domain::{DecodeReservation, PcmBlock, PcmStreamInfo};
use serde::{Deserialize, Serialize};
use std::{
    io,
    num::NonZeroUsize,
    path::PathBuf,
    sync::mpsc::{SyncSender, sync_channel},
    thread,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeRequest {
    pub path: PathBuf,
}

impl AnalyzeRequest {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

/// The exact application execution selected by a non-default performance run.
///
/// This type is deliberately absent from ordinary builds and from every product
/// report/wire field. The ADR-0007 worker uses it to prove that a case labelled
/// with a worker count actually received that grant and reached the intended
/// decode/analysis topology.
#[cfg(feature = "performance-probes")]
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplicationPerformanceProbe {
    granted_decode_workers: usize,
    selected_engine: DecodeEngineKind,
    selected_total_workers: usize,
    selected_decoder_workers: usize,
    selected_hasher_workers: usize,
    decode_analysis_overlapped: bool,
    requested_overlap_channel_depth: usize,
    applied_overlap_channel_depth: Option<usize>,
    decoded_blocks: u64,
    final_block_frames: usize,
}

#[cfg(feature = "performance-probes")]
impl ApplicationPerformanceProbe {
    const fn new(
        reservation: DecodeReservation,
        execution: DecodeExecution,
        channel_depth: usize,
        opened: &OpenedAnalysis,
    ) -> Self {
        Self {
            granted_decode_workers: reservation.workers().get(),
            selected_engine: execution.engine(),
            selected_total_workers: execution.workers().get(),
            selected_decoder_workers: execution.decoder_workers().get(),
            selected_hasher_workers: execution.hasher_workers(),
            decode_analysis_overlapped: opened.overlapped,
            requested_overlap_channel_depth: channel_depth,
            // A refused shape creates no hand-off at all. Keep that distinct
            // from the request so a benchmark cannot label a serial fallback
            // with the depth it failed to apply.
            applied_overlap_channel_depth: if opened.overlapped {
                Some(channel_depth)
            } else {
                None
            },
            decoded_blocks: opened.decoded_blocks,
            final_block_frames: opened.final_block_frames,
        }
    }

    pub const fn granted_decode_workers(self) -> usize {
        self.granted_decode_workers
    }

    pub const fn selected_engine(self) -> &'static str {
        match self.selected_engine {
            DecodeEngineKind::Serial => "Serial",
            DecodeEngineKind::AlacPacketWorkers => "AlacPacketWorkers",
            DecodeEngineKind::FlacPacketWorkers => "FlacPacketWorkers",
        }
    }

    pub const fn selected_total_workers(self) -> usize {
        self.selected_total_workers
    }

    pub const fn selected_decoder_workers(self) -> usize {
        self.selected_decoder_workers
    }

    pub const fn selected_hasher_workers(self) -> usize {
        self.selected_hasher_workers
    }

    pub const fn decode_analysis_overlapped(self) -> bool {
        self.decode_analysis_overlapped
    }

    pub const fn requested_overlap_channel_depth(self) -> usize {
        self.requested_overlap_channel_depth
    }

    pub const fn applied_overlap_channel_depth(self) -> Option<usize> {
        self.applied_overlap_channel_depth
    }

    pub const fn decoded_blocks(self) -> u64 {
        self.decoded_blocks
    }

    pub const fn final_block_frames(self) -> usize {
        self.final_block_frames
    }
}

struct AnalyzedFile {
    report: AnalysisReport,
    #[cfg(feature = "performance-probes")]
    probe: ApplicationPerformanceProbe,
}

struct OpenedAnalysis {
    report: AnalysisReport,
    #[cfg(feature = "performance-probes")]
    overlapped: bool,
    #[cfg(feature = "performance-probes")]
    decoded_blocks: u64,
    #[cfg(feature = "performance-probes")]
    final_block_frames: usize,
}

#[cfg(feature = "performance-probes")]
#[derive(Debug, Default)]
struct DecodeBlockGeometry {
    blocks: u64,
    final_block_frames: usize,
}

#[cfg(feature = "performance-probes")]
impl DecodeBlockGeometry {
    fn record(&mut self, block: &PcmBlock) -> Result<(), AnalysisError> {
        self.blocks = self.blocks.checked_add(1).ok_or_else(|| {
            AnalysisError::new(
                ErrorCode::Internal,
                AnalysisStage::Internal,
                "application performance-probe block count overflowed",
            )
        })?;
        self.final_block_frames = block.frames();
        Ok(())
    }
}

#[cfg(test)]
thread_local! {
    /// The engine the most recent analysis on this thread actually selected.
    pub(crate) static LAST_DECODE_EXECUTION:
        std::cell::Cell<Option<macinmeter_codecs::DecodeExecution>> =
        const { std::cell::Cell::new(None) };
    /// Whether the most recent analysis on this thread ran decode and analysis
    /// on separate threads. A differential that silently stayed serial would
    /// pass while proving nothing, exactly as for the decode engine above.
    pub(crate) static LAST_ANALYSIS_OVERLAP: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

#[derive(Debug, Default)]
pub(crate) struct Analyzer {
    decoder_factory: DecoderFactory,
    /// The same grant the factory decodes inside. Overlap is charged against
    /// it, so this is a copy of one plan rather than a second budget.
    reservation: DecodeReservation,
    /// Default unless an explicit measurement asked for another shape.
    overlap_shape: OverlapShape,
}

impl Analyzer {
    /// Build an analyzer that decodes inside an already-granted permit, with an
    /// explicit hand-off shape.
    ///
    /// There is one constructor rather than a default-shaped convenience so
    /// that no call site can acquire a shape by omission. The shape is still
    /// priced against `decode`'s own in-flight allowance, so it cannot widen
    /// retention past the granted plan; one that does not fit leaves the stream
    /// serial.
    pub(crate) fn with_overlap_shape(decode: DecodeReservation, shape: OverlapShape) -> Self {
        Self {
            decoder_factory: DecoderFactory::with_application_reservation(decode),
            reservation: decode,
            overlap_shape: shape,
        }
    }

    pub(crate) fn analyze_file_with_control(
        &self,
        request: AnalyzeRequest,
        control: &ExecutionControl<'_>,
    ) -> Result<AnalysisReport, AnalysisError> {
        self.analyze_file_at(request, 0, control)
    }

    #[cfg(feature = "performance-probes")]
    pub(crate) fn analyze_file_with_performance_probe(
        &self,
        request: AnalyzeRequest,
        control: &ExecutionControl<'_>,
    ) -> Result<(AnalysisReport, ApplicationPerformanceProbe), AnalysisError> {
        self.analyze_file_at_run(request, 0, control)
            .map(|analyzed| (analyzed.report, analyzed.probe))
    }

    pub(crate) fn analyze_file_at(
        &self,
        request: AnalyzeRequest,
        item_index: usize,
        control: &ExecutionControl<'_>,
    ) -> Result<AnalysisReport, AnalysisError> {
        self.analyze_file_at_run(request, item_index, control)
            .map(|analyzed| analyzed.report)
    }

    fn analyze_file_at_run(
        &self,
        request: AnalyzeRequest,
        item_index: usize,
        control: &ExecutionControl<'_>,
    ) -> Result<AnalyzedFile, AnalysisError> {
        ensure_not_cancelled(control)?;
        let display_path = request.path.display().to_string();
        control.progress.emit(AnalysisEvent::FileStarted {
            index: item_index,
            display_path: display_path.clone(),
        });

        let result = self.analyze_started(request, item_index, control, &display_path);
        control.progress.emit(AnalysisEvent::FileFinished {
            index: item_index,
            display_path,
            success: result.is_ok(),
        });
        result
    }

    fn analyze_started(
        &self,
        request: AnalyzeRequest,
        item_index: usize,
        control: &ExecutionControl<'_>,
        display_path: &str,
    ) -> Result<AnalyzedFile, AnalysisError> {
        // The selected engine, not the requested reservation, decides what is
        // left for the overlap: every route that has not graduated falls back
        // to the serial engine and spends one permit however many it was
        // granted, and those unspent permits are exactly what the overlap runs
        // on. A route that spent them all leaves nothing and stays as it was.
        let (opened, execution) = self.decoder_factory.open_with_execution(&request.path)?;
        #[cfg(test)]
        LAST_DECODE_EXECUTION.with(|last| last.set(Some(execution)));
        let overlap = OverlapBudget {
            shape: self.overlap_shape,
            spare_permits: self
                .reservation
                .workers()
                .get()
                .saturating_sub(execution.workers().get()),
            max_in_flight_pcm_bytes: self.reservation.max_in_flight_pcm_bytes(),
            max_pcm_block_bytes: execution.max_pcm_block_bytes(),
            inject_spawn_failure: false,
            schedule: OverlapSchedule::default(),
        };
        #[cfg(feature = "performance-probes")]
        let channel_depth = overlap.channel_depth();
        let opened = Self::analyze_opened_run(opened, overlap, item_index, control, display_path)?;
        #[cfg(feature = "performance-probes")]
        let probe =
            ApplicationPerformanceProbe::new(self.reservation, execution, channel_depth, &opened);
        Ok(AnalyzedFile {
            report: opened.report,
            #[cfg(feature = "performance-probes")]
            probe,
        })
    }

    #[cfg(test)]
    pub(crate) fn analyze_opened(
        opened: OpenedAudio,
        budget: OverlapBudget,
        item_index: usize,
        control: &ExecutionControl<'_>,
        display_path: &str,
    ) -> Result<AnalysisReport, AnalysisError> {
        Self::analyze_opened_run(opened, budget, item_index, control, display_path)
            .map(|analyzed| analyzed.report)
    }

    fn analyze_opened_run(
        mut opened: OpenedAudio,
        budget: OverlapBudget,
        item_index: usize,
        control: &ExecutionControl<'_>,
        display_path: &str,
    ) -> Result<OpenedAnalysis, AnalysisError> {
        #[cfg(test)]
        LAST_ANALYSIS_OVERLAP.with(|last| last.set(false));
        #[cfg(feature = "performance-probes")]
        let mut block_geometry = DecodeBlockGeometry::default();
        let pcm = opened.reader.stream_info().clone();
        let mut session = AnalyzerSession::new(pcm.spec.clone())?;

        // Read one block on the calling thread first. Overlap is admitted only
        // once a real block has proven its retention fits the granted budget,
        // so a stream that cannot prove that bound stays serial before any
        // thread exists, exactly as an over-wide FLAC reorder window does.
        ensure_not_cancelled(control)?;
        let first = read_checked(&mut opened, &pcm, display_path)?;

        let (analysis, _overlapped) = match first {
            None => {
                // Preserve the serial boundary: EOF progress is observable and
                // may itself request cancellation, which must win before a
                // zero-frame report is finalized.
                emit_decode_progress(&opened, item_index, control, display_path);
                ensure_not_cancelled(control)?;
                (session.finish()?, false)
            }
            Some(block) if budget.admits(&block) => {
                #[cfg(feature = "performance-probes")]
                block_geometry.record(&block)?;
                // The first block establishes the same public commit boundary
                // as the serial path. Overlap begins with the next decode, so
                // analysis failures still precede this block's progress event.
                session.push_interleaved(block.samples())?;
                emit_decode_progress(&opened, item_index, control, display_path);
                ensure_not_cancelled(control)?;
                let analysis = analyze_overlapped(
                    session,
                    &mut opened,
                    item_index,
                    control,
                    display_path,
                    &budget,
                    #[cfg(feature = "performance-probes")]
                    &mut block_geometry,
                )?;
                (analysis, true)
            }
            Some(block) => {
                #[cfg(feature = "performance-probes")]
                block_geometry.record(&block)?;
                session.push_interleaved(block.samples())?;
                emit_decode_progress(&opened, item_index, control, display_path);
                let analysis = analyze_serially(
                    session,
                    &mut opened,
                    &pcm,
                    item_index,
                    control,
                    display_path,
                    #[cfg(feature = "performance-probes")]
                    &mut block_geometry,
                )?;
                (analysis, false)
            }
        };

        let diagnostics = opened.reader.diagnostics().clone();
        match AnalysisReport::try_new(opened.source, pcm, analysis, diagnostics) {
            Ok(report) => Ok(OpenedAnalysis {
                report,
                #[cfg(feature = "performance-probes")]
                overlapped: _overlapped,
                #[cfg(feature = "performance-probes")]
                decoded_blocks: block_geometry.blocks,
                #[cfg(feature = "performance-probes")]
                final_block_frames: block_geometry.final_block_frames,
            }),
            Err(error) => Err(error
                .with_display_path(display_path)
                .with_backend(opened.reader.diagnostics().backend.clone())),
        }
    }
}

/// Bytes the hand-off may keep in flight, across every block it retains.
///
/// This is a cache-residency bound, not a memory quota: the plan's own
/// `max_in_flight_pcm_bytes` is megabytes and would permit depths whose retained
/// blocks are evicted before the analyst reads them. The batching candidate
/// established that failure mode directly — a hand-off big enough to leave the
/// analyst's cache regressed below no change at all — and the plan cannot see a
/// cache size to bound it with. 320 KiB is chosen to stay inside any per-core L2
/// the product plausibly runs on, and so that the most common geometry lands on
/// the depth the two-host A/B actually measured.
const OVERLAP_RETAINED_BYTES: u64 = 320 * 1024;

/// The deepest hand-off the derivation will ask for.
///
/// The measured curve is flat well before here on every host and track swept, so
/// this is the end of the plateau rather than a limit the budget would not
/// otherwise reach.
const MAX_OVERLAP_CHANNEL_DEPTH: usize = 16;

/// How the overlap hand-off is shaped, in blocks.
///
/// This does not change what the analyzer sees: one block crosses per message,
/// in stream order, at every depth. It only trades retained PCM against how
/// often the producer parks.
///
/// The depth is **derived from the stream's own worst-case block size** rather
/// than fixed, for the same reason file lanes derive their width: a constant
/// good for two channels is not good for sixty-four, and the product supports
/// up to `MAX_ANALYSIS_CHANNELS`. Deriving it also bounds the blast radius —
/// every stream lands either on the depth the A/B measured or on something
/// shallower, down to the depth-one hand-off that graduated before it, so no
/// geometry reaches a configuration with neither evidence behind it.
///
/// Batching several blocks per message was explored alongside this and is
/// deliberately absent. Its host- and composition-sensitive behavior did not
/// justify adding another hand-off rule whose cache residency the plan cannot
/// price. Depth keeps the existing one-block-per-message invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlapShape {
    /// Take the depth from the stream's proven worst-case block size.
    Derived,
    /// An explicit depth. Reachable only from a measurement build, and still
    /// priced against the granted allowance like any other.
    Explicit(NonZeroUsize),
}

impl Default for OverlapShape {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl OverlapShape {
    /// What an ordinary build uses, and the only shape it can reach.
    pub(crate) const DEFAULT: Self = Self::Derived;

    #[cfg(any(test, feature = "performance-probes"))]
    pub(crate) const fn new(channel_depth: NonZeroUsize) -> Self {
        Self::Explicit(channel_depth)
    }

    /// The depth this shape asks for on a stream whose worst-case block is
    /// `max_block_bytes`.
    ///
    /// The derivation spends [`OVERLAP_RETAINED_BYTES`] on `depth + 1` blocks —
    /// the channel's `depth` plus the one the analyst is pushing — and never
    /// drops below one, which is the hand-off that shipped before depth existed.
    pub(crate) const fn channel_depth(self, max_block_bytes: u64) -> usize {
        match self {
            Self::Explicit(depth) => depth.get(),
            Self::Derived => {
                if max_block_bytes == 0 {
                    return MAX_OVERLAP_CHANNEL_DEPTH;
                }
                let affordable = OVERLAP_RETAINED_BYTES / max_block_bytes;
                let depth = affordable.saturating_sub(1);
                if depth >= MAX_OVERLAP_CHANNEL_DEPTH as u64 {
                    MAX_OVERLAP_CHANNEL_DEPTH
                } else if depth < 1 {
                    1
                } else {
                    depth as usize
                }
            }
        }
    }

    /// Blocks this shape retains beyond the one a serial run already holds.
    ///
    /// The channel holds `depth` blocks and the analyst holds the one it is
    /// pushing, so a derived depth never exceeds [`OVERLAP_RETAINED_BYTES`] and
    /// the depth-one floor retains two, exactly as before depth existed.
    const fn retained_blocks(self, max_block_bytes: u64) -> u64 {
        (self.channel_depth(max_block_bytes) as u64).saturating_add(1)
    }
}

/// What an already-granted plan leaves available for decode/analysis overlap.
///
/// Every field comes from the one reservation the route decodes inside.
/// Overlap spends a permit that route did not, so it can never add a thread
/// the plan has not already counted.
#[derive(Debug, Clone, Default)]
pub(crate) struct OverlapBudget {
    spare_permits: usize,
    max_in_flight_pcm_bytes: u64,
    /// Probe-time upper bound for one decoded block. `None` means the route
    /// cannot prove safe retention and must stay serial.
    max_pcm_block_bytes: Option<u64>,
    /// How the hand-off is shaped. Default outside an explicit measurement.
    shape: OverlapShape,
    /// Instance-owned deterministic fault injection; production always leaves
    /// it false.
    inject_spawn_failure: bool,
    /// Deterministic schedule seam across the hand-off. Zero-sized outside
    /// tests, so production carries no hook or branch for it.
    schedule: OverlapSchedule,
}

impl OverlapBudget {
    /// Whether this block's stream may run decode and analysis concurrently.
    ///
    /// Overlap retains blocks beyond the one a serial run already holds, and
    /// how many depends only on the shape. The decoder proves a worst-case
    /// block bound during probe. Pricing that bound rather than the first
    /// observed block keeps valid variable-block streams from making retention
    /// depend on their first packet geometry. A shape too wide for the granted
    /// in-flight allowance is refused here, so it degrades to serial rather
    /// than exceeding a budget the plan already handed out.
    fn admits(&self, block: &PcmBlock) -> bool {
        let first_bytes = (block.samples().len() as u64).saturating_mul(size_of::<f64>() as u64);
        let Some(max_block_bytes) = self.max_pcm_block_bytes else {
            return false;
        };
        let retained = max_block_bytes.saturating_mul(self.shape.retained_blocks(max_block_bytes));
        self.spare_permits >= 1
            && first_bytes <= max_block_bytes
            && retained <= self.max_in_flight_pcm_bytes
    }

    /// The depth this budget's hand-off will actually use.
    ///
    /// A stream with no proven block bound never overlaps, so the value it
    /// would take is immaterial; the derivation still answers with the floor
    /// rather than a shape nothing priced.
    fn channel_depth(&self) -> usize {
        self.shape
            .channel_depth(self.max_pcm_block_bytes.unwrap_or(u64::MAX))
    }

    fn schedule(&self) -> OverlapSchedule {
        self.schedule.clone()
    }
}

enum AnalysisInput {
    Block(PcmBlock),
    Finish,
}

#[cfg(not(test))]
fn send_analysis_block(sender: &SyncSender<AnalysisInput>, block: PcmBlock) -> bool {
    sender.send(AnalysisInput::Block(block)).is_ok()
}

#[cfg(test)]
fn send_analysis_block(
    sender: &SyncSender<AnalysisInput>,
    block: PcmBlock,
    block_index: usize,
    schedule: &OverlapSchedule,
) -> bool {
    let sent = match sender.try_send(AnalysisInput::Block(block)) {
        Ok(()) => true,
        Err(std::sync::mpsc::TrySendError::Full(AnalysisInput::Block(block))) => {
            // This hook runs only after the real bounded channel reports
            // itself full, immediately before the same input enters the
            // blocking send. Tests can therefore release a held analyst
            // without inferring queue state from progress timing.
            schedule.on_full_send(block_index);
            sender.send(AnalysisInput::Block(block)).is_ok()
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => false,
        Err(std::sync::mpsc::TrySendError::Full(AnalysisInput::Finish)) => {
            unreachable!("send_analysis_block only constructs block inputs")
        }
    };
    if sent {
        schedule.after_send(block_index);
    }
    sent
}

/// A deterministic seam around the overlap hand-off, for forcing schedules
/// the production build can reach but a plain fixture cannot.
///
/// In an ordinary build this is a zero-sized no-op, so the overlap carries no
/// hook, branch or storage for it.
#[cfg(not(test))]
#[derive(Debug, Clone, Default)]
pub(crate) struct OverlapSchedule {}

#[cfg(not(test))]
impl OverlapSchedule {
    #[inline]
    fn before_push(&self, _block: &PcmBlock) {}
}

#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct OverlapSchedule {
    #[allow(clippy::type_complexity)]
    before_push: Option<std::sync::Arc<dyn Fn(usize, &PcmBlock) + Send + Sync>>,
    #[allow(clippy::type_complexity)]
    on_full_send: Option<std::sync::Arc<dyn Fn(usize) + Send + Sync>>,
    #[allow(clippy::type_complexity)]
    after_send: Option<std::sync::Arc<dyn Fn(usize) + Send + Sync>>,
}

#[cfg(test)]
impl std::fmt::Debug for OverlapSchedule {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OverlapSchedule")
            .field("before_push", &self.before_push.is_some())
            .field("on_full_send", &self.on_full_send.is_some())
            .field("after_send", &self.after_send.is_some())
            .finish()
    }
}

#[cfg(test)]
impl OverlapSchedule {
    fn new(before_push: impl Fn(usize, &PcmBlock) + Send + Sync + 'static) -> Self {
        Self {
            before_push: Some(std::sync::Arc::new(before_push)),
            on_full_send: None,
            after_send: None,
        }
    }

    fn with_full_send_hook(mut self, on_full_send: impl Fn(usize) + Send + Sync + 'static) -> Self {
        self.on_full_send = Some(std::sync::Arc::new(on_full_send));
        self
    }

    fn with_after_send_hook(mut self, after_send: impl Fn(usize) + Send + Sync + 'static) -> Self {
        self.after_send = Some(std::sync::Arc::new(after_send));
        self
    }

    fn before_push(&self, block_index: usize, block: &PcmBlock) {
        if let Some(hook) = &self.before_push {
            hook(block_index, block);
        }
    }

    fn on_full_send(&self, block_index: usize) {
        if let Some(hook) = &self.on_full_send {
            hook(block_index);
        }
    }

    fn after_send(&self, block_index: usize) {
        if let Some(hook) = &self.after_send {
            hook(block_index);
        }
    }
}

/// Drive decode and analysis on separate threads, committing in read order.
///
/// The hand-off is a single ordered channel consumed by one thread, so the
/// analyzer sees exactly the block sequence a serial run would push and the
/// result cannot depend on the overlap.
fn analyze_overlapped(
    mut session: AnalyzerSession,
    opened: &mut OpenedAudio,
    item_index: usize,
    control: &ExecutionControl<'_>,
    display_path: &str,
    budget: &OverlapBudget,
    #[cfg(feature = "performance-probes")] block_geometry: &mut DecodeBlockGeometry,
) -> Result<macinmeter_domain::AnalysisResult, AnalysisError> {
    // Two stages, so no depth buys parallelism. Depth only trades retained PCM
    // against how often the producer parks, and `admits` already refused any
    // depth whose retention the plan has not granted.
    let (blocks, incoming) = sync_channel::<AnalysisInput>(budget.channel_depth());

    let analyst_schedule = budget.schedule();
    #[cfg(test)]
    let producer_schedule = analyst_schedule.clone();
    thread::scope(|scope| {
        let operation = move || -> Result<Option<_>, AnalysisError> {
            // The caller thread already pushed block 0, so the first block this
            // thread sees is block 1 of the stream.
            #[cfg(test)]
            let mut block_index = 1_usize;
            loop {
                match incoming.recv() {
                    Ok(AnalysisInput::Block(block)) => {
                        analyst_schedule.before_push(
                            #[cfg(test)]
                            block_index,
                            &block,
                        );
                        #[cfg(test)]
                        {
                            block_index = block_index.saturating_add(1);
                        }
                        session.push_interleaved(block.samples())?;
                    }
                    Ok(AnalysisInput::Finish) => {
                        // This is the serial pre-finish cancellation boundary,
                        // after every accepted block has reached the analyzer.
                        ensure_not_cancelled(control)?;
                        return session.finish().map(Some);
                    }
                    // Decode failure and cancellation deliberately disconnect
                    // without sending Finish. Do not finalize a partial prefix.
                    Err(_) => return Ok(None),
                }
            }
        };
        let builder = thread::Builder::new().name("macinmeter-analysis".to_owned());
        let spawned = if budget.inject_spawn_failure {
            Err(io::Error::other(
                "injected overlapped analysis spawn failure",
            ))
        } else {
            builder.spawn_scoped(scope, operation)
        };
        let analyst = spawned.map_err(|error| analyst_spawn_error(display_path, error))?;
        #[cfg(test)]
        LAST_ANALYSIS_OVERLAP.with(|last| last.set(true));
        let pcm = opened.reader.stream_info().clone();

        let decoded = (|| -> Result<(), AnalysisError> {
            #[cfg(test)]
            let mut block_index = 1_usize;
            loop {
                ensure_not_cancelled(control)?;
                let Some(block) = read_checked(opened, &pcm, display_path)? else {
                    emit_decode_progress(opened, item_index, control, display_path);
                    // A progress observer can cancel at EOF. The analysis
                    // thread performs the authoritative pre-finish check after
                    // all preceding blocks, so send the explicit terminator.
                    if blocks.send(AnalysisInput::Finish).is_err() {
                        return Ok(());
                    }
                    return Ok(());
                };
                #[cfg(feature = "performance-probes")]
                block_geometry.record(&block)?;
                emit_decode_progress(opened, item_index, control, display_path);
                if !send_analysis_block(
                    &blocks,
                    block,
                    #[cfg(test)]
                    block_index,
                    #[cfg(test)]
                    &producer_schedule,
                ) {
                    // The analyzer stopped early, so it holds the failure that
                    // decides this stream. Stop feeding it and let the join
                    // below report that error rather than a disconnect.
                    return Ok(());
                }
                #[cfg(test)]
                {
                    block_index = block_index.saturating_add(1);
                }
            }
        })();
        drop(blocks);

        // Join before deciding anything: the analyzer thread must be finished
        // before its session or its error can be read.
        let analysed = match analyst.join() {
            Ok(analysed) => analysed,
            Err(_) => return Err(analyst_panic_error(display_path)),
        };

        // An analysis failure is always the earlier one. The analyzer only sees
        // blocks decode already produced, so a failure at block J means decode
        // got at least to J, and any decode failure is at a later block.
        match analysed {
            Err(error) => Err(error),
            Ok(Some(analysis)) => {
                decoded?;
                Ok(analysis)
            }
            Ok(None) => match decoded {
                Err(error) => Err(error),
                Ok(()) => Err(analysis_channel_error(display_path)),
            },
        }
    })
}

/// Drive decode and analysis on the calling thread alone.
fn analyze_serially(
    mut session: AnalyzerSession,
    opened: &mut OpenedAudio,
    pcm: &PcmStreamInfo,
    item_index: usize,
    control: &ExecutionControl<'_>,
    display_path: &str,
    #[cfg(feature = "performance-probes")] block_geometry: &mut DecodeBlockGeometry,
) -> Result<macinmeter_domain::AnalysisResult, AnalysisError> {
    loop {
        ensure_not_cancelled(control)?;
        let Some(block) = read_checked(opened, pcm, display_path)? else {
            emit_decode_progress(opened, item_index, control, display_path);
            break;
        };
        #[cfg(feature = "performance-probes")]
        block_geometry.record(&block)?;
        session.push_interleaved(block.samples())?;
        emit_decode_progress(opened, item_index, control, display_path);
    }

    ensure_not_cancelled(control)?;
    session.finish()
}

/// Read the next block, rejecting one whose geometry contradicts the stream.
fn read_checked(
    opened: &mut OpenedAudio,
    pcm: &PcmStreamInfo,
    display_path: &str,
) -> Result<Option<PcmBlock>, AnalysisError> {
    match opened.reader.read_block()? {
        ReadOutcome::Data(block) => {
            if block.channels() != pcm.spec.channels {
                return Err(AnalysisError::new(
                    ErrorCode::DecodeFailed,
                    AnalysisStage::Decode,
                    format!(
                        "decoder produced a {}-channel PCM block for a {}-channel stream",
                        block.channels().get(),
                        pcm.spec.channels.get()
                    ),
                )
                .with_display_path(display_path)
                .with_backend(opened.reader.diagnostics().backend.clone())
                .with_details(format!(
                    "block_channels={}; stream_channels={}",
                    block.channels().get(),
                    pcm.spec.channels.get()
                )));
            }
            Ok(Some(block))
        }
        ReadOutcome::Eof => Ok(None),
    }
}

fn emit_decode_progress(
    opened: &OpenedAudio,
    item_index: usize,
    control: &ExecutionControl<'_>,
    display_path: &str,
) {
    control.progress.emit(AnalysisEvent::DecodeProgress {
        index: item_index,
        display_path: display_path.to_owned(),
        progress: opened.reader.progress(),
    });
}

fn analyst_panic_error(display_path: &str) -> AnalysisError {
    AnalysisError::new(
        ErrorCode::Internal,
        AnalysisStage::Analysis,
        "the overlapped analysis thread panicked",
    )
    .with_display_path(display_path)
}

fn analyst_spawn_error(display_path: &str, error: io::Error) -> AnalysisError {
    AnalysisError::new(
        ErrorCode::ResourceExhausted,
        AnalysisStage::Analysis,
        "failed to start the overlapped analysis thread",
    )
    .with_display_path(display_path)
    .with_details(error.to_string())
    .recoverable(true)
}

fn analysis_channel_error(display_path: &str) -> AnalysisError {
    AnalysisError::new(
        ErrorCode::Internal,
        AnalysisStage::Analysis,
        "the overlapped analysis channel disconnected without a terminal outcome",
    )
    .with_display_path(display_path)
}

fn ensure_not_cancelled(control: &ExecutionControl<'_>) -> Result<(), AnalysisError> {
    if control.cancellation.is_cancelled() {
        Err(AnalysisError::cancelled())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CancellationToken, ChannelCount, ChannelLayout, ContainerFormat, DecodeDiagnostics,
        DecodeProgress, NoopProgressSink, PcmBlock, PcmStreamInfo, SampleRate, SourceCodec,
        SourceInfo, StreamSpec, concurrency::ConcurrencyPlan,
    };
    use macinmeter_codecs::PcmSource;
    use std::{
        collections::VecDeque,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize as TestCounter, Ordering},
        },
        time::Duration,
    };

    struct FakeSource {
        stream_info: PcmStreamInfo,
        blocks: VecDeque<PcmBlock>,
        decoded_frames: u64,
        eof: bool,
        diagnostics: DecodeDiagnostics,
        terminal_diagnostic_frames: Option<u64>,
        terminal_error: Option<AnalysisError>,
    }

    impl PcmSource for FakeSource {
        fn stream_info(&self) -> &PcmStreamInfo {
            &self.stream_info
        }

        fn read_block(&mut self) -> Result<ReadOutcome, AnalysisError> {
            if let Some(block) = self.blocks.pop_front() {
                self.decoded_frames += u64::try_from(block.frames()).unwrap();
                self.diagnostics.decoded_frames = self.decoded_frames;
                return Ok(ReadOutcome::Data(block));
            }

            if let Some(error) = self.terminal_error.as_ref() {
                return Err(error.clone());
            }

            self.eof = true;
            if let Some(decoded_frames) = self.terminal_diagnostic_frames {
                self.diagnostics.decoded_frames = decoded_frames;
            }
            Ok(ReadOutcome::Eof)
        }

        fn progress(&self) -> DecodeProgress {
            DecodeProgress::new(self.decoded_frames, None, self.eof)
        }

        fn diagnostics(&self) -> &DecodeDiagnostics {
            &self.diagnostics
        }
    }

    fn opened_audio(
        source_channels: ChannelCount,
        stream_channels: ChannelCount,
        blocks: Vec<PcmBlock>,
    ) -> OpenedAudio {
        opened_audio_with_terminal_diagnostics(source_channels, stream_channels, blocks, None)
    }

    fn opened_audio_with_terminal_diagnostics(
        source_channels: ChannelCount,
        stream_channels: ChannelCount,
        blocks: Vec<PcmBlock>,
        terminal_diagnostic_frames: Option<u64>,
    ) -> OpenedAudio {
        opened_audio_with_terminal(
            source_channels,
            stream_channels,
            blocks,
            terminal_diagnostic_frames,
            None,
        )
    }

    fn opened_audio_with_terminal_error(
        source_channels: ChannelCount,
        stream_channels: ChannelCount,
        blocks: Vec<PcmBlock>,
        terminal_error: AnalysisError,
    ) -> OpenedAudio {
        opened_audio_with_terminal(
            source_channels,
            stream_channels,
            blocks,
            None,
            Some(terminal_error),
        )
    }

    fn opened_audio_with_terminal(
        source_channels: ChannelCount,
        stream_channels: ChannelCount,
        blocks: Vec<PcmBlock>,
        terminal_diagnostic_frames: Option<u64>,
        terminal_error: Option<AnalysisError>,
    ) -> OpenedAudio {
        let stream_info = PcmStreamInfo {
            spec: StreamSpec::new(48_000, stream_channels.get(), ChannelLayout::Unknown).unwrap(),
            expected_frames: None,
        };
        OpenedAudio {
            source: SourceInfo {
                display_path: "source.fake".to_owned(),
                container: ContainerFormat::Wave,
                codec: SourceCodec::PcmFloat,
                sample_rate: SampleRate::new(48_000).unwrap(),
                channels: source_channels,
                bits_per_sample: Some(64),
                expected_frames: None,
            },
            reader: Box::new(FakeSource {
                stream_info,
                blocks: blocks.into(),
                decoded_frames: 0,
                eof: false,
                diagnostics: DecodeDiagnostics {
                    backend: "fake-source".to_owned(),
                    decoded_frames: 0,
                    warnings: Vec::new(),
                },
                terminal_diagnostic_frames,
                terminal_error,
            }),
        }
    }

    #[test]
    fn rejects_pcm_blocks_built_with_a_different_channel_geometry() {
        let stream_channels = ChannelCount::new(2).unwrap();
        let block_channels = ChannelCount::new(1).unwrap();
        let opened = opened_audio(
            block_channels,
            stream_channels,
            // If this block reaches the two-channel analyzer, squaring f64::MAX fails with an
            // analysis error. The expected decode error therefore proves geometry is checked first.
            vec![PcmBlock::new(vec![f64::MAX, 0.0], block_channels).unwrap()],
        );
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;

        let error = Analyzer::analyze_opened(
            opened,
            OverlapBudget::default(),
            0,
            &ExecutionControl::new(&cancellation, &progress),
            "event-path.fake",
        )
        .expect_err("block geometry must match the immutable stream geometry");

        assert_eq!(error.code, ErrorCode::DecodeFailed);
        assert_eq!(error.stage, AnalysisStage::Decode);
        assert_eq!(error.display_path.as_deref(), Some("event-path.fake"));
        assert_eq!(error.backend.as_deref(), Some("fake-source"));
        let details = error.details.expect("geometry details should be present");
        assert!(details.contains("block_channels=1"));
        assert!(details.contains("stream_channels=2"));
    }

    #[test]
    fn validates_every_block_before_analysis_and_progress_publication() {
        let stream_channels = ChannelCount::new(2).unwrap();
        let opened = opened_audio(
            ChannelCount::new(1).unwrap(),
            stream_channels,
            vec![
                PcmBlock::new(vec![0.25, -0.25], stream_channels).unwrap(),
                // This must be rejected before it can mutate or fail the analyzer.
                PcmBlock::new(vec![f64::MAX, 0.0], ChannelCount::new(1).unwrap()).unwrap(),
            ],
        );
        let cancellation = CancellationToken::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_for_sink = Arc::clone(&events);
        let progress = move |event| {
            events_for_sink.lock().unwrap().push(event);
        };

        let error = Analyzer::analyze_opened(
            opened,
            OverlapBudget::default(),
            7,
            &ExecutionControl::new(&cancellation, &progress),
            "event-path.fake",
        )
        .expect_err("a later mismatched block must prevent a partial report");

        assert_eq!(error.code, ErrorCode::DecodeFailed);
        assert_eq!(error.stage, AnalysisStage::Decode);
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            AnalysisEvent::DecodeProgress {
                index: 7,
                display_path,
                progress,
            } if display_path == "event-path.fake"
                && progress.decoded_frames() == 1
                && !progress.is_eof()
        ));
    }

    #[test]
    fn rejects_diagnostics_that_disagree_with_the_finished_analysis() {
        let channels = ChannelCount::new(1).unwrap();
        let opened = opened_audio_with_terminal_diagnostics(
            channels,
            channels,
            vec![PcmBlock::new(vec![0.25], channels).unwrap()],
            Some(2),
        );
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;

        let error = Analyzer::analyze_opened(
            opened,
            OverlapBudget::default(),
            0,
            &ExecutionControl::new(&cancellation, &progress),
            "diagnostics.fake",
        )
        .expect_err("mismatched terminal diagnostics must not escape as a successful report");

        assert_eq!(error.code, ErrorCode::DecodeFailed);
        assert_eq!(error.stage, AnalysisStage::Decode);
        assert_eq!(error.display_path.as_deref(), Some("diagnostics.fake"));
        assert_eq!(error.backend.as_deref(), Some("fake-source"));
        assert!(error.message.contains("diagnostics"));
    }

    /// A budget wide enough to admit every block these tests build.
    fn overlapping() -> OverlapBudget {
        OverlapBudget {
            spare_permits: 1,
            max_in_flight_pcm_bytes: 4 * 1024 * 1024,
            max_pcm_block_bytes: Some(256 * size_of::<f64>() as u64),
            shape: OverlapShape::DEFAULT,
            inject_spawn_failure: false,
            schedule: OverlapSchedule::default(),
        }
    }

    /// The overlap budget with deterministic hand-off hooks.
    fn overlapping_with(schedule: OverlapSchedule) -> OverlapBudget {
        OverlapBudget {
            schedule,
            ..overlapping()
        }
    }

    fn varied_blocks(channels: ChannelCount) -> Vec<PcmBlock> {
        // Values chosen so every window accumulator carries a different
        // magnitude: an overlap that dropped, reordered or duplicated a block
        // would move peak, RMS or the frame count.
        (1..=64_u32)
            .map(|step| {
                let base = f64::from(step) / 512.0;
                PcmBlock::new(
                    (0..256)
                        .map(|index| {
                            let offset = f64::from(index) / 4096.0;
                            if index % 3 == 0 {
                                base - offset
                            } else {
                                offset - base
                            }
                        })
                        .collect(),
                    channels,
                )
                .unwrap()
            })
            .collect()
    }

    #[test]
    fn overlapped_and_serial_analysis_agree_bit_for_bit() {
        let channels = ChannelCount::new(2).unwrap();
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;
        let control = ExecutionControl::new(&cancellation, &progress);

        let serial = Analyzer::analyze_opened(
            opened_audio(channels, channels, varied_blocks(channels)),
            OverlapBudget::default(),
            0,
            &control,
            "overlap.fake",
        )
        .expect("the serial path must analyze the fixture");
        assert!(
            !LAST_ANALYSIS_OVERLAP.with(std::cell::Cell::get),
            "a budget without a spare permit must not start an analysis thread"
        );

        let overlapped = Analyzer::analyze_opened(
            opened_audio(channels, channels, varied_blocks(channels)),
            overlapping(),
            0,
            &control,
            "overlap.fake",
        )
        .expect("the overlapped path must analyze the same fixture");
        assert!(
            LAST_ANALYSIS_OVERLAP.with(std::cell::Cell::get),
            "this fixture must actually overlap, or the comparison proves nothing"
        );

        // Raw bits, not an approximate compare: overlap may not perturb a
        // single float, and the analysis is the whole product of this path.
        assert_eq!(
            format!("{:?}", serial.analysis()),
            format!("{:?}", overlapped.analysis())
        );
        assert_eq!(serial.analysis().frames_seen(), 64 * 128);
    }

    #[test]
    fn overlap_stays_serial_when_retention_exceeds_the_granted_budget() {
        let channels = ChannelCount::new(2).unwrap();
        let block = PcmBlock::new(vec![0.5; 64], channels).unwrap();
        let max_block_bytes = 256 * size_of::<f64>() as u64;
        // The default's own retention, not a literal: the point is that the
        // boundary is the price of whatever depth ships.
        let retained = OverlapShape::DEFAULT.retained_blocks(max_block_bytes);

        assert!(
            !OverlapBudget {
                spare_permits: 1,
                // One byte short of the worst case, even though the observed
                // first block is much smaller.
                max_in_flight_pcm_bytes: max_block_bytes * retained - 1,
                max_pcm_block_bytes: Some(max_block_bytes),
                ..OverlapBudget::default()
            }
            .admits(&block),
            "a stream that cannot prove its retention must stay serial"
        );
        assert!(
            OverlapBudget {
                spare_permits: 1,
                max_in_flight_pcm_bytes: max_block_bytes * retained,
                max_pcm_block_bytes: Some(max_block_bytes),
                ..OverlapBudget::default()
            }
            .admits(&block)
        );
        assert!(
            !OverlapBudget {
                spare_permits: 0,
                max_in_flight_pcm_bytes: u64::MAX,
                max_pcm_block_bytes: Some(max_block_bytes),
                ..OverlapBudget::default()
            }
            .admits(&block),
            "a route that spent every permit leaves nothing for an overlap thread"
        );
        assert!(
            !OverlapBudget {
                spare_permits: 1,
                max_in_flight_pcm_bytes: u64::MAX,
                ..OverlapBudget::default()
            }
            .admits(&block),
            "a route without a probe-time block bound must stay serial"
        );
    }

    #[test]
    fn a_max_channel_stream_derives_a_depth_that_fits_instead_of_losing_the_overlap() {
        let allocation = ConcurrencyPlan::bounded_for_test(nonzero(8), nonzero(8))
            .allocate(nonzero(3))
            .unwrap();
        let reservation = allocation.decode();
        assert_eq!(allocation.file_lanes().get(), 3);
        assert_eq!(reservation.workers().get(), 2);
        assert_eq!(reservation.max_in_flight_pcm_bytes(), 8 * 1024 * 1024);

        // The source-bound WAV/AIFF performance track proves at most 1,152
        // frames per block. At the stable 64-channel ceiling one block is
        // 576 KiB, so a fixed depth of sixteen would ask for 17 of them and be
        // refused, costing this stream the overlap entirely. The derivation
        // spends the residency budget instead and lands on the depth-one
        // hand-off, which is what shipped before depth existed.
        let channels = ChannelCount::new(64).unwrap();
        let block = PcmBlock::new(vec![0.0; usize::from(channels.get())], channels).unwrap();
        let max_block_bytes = 1_152_u64
            .saturating_mul(u64::from(channels.get()))
            .saturating_mul(size_of::<f64>() as u64);
        let available = reservation.max_in_flight_pcm_bytes();
        assert_eq!(max_block_bytes, 589_824);
        assert_eq!(OverlapShape::DEFAULT.channel_depth(max_block_bytes), 1);
        assert_eq!(
            max_block_bytes * OverlapShape::DEFAULT.retained_blocks(max_block_bytes),
            1_179_648
        );

        let wide_geometry = |shape| OverlapBudget {
            spare_permits: reservation.workers().get() - 1,
            max_in_flight_pcm_bytes: available,
            max_pcm_block_bytes: Some(max_block_bytes),
            shape,
            ..OverlapBudget::default()
        };
        assert!(
            wide_geometry(OverlapShape::DEFAULT).admits(&block),
            "the derived depth must keep the overlap this geometry can afford"
        );
        assert!(
            !wide_geometry(OverlapShape::new(nonzero(16))).admits(&block),
            "a fixed depth of sixteen is what this geometry cannot afford"
        );
    }

    #[test]
    fn the_derived_depth_falls_with_the_block_size_and_never_below_one() {
        // Channel count is what moves a block's size in practice, and the
        // product supports up to `MAX_ANALYSIS_CHANNELS`. The derivation was
        // measured at two and six; every wider geometry has to degrade toward
        // the depth-one hand-off rather than off a cliff, so this pins the whole
        // supported range rather than the two that were swept.
        let mut previous = usize::MAX;
        for (channels, expected) in [(2_u64, 16_usize), (6, 4), (8, 3), (16, 1), (64, 1)] {
            let block_bytes = 1_152 * channels * size_of::<f64>() as u64;
            let depth = OverlapShape::DEFAULT.channel_depth(block_bytes);
            assert_eq!(depth, expected, "{channels} channels");
            assert!(depth <= previous, "the derivation must not rise with width");
            assert!(
                depth >= 1,
                "{channels} channels fell below the shipped floor"
            );
            assert!(
                OverlapShape::DEFAULT.retained_blocks(block_bytes) * block_bytes
                    <= OVERLAP_RETAINED_BYTES.max(2 * block_bytes),
                "{channels} channels exceeded the residency budget without hitting the floor"
            );
            previous = depth;
        }
    }

    /// The measurement depths, plus the default they have to reduce to.
    fn shapes() -> Vec<OverlapShape> {
        [1, 2, 3, 8, 16, 64]
            .into_iter()
            .map(|depth| OverlapShape::new(nonzero(depth)))
            .collect()
    }

    fn nonzero(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).unwrap()
    }

    /// The worst-case block these fixtures declare, in bytes.
    const TEST_MAX_BLOCK_BYTES: u64 = 256 * size_of::<f64>() as u64;

    fn shaped(shape: OverlapShape) -> OverlapBudget {
        OverlapBudget {
            shape,
            // Wide enough that no depth in the grid is refused for retention;
            // the pricing itself is asserted separately below.
            max_in_flight_pcm_bytes: u64::MAX,
            ..overlapping()
        }
    }

    #[test]
    fn a_deeper_hand_off_costs_exactly_the_blocks_it_retains() {
        // The channel holds `depth` blocks and the analyst holds the one it is
        // pushing. Depth is bought out of the allowance the route already
        // holds, so the price is asserted at the byte, not asserted to be
        // "small".
        for (depth, blocks) in [(1, 2_u64), (2, 3), (8, 9), (16, 17), (64, 65)] {
            let shape = OverlapShape::new(nonzero(depth));
            assert_eq!(shape.retained_blocks(0), blocks, "depth {depth}");

            let channels = ChannelCount::new(2).unwrap();
            let block = PcmBlock::new(vec![0.5; 64], channels).unwrap();
            let max_block_bytes = 256 * size_of::<f64>() as u64;
            assert!(
                !OverlapBudget {
                    shape,
                    max_in_flight_pcm_bytes: max_block_bytes * blocks - 1,
                    ..overlapping()
                }
                .admits(&block),
                "depth {depth} must stay serial one byte short"
            );
            assert!(
                OverlapBudget {
                    shape,
                    max_in_flight_pcm_bytes: max_block_bytes * blocks,
                    ..overlapping()
                }
                .admits(&block),
                "depth {depth} fits exactly at its own price"
            );
        }
    }

    #[test]
    fn every_hand_off_depth_produces_the_same_analysis_bit_for_bit() {
        let channels = ChannelCount::new(2).unwrap();
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;
        let control = ExecutionControl::new(&cancellation, &progress);

        let serial = Analyzer::analyze_opened(
            opened_audio(channels, channels, varied_blocks(channels)),
            OverlapBudget::default(),
            0,
            &control,
            "shape.fake",
        )
        .expect("the serial path must analyze the fixture");

        for shape in shapes() {
            let shaped_result = Analyzer::analyze_opened(
                opened_audio(channels, channels, varied_blocks(channels)),
                shaped(shape),
                0,
                &control,
                "shape.fake",
            )
            .expect("every shape must analyze the same fixture");
            assert!(
                LAST_ANALYSIS_OVERLAP.with(std::cell::Cell::get),
                "{shape:?} must actually overlap, or it proves nothing"
            );
            // Every depth still transfers exactly one block per message, so
            // the final block and terminator preserve the serial boundary.
            assert_eq!(
                format!("{:?}", serial.analysis()),
                format!("{:?}", shaped_result.analysis()),
                "{shape:?} changed the analysis"
            );
        }
    }

    #[test]
    fn a_deeper_hand_off_delivers_every_block_in_read_order() {
        let channels = ChannelCount::new(2).unwrap();
        for shape in shapes() {
            let seen = Arc::new(Mutex::new(Vec::new()));
            let recorder = Arc::clone(&seen);
            let schedule = OverlapSchedule::new(move |_, block: &PcmBlock| {
                // The block's own first sample, not a counter the analyst
                // increments: a recorder that counted itself would agree with
                // any order at all.
                recorder.lock().unwrap().push(block.samples()[0].to_bits());
            });
            let blocks = indexed_blocks(37, channels);
            let expected: Vec<u64> = blocks
                .iter()
                .skip(1)
                .map(|block| block.samples()[0].to_bits())
                .collect();
            let cancellation = CancellationToken::new();
            let progress = NoopProgressSink;

            Analyzer::analyze_opened(
                opened_audio(channels, channels, blocks),
                OverlapBudget {
                    shape,
                    max_in_flight_pcm_bytes: u64::MAX,
                    ..overlapping_with(schedule)
                },
                0,
                &ExecutionControl::new(&cancellation, &progress),
                "order.fake",
            )
            .expect("the shaped hand-off must analyze the fixture");

            assert_eq!(
                *seen.lock().unwrap(),
                expected,
                "{shape:?} did not preserve read order"
            );
        }
    }

    #[test]
    fn an_analysis_failure_still_outranks_a_later_decode_failure_at_every_depth() {
        // Depth decides how many decoded blocks sit between the producer and
        // the analyst when something ends the stream. Those blocks precede the
        // ending in input order, so a deeper channel must not let a later
        // decode failure decide a stream an earlier analysis failure owns.
        let channels = ChannelCount::new(2).unwrap();
        let mono = ChannelCount::new(1).unwrap();
        for shape in shapes() {
            for lead in [1_usize, 12, 28] {
                let mut blocks = indexed_blocks(32, channels);
                blocks[lead] = PcmBlock::new(vec![f64::MAX, f64::MAX], channels).unwrap();
                blocks[lead + 1] = PcmBlock::new(vec![0.25], mono).unwrap();
                let cancellation = CancellationToken::new();
                let progress = NoopProgressSink;

                let error = Analyzer::analyze_opened(
                    opened_audio(channels, channels, blocks),
                    shaped(shape),
                    0,
                    &ExecutionControl::new(&cancellation, &progress),
                    "precedence.fake",
                )
                .expect_err("the earliest failure in input order must decide the stream");

                assert_eq!(
                    error.stage,
                    AnalysisStage::Analysis,
                    "{shape:?}: analysis failed at block {lead}, decode at {}",
                    lead + 1
                );
            }
        }
    }

    #[test]
    fn an_overlapped_analysis_failure_outranks_the_later_decode_failure() {
        let channels = ChannelCount::new(2).unwrap();
        let mut blocks = vec![PcmBlock::new(vec![0.25, 0.25], channels).unwrap()];
        // Block 1 fails in the analyzer; block 2 fails in decode. Serial order
        // reaches the analysis failure first, so overlap must report it even
        // though the decoding thread runs ahead.
        blocks.push(PcmBlock::new(vec![f64::MAX, f64::MAX], channels).unwrap());
        blocks.push(PcmBlock::new(vec![0.25], ChannelCount::new(1).unwrap()).unwrap());
        let opened = opened_audio(channels, channels, blocks);
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;

        let error = Analyzer::analyze_opened(
            opened,
            overlapping(),
            0,
            &ExecutionControl::new(&cancellation, &progress),
            "precedence.fake",
        )
        .expect_err("the earliest failure in input order must decide the stream");

        assert_eq!(
            error.stage,
            AnalysisStage::Analysis,
            "a later decode failure must not outrank the earlier analysis failure"
        );
    }

    #[test]
    fn a_decode_failure_does_not_finalize_or_mask_itself_with_a_partial_prefix() {
        let channels = ChannelCount::new(1).unwrap();
        let terminal_error = AnalysisError::new(
            ErrorCode::DecodeFailed,
            AnalysisStage::Decode,
            "injected later decode failure",
        );
        let block = || PcmBlock::new(vec![1.0e100], channels).unwrap();

        // The accepted prefix fails only at finish when its report values are
        // narrowed. A serial decode failure arrives before finish and must
        // therefore remain the result of the overlapped path too.
        let mut prefix = AnalyzerSession::new(
            StreamSpec::new(48_000, channels.get(), ChannelLayout::Unknown).unwrap(),
        )
        .unwrap();
        prefix.push_interleaved(block().samples()).unwrap();
        assert_eq!(prefix.finish().unwrap_err().stage, AnalysisStage::Analysis);

        let serial_error = Analyzer::analyze_opened(
            opened_audio_with_terminal_error(
                channels,
                channels,
                vec![block()],
                terminal_error.clone(),
            ),
            OverlapBudget::default(),
            0,
            &ExecutionControl::new(&CancellationToken::new(), &NoopProgressSink),
            "decode-error.fake",
        )
        .expect_err("the serial oracle must report the decoder failure");
        let overlapped_error = Analyzer::analyze_opened(
            opened_audio_with_terminal_error(channels, channels, vec![block()], terminal_error),
            overlapping(),
            0,
            &ExecutionControl::new(&CancellationToken::new(), &NoopProgressSink),
            "decode-error.fake",
        )
        .expect_err("disconnect must not finalize a partial analysis prefix");

        assert_eq!(overlapped_error.code, ErrorCode::DecodeFailed);
        assert_eq!(overlapped_error.stage, AnalysisStage::Decode);
        assert_eq!(overlapped_error.message, serial_error.message);
    }

    #[test]
    fn analysis_thread_spawn_failure_is_structured() {
        let channels = ChannelCount::new(1).unwrap();
        let mut budget = overlapping();
        budget.inject_spawn_failure = true;
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;

        let error = Analyzer::analyze_opened(
            opened_audio(
                channels,
                channels,
                vec![PcmBlock::new(vec![0.25], channels).unwrap()],
            ),
            budget,
            0,
            &ExecutionControl::new(&cancellation, &progress),
            "spawn.fake",
        )
        .expect_err("thread construction failure must not unwind the application");

        assert_eq!(error.code, ErrorCode::ResourceExhausted);
        assert_eq!(error.stage, AnalysisStage::Analysis);
        assert_eq!(error.display_path.as_deref(), Some("spawn.fake"));
        assert!(error.recoverable);
        assert!(
            !LAST_ANALYSIS_OVERLAP.with(std::cell::Cell::get),
            "a failed spawn must not claim that overlap actually ran"
        );
        assert!(
            error
                .details
                .as_deref()
                .is_some_and(|details| details.contains("injected"))
        );
    }

    #[test]
    fn a_cancelled_overlap_joins_its_analysis_thread_and_reports_no_report() {
        let channels = ChannelCount::new(2).unwrap();
        let opened = opened_audio(channels, channels, varied_blocks(channels));
        let cancellation = CancellationToken::new();
        let token_for_progress = cancellation.clone();
        let progress_events = Arc::new(TestCounter::new(0));
        let events_for_progress = Arc::clone(&progress_events);
        let progress = move |event| {
            if matches!(event, AnalysisEvent::DecodeProgress { .. })
                && events_for_progress.fetch_add(1, Ordering::Relaxed) == 1
            {
                token_for_progress.cancel();
            }
        };

        let error = Analyzer::analyze_opened(
            opened,
            overlapping(),
            0,
            &ExecutionControl::new(&cancellation, &progress),
            "cancel.fake",
        )
        .expect_err("a cancelled overlap must not produce a partial report");

        assert_eq!(error.code, ErrorCode::Cancelled);
        assert!(
            progress_events.load(Ordering::Relaxed) >= 2,
            "cancellation must happen after the overlap thread starts"
        );
        assert!(
            LAST_ANALYSIS_OVERLAP.with(std::cell::Cell::get),
            "cancellation coverage must actually start the overlap path"
        );
    }

    #[test]
    fn cancellation_at_zero_frame_eof_prevents_a_report() {
        let channels = ChannelCount::new(1).unwrap();
        let cancellation = CancellationToken::new();
        let token_for_progress = cancellation.clone();
        let progress = move |event| {
            if matches!(
                event,
                AnalysisEvent::DecodeProgress { progress, .. } if progress.is_eof()
            ) {
                token_for_progress.cancel();
            }
        };

        let error = Analyzer::analyze_opened(
            opened_audio(channels, channels, Vec::new()),
            overlapping(),
            0,
            &ExecutionControl::new(&cancellation, &progress),
            "empty.fake",
        )
        .expect_err("EOF cancellation must win before an empty report is finalized");

        assert_eq!(error.code, ErrorCode::Cancelled);
    }

    /// Blocks whose first sample is the block index, so a recorded push order
    /// can be checked against the read order rather than inferred from a total.
    fn indexed_blocks(count: usize, channels: ChannelCount) -> Vec<PcmBlock> {
        (0..count)
            .map(|index| {
                PcmBlock::new(
                    (0..8)
                        .map(|slot| (index * 8 + slot) as f64 / 4096.0)
                        .collect(),
                    channels,
                )
                .unwrap()
            })
            .collect()
    }

    #[test]
    fn the_analyzer_receives_every_block_in_read_order() {
        let channels = ChannelCount::new(2).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;

        let report = Analyzer::analyze_opened(
            opened_audio(channels, channels, indexed_blocks(32, channels)),
            overlapping_with(OverlapSchedule::new(move |_position, block| {
                recorder.lock().unwrap().push(block.samples()[0].to_bits());
            })),
            0,
            &ExecutionControl::new(&cancellation, &progress),
            "order.fake",
        )
        .expect("an ordered hand-off must analyze the whole stream");

        assert!(LAST_ANALYSIS_OVERLAP.with(std::cell::Cell::get));
        // Block 0 is pushed on the calling thread, so the analysis thread sees
        // the identities encoded in blocks 1..32, strictly ascending and with
        // nothing repeated. The expectation comes from the block data rather
        // than a receiver-owned counter.
        let expected = (1..32)
            .map(|index| ((index * 8) as f64 / 4096.0).to_bits())
            .collect::<Vec<_>>();
        assert_eq!(*seen.lock().unwrap(), expected);
        assert_eq!(report.analysis().frames_seen(), 32 * 4);
    }

    #[test]
    fn a_forced_full_handoff_changes_nothing_about_the_result() {
        let channels = ChannelCount::new(2).unwrap();
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;
        let control = ExecutionControl::new(&cancellation, &progress);

        let prompt = Analyzer::analyze_opened(
            opened_audio(channels, channels, varied_blocks(channels)),
            overlapping(),
            0,
            &control,
            "lag.fake",
        )
        .expect("the unhindered overlap must analyze the fixture");

        // Hold the analysis thread on block 1. The following blocks then fill
        // the channel, and the test-only send seam releases the analyst only
        // after the real SyncSender reports Full. That happens at block
        // `depth + 2`: block 1 is in the analyst's hands and blocks 2..=depth+1
        // occupy the channel. Deriving the index from the default rather than
        // writing a number keeps this observing the queue directly, whatever
        // depth the product ships.
        // The fixture's worst-case block is small, so the derivation runs at its
        // ceiling here; taking it from the shape keeps this observing the queue.
        let first_full = OverlapShape::DEFAULT.channel_depth(TEST_MAX_BLOCK_BYTES) + 2;
        let (release, release_receiver) = std::sync::mpsc::channel::<()>();
        let release_receiver = Mutex::new(release_receiver);
        let (analyst_held, wait_until_held) = std::sync::mpsc::channel::<()>();
        let wait_until_held = Mutex::new(wait_until_held);
        let full_sends = Arc::new(Mutex::new(Vec::new()));
        let observed_full_sends = Arc::clone(&full_sends);
        let release = Mutex::new(Some(release));
        let schedule = OverlapSchedule::new(move |index, _block| {
            if index == 1 {
                let _ = analyst_held.send(());
                release_receiver
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(30))
                    .expect("the hand-off must become full");
            }
        })
        .with_after_send_hook(move |index| {
            if index == 1 {
                wait_until_held
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(30))
                    .expect("the analyst must hold block 1 before decode continues");
            }
        })
        .with_full_send_hook(move |index| {
            observed_full_sends.lock().unwrap().push(index);
            if let Some(release) = release.lock().unwrap().take() {
                let _ = release.send(());
            }
        });
        let lagged = Analyzer::analyze_opened(
            opened_audio(channels, channels, varied_blocks(channels)),
            overlapping_with(schedule),
            0,
            &control,
            "lag.fake",
        )
        .expect("a lagging analysis thread must still analyze the fixture");

        assert_eq!(full_sends.lock().unwrap().first(), Some(&first_full));
        assert_eq!(
            format!("{:?}", prompt.analysis()),
            format!("{:?}", lagged.analysis()),
            "the schedule of the two threads may not reach the result"
        );
    }

    #[test]
    fn an_analyst_panic_is_structured_at_the_first_middle_and_last_block() {
        let channels = ChannelCount::new(2).unwrap();
        // Block 1 is the analysis thread's first, 16 its middle and 31 its last.
        for at in [1_usize, 16, 31] {
            let cancellation = CancellationToken::new();
            let progress = NoopProgressSink;
            let error = Analyzer::analyze_opened(
                opened_audio(channels, channels, indexed_blocks(32, channels)),
                overlapping_with(OverlapSchedule::new(move |index, _block| {
                    assert!(index != at, "injected analysis panic at block {index}");
                })),
                0,
                &ExecutionControl::new(&cancellation, &progress),
                "panic.fake",
            )
            .expect_err("an analysis thread panic must not escape the facade");

            assert_eq!(error.code, ErrorCode::Internal, "panic at block {at}");
            assert_eq!(error.stage, AnalysisStage::Analysis, "panic at block {at}");
            assert_eq!(error.message, "the overlapped analysis thread panicked");
        }
    }

    #[test]
    fn cancellation_raised_mid_stream_joins_and_reports_no_partial_report() {
        let channels = ChannelCount::new(2).unwrap();
        // Cancel from the progress observer partway through, which is where a
        // real adapter cancels: on the decoding thread, between blocks.
        for after_events in [1_usize, 8, 24] {
            let cancellation = CancellationToken::new();
            let seen = TestCounter::new(0);
            let token = cancellation.clone();
            let progress = |_event: AnalysisEvent| {
                if seen.fetch_add(1, Ordering::SeqCst) == after_events {
                    token.cancel();
                }
            };
            let error = Analyzer::analyze_opened(
                opened_audio(channels, channels, indexed_blocks(32, channels)),
                overlapping(),
                0,
                &ExecutionControl::new(&cancellation, &progress),
                "cancel-mid.fake",
            )
            .expect_err("a cancelled overlap may not produce a report");

            assert_eq!(
                error.code,
                ErrorCode::Cancelled,
                "cancelled after {after_events} events"
            );
        }
    }

    #[test]
    fn an_analysis_failure_outranks_a_later_decode_failure_at_every_position() {
        let channels = ChannelCount::new(2).unwrap();
        let mono = ChannelCount::new(1).unwrap();
        // The analyzer rejects the non-finite block; decode rejects the mono one
        // that follows it. Serial order reaches the analysis failure first, so
        // overlap must report it wherever the pair sits in the stream.
        for lead in [1_usize, 12, 28] {
            let mut blocks = indexed_blocks(32, channels);
            blocks[lead] = PcmBlock::new(vec![f64::MAX, f64::MAX], channels).unwrap();
            blocks[lead + 1] = PcmBlock::new(vec![0.25], mono).unwrap();
            let cancellation = CancellationToken::new();
            let progress = NoopProgressSink;

            let error = Analyzer::analyze_opened(
                opened_audio(channels, channels, blocks),
                overlapping(),
                0,
                &ExecutionControl::new(&cancellation, &progress),
                "precedence.fake",
            )
            .expect_err("the earliest failure in input order must decide the stream");

            assert_eq!(
                error.stage,
                AnalysisStage::Analysis,
                "analysis failed at block {lead}, decode at {}",
                lead + 1
            );
        }
    }

    #[test]
    fn a_decode_failure_is_reported_at_the_first_middle_and_last_block() {
        let channels = ChannelCount::new(2).unwrap();
        let mono = ChannelCount::new(1).unwrap();
        for at in [1_usize, 16, 31] {
            let mut blocks = indexed_blocks(32, channels);
            blocks[at] = PcmBlock::new(vec![0.25], mono).unwrap();
            let cancellation = CancellationToken::new();
            let progress = NoopProgressSink;

            let error = Analyzer::analyze_opened(
                opened_audio(channels, channels, blocks),
                overlapping(),
                0,
                &ExecutionControl::new(&cancellation, &progress),
                "decode-fail.fake",
            )
            .expect_err("a decode failure must never be finalized as a report");

            assert_eq!(error.code, ErrorCode::DecodeFailed, "decode failed at {at}");
            assert_eq!(error.stage, AnalysisStage::Decode, "decode failed at {at}");
        }
    }

    #[test]
    fn a_multi_channel_short_tail_stream_matches_the_serial_path() {
        // Six channels with a final block shorter than the rest: the geometry
        // most likely to expose a hand-off that assumed uniform blocks.
        let channels = ChannelCount::new(6).unwrap();
        let mut blocks: Vec<PcmBlock> = (0..48)
            .map(|index| {
                PcmBlock::new(
                    (0..(6 * 32))
                        .map(|slot| ((index * 7 + slot) % 601) as f64 / 1201.0)
                        .collect(),
                    channels,
                )
                .unwrap()
            })
            .collect();
        blocks.push(PcmBlock::new(vec![0.125; 6], channels).unwrap());
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;
        let control = ExecutionControl::new(&cancellation, &progress);

        let serial = Analyzer::analyze_opened(
            opened_audio(channels, channels, blocks.clone()),
            OverlapBudget::default(),
            0,
            &control,
            "tail.fake",
        )
        .expect("the serial path must analyze the short-tail fixture");
        assert!(!LAST_ANALYSIS_OVERLAP.with(std::cell::Cell::get));

        let overlapped = Analyzer::analyze_opened(
            opened_audio(channels, channels, blocks),
            OverlapBudget {
                max_pcm_block_bytes: Some(6 * 32 * size_of::<f64>() as u64),
                ..overlapping()
            },
            0,
            &control,
            "tail.fake",
        )
        .expect("the overlapped path must analyze the same fixture");
        assert!(LAST_ANALYSIS_OVERLAP.with(std::cell::Cell::get));

        assert_eq!(
            format!("{:?}", serial.analysis()),
            format!("{:?}", overlapped.analysis())
        );
        assert_eq!(overlapped.analysis().frames_seen(), 48 * 32 + 1);
    }
}
