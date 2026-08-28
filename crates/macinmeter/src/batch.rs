#[cfg(feature = "performance-probes")]
use crate::concurrency::PlanAllocation;
use crate::{
    AnalysisError, AnalysisEvent, AnalysisReport, AnalysisStage, AnalyzeRequest, CancellationToken,
    ErrorCode, ExecutionControl,
    application::{Analyzer, OverlapShape, PhaseTimings, SharedPhaseTimings},
    concurrency::ConcurrencyPlan,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    io,
    num::NonZeroUsize,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::channel,
    },
    thread,
};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchRequest {
    pub inputs: Vec<PathBuf>,
    pub recursive: bool,
}

impl BatchRequest {
    pub fn new(inputs: Vec<PathBuf>, recursive: bool) -> Self {
        Self { inputs, recursive }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    Succeeded,
    PartiallySucceeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BatchItemOutcome {
    Success { report: Box<AnalysisReport> },
    Failure { error: AnalysisError },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchItem {
    pub display_path: String,
    pub outcome: BatchItemOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSummary {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchReport {
    pub status: BatchStatus,
    pub items: Vec<BatchItem>,
    pub summary: BatchSummary,
}

/// One completed item delivered by a chunked batch execution.
///
/// File lanes may finish out of order, so `index` is the item's stable
/// discovery-order identity. Consumers must use it rather than treating chunk
/// arrival order as report order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchChunkItem {
    pub index: usize,
    pub item: BatchItem,
}

/// A bounded group of completed batch items.
///
/// Chunks are runtime delivery values, not wire envelopes. Every chunk is
/// non-empty and contains at most [`BatchChunk::MAX_ITEMS`] items. Calls into a
/// sink may come from different file lanes and chunks may therefore arrive out
/// of discovery order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchChunk {
    pub items: Vec<BatchChunkItem>,
}

impl BatchChunk {
    /// The fixed maximum number of reports retained by the core chunk buffer.
    pub const MAX_ITEMS: usize = 8;
}

/// Terminal metadata returned after every accepted chunk has been delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchCompletion {
    pub status: BatchStatus,
    pub summary: BatchSummary,
}

/// Receives owned result chunks from a batch execution.
///
/// `emit` is synchronous and may be called concurrently by the bounded file
/// lanes. A slow sink therefore applies backpressure instead of allowing an
/// unbounded application-owned result queue. Returning an error stops the
/// whole batch after all in-flight lanes have joined.
pub trait BatchChunkSink: Send + Sync {
    fn emit(&self, chunk: BatchChunk) -> Result<(), AnalysisError>;
}

impl<F> BatchChunkSink for F
where
    F: Fn(BatchChunk) -> Result<(), AnalysisError> + Send + Sync,
{
    fn emit(&self, chunk: BatchChunk) -> Result<(), AnalysisError> {
        self(chunk)
    }
}

#[derive(Debug, Default)]
pub struct NoopBatchChunkSink;

impl BatchChunkSink for NoopBatchChunkSink {
    fn emit(&self, _chunk: BatchChunk) -> Result<(), AnalysisError> {
        Ok(())
    }
}

/// Explicit adapter-side collector for formats that require one complete
/// ordered document, such as the stable CLI JSON envelope.
///
/// Batch execution itself never constructs a [`BatchReport`]. Using this type
/// is an adapter choice and intentionally restores O(items) result retention
/// only for that caller.
#[derive(Debug, Default)]
pub struct BatchReportCollector {
    items: Mutex<Vec<BatchChunkItem>>,
}

impl BatchReportCollector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn finish(self, completion: BatchCompletion) -> Result<BatchReport, AnalysisError> {
        let mut indexed = self
            .items
            .into_inner()
            .map_err(|_| batch_collector_error("batch report collector is poisoned"))?;
        indexed.sort_by_key(|entry| entry.index);
        if indexed.len() != completion.summary.total {
            return Err(batch_collector_error(format!(
                "batch report collector received {} of {} items",
                indexed.len(),
                completion.summary.total
            )));
        }

        let mut items = Vec::with_capacity(indexed.len());
        for (expected, entry) in indexed.into_iter().enumerate() {
            if entry.index != expected {
                return Err(batch_collector_error(format!(
                    "batch report collector expected index {expected}, found {}",
                    entry.index
                )));
            }
            items.push(entry.item);
        }
        Ok(BatchReport {
            status: completion.status,
            items,
            summary: completion.summary,
        })
    }
}

impl BatchChunkSink for BatchReportCollector {
    fn emit(&self, chunk: BatchChunk) -> Result<(), AnalysisError> {
        self.items
            .lock()
            .map_err(|_| batch_collector_error("batch report collector is poisoned"))?
            .extend(chunk.items);
        Ok(())
    }
}

/// The exact application allocation selected by a non-default batch performance run.
///
/// This type is deliberately absent from ordinary builds and from every batch
/// report/wire field. The ADR-0007 worker uses it to prove that a case labelled
/// with a file-lane width actually received that plan and split.
#[cfg(feature = "performance-probes")]
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchPerformanceProbe {
    granted_plan_workers: usize,
    allocated_file_lanes: usize,
    decoder_workers_per_lane: usize,
}

#[cfg(feature = "performance-probes")]
impl BatchPerformanceProbe {
    const fn new(plan: ConcurrencyPlan, allocation: PlanAllocation) -> Self {
        Self {
            granted_plan_workers: plan.total_workers().get(),
            allocated_file_lanes: allocation.file_lanes().get(),
            decoder_workers_per_lane: allocation.decode().workers().get(),
        }
    }

