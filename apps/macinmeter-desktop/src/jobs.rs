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
    AnalysisError, AnalysisEvent, AnalysisStage, AnalyzeRequest, Application, ApplicationJob,
    BatchItem, BatchRequest, CancellationToken, ErrorCode, NoopProgressSink, PhaseTimings,
    ProgressSink, WireEnvelope,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    thread::{self, JoinHandle},
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
pub struct Finished {
    pub envelope: WireEnvelope,
    pub timings: Option<PhaseTimings>,
    pub elapsed: Duration,
}

#[derive(Debug)]
struct State {
    phase: Phase,
    progress: BatchProgress,
    current: Option<(usize, String)>,
    streamed: Vec<(usize, BatchItem)>,
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
                AnalysisEvent::BatchItemFinished { index, item } => {
                    state.streamed.push((index, item));
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
    thread: Option<JoinHandle<()>>,
}

impl AnalysisJob {
    /// Reserves first, then spawns. A refused reservation or spawn becomes an
    /// already-finished job carrying the error envelope, so the UI has one
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
            streamed: Vec::new(),
            finished: None,
        }));
        let mut job = Self {
            token: token.clone(),
            state: Arc::clone(&state),
            thread: None,
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
        let spawned = thread::Builder::new()
            .name("macinmeter-analysis-job".to_owned())
            .spawn(move || {
                let (envelope, timings) = execute(reservation, request, timing, &sink);
                lock(&state).finished = Some(Finished {
                    envelope,
                    timings,
                    elapsed: started.elapsed(),
                });
                (wake)();
            });
        match spawned {
            // A failed spawn drops the closure, and with it the reservation.
            Ok(handle) => job.thread = Some(handle),
            Err(error) => job.finish_now(
                internal_error(format!("analysis thread failed to start: {error}")),
                started,
            ),
        }
        job
    }

    fn finish_now(&mut self, error: AnalysisError, started: Instant) {
        lock(&self.state).finished = Some(Finished {
            envelope: WireEnvelope::error(error),
            timings: None,
            elapsed: started.elapsed(),
        });
    }

    pub fn cancel(&self) {
        self.token.cancel();
    }

    pub fn snapshot(&self) -> Snapshot {
        let state = lock(&self.state);
        Snapshot {
            phase: state.phase,
            total: state.progress.total(),
            fraction: state.progress.fraction(),
            current: state.current.clone(),
        }
    }

    /// Batch items completed since the last call, in completion order.
    pub fn take_streamed(&self) -> Vec<(usize, BatchItem)> {
        std::mem::take(&mut lock(&self.state).streamed)
    }

    pub fn take_finished(&mut self) -> Option<Finished> {
        let finished = lock(&self.state).finished.take()?;
        if let Some(handle) = self.thread.take() {
            // The result is already published; the thread is only returning.
            let _ = handle.join();
        }
        Some(finished)
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
    sink: &dyn ProgressSink,
) -> (WireEnvelope, Option<PhaseTimings>) {
    match request {
        Request::File(path) => {
            let request = AnalyzeRequest::new(path);
            if timing {
                match job.analyze_file_timed(request, sink) {
                    Ok((report, timings)) => (WireEnvelope::analysis(report), Some(timings)),
                    Err(error) => (WireEnvelope::error(error), None),
                }
            } else {
                let envelope = job
                    .analyze_file(request, sink)
                    .map_or_else(WireEnvelope::error, WireEnvelope::analysis);
                (envelope, None)
            }
        }
        Request::Batch { inputs, recursive } => {
            let request = BatchRequest::new(inputs, recursive);
            if timing {
                match job.run_batch_timed(request, sink) {
                    Ok((report, timings)) => (WireEnvelope::batch(report), Some(timings)),
                    Err(error) => (WireEnvelope::error(error), None),
                }
            } else {
                let envelope = job
                    .run_batch(request, sink)
                    .map_or_else(WireEnvelope::error, WireEnvelope::batch);
                (envelope, None)
            }
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
                let found = reservation.discover_inputs(&inputs, recursive, &NoopProgressSink);
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

fn internal_error(message: String) -> AnalysisError {
    AnalysisError::new(ErrorCode::Internal, AnalysisStage::Internal, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use macinmeter::{WirePayload, capabilities};
    use std::{path::Path, sync::mpsc};

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name)
    }

    fn no_wake() -> Wake {
        Arc::new(|| {})
    }

    fn wait(job: &mut AnalysisJob) -> Finished {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(finished) = job.take_finished() {
                return finished;
            }
            assert!(Instant::now() < deadline, "job did not finish");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_file_job_returns_exactly_the_application_report() {
        let path = fixture("native-alac-v1/alac16-mono-44100.m4a");
        let application = Application::new();
        let mut job = AnalysisJob::start(
            &application,
            Request::File(path.clone()),
            false,
            1,
            no_wake(),
        );
        let finished = wait(&mut job);
        let direct = Application::new()
            .analyze_file(AnalyzeRequest::new(path))
            .map_or_else(WireEnvelope::error, WireEnvelope::analysis);
        assert!(matches!(
            finished.envelope.payload,
            WirePayload::Analysis(_)
        ));
        assert_eq!(finished.envelope, direct);
        assert!(finished.timings.is_none());
    }

    #[test]
    fn timing_is_reported_beside_the_envelope_not_inside_it() {
        let path = fixture("tiny_duration.wav");
        let application = Application::new();
        let mut timed = AnalysisJob::start(
            &application,
            Request::File(path.clone()),
            true,
            1,
            no_wake(),
        );
        let timed = wait(&mut timed);
        let mut plain = AnalysisJob::start(&application, Request::File(path), false, 1, no_wake());
        let plain = wait(&mut plain);
        assert!(timed.timings.is_some());
        assert_eq!(timed.envelope, plain.envelope);
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
        let mut queued = AnalysisJob::start(&application, Request::File(path), false, 1, wake);
        assert!(
            started_rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "a job queued behind an active reservation must not emit progress"
        );
        queued.cancel();
        drop(gate);
        let finished = wait(&mut queued);
        assert!(matches!(
            finished.envelope.payload,
            WirePayload::Error(AnalysisError {
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
        let mut job = AnalysisJob::start(
            &application,
            Request::Batch {
                inputs,
                recursive: false,
            },
            false,
            2,
            no_wake(),
        );
        let finished = wait(&mut job);
        let mut streamed = job.take_streamed();
        streamed.sort_by_key(|(index, _)| *index);
        let WirePayload::Batch(batch) = &finished.envelope.payload else {
            panic!("a two-file batch must return a batch envelope");
        };
        assert_eq!(batch.items.len(), 2);
        let streamed: Vec<_> = streamed.into_iter().map(|(_, item)| item).collect();
        assert_eq!(streamed, batch.items);
        assert!((job.snapshot().fraction - 1.0).abs() < 1e-12);
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
