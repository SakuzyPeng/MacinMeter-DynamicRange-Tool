//! Analysis and discovery jobs over the shared [`Application`].
//!
//! The application contract is the same one the Tauri adapter honoured: a job
//! is reserved from the single `Application` *before* any thread is created,
//! the reservation queues FIFO behind the active job, cancelling a queued job
//! removes only that reservation, and dropping the `ApplicationJob` releases
//! it. What disappears is the IPC boundary. The Tauri adapter addressed jobs by
//! string id because JavaScript could only name them; here the GUI holds the
//! `CancellationToken` itself, and results stay Rust values instead of being
//! serialised per event.

use crate::progress::BatchProgress;
use macinmeter::{
    AnalysisError, AnalysisEvent, AnalysisReport, AnalysisStage, AnalyzeRequest, Application,
    ApplicationJob, BatchChunk, BatchCompletion, BatchDetailStore, BatchListItem, BatchRequest,
    CancellationToken, ErrorCode, NoopProgressSink, PhaseTimings, ProgressSink,
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    thread,
    time::{Duration, Instant},
};

/// Asks the UI for another pass. The app passes a closure over
/// `egui::Context::request_repaint_after`; tests pass a no-op.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    File(PathBuf),
    Batch {
        inputs: Vec<PathBuf>,
        recursive: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Running,
    Discovering,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub phase: Phase,
    pub total: usize,
    pub fraction: f64,
    /// Index and display path of the most recently reported item.
    pub current: Option<(usize, String)>,
}

#[derive(Debug)]
pub enum JobResult {
    Analysis(Box<AnalysisReport>),
    Batch {
        completion: BatchCompletion,
        details: BatchDetailStore,
    },
    Error(AnalysisError),
}

#[derive(Debug)]
pub struct Finished {
    pub result: JobResult,
    pub timings: Option<PhaseTimings>,
    pub elapsed: Duration,
}

pub struct Update {
    pub snapshot: Snapshot,
    pub rows: Vec<BatchListItem>,
    pub finished: Option<Finished>,
}

#[derive(Debug)]
struct State {
    phase: Phase,
    progress: BatchProgress,
    current: Option<(usize, String)>,
    rows: Vec<BatchListItem>,
    finished: Option<Finished>,
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    // A panic while holding the lock can only come from this module's own
    // bookkeeping; the data is still a consistent snapshot, so keep going.
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct Sink {
    state: Arc<Mutex<State>>,
    wake: Wake,
}

impl ProgressSink for Sink {
    fn emit(&self, event: AnalysisEvent) {
        {
            let mut state = lock(&self.state);
            match event {
                AnalysisEvent::DiscoveryStarted => state.phase = Phase::Discovering,
                AnalysisEvent::DiscoveryFinished { files } => {
                    state.progress.reset(files);
                    state.phase = Phase::Running;
                }
                AnalysisEvent::FileStarted {
                    index,
                    display_path,
                } => {
                    state.progress.update(index, 0.0);
                    state.current = Some((index, display_path));
                }
                AnalysisEvent::DecodeProgress {
                    index,
                    display_path,
                    progress,
                } => {
                    state
                        .progress
                        .update(index, progress.fraction().unwrap_or(0.0));
                    state.current = Some((index, display_path));
                }
                AnalysisEvent::FileFinished {
                    index,
                    display_path,
                    ..
                } => {
                    state.progress.update(index, 1.0);
                    state.current = Some((index, display_path));
                }
                AnalysisEvent::BatchFinished { .. } => {}
            }
        }
        (self.wake)();
    }
}

/// One admitted analysis or batch.
pub struct AnalysisJob {
    token: CancellationToken,
    state: Arc<Mutex<State>>,
}

impl AnalysisJob {
    /// Reserves first, then spawns. A refused reservation or spawn becomes an
    /// already-finished job carrying a structured error, so the UI has one
    /// path for every outcome.
    pub fn start(
        application: &Application,
        request: Request,
        timing: bool,
        expected_total: usize,
        wake: Wake,
    ) -> Self {
        let started = Instant::now();
        let token = CancellationToken::new();
        let state = Arc::new(Mutex::new(State {
            phase: Phase::Running,
            progress: BatchProgress::new(expected_total),
            current: None,
            rows: Vec::new(),
            finished: None,
        }));
        let job = Self {
            token: token.clone(),
            state: Arc::clone(&state),
        };
        let reservation = match application.reserve(&token) {
            Ok(reservation) => reservation,
            Err(error) => {
                job.finish_now(error, started);
                return job;
            }
        };
        let sink = Sink {
            state: Arc::clone(&state),
            wake: Arc::clone(&wake),
        };
        // The worker publishes a terminal state even if the operation unwinds.
        // Dropping the UI's job only cancels the token and never joins a worker.
        if let Err(error) = thread::Builder::new()
            .name("macinmeter-analysis-job".to_owned())
            .spawn(move || {
                let (result, timings) = run_worker("analysis thread panicked", || {
                    execute(reservation, request, timing, &sink)
                })
                .unwrap_or_else(|error| (JobResult::Error(error), None));
                lock(&state).finished = Some(Finished {
                    result,
                    timings,
                    elapsed: started.elapsed(),
                });
                (wake)();
            })
        {
            // A failed spawn drops the closure, and with it the reservation.
            job.finish_now(
                internal_error(format!("analysis thread failed to start: {error}")),
                started,
            );
        }
        job
    }