    pub const fn granted_plan_workers(self) -> usize {
        self.granted_plan_workers
    }

    pub const fn allocated_file_lanes(self) -> usize {
        self.allocated_file_lanes
    }

    pub const fn decoder_workers_per_lane(self) -> usize {
        self.decoder_workers_per_lane
    }
}

struct BatchExecution {
    completion: BatchCompletion,
    timings: PhaseTimings,
    #[cfg(feature = "performance-probes")]
    allocation: PlanAllocation,
}

#[derive(Debug)]
pub(crate) struct BatchRunner {
    plan: ConcurrencyPlan,
    requested_file_lanes: NonZeroUsize,
    overlap_shape: OverlapShape,
    collect_timings: bool,
    #[cfg(test)]
    fault: Option<LaneFault>,
}

impl Default for BatchRunner {
    fn default() -> Self {
        Self {
            plan: ConcurrencyPlan::serial(),
            requested_file_lanes: NonZeroUsize::MIN,
            overlap_shape: OverlapShape::DEFAULT,
            collect_timings: false,
            #[cfg(test)]
            fault: None,
        }
    }
}

impl BatchRunner {
    /// Build a runner over the job's own plan.
    ///
    /// The runner takes the plan rather than an already-split allocation
    /// because the split cannot be correct before discovery: lane executors and
    /// per-lane decode permits are two halves of one division, so choosing
    /// lanes for a batch that turns out to hold one file would narrow that
    /// file's decoder for no reason. Splitting is still done exactly once, and
    /// still by the plan; it just happens once the item count is known.
    /// Every lane shares the one hand-off shape, since they share one plan.
    pub(crate) const fn with_overlap_shape(
        plan: ConcurrencyPlan,
        requested_file_lanes: NonZeroUsize,
        overlap_shape: OverlapShape,
    ) -> Self {
        Self {
            plan,
            requested_file_lanes,
            overlap_shape,
            collect_timings: false,
            #[cfg(test)]
            fault: None,
        }
    }

    /// Turn on the per-block clocks in every lane's analyzer.
    pub(crate) const fn collecting_timings(mut self) -> Self {
        self.collect_timings = true;
        self
    }

    #[cfg(test)]
    fn with_fault(mut self, fault: LaneFault) -> Self {
        self.fault = Some(fault);
        self
    }

    pub(crate) fn run(
        &self,
        request: BatchRequest,
        chunks: &dyn BatchChunkSink,
        control: &ExecutionControl<'_>,
    ) -> Result<BatchCompletion, AnalysisError> {
        self.run_execution(request, chunks, control)
            .map(|execution| execution.completion)
    }

    pub(crate) fn run_timed(
        &self,
        request: BatchRequest,
        chunks: &dyn BatchChunkSink,
        control: &ExecutionControl<'_>,
    ) -> Result<(BatchCompletion, PhaseTimings), AnalysisError> {
        self.run_execution(request, chunks, control)
            .map(|execution| (execution.completion, execution.timings))
    }

    #[cfg(feature = "performance-probes")]
    pub(crate) fn run_with_performance_probe(
        &self,
        request: BatchRequest,
        chunks: &dyn BatchChunkSink,
        control: &ExecutionControl<'_>,
    ) -> Result<(BatchCompletion, BatchPerformanceProbe), AnalysisError> {
        let execution = self.run_execution(request, chunks, control)?;
        let probe = BatchPerformanceProbe::new(self.plan, execution.allocation);
        Ok((execution.completion, probe))
    }

