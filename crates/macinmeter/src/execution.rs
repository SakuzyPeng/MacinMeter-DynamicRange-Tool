use crate::{
    AnalysisError, AnalysisReport, AnalysisStage, AnalyzeRequest, BatchReport, BatchRequest,
    CancellationToken, ErrorCode, ExecutionControl, NoopProgressSink, ProgressSink,
    application::Analyzer,
    batch::{BatchRunner, discover_inputs_with_control},
};
use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::Duration,
};

const SERIAL_ACTIVE_JOBS: usize = 1;
const DEFAULT_MAX_QUEUED_JOBS: usize = 64;
const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// The process-local application execution budget.
///
/// M3 deliberately exposes only a serial policy. The queue bound limits work
/// admitted by adapters before it enters their blocking thread pool; it does
/// not claim to be a byte-accurate decoder memory quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionBudget {
    max_queued_jobs: usize,
}

impl ExecutionBudget {
    /// The product default: one active job and at most 64 queued jobs.
    pub const fn serial() -> Self {
        Self {
            max_queued_jobs: DEFAULT_MAX_QUEUED_JOBS,
        }
    }

    /// Build a serial budget with an explicit queue bound.
    ///
    /// A zero queue bound still permits the one active reservation.
    pub fn serial_with_queue_capacity(max_queued_jobs: usize) -> Result<Self, AnalysisError> {
        SERIAL_ACTIVE_JOBS
            .checked_add(max_queued_jobs)
            .ok_or_else(|| {
                AnalysisError::new(
                    ErrorCode::InvalidRequest,
                    AnalysisStage::Validation,
                    "execution queue capacity is too large",
                )
            })?;
        Ok(Self { max_queued_jobs })
    }

    pub const fn max_active_jobs(self) -> usize {
        SERIAL_ACTIVE_JOBS
    }

    pub const fn max_queued_jobs(self) -> usize {
        self.max_queued_jobs
    }

    fn max_admitted_jobs(self) -> usize {
        SERIAL_ACTIVE_JOBS + self.max_queued_jobs
    }
}

impl Default for ExecutionBudget {
    fn default() -> Self {
        Self::serial()
    }
}

/// A cloneable application façade whose clones share one execution budget.
///
/// Adapters should keep one `Application` per process/runtime and reserve a
/// job before submitting blocking work. This makes admission order explicit
/// and prevents the adapter thread pool from becoming an unbounded hidden
/// scheduler.
#[derive(Debug, Clone)]
pub struct Application {
    coordinator: ExecutionCoordinator,
}

impl Application {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_budget(budget: ExecutionBudget) -> Self {
        Self {
            coordinator: ExecutionCoordinator::new(budget),
        }
    }

    pub fn budget(&self) -> ExecutionBudget {
        self.coordinator.budget()
    }

    /// Reserve one bounded application job before entering a blocking worker.
    pub fn reserve(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<ApplicationJob, AnalysisError> {
        self.coordinator.reserve(cancellation)
    }

    pub fn analyze_file(&self, request: AnalyzeRequest) -> Result<AnalysisReport, AnalysisError> {
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;
        self.analyze_file_with_control(request, &ExecutionControl::new(&cancellation, &progress))
    }

    pub fn analyze_file_with_control(
        &self,
        request: AnalyzeRequest,
        control: &ExecutionControl<'_>,
    ) -> Result<AnalysisReport, AnalysisError> {
        self.reserve(control.cancellation)?
            .analyze_file(request, control.progress)
    }

    pub fn run_batch(
        &self,
        request: BatchRequest,
        control: &ExecutionControl<'_>,
    ) -> Result<BatchReport, AnalysisError> {
        self.reserve(control.cancellation)?
            .run_batch(request, control.progress)
    }

    pub fn discover_inputs_with_control(
        &self,
        inputs: &[std::path::PathBuf],
        recursive: bool,
        control: &ExecutionControl<'_>,
    ) -> Result<Vec<std::path::PathBuf>, AnalysisError> {
        self.reserve(control.cancellation)?
            .discover_inputs(inputs, recursive, control.progress)
    }

    pub fn discover_inputs(
        &self,
        inputs: &[std::path::PathBuf],
        recursive: bool,
    ) -> Result<Vec<std::path::PathBuf>, AnalysisError> {
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;
        self.discover_inputs_with_control(
            inputs,
            recursive,
            &ExecutionControl::new(&cancellation, &progress),
        )
    }
}

impl Default for Application {
    fn default() -> Self {
        Self::with_budget(ExecutionBudget::default())
    }
}

/// A single-use, bounded reservation for one top-level application operation.
///
/// Dropping a reservation before it starts removes it from the FIFO. Dropping
/// it while active releases the serial slot. This remains true during stack
/// unwinding.
#[derive(Debug)]
pub struct ApplicationJob {
    reservation: ExecutionReservation,
}

impl ApplicationJob {
    pub fn analyze_file(
        self,
        request: AnalyzeRequest,
        progress: &dyn ProgressSink,
    ) -> Result<AnalysisReport, AnalysisError> {
        self.execute(progress, |control| {
            Analyzer::new().analyze_file_with_control(request, control)
        })
    }

