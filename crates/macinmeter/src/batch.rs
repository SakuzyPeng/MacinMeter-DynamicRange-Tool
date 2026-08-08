#[cfg(feature = "performance-probes")]
use crate::concurrency::PlanAllocation;
use crate::{
    AnalysisError, AnalysisEvent, AnalysisReport, AnalysisStage, AnalyzeRequest, CancellationToken,
    ErrorCode, ExecutionControl,
    application::{Analyzer, OverlapShape},
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
    report: BatchReport,
    #[cfg(feature = "performance-probes")]
    allocation: PlanAllocation,
}

#[derive(Debug)]
pub(crate) struct BatchRunner {
    plan: ConcurrencyPlan,
    requested_file_lanes: NonZeroUsize,
    overlap_shape: OverlapShape,
    #[cfg(test)]
    fault: Option<LaneFault>,
}

impl Default for BatchRunner {
    fn default() -> Self {
        Self {
            plan: ConcurrencyPlan::serial(),
            requested_file_lanes: NonZeroUsize::MIN,
            overlap_shape: OverlapShape::DEFAULT,
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
            #[cfg(test)]
            fault: None,
        }
    }

    #[cfg(test)]
    fn with_fault(mut self, fault: LaneFault) -> Self {
        self.fault = Some(fault);
        self
    }

    pub(crate) fn run(
        &self,
        request: BatchRequest,
        control: &ExecutionControl<'_>,
    ) -> Result<BatchReport, AnalysisError> {
        self.run_execution(request, control)
            .map(|execution| execution.report)
    }

    #[cfg(feature = "performance-probes")]
    pub(crate) fn run_with_performance_probe(
        &self,
        request: BatchRequest,
        control: &ExecutionControl<'_>,
    ) -> Result<(BatchReport, BatchPerformanceProbe), AnalysisError> {
        let execution = self.run_execution(request, control)?;
        let probe = BatchPerformanceProbe::new(self.plan, execution.allocation);
        Ok((execution.report, probe))
    }