    fn run_execution(
        &self,
        request: BatchRequest,
        chunks: &dyn BatchChunkSink,
        control: &ExecutionControl<'_>,
    ) -> Result<BatchExecution, AnalysisError> {
        if control.cancellation.is_cancelled() {
            return Err(AnalysisError::cancelled());
        }
        let files = discover_inputs_with_control(&request.inputs, request.recursive, control)?;

        // Split the plan now that the item count is known. Asking for more
        // lanes than there are files would spend permits on executors that
        // could never claim an item, and would narrow every decoder to pay for
        // them. This is still the one split the job performs: `allocate` is
        // arithmetic over a total this job already holds exclusively, it cannot
        // block and cannot fail for want of a resource, so no worker below ever
        // waits for a second grant.
        let lanes = NonZeroUsize::new(self.requested_file_lanes.get().min(files.len().max(1)))
            .unwrap_or(NonZeroUsize::MIN);
        let allocation = self.plan.allocate(lanes)?;
        let analyzer = Analyzer::with_overlap_shape(allocation.decode(), self.overlap_shape);
        let analyzer = if self.collect_timings {
            analyzer.collecting_timings()
        } else {
            analyzer
        };
        let timings = self.collect_timings.then(SharedPhaseTimings::default);

        // Discovery already fixed the input order, so an item's index names one
        // file no matter which lane produces it. Lanes claim by index rather
        // than by a fixed stride: a static split would decide the tail from the
        // input order, and a batch's item costs are not known before decoding.
        let claim = AtomicUsize::new(0);
        let stop = AtomicBool::new(false);
        let failure = Mutex::new(None);
        let output = BatchChunkEmitter::new(chunks);
        let lanes = allocation.file_lanes().get();
        let shared = LaneShared {
            claim: &claim,
            stop: &stop,
            failure: &failure,
            output: &output,
            timings: timings.as_ref(),
        };

        thread::scope(|scope| -> Result<(), AnalysisError> {
            let mut starts = Vec::with_capacity(lanes.saturating_sub(1));
            let mut handles = Vec::with_capacity(lanes.saturating_sub(1));
            let mut spawn_error = None;

            // Keep every successfully created lane behind a start gate until
            // construction is complete. If a later spawn fails, dropping the
            // gates wakes and joins the earlier lanes before any item starts.
            for lane in 1..lanes {
                let (start, start_gate) = channel::<()>();
                let runner = self;
                let lane_analyzer = &analyzer;
                let lane_files = &files;
                let lane_shared = &shared;
                let operation = move || {
                    if start_gate.recv().is_err() {
                        return false;
                    }
                    runner.run_lane_catching(lane_analyzer, lane_files, lane_shared, control)
                };
                let builder = thread::Builder::new().name(format!("macinmeter-file-lane-{lane}"));
                #[cfg(test)]
                let spawned = if self.fault == Some(LaneFault::Spawn(lane)) {
                    Err(io::Error::other("injected batch file-lane spawn failure"))
                } else {
                    builder.spawn_scoped(scope, operation)
                };
                #[cfg(not(test))]
                let spawned = builder.spawn_scoped(scope, operation);

                match spawned {
                    Ok(handle) => {
                        starts.push(start);
                        handles.push(handle);
                    }
                    Err(error) => {
                        stop.store(true, Ordering::Release);
                        spawn_error = Some(error);
                        break;
                    }
                }
            }

            let mut lane_panicked = false;
            if spawn_error.is_some() {
                drop(starts);
            } else {
                for start in starts {
                    if start.send(()).is_err() {
                        stop.store(true, Ordering::Release);
                        lane_panicked = true;
                    }
                }
            }

            // The calling thread is one of the lanes, so a single-lane batch
            // creates no lane thread. Catching its unwind keeps the same
            // structured failure contract as the spawned lanes.
            if spawn_error.is_none()
                && !lane_panicked
                && self.run_lane_catching(&analyzer, &files, &shared, control)
            {
                lane_panicked = true;
            }

            // Join manually: dropping a panicked scoped handle would make
            // `thread::scope` resume the panic instead of returning an
            // `AnalysisError` through the application facade.
            for handle in handles {
                match handle.join() {
                    Ok(panicked) => lane_panicked |= panicked,
                    Err(_) => {
                        stop.store(true, Ordering::Release);
                        lane_panicked = true;
                    }
                }
            }

            if let Some(error) = spawn_error {
                return Err(lane_spawn_error(error));
            }
            if lane_panicked {
                return Err(lane_panic_error());
            }
            Ok(())
        })?;

        // Cancellation is decided after every lane has joined. Reporting it
        // earlier would return while in-flight items were still decoding.
        if control.cancellation.is_cancelled() {
            return Err(AnalysisError::cancelled());
        }
        if let Some(error) = failure
            .into_inner()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
        {
            return Err(error);
        }

        let completion = output.finish(files.len())?;
        let BatchSummary {
            succeeded, failed, ..
        } = completion.summary;
        control
            .progress
            .emit(AnalysisEvent::BatchFinished { succeeded, failed });
        Ok(BatchExecution {
            completion,
            timings: timings
                .as_ref()
                .map_or_else(PhaseTimings::default, SharedPhaseTimings::total),
            #[cfg(feature = "performance-probes")]
            allocation,
        })
    }