    pub fn run_batch(
        self,
        request: BatchRequest,
        progress: &dyn ProgressSink,
    ) -> Result<BatchReport, AnalysisError> {
        self.execute(progress, |control| BatchRunner::new().run(request, control))
    }

    pub fn discover_inputs(
        self,
        inputs: &[std::path::PathBuf],
        recursive: bool,
        progress: &dyn ProgressSink,
    ) -> Result<Vec<std::path::PathBuf>, AnalysisError> {
        self.execute(progress, |control| {
            discover_inputs_with_control(inputs, recursive, control)
        })
    }

    fn execute<T>(
        mut self,
        progress: &dyn ProgressSink,
        operation: impl FnOnce(&ExecutionControl<'_>) -> Result<T, AnalysisError>,
    ) -> Result<T, AnalysisError> {
        self.reservation.activate()?;
        let control = ExecutionControl::new(&self.reservation.cancellation, progress);
        operation(&control)
    }
}

#[derive(Debug, Clone)]
struct ExecutionCoordinator {
    inner: Arc<CoordinatorInner>,
}

impl ExecutionCoordinator {
    fn new(budget: ExecutionBudget) -> Self {
        Self {
            inner: Arc::new(CoordinatorInner {
                budget,
                state: Mutex::new(CoordinatorState::default()),
                changed: Condvar::new(),
            }),
        }
    }

    fn budget(&self) -> ExecutionBudget {
        self.inner.budget
    }