    fn finish_now(&self, error: AnalysisError, started: Instant) {
        lock(&self.state).finished = Some(Finished {
            result: JobResult::Error(error),
            timings: None,
            elapsed: started.elapsed(),
        });
    }

    pub fn cancel(&self) {
        self.token.cancel();
    }

    /// Drain new rows with the terminal state so the UI sees the tail chunk
    /// before handling completion.
    pub fn poll(&self) -> Update {
        let mut state = lock(&self.state);
        Update {
            snapshot: Snapshot {
                phase: state.phase,
                total: state.progress.total(),
                fraction: state.progress.fraction(),
                current: state.current.clone(),
            },
            rows: std::mem::take(&mut state.rows),
            finished: state.finished.take(),
        }
    }
}

impl Drop for AnalysisJob {
    /// An abandoned job (window closed mid-run) is cancelled rather than
    /// joined, so closing never waits on a decoder. The worker still drops its
    /// reservation on the way out.
    fn drop(&mut self) {
        self.token.cancel();
    }
}

fn execute(
    job: ApplicationJob,
    request: Request,
    timing: bool,
    sink: &Sink,
) -> Result<(JobResult, Option<PhaseTimings>), AnalysisError> {
    match request {
        Request::File(path) => {
            let request = AnalyzeRequest::new(path);
            let (report, timings) = if timing {
                let (report, timings) = job.analyze_file_timed(request, sink)?;
                (report, Some(timings))
            } else {
                (job.analyze_file(request, sink)?, None)
            };
            Ok((JobResult::Analysis(Box::new(report)), timings))
        }
        Request::Batch { inputs, recursive } => {
            let request = BatchRequest::new(inputs, recursive);
            let details = BatchDetailStore::new();
            let chunks = |chunk: BatchChunk| {
                let list = details.store_chunk(chunk)?;
                lock(&sink.state).rows.extend(list.items);
                (sink.wake)();
                Ok(())
            };
            let (completion, timings) = if timing {
                let (completion, timings) = job.run_batch_timed(request, &chunks, sink)?;
                (completion, Some(timings))
            } else {
                (job.run_batch(request, &chunks, sink)?, None)
            };
            details.finish(completion)?;
            Ok((
                JobResult::Batch {
                    completion,
                    details,
                },
                timings,
            ))
        }
    }
}

type DiscoveryResult = Option<Result<Vec<PathBuf>, AnalysisError>>;

/// A cancellable preview of what a selection will analyze. It is a job like
/// any other, so it can never race a batch's own directory walk.
pub struct DiscoveryJob {
    token: CancellationToken,
    result: Arc<Mutex<DiscoveryResult>>,
}

impl DiscoveryJob {
    pub fn start(
        application: &Application,
        inputs: Vec<PathBuf>,
        recursive: bool,
        wake: Wake,
    ) -> Self {
        let token = CancellationToken::new();
        let result = Arc::new(Mutex::new(None));
        let job = Self {
            token: token.clone(),
            result: Arc::clone(&result),
        };
        let reservation = match application.reserve(&token) {
            Ok(reservation) => reservation,
            Err(error) => {
                *lock_result(&result) = Some(Err(error));
                return job;
            }
        };
        let spawned = thread::Builder::new()
            .name("macinmeter-discovery".to_owned())
            .spawn(move || {
                let found = run_worker("discovery thread panicked", || {
                    reservation.discover_inputs(&inputs, recursive, &NoopProgressSink)
                });
                *lock_result(&result) = Some(found);
                (wake)();
            });
        if let Err(error) = spawned {
            *lock_result(&job.result) = Some(Err(internal_error(format!(
                "discovery thread failed to start: {error}"
            ))));
        }
        job
    }