    /// Claim and analyze items until the batch is exhausted or cancelled.
    ///
    /// An ordinary item failure is published and the lane continues, which keeps
    /// one unreadable file from deciding the fate of items the user also asked
    /// for. Cancellation is the one condition that stops every lane; the caller
    /// reports it once, after all lanes have joined.
    fn run_lane(
        &self,
        analyzer: &Analyzer,
        files: &[PathBuf],
        shared: &LaneShared<'_>,
        control: &ExecutionControl<'_>,
    ) {
        loop {
            if shared.stop.load(Ordering::Acquire) {
                return;
            }
            let index = shared.claim.fetch_add(1, Ordering::Relaxed);
            let Some(path) = files.get(index) else {
                return;
            };
            if shared.stop.load(Ordering::Acquire) {
                return;
            }
            #[cfg(test)]
            if self.fault == Some(LaneFault::Panic(index)) {
                panic!("injected batch file-lane panic at item {index}");
            }
            if control.cancellation.is_cancelled() {
                return;
            }
            let request = AnalyzeRequest { path: path.clone() };
            let analyzed = match shared.timings {
                Some(timings) => {
                    analyzer.analyze_file_at_with_timing_sink(request, index, control, timings)
                }
                None => analyzer.analyze_file_at(request, index, control),
            };
            let item = match analyzed {
                Ok(report) => BatchItem {
                    display_path: path.display().to_string(),
                    outcome: BatchItemOutcome::Success {
                        report: Box::new(report),
                    },
                },
                Err(error) if error.code == ErrorCode::Cancelled => return,
                Err(error) => BatchItem {
                    display_path: path.display().to_string(),
                    outcome: BatchItemOutcome::Failure { error },
                },
            };
            if shared.stop.load(Ordering::Acquire) {
                return;
            }
            if let Err(error) = shared.output.publish(BatchChunkItem { index, item }) {
                shared.fail(error);
                return;
            }
        }
    }

    /// Run one lane behind an unwind boundary and publish a stop request before
    /// the other lanes can claim more work.
    fn run_lane_catching(
        &self,
        analyzer: &Analyzer,
        files: &[PathBuf],
        shared: &LaneShared<'_>,
        control: &ExecutionControl<'_>,
    ) -> bool {
        let panicked = catch_unwind(AssertUnwindSafe(|| {
            self.run_lane(analyzer, files, shared, control);
        }))
        .is_err();
        if panicked {
            shared.stop.store(true, Ordering::Release);
        }
        panicked
    }
}

/// What every lane in one batch shares while claiming items.
///
/// One parameter rather than four keeps a lane's signature stable as shared
/// state is added, and makes it obvious that all of it is shared.
struct LaneShared<'a> {
    claim: &'a AtomicUsize,
    stop: &'a AtomicBool,
    failure: &'a Mutex<Option<AnalysisError>>,
    output: &'a BatchChunkEmitter<'a>,
    timings: Option<&'a SharedPhaseTimings>,
}

impl LaneShared<'_> {
    fn fail(&self, error: AnalysisError) {
        self.stop.store(true, Ordering::Release);
        let mut failure = self
            .failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if failure.is_none() {
            *failure = Some(error);
        }
    }
}

struct BatchChunkEmitter<'a> {
    sink: &'a dyn BatchChunkSink,
    state: Mutex<BatchChunkState>,
}

struct BatchChunkState {
    pending: Vec<BatchChunkItem>,
    published: usize,
    succeeded: usize,
    failed: usize,
}

impl<'a> BatchChunkEmitter<'a> {
    fn new(sink: &'a dyn BatchChunkSink) -> Self {
        Self {
            sink,
            state: Mutex::new(BatchChunkState {
                pending: Vec::with_capacity(BatchChunk::MAX_ITEMS),
                published: 0,
                succeeded: 0,
                failed: 0,
            }),
        }
    }