    fn reserve(&self, cancellation: &CancellationToken) -> Result<ApplicationJob, AnalysisError> {
        if cancellation.is_cancelled() {
            return Err(AnalysisError::cancelled());
        }

        let mut state = self.inner.lock_state()?;
        if cancellation.is_cancelled() {
            return Err(AnalysisError::cancelled());
        }
        if state.admitted_jobs() >= self.inner.budget.max_admitted_jobs() {
            return Err(AnalysisError::new(
                ErrorCode::ResourceExhausted,
                AnalysisStage::Validation,
                "application execution queue is full",
            )
            .with_details(format!(
                "max_active_jobs={}; max_queued_jobs={}",
                self.inner.budget.max_active_jobs(),
                self.inner.budget.max_queued_jobs()
            ))
            .recoverable(true));
        }

        let ticket = state.next_ticket;
        state.next_ticket = state.next_ticket.checked_add(1).ok_or_else(|| {
            AnalysisError::new(
                ErrorCode::ResourceExhausted,
                AnalysisStage::Internal,
                "application execution ticket space was exhausted",
            )
        })?;
        state.queue.push_back(ticket);
        drop(state);
        self.inner.changed.notify_all();

        Ok(ApplicationJob {
            reservation: ExecutionReservation {
                inner: Arc::clone(&self.inner),
                cancellation: cancellation.clone(),
                ticket,
                phase: ReservationPhase::Queued,
            },
        })
    }
}

#[derive(Debug)]
struct CoordinatorInner {
    budget: ExecutionBudget,
    state: Mutex<CoordinatorState>,
    changed: Condvar,
}

impl CoordinatorInner {
    fn lock_state(&self) -> Result<MutexGuard<'_, CoordinatorState>, AnalysisError> {
        self.state.lock().map_err(|_| {
            AnalysisError::new(
                ErrorCode::Internal,
                AnalysisStage::Internal,
                "application execution coordinator is poisoned",
            )
        })
    }
}

#[derive(Debug, Default)]
struct CoordinatorState {
    active_jobs: usize,
    next_ticket: u64,
    queue: VecDeque<u64>,
}

impl CoordinatorState {
    fn admitted_jobs(&self) -> usize {
        self.active_jobs + self.queue.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReservationPhase {
    Queued,
    Active,
}

#[derive(Debug)]
struct ExecutionReservation {
    inner: Arc<CoordinatorInner>,
    cancellation: CancellationToken,
    ticket: u64,
    phase: ReservationPhase,
}

impl ExecutionReservation {
    fn activate(&mut self) -> Result<(), AnalysisError> {
        let mut state = self.inner.lock_state()?;
        loop {
            if self.cancellation.is_cancelled() {
                return Err(AnalysisError::cancelled());
            }

            if state.active_jobs < self.inner.budget.max_active_jobs()
                && state.queue.front() == Some(&self.ticket)
            {
                let admitted = state.queue.pop_front();
                debug_assert_eq!(admitted, Some(self.ticket));
                state.active_jobs += 1;
                self.phase = ReservationPhase::Active;
                return Ok(());
            }

            let waited = self
                .inner
                .changed
                .wait_timeout(state, CANCELLATION_POLL_INTERVAL)
                .map_err(|_| {
                    AnalysisError::new(
                        ErrorCode::Internal,
                        AnalysisStage::Internal,
                        "application execution coordinator is poisoned",
                    )
                })?;
            state = waited.0;
        }
    }
}

impl Drop for ExecutionReservation {
    fn drop(&mut self) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match self.phase {
            ReservationPhase::Queued => {
                if let Some(index) = state.queue.iter().position(|ticket| *ticket == self.ticket) {
                    state.queue.remove(index);
                }
            }
            ReservationPhase::Active => {
                debug_assert!(state.active_jobs > 0);
                state.active_jobs = state.active_jobs.saturating_sub(1);
            }
        }
        drop(state);
        self.inner.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        sync::mpsc,
        thread,
    };

    fn run_test_job(
        job: ApplicationJob,
        operation: impl FnOnce() -> Result<(), AnalysisError>,
    ) -> Result<(), AnalysisError> {
        let progress = NoopProgressSink;
        job.execute(&progress, |_| operation())
    }

    #[test]
    fn reservations_execute_in_fifo_order_with_one_active_job() {
        let application =
            Application::with_budget(ExecutionBudget::serial_with_queue_capacity(3).unwrap());
        let first = application.reserve(&CancellationToken::new()).unwrap();
        let second = application.reserve(&CancellationToken::new()).unwrap();
        let third = application.reserve(&CancellationToken::new()).unwrap();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();

        let third_tx = entered_tx.clone();
        let third_thread = thread::spawn(move || {
            run_test_job(third, || {
                third_tx.send(3).unwrap();
                Ok(())
            })
        });
        let second_tx = entered_tx.clone();
        let second_thread = thread::spawn(move || {
            run_test_job(second, || {
                second_tx.send(2).unwrap();
                Ok(())
            })
        });
        let first_thread = thread::spawn(move || {
            run_test_job(first, || {
                entered_tx.send(1).unwrap();
                release_first_rx.recv().unwrap();
                Ok(())
            })
        });

        assert_eq!(entered_rx.recv_timeout(Duration::from_secs(1)).unwrap(), 1);
        assert!(entered_rx.recv_timeout(Duration::from_millis(100)).is_err());
        release_first_tx.send(()).unwrap();
        assert_eq!(entered_rx.recv_timeout(Duration::from_secs(1)).unwrap(), 2);
        assert_eq!(entered_rx.recv_timeout(Duration::from_secs(1)).unwrap(), 3);

        first_thread.join().unwrap().unwrap();
        second_thread.join().unwrap().unwrap();
        third_thread.join().unwrap().unwrap();
    }

    #[test]
    fn cancelling_a_queued_reservation_does_not_cancel_other_jobs() {
        let application =
            Application::with_budget(ExecutionBudget::serial_with_queue_capacity(2).unwrap());
        let first_token = CancellationToken::new();
        let second_token = CancellationToken::new();
        let third_token = CancellationToken::new();
        let first = application.reserve(&first_token).unwrap();
        let second = application.reserve(&second_token).unwrap();
        let third = application.reserve(&third_token).unwrap();
        let (first_entered_tx, first_entered_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();
        let (third_entered_tx, third_entered_rx) = mpsc::channel();

        let first_thread = thread::spawn(move || {
            run_test_job(first, || {
                first_entered_tx.send(()).unwrap();
                release_first_rx.recv().unwrap();
                Ok(())
            })
        });
        first_entered_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap();

        let second_thread = thread::spawn(move || run_test_job(second, || Ok(())));
        let third_thread = thread::spawn(move || {
            run_test_job(third, || {
                third_entered_tx.send(()).unwrap();
                Ok(())
            })
        });

        second_token.cancel();
        let error = second_thread
            .join()
            .unwrap()
            .expect_err("queued cancellation must fail the cancelled job");
        assert_eq!(error.code, ErrorCode::Cancelled);
        assert!(!third_token.is_cancelled());
        release_first_tx.send(()).unwrap();
        third_entered_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap();

        first_thread.join().unwrap().unwrap();
        third_thread.join().unwrap().unwrap();
    }

    #[test]
    fn dropping_or_unwinding_a_reservation_releases_its_budget() {
        let application =
            Application::with_budget(ExecutionBudget::serial_with_queue_capacity(1).unwrap());
        let dropped = application.reserve(&CancellationToken::new()).unwrap();
        drop(dropped);

        let panicking = application.reserve(&CancellationToken::new()).unwrap();
        let unwind = catch_unwind(AssertUnwindSafe(|| {
            let _ = run_test_job(panicking, || panic!("intentional test panic"));
        }));
        assert!(unwind.is_err());

        let successor = application.reserve(&CancellationToken::new()).unwrap();
        run_test_job(successor, || Ok(())).unwrap();
    }

    #[test]
    fn admission_is_bounded_before_blocking_work_starts() {
        let application =
            Application::with_budget(ExecutionBudget::serial_with_queue_capacity(1).unwrap());
        let _first = application.reserve(&CancellationToken::new()).unwrap();
        let _second = application.reserve(&CancellationToken::new()).unwrap();

        let error = application
            .reserve(&CancellationToken::new())
            .expect_err("the third reservation must exceed one active plus one queued job");
        assert_eq!(error.code, ErrorCode::ResourceExhausted);
        assert_eq!(error.stage, AnalysisStage::Validation);
        assert!(error.recoverable);
    }

    #[test]
    fn cancellation_before_admission_does_not_consume_queue_capacity() {
        let application =
            Application::with_budget(ExecutionBudget::serial_with_queue_capacity(0).unwrap());
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        let error = application
            .reserve(&cancelled)
            .expect_err("pre-cancelled work must not enter the queue");
        assert_eq!(error.code, ErrorCode::Cancelled);

        let admitted = application.reserve(&CancellationToken::new()).unwrap();
        run_test_job(admitted, || Ok(())).unwrap();
    }

    #[test]
    fn application_clones_share_the_same_execution_domain() {
        let application =
            Application::with_budget(ExecutionBudget::serial_with_queue_capacity(0).unwrap());
        let clone = application.clone();
        let _admitted = application.reserve(&CancellationToken::new()).unwrap();

        let error = clone
            .reserve(&CancellationToken::new())
            .expect_err("a clone must not create an independent execution budget");
        assert_eq!(error.code, ErrorCode::ResourceExhausted);
    }
}