    pub fn take_result(&self) -> DiscoveryResult {
        lock_result(&self.result).take()
    }
}

impl Drop for DiscoveryJob {
    /// Replacing or clearing a selection drops its preview, which cancels it.
    fn drop(&mut self) {
        self.token.cancel();
    }
}

fn lock_result(result: &Mutex<DiscoveryResult>) -> MutexGuard<'_, DiscoveryResult> {
    result
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn run_worker<T>(
    panic_message: &'static str,
    operation: impl FnOnce() -> Result<T, AnalysisError>,
) -> Result<T, AnalysisError> {
    catch_unwind(AssertUnwindSafe(operation)).unwrap_or_else(|_| Err(internal_error(panic_message)))
}

fn internal_error(message: impl Into<String>) -> AnalysisError {
    AnalysisError::new(ErrorCode::Internal, AnalysisStage::Internal, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use macinmeter::{BatchReportCollector, capabilities};
    use std::{
        path::Path,
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
    };

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name)
    }

    fn no_wake() -> Wake {
        Arc::new(|| {})
    }

    fn wait(job: &AnalysisJob) -> (Finished, Vec<BatchListItem>, Snapshot) {
        let mut rows = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let update = job.poll();
            rows.extend(update.rows);
            if let Some(finished) = update.finished {
                return (finished, rows, update.snapshot);
            }
            assert!(Instant::now() < deadline, "job did not finish");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_file_job_returns_exactly_the_application_report() {
        let path = fixture("native-alac-v1/alac16-mono-44100.m4a");
        let application = Application::new();
        let job = AnalysisJob::start(
            &application,
            Request::File(path.clone()),
            false,
            1,
            no_wake(),
        );
        let (finished, rows, _) = wait(&job);
        let direct = Application::new()
            .analyze_file(AnalyzeRequest::new(path))
            .unwrap();
        let JobResult::Analysis(report) = finished.result else {
            panic!("a file job must return a single analysis report");
        };
        assert_eq!(*report, direct);
        assert!(rows.is_empty());
        assert!(finished.timings.is_none());
    }

    #[test]
    fn timing_is_reported_beside_the_envelope_not_inside_it() {
        let path = fixture("tiny_duration.wav");
        let application = Application::new();
        let timed = AnalysisJob::start(
            &application,
            Request::File(path.clone()),
            true,
            1,
            no_wake(),
        );
        let timed = wait(&timed).0;
        let plain = AnalysisJob::start(&application, Request::File(path), false, 1, no_wake());
        let plain = wait(&plain).0;
        assert!(timed.timings.is_some());
        assert_eq!(
            crate::export::envelope(&timed.result).unwrap(),
            crate::export::envelope(&plain.result).unwrap()
        );
    }

    #[test]
    fn a_queued_job_cancels_alone_and_never_starts() {
        let path = fixture("tiny_duration.wav");
        let application = Application::new();
        // Hold the active slot with a reservation that has not run yet.
        let gate_token = CancellationToken::new();
        let gate = application.reserve(&gate_token).unwrap();

        let (started_tx, started_rx) = mpsc::channel::<()>();
        let started_tx = Mutex::new(started_tx);
        let wake: Wake = Arc::new(move || {
            let _ = started_tx.lock().unwrap().send(());
        });
        let queued = AnalysisJob::start(&application, Request::File(path), false, 1, wake);
        assert!(
            started_rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "a job queued behind an active reservation must not emit progress"
        );
        queued.cancel();
        drop(gate);
        let finished = wait(&queued).0;
        assert!(matches!(
            finished.result,
            JobResult::Error(AnalysisError {
                code: ErrorCode::Cancelled,
                ..
            })
        ));
        assert!(!gate_token.is_cancelled());
    }

    #[test]
    fn a_batch_streams_every_item_before_finishing_in_stable_order() {
        let application = Application::new();
        let inputs = vec![
            fixture("tiny_duration.wav"),
            fixture("native-alac-v1/alac16-mono-44100.m4a"),
        ];
        let collector = BatchReportCollector::new();
        let token = CancellationToken::new();
        let completion = application
            .reserve(&token)
            .unwrap()
            .run_batch(
                BatchRequest::new(inputs.clone(), false),
                &collector,
                &NoopProgressSink,
            )
            .unwrap();
        let expected = collector.finish(completion).unwrap();

        for timing in [false, true] {
            let job = AnalysisJob::start(
                &application,
                Request::Batch {
                    inputs: inputs.clone(),
                    recursive: false,
                },
                timing,
                2,
                no_wake(),
            );
            let (finished, mut rows, snapshot) = wait(&job);
            rows.sort_by_key(|row| row.index);
            let JobResult::Batch {
                details,
                completion: terminal,
            } = finished.result
            else {
                panic!("a two-file batch must return a completed detail store");
            };
            assert_eq!(terminal, completion);
            assert_eq!(finished.timings.is_some(), timing);
            assert_eq!(rows.len(), expected.items.len());
            for (index, row) in rows.into_iter().enumerate() {
                assert_eq!(row.index, index);
                assert_eq!(row.item, expected.items[index].summary());
                assert_eq!(
                    *details.detail(index).unwrap().unwrap(),
                    expected.items[index]
                );
            }
            assert_eq!(details.report().unwrap(), expected);
            assert!((snapshot.fraction - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn a_panicking_worker_publishes_an_error_and_releases_the_reservation() {
        let application = Application::new();
        let path = fixture("tiny_duration.wav");
        let fail = AtomicBool::new(true);
        let (woke, wakeups) = mpsc::channel();
        let wake: Wake = Arc::new(move || {
            assert!(
                !fail.swap(false, Ordering::Relaxed),
                "injected progress panic"
            );
            let _ = woke.send(());
        });
        let job = AnalysisJob::start(&application, Request::File(path.clone()), false, 1, wake);
        wakeups.recv_timeout(Duration::from_secs(5)).unwrap();
        let result = job
            .poll()
            .finished
            .expect("terminal state published before wake");
        assert!(matches!(
            result.result,
            JobResult::Error(AnalysisError {
                code: ErrorCode::Internal,
                ..
            })
        ));

        let next = AnalysisJob::start(&application, Request::File(path), false, 1, no_wake());
        assert!(matches!(wait(&next).0.result, JobResult::Analysis(_)));
    }

    #[test]
    fn discovery_uses_the_capability_catalog() {
        let application = Application::new();
        let job = DiscoveryJob::start(
            &application,
            vec![fixture("native-alac-v1")],
            false,
            no_wake(),
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        let files = loop {
            if let Some(result) = job.take_result() {
                break result.unwrap();
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        };
        assert!(!files.is_empty());
        let extensions = capabilities().stable_discovery_extensions;
        for file in files {
            let extension = file.extension().unwrap().to_string_lossy().to_lowercase();
            assert!(extensions.contains(&extension), "{}", file.display());
        }
    }
}