    fn run_execution(
        &self,
        request: BatchRequest,
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

        // Discovery already fixed the input order, so an item's index names one
        // file no matter which lane produces it. Lanes claim by index rather
        // than by a fixed stride: a static split would decide the tail from the
        // input order, and a batch's item costs are not known before decoding.
        let claim = AtomicUsize::new(0);
        let stop = AtomicBool::new(false);
        let outcomes: Vec<Mutex<Option<LaneOutcome>>> =
            files.iter().map(|_| Mutex::new(None)).collect();
        let lanes = allocation.file_lanes().get();

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
                let lane_claim = &claim;
                let lane_outcomes = &outcomes;
                let lane_stop = &stop;
                let operation = move || {
                    if start_gate.recv().is_err() {
                        return false;
                    }
                    runner.run_lane_catching(
                        lane_analyzer,
                        lane_files,
                        lane_claim,
                        lane_outcomes,
                        lane_stop,
                        control,
                    )
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
                && self.run_lane_catching(&analyzer, &files, &claim, &outcomes, &stop, control)
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

        let mut items = Vec::with_capacity(files.len());
        let mut succeeded = 0;
        let mut failed = 0;
        for (path, slot) in files.iter().zip(outcomes) {
            let outcome = slot
                .into_inner()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .ok_or_else(unclaimed_item_error)?;
            match outcome {
                LaneOutcome::Cancelled(error) => return Err(error),
                LaneOutcome::Finished(BatchItemOutcome::Success { report }) => {
                    succeeded += 1;
                    items.push(BatchItem {
                        display_path: path.display().to_string(),
                        outcome: BatchItemOutcome::Success { report },
                    });
                }
                LaneOutcome::Finished(outcome) => {
                    failed += 1;
                    items.push(BatchItem {
                        display_path: path.display().to_string(),
                        outcome,
                    });
                }
            }
        }

        let status = match (succeeded, failed) {
            (_, 0) => BatchStatus::Succeeded,
            (0, _) => BatchStatus::Failed,
            _ => BatchStatus::PartiallySucceeded,
        };
        let summary = BatchSummary {
            total: items.len(),
            succeeded,
            failed,
        };
        control
            .progress
            .emit(AnalysisEvent::BatchFinished { succeeded, failed });
        Ok(BatchExecution {
            report: BatchReport {
                status,
                items,
                summary,
            },
            #[cfg(feature = "performance-probes")]
            allocation,
        })
    }

    /// Claim and analyze items until the batch is exhausted or cancelled.
    ///
    /// An ordinary item failure is recorded and the lane continues, which keeps
    /// one unreadable file from deciding the fate of items the user also asked
    /// for. Cancellation is the one condition that stops every lane, and it is
    /// stored rather than returned so the caller reports it once, after all
    /// lanes have joined.
    fn run_lane(
        &self,
        analyzer: &Analyzer,
        files: &[PathBuf],
        claim: &AtomicUsize,
        outcomes: &[Mutex<Option<LaneOutcome>>],
        stop: &AtomicBool,
        control: &ExecutionControl<'_>,
    ) {
        loop {
            if stop.load(Ordering::Acquire) {
                return;
            }
            let index = claim.fetch_add(1, Ordering::Relaxed);
            let Some(path) = files.get(index) else {
                return;
            };
            if stop.load(Ordering::Acquire) {
                return;
            }
            #[cfg(test)]
            if self.fault == Some(LaneFault::Panic(index)) {
                panic!("injected batch file-lane panic at item {index}");
            }
            if control.cancellation.is_cancelled() {
                Self::store(&outcomes[index], LaneOutcome::Cancelled(cancelled()));
                return;
            }
            let request = AnalyzeRequest { path: path.clone() };
            let outcome = match analyzer.analyze_file_at(request, index, control) {
                Ok(report) => LaneOutcome::Finished(BatchItemOutcome::Success {
                    report: Box::new(report),
                }),
                Err(error) if error.code == ErrorCode::Cancelled => LaneOutcome::Cancelled(error),
                Err(error) => LaneOutcome::Finished(BatchItemOutcome::Failure { error }),
            };
            let cancelled = matches!(outcome, LaneOutcome::Cancelled(_));
            Self::store(&outcomes[index], outcome);
            if cancelled {
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
        claim: &AtomicUsize,
        outcomes: &[Mutex<Option<LaneOutcome>>],
        stop: &AtomicBool,
        control: &ExecutionControl<'_>,
    ) -> bool {
        let panicked = catch_unwind(AssertUnwindSafe(|| {
            self.run_lane(analyzer, files, claim, outcomes, stop, control);
        }))
        .is_err();
        if panicked {
            stop.store(true, Ordering::Release);
        }
        panicked
    }

    fn store(slot: &Mutex<Option<LaneOutcome>>, outcome: LaneOutcome) {
        *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(outcome);
    }
}

/// One item's result as its lane left it.
///
/// Cancellation travels beside the ordinary outcomes rather than short-circuiting
/// a lane's siblings, so the whole batch still joins before anything is reported.
enum LaneOutcome {
    Finished(BatchItemOutcome),
    Cancelled(AnalysisError),
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaneFault {
    Spawn(usize),
    Panic(usize),
}

fn cancelled() -> AnalysisError {
    AnalysisError::cancelled()
}

fn unclaimed_item_error() -> AnalysisError {
    AnalysisError::new(
        ErrorCode::Internal,
        AnalysisStage::Internal,
        "a batch item was never claimed by a file lane",
    )
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
    use std::sync::atomic::AtomicUsize as TestCounter;

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
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(relative)
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

    const MAX_LANE_PLAN_WORKERS: usize = 8;

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
        runner_for(lanes).run(BatchRequest::new(inputs, false), &control)
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
    fn every_progress_event_names_the_item_it_belongs_to() {
        for lanes in LANE_COUNTS {
            let inputs = mixed_inputs();
            let seen: Mutex<Vec<(usize, String, &'static str)>> = Mutex::new(Vec::new());
            let cancellation = CancellationToken::new();
            let sink = |event: AnalysisEvent| {
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
                    seen.lock().unwrap().push(entry);
                }
            };
            let control = ExecutionControl::new(&cancellation, &sink);
            runner_for(lanes)
                .run(BatchRequest::new(inputs.clone(), false), &control)
                .expect("the batch must complete");

            let seen = seen.into_inner().unwrap();
            // Events may interleave across lanes, so order proves nothing. What
            // must hold is that each event's index still names its own file and
            // that every item reported both of its boundaries exactly once.
            for (index, display_path, _) in &seen {
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
                for kind in ["started", "finished"] {
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
            let outcome = runner_for(lanes).run(BatchRequest::new(mixed_inputs(), false), &control);

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
        let error = runner_for(4)
            .with_fault(LaneFault::Spawn(2))
            .run(BatchRequest::new(mixed_inputs(), false), &control)
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
        let error = runner_for(4)
            .with_fault(LaneFault::Panic(0))
            .run(BatchRequest::new(mixed_inputs(), false), &control)
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