    fn publish(&self, entry: BatchChunkItem) -> Result<(), AnalysisError> {
        let chunk = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| batch_stream_error("batch chunk buffer is poisoned"))?;
            match &entry.item.outcome {
                BatchItemOutcome::Success { .. } => state.succeeded += 1,
                BatchItemOutcome::Failure { .. } => state.failed += 1,
            }
            state.published += 1;
            state.pending.push(entry);
            (state.pending.len() == BatchChunk::MAX_ITEMS).then(|| BatchChunk {
                items: std::mem::replace(
                    &mut state.pending,
                    Vec::with_capacity(BatchChunk::MAX_ITEMS),
                ),
            })
        };
        match chunk {
            Some(chunk) => self.sink.emit(chunk),
            None => Ok(()),
        }
    }

    fn finish(self, expected: usize) -> Result<BatchCompletion, AnalysisError> {
        let state = self
            .state
            .into_inner()
            .map_err(|_| batch_stream_error("batch chunk buffer is poisoned"))?;
        if state.published != expected {
            return Err(unclaimed_item_error(state.published, expected));
        }
        if !state.pending.is_empty() {
            self.sink.emit(BatchChunk {
                items: state.pending,
            })?;
        }
        let summary = BatchSummary {
            total: state.published,
            succeeded: state.succeeded,
            failed: state.failed,
        };
        let status = match (summary.succeeded, summary.failed) {
            (_, 0) => BatchStatus::Succeeded,
            (0, _) => BatchStatus::Failed,
            _ => BatchStatus::PartiallySucceeded,
        };
        Ok(BatchCompletion { status, summary })
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaneFault {
    Spawn(usize),
    Panic(usize),
}

fn unclaimed_item_error(published: usize, expected: usize) -> AnalysisError {
    AnalysisError::new(
        ErrorCode::Internal,
        AnalysisStage::Internal,
        "a batch item was never claimed by a file lane",
    )
    .with_details(format!("published={published}; expected={expected}"))
}

fn batch_stream_error(message: impl Into<String>) -> AnalysisError {
    AnalysisError::new(ErrorCode::Internal, AnalysisStage::Internal, message)
}

fn batch_collector_error(message: impl Into<String>) -> AnalysisError {
    AnalysisError::new(ErrorCode::Internal, AnalysisStage::Output, message)
}

fn lane_spawn_error(error: io::Error) -> AnalysisError {
    AnalysisError::new(
        ErrorCode::ResourceExhausted,
        AnalysisStage::Internal,
        "failed to start a batch file lane",
    )
    .with_details(error.to_string())
    .recoverable(true)
}

fn lane_panic_error() -> AnalysisError {
    AnalysisError::new(
        ErrorCode::Internal,
        AnalysisStage::Internal,
        "a batch file lane panicked",
    )
}

pub(crate) fn discover_inputs_with_control(
    inputs: &[PathBuf],
    recursive: bool,
    control: &ExecutionControl<'_>,
) -> Result<Vec<PathBuf>, AnalysisError> {
    if control.cancellation.is_cancelled() {
        return Err(AnalysisError::cancelled());
    }
    control.progress.emit(AnalysisEvent::DiscoveryStarted);
    let files = discover_inputs_with_cancellation(inputs, recursive, Some(control.cancellation))?;
    control
        .progress
        .emit(AnalysisEvent::DiscoveryFinished { files: files.len() });
    Ok(files)
}

fn discover_inputs_with_cancellation(
    inputs: &[PathBuf],
    recursive: bool,
    cancellation: Option<&CancellationToken>,
) -> Result<Vec<PathBuf>, AnalysisError> {
    if inputs.is_empty() {
        return Err(AnalysisError::new(
            ErrorCode::NoInputs,
            AnalysisStage::Discovery,
            "no input paths were provided",
        ));
    }

    let mut discovered = Vec::new();
    let mut seen = HashSet::new();

    for input in inputs {
        ensure_discovery_not_cancelled(cancellation)?;
        if input.is_dir() {
            let max_depth = if recursive { usize::MAX } else { 1 };
            let mut directory_files = Vec::new();
            for entry in WalkDir::new(input)
                .follow_links(false)
                .min_depth(1)
                .max_depth(max_depth)
            {
                ensure_discovery_not_cancelled(cancellation)?;
                let entry = entry.map_err(|error| {
                    AnalysisError::new(
                        ErrorCode::PermissionDenied,
                        AnalysisStage::Discovery,
                        "failed to scan an input directory",
                    )
                    .with_display_path(input.display().to_string())
                    .with_details(error.to_string())
                })?;
                if entry.file_type().is_file() && is_discoverable(entry.path()) {
                    directory_files.push(entry.into_path());
                }
            }
            directory_files.sort();
            for file in directory_files {
                if seen.insert(file.clone()) {
                    discovered.push(file);
                }
            }
        } else if seen.insert(input.clone()) {
            discovered.push(input.clone());
        }
    }

    if discovered.is_empty() {
        return Err(AnalysisError::new(
            ErrorCode::NoInputs,
            AnalysisStage::Discovery,
            "no supported audio inputs were found",
        ));
    }
    Ok(discovered)
}

fn ensure_discovery_not_cancelled(
    cancellation: Option<&CancellationToken>,
) -> Result<(), AnalysisError> {
    if cancellation.is_some_and(CancellationToken::is_cancelled) {
        Err(AnalysisError::cancelled())
    } else {
        Ok(())
    }
}

fn is_discoverable(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(crate::capability::is_stable_discovery_extension)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_is_sorted_deduplicated_and_non_recursive_by_default() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("b.wav"), b"x").unwrap();
        std::fs::write(root.path().join("a.flac"), b"x").unwrap();
        std::fs::write(root.path().join("ignored.mp3"), b"x").unwrap();
        std::fs::create_dir(root.path().join("nested")).unwrap();
        std::fs::write(root.path().join("nested/c.aiff"), b"x").unwrap();

        let direct = root.path().join("b.wav");
        let files =
            discover_inputs_with_cancellation(&[root.path().to_path_buf(), direct], false, None)
                .unwrap();
        assert_eq!(files.len(), 2);
        assert!(files[0].ends_with("a.flac"));
        assert!(files[1].ends_with("b.wav"));

        let recursive =
            discover_inputs_with_cancellation(&[root.path().to_path_buf()], true, None).unwrap();
        assert_eq!(recursive.len(), 3);
    }

    // ADR-0014 §7 graduation: batch item order, independent failure semantics,
    // interleaved event identity and whole-batch cancellation must be identical
    // at every lane count, and mixed routes must not multiply into nested
    // concurrency. These drive the real runner over a real allocation rather
    // than a mirrored constant, so the split under test is the one production
    // would perform.

    use crate::{NoopProgressSink, concurrency::ConcurrencyPlan};
    use std::sync::{Arc, atomic::AtomicUsize as TestCounter};

    type PublishedIdentity = (usize, String, &'static str);
    type PublishedIdentities = Arc<Mutex<Vec<PublishedIdentity>>>;

    /// Three lanes is not a rounder number than the rest: on the eight-worker
    /// plan below it is the widest split that still grants a packet pool, and
    /// the only one whose lane executors and decoders together consume the whole
    /// plan. A sweep that jumps from two to four skips the one width where both
    /// parallel axes are live at their fullest, which is exactly where lane and
    /// packet ordering could interfere.
    const LANE_COUNTS: [usize; 5] = [1, 2, 3, 4, 8];

    fn nonzero(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).unwrap()
    }

    fn fixture(relative: &str) -> PathBuf {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let packaged = manifest.join("package-fixtures");
        let fixtures = if packaged.is_dir() {
            packaged
        } else {
            manifest.join("../../tests/fixtures")
        };
        fixtures.join(relative)
    }

    /// The fixed test plan the runner splits for itself.
    fn test_plan() -> ConcurrencyPlan {
        ConcurrencyPlan::bounded_for_test(
            nonzero(MAX_LANE_PLAN_WORKERS),
            nonzero(MAX_LANE_PLAN_WORKERS),
        )
    }

    /// One allocation split the same way the runner would split it.
    fn allocation_for(lanes: usize) -> crate::concurrency::PlanAllocation {
        test_plan()
            .allocate(nonzero(lanes))
            .expect("a lane split of the fixed test plan must succeed")
    }

    fn runner_for(lanes: usize) -> BatchRunner {
        BatchRunner::with_overlap_shape(test_plan(), nonzero(lanes), OverlapShape::DEFAULT)
    }

    fn fake_batch(item_count: usize) -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        for index in 0..item_count {
            std::fs::write(
                directory.path().join(format!("{index:04}.wav")),
                b"not a wave file",
            )
            .unwrap();
        }
        directory
    }

    const MAX_LANE_PLAN_WORKERS: usize = 8;

    #[test]
    fn completed_items_are_delivered_in_bounded_chunks_with_a_tail() {
        let directory = fake_batch(BatchChunk::MAX_ITEMS + 2);
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;
        let control = ExecutionControl::new(&cancellation, &progress);
        let emitted: Mutex<Vec<BatchChunk>> = Mutex::new(Vec::new());
        let chunks = |chunk: BatchChunk| {
            assert!(!chunk.items.is_empty());
            assert!(chunk.items.len() <= BatchChunk::MAX_ITEMS);
            emitted.lock().unwrap().push(chunk);
            Ok(())
        };

        let completion = runner_for(3)
            .run(
                BatchRequest::new(vec![directory.path().to_path_buf()], false),
                &chunks,
                &control,
            )
            .expect("ordinary item failures must be delivered as chunks");

        assert_eq!(completion.status, BatchStatus::Failed);
        assert_eq!(completion.summary.total, BatchChunk::MAX_ITEMS + 2);
        assert_eq!(completion.summary.succeeded, 0);
        assert_eq!(completion.summary.failed, BatchChunk::MAX_ITEMS + 2);
        let emitted = emitted.into_inner().unwrap();
        assert_eq!(
            emitted
                .iter()
                .map(|chunk| chunk.items.len())
                .collect::<Vec<_>>(),
            [BatchChunk::MAX_ITEMS, 2]
        );
        let mut indexes = emitted
            .into_iter()
            .flat_map(|chunk| chunk.items.into_iter().map(|entry| entry.index))
            .collect::<Vec<_>>();
        indexes.sort_unstable();
        assert_eq!(indexes, (0..BatchChunk::MAX_ITEMS + 2).collect::<Vec<_>>());
    }

    #[test]
    fn a_chunk_sink_error_stops_the_batch_without_a_terminal_event() {
        let directory = fake_batch(BatchChunk::MAX_ITEMS);
        let cancellation = CancellationToken::new();
        let terminal = AtomicBool::new(false);
        let progress = |event: AnalysisEvent| {
            if matches!(event, AnalysisEvent::BatchFinished { .. }) {
                terminal.store(true, Ordering::SeqCst);
            }
        };
        let control = ExecutionControl::new(&cancellation, &progress);
        let chunks = |_chunk: BatchChunk| {
            Err(AnalysisError::new(
                ErrorCode::OutputFailed,
                AnalysisStage::Output,
                "injected chunk sink failure",
            ))
        };

        let error = runner_for(3)
            .run(
                BatchRequest::new(vec![directory.path().to_path_buf()], false),
                &chunks,
                &control,
            )
            .expect_err("a chunk sink failure must fail the whole batch");

        assert_eq!(error.code, ErrorCode::OutputFailed);
        assert_eq!(error.stage, AnalysisStage::Output);
        assert_eq!(error.message, "injected chunk sink failure");
        assert!(!terminal.load(Ordering::SeqCst));
    }

    #[test]
    fn the_explicit_report_collector_restores_stable_index_order() {
        let item = |index: usize| BatchChunkItem {
            index,
            item: BatchItem {
                display_path: format!("item-{index}"),
                outcome: BatchItemOutcome::Failure {
                    error: AnalysisError::new(
                        ErrorCode::UnsupportedFormat,
                        AnalysisStage::Probe,
                        "test failure",
                    ),
                },
            },
        };
        let collector = BatchReportCollector::new();
        collector
            .emit(BatchChunk {
                items: vec![item(1), item(0)],
            })
            .unwrap();
        let report = collector
            .finish(BatchCompletion {
                status: BatchStatus::Failed,
                summary: BatchSummary {
                    total: 2,
                    succeeded: 0,
                    failed: 2,
                },
            })
            .unwrap();

        assert_eq!(report.items[0].display_path, "item-0");
        assert_eq!(report.items[1].display_path, "item-1");
    }

    /// A mixed-route, mixed-outcome input set.
    ///
    /// It deliberately holds a packet-parallel ALAC route beside serial WAV
    /// routes and two items that fail for different reasons, so one run
    /// exercises route mixing and independent failure together.
    fn mixed_inputs() -> Vec<PathBuf> {
        vec![
            fixture("native-alac-v1/alac16-stereo-48000-multipacket.m4a"),
            fixture("tiny_duration.wav"),
            fixture("fake_audio.wav"),
            fixture("full_scale_clipping.wav"),
            fixture("native-alac-v1/alac16-mono-44100.m4a"),
            fixture("truncated.wav"),
            fixture("silence.wav"),
            fixture("edge_cases.wav"),
        ]
    }

    fn run_at(lanes: usize, inputs: Vec<PathBuf>) -> Result<BatchReport, AnalysisError> {
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;
        let control = ExecutionControl::new(&cancellation, &progress);
        let collector = BatchReportCollector::new();
        let completion =
            runner_for(lanes).run(BatchRequest::new(inputs, false), &collector, &control)?;
        collector.finish(completion)
    }

    fn shape(report: &BatchReport) -> Vec<(String, bool)> {
        report
            .items
            .iter()
            .map(|item| {
                (
                    item.display_path.clone(),
                    matches!(item.outcome, BatchItemOutcome::Success { .. }),
                )
            })
            .collect()
    }

    #[test]
    fn item_order_and_outcomes_are_identical_at_every_lane_count() {
        let serial = run_at(1, mixed_inputs()).expect("the serial reference must complete");
        let reference = shape(&serial);
        assert!(
            reference.iter().any(|(_, ok)| *ok) && reference.iter().any(|(_, ok)| !*ok),
            "the fixture set must mix successes and failures to be worth comparing"
        );

        for lanes in LANE_COUNTS {
            let report = run_at(lanes, mixed_inputs())
                .unwrap_or_else(|error| panic!("{lanes} lanes must complete: {error}"));
            assert_eq!(
                shape(&report),
                reference,
                "{lanes} lanes changed item order or per-item outcome"
            );
            assert_eq!(report.status, serial.status, "{lanes} lanes changed status");
            assert_eq!(
                report.summary, serial.summary,
                "{lanes} lanes changed the summary"
            );
        }
    }

    #[test]
    fn one_failing_item_never_cancels_or_swallows_the_others() {
        for lanes in LANE_COUNTS {
            let report = run_at(lanes, mixed_inputs()).expect("failures are not batch errors");
            assert_eq!(
                report.summary.total,
                mixed_inputs().len(),
                "{lanes} lanes dropped an admitted item"
            );
            assert!(
                report.summary.failed > 0 && report.summary.succeeded > 0,
                "{lanes} lanes lost the partial-success shape"
            );
            assert_eq!(report.status, BatchStatus::PartiallySucceeded);
        }
    }

    #[test]
    fn every_progress_event_and_chunk_item_names_its_input() {
        for lanes in LANE_COUNTS {
            let inputs = mixed_inputs();
            let seen: PublishedIdentities = Arc::new(Mutex::new(Vec::new()));
            let cancellation = CancellationToken::new();
            let progress_seen = Arc::clone(&seen);
            let progress = move |event: AnalysisEvent| {
                let entry = match event {
                    AnalysisEvent::FileStarted {
                        index,
                        display_path,
                    } => Some((index, display_path, "started")),
                    AnalysisEvent::FileFinished {
                        index,
                        display_path,
                        ..
                    } => Some((index, display_path, "finished")),
                    _ => None,
                };
                if let Some(entry) = entry {
                    progress_seen.lock().unwrap().push(entry);
                }
            };
            let chunk_seen = Arc::clone(&seen);
            let chunks = move |chunk: BatchChunk| {
                let mut seen = chunk_seen.lock().unwrap();
                seen.extend(
                    chunk
                        .items
                        .into_iter()
                        .map(|entry| (entry.index, entry.item.display_path, "published")),
                );
                Ok(())
            };
            let control = ExecutionControl::new(&cancellation, &progress);
            runner_for(lanes)
                .run(BatchRequest::new(inputs.clone(), false), &chunks, &control)
                .expect("the batch must complete");

            let seen = seen.lock().unwrap();
            // Events may interleave across lanes, so order proves nothing. What
            // must hold is that each event's index still names its own file and
            // that every item reported both lifecycle boundaries and its
            // renderable outcome exactly once.
            for (index, display_path, _) in seen.iter() {
                let expected = inputs
                    .get(*index)
                    .unwrap_or_else(|| panic!("{lanes} lanes emitted index {index} out of range"));
                assert_eq!(
                    display_path,
                    &expected.display().to_string(),
                    "{lanes} lanes attached index {index} to the wrong path"
                );
            }
            for (index, _) in inputs.iter().enumerate() {
                for kind in ["started", "finished", "published"] {
                    let count = seen
                        .iter()
                        .filter(|(seen_index, _, seen_kind)| {
                            *seen_index == index && *seen_kind == kind
                        })
                        .count();
                    assert_eq!(
                        count, 1,
                        "{lanes} lanes emitted {count} {kind} for item {index}"
                    );
                }
            }
        }
    }

    #[test]
    fn cancellation_stops_the_batch_and_joins_every_lane() {
        for lanes in LANE_COUNTS {
            let cancellation = CancellationToken::new();
            let started = TestCounter::new(0);
            let finished = TestCounter::new(0);
            let sink = |event: AnalysisEvent| match event {
                AnalysisEvent::FileStarted { .. } => {
                    // Cancel while the batch still has unclaimed items, so the
                    // request lands mid-flight rather than after the last one.
                    if started.fetch_add(1, Ordering::SeqCst) == 0 {
                        cancellation.cancel();
                    }
                }
                AnalysisEvent::FileFinished { .. } => {
                    finished.fetch_add(1, Ordering::SeqCst);
                }
                _ => {}
            };
            let control = ExecutionControl::new(&cancellation, &sink);
            let chunks = NoopBatchChunkSink;
            let outcome =
                runner_for(lanes).run(BatchRequest::new(mixed_inputs(), false), &chunks, &control);

            let error = outcome.expect_err("a cancelled batch must not return a report");
            assert_eq!(error.code, ErrorCode::Cancelled, "{lanes} lanes");
            // `thread::scope` joins before `run` returns, so every item that
            // announced a start has also announced its finish by the time the
            // cancellation is observable to the caller.
            assert_eq!(
                started.load(Ordering::SeqCst),
                finished.load(Ordering::SeqCst),
                "{lanes} lanes returned with an item still in flight"
            );
        }
    }

    #[test]
    fn lane_spawn_failure_is_structured_and_starts_no_item() {
        let cancellation = CancellationToken::new();
        let started = TestCounter::new(0);
        let sink = |event: AnalysisEvent| {
            if matches!(event, AnalysisEvent::FileStarted { .. }) {
                started.fetch_add(1, Ordering::SeqCst);
            }
        };
        let control = ExecutionControl::new(&cancellation, &sink);
        let chunks = NoopBatchChunkSink;
        let error = runner_for(4)
            .with_fault(LaneFault::Spawn(2))
            .run(BatchRequest::new(mixed_inputs(), false), &chunks, &control)
            .expect_err("an injected lane spawn failure must be structured");

        assert_eq!(error.code, ErrorCode::ResourceExhausted);
        assert_eq!(error.stage, AnalysisStage::Internal);
        assert_eq!(error.message, "failed to start a batch file lane");
        assert_eq!(
            started.load(Ordering::SeqCst),
            0,
            "the start gate must keep earlier lanes idle until every spawn succeeds"
        );
    }

    #[test]
    fn lane_panic_is_joined_and_returned_as_a_structured_error() {
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;
        let control = ExecutionControl::new(&cancellation, &progress);
        let chunks = NoopBatchChunkSink;
        let error = runner_for(4)
            .with_fault(LaneFault::Panic(0))
            .run(BatchRequest::new(mixed_inputs(), false), &chunks, &control)
            .expect_err("an injected lane panic must not escape the application facade");

        assert_eq!(error.code, ErrorCode::Internal);
        assert_eq!(error.stage, AnalysisStage::Internal);
        assert_eq!(error.message, "a batch file lane panicked");
    }

    #[test]
    fn lanes_and_their_decoders_never_exceed_the_plan_that_granted_them() {
        for lanes in LANE_COUNTS {
            let allocation = allocation_for(lanes);
            let granted = allocation.file_lanes().get();
            let per_lane = allocation.decode().workers().get();
            let lane_threads = granted.saturating_sub(1);
            let decoder_threads = if per_lane > 1 {
                granted.saturating_mul(per_lane)
            } else {
                0
            };
            assert!(
                lane_threads.saturating_add(decoder_threads) <= MAX_LANE_PLAN_WORKERS,
                "{lanes} requested lanes produced {lane_threads} lane threads plus \
                 {decoder_threads} decoder threads over a {MAX_LANE_PLAN_WORKERS}-worker plan"
            );
            // A mixed batch must not let the packet-parallel route reclaim what
            // widening lanes already spent.
            let runner = runner_for(lanes);
            assert_eq!(runner.requested_file_lanes.get(), lanes);
        }
    }
}
