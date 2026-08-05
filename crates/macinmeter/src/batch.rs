use crate::{
    AnalysisError, AnalysisEvent, AnalysisReport, AnalysisStage, AnalyzeRequest, CancellationToken,
    ErrorCode, ExecutionControl, application::Analyzer, concurrency::PlanAllocation,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
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

#[derive(Debug)]
pub(crate) struct BatchRunner {
    analyzer: Analyzer,
    file_lanes: NonZeroUsize,
}

impl Default for BatchRunner {
    fn default() -> Self {
        Self {
            analyzer: Analyzer::default(),
            file_lanes: NonZeroUsize::MIN,
        }
    }
}

impl BatchRunner {
    /// Build a runner over the job's allocation.
    ///
    /// The lane count and the per-lane decode permit are two halves of one
    /// split the plan already performed, so widening lanes has necessarily
    /// already narrowed each lane's decoder. Nothing here may request a second
    /// permit or size a pool from the batch length.
    pub(crate) const fn new(allocation: PlanAllocation) -> Self {
        Self {
            analyzer: Analyzer::new(allocation.decode()),
            file_lanes: allocation.file_lanes(),
        }
    }

    pub(crate) fn run(
        &self,
        request: BatchRequest,
        control: &ExecutionControl<'_>,
    ) -> Result<BatchReport, AnalysisError> {
        if control.cancellation.is_cancelled() {
            return Err(AnalysisError::cancelled());
        }
        let files = discover_inputs_with_control(&request.inputs, request.recursive, control)?;

        // Discovery already fixed the input order, so an item's index names one
        // file no matter which lane produces it. Lanes claim by index rather
        // than by a fixed stride: a static split would decide the tail from the
        // input order, and a batch's item costs are not known before decoding.
        let claim = AtomicUsize::new(0);
        let outcomes: Vec<Mutex<Option<LaneOutcome>>> =
            files.iter().map(|_| Mutex::new(None)).collect();
        let lanes = self.file_lanes.get().min(files.len().max(1));

        thread::scope(|scope| {
            for _ in 1..lanes {
                scope.spawn(|| self.run_lane(&files, &claim, &outcomes, control));
            }
            // The calling thread is one of the lanes, so a batch never costs a
            // thread it does not use and a single-lane batch spawns nothing.
            self.run_lane(&files, &claim, &outcomes, control);
        });

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
        Ok(BatchReport {
            status,
            items,
            summary,
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
        files: &[PathBuf],
        claim: &AtomicUsize,
        outcomes: &[Mutex<Option<LaneOutcome>>],
        control: &ExecutionControl<'_>,
    ) {
        loop {
            let index = claim.fetch_add(1, Ordering::Relaxed);
            let Some(path) = files.get(index) else {
                return;
            };
            if control.cancellation.is_cancelled() {
                Self::store(&outcomes[index], LaneOutcome::Cancelled(cancelled()));
                return;
            }
            let request = AnalyzeRequest { path: path.clone() };
            let outcome = match self.analyzer.analyze_file_at(request, index, control) {
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

    const LANE_COUNTS: [usize; 4] = [1, 2, 4, 8];

    fn nonzero(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).unwrap()
    }

    fn fixture(relative: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(relative)
    }

    /// One allocation split the same way production would split it.
    fn allocation_for(lanes: usize) -> PlanAllocation {
        ConcurrencyPlan::bounded_for_test(
            nonzero(MAX_LANE_PLAN_WORKERS),
            nonzero(MAX_LANE_PLAN_WORKERS),
        )
        .allocate(nonzero(lanes))
        .expect("a lane split of the fixed test plan must succeed")
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
        BatchRunner::new(allocation_for(lanes)).run(BatchRequest::new(inputs, false), &control)
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
            BatchRunner::new(allocation_for(lanes))
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
            let outcome = BatchRunner::new(allocation_for(lanes))
                .run(BatchRequest::new(mixed_inputs(), false), &control);

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
    fn lanes_and_their_decoders_never_exceed_the_plan_that_granted_them() {
        for lanes in LANE_COUNTS {
            let allocation = allocation_for(lanes);
            let granted = allocation.file_lanes().get();
            let per_lane = allocation.decode().workers().get();
            assert!(
                granted.saturating_mul(per_lane) <= MAX_LANE_PLAN_WORKERS,
                "{lanes} requested lanes produced {granted}x{per_lane} over a \
                 {MAX_LANE_PLAN_WORKERS}-worker plan"
            );
            // A mixed batch must not let the packet-parallel route reclaim what
            // widening lanes already spent.
            let runner = BatchRunner::new(allocation);
            assert_eq!(runner.file_lanes.get(), granted);
        }
    }
}
