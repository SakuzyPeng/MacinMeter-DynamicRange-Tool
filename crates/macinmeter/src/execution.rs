use crate::{
    AnalysisError, AnalysisReport, AnalysisStage, AnalyzeRequest, BatchReport, BatchRequest,
    CancellationToken, ErrorCode, ExecutionControl, NoopProgressSink, ProgressSink,
    application::Analyzer,
    batch::{BatchRunner, discover_inputs_with_control},
    concurrency::{ConcurrencyPlan, PlanAllocation},
};
use std::{
    collections::VecDeque,
    num::NonZeroUsize,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::Duration,
};

const SERIAL_ACTIVE_JOBS: usize = 1;
const DEFAULT_MAX_QUEUED_JOBS: usize = 64;
const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Batch items still run one at a time. ADR-0014 admits file lanes as P1, after
/// packet-level decoding.
const PRODUCTION_FILE_LANES: NonZeroUsize = NonZeroUsize::MIN;

/// Decode workers the product asks for before host and ceiling clamping.
///
/// ADR-0014 caps this in `domain`; asking for the ceiling means the granted
/// count is whichever of the ceiling and the host's parallelism is smaller.
const PRODUCTION_DECODE_WORKERS: NonZeroUsize =
    match NonZeroUsize::new(macinmeter_domain::MAX_DECODE_WORKERS) {
        Some(workers) => workers,
        None => NonZeroUsize::MIN,
    };

/// The process-local application execution budget.
///
/// The queue bound limits work admitted by adapters before it enters their
/// blocking thread pool; it does not claim to be a byte-accurate decoder memory
/// quota. The internal `ConcurrencyPlan` is the separate bound on the workers
/// and memory one admitted job may spend.
///
/// [`ExecutionBudget::product`] draws that plan from the host, which is what
/// enables the graduated ALAC packet workers. [`ExecutionBudget::serial`] keeps
/// the fully serial plan and stays available as the differential reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionBudget {
    max_queued_jobs: usize,
    concurrency: ConcurrencyPlan,
}

impl ExecutionBudget {
    /// The product default: one active job, at most 64 queued jobs, and a
    /// bounded internal plan drawn from the host.
    ///
    /// The plan only changes which engine a graduated route may select; it
    /// never changes a result. Routes that have not graduated, and hosts that
    /// grant a single worker, stay on the serial engine.
    pub fn product() -> Self {
        Self {
            max_queued_jobs: DEFAULT_MAX_QUEUED_JOBS,
            concurrency: ConcurrencyPlan::bounded(PRODUCTION_DECODE_WORKERS),
        }
    }

    /// One active job, at most 64 queued jobs, and a fully serial plan.
    ///
    /// This is the differential reference every parallel axis is graduated
    /// against, so it stays reachable rather than becoming an alias of the
    /// product budget.
    pub const fn serial() -> Self {
        Self {
            max_queued_jobs: DEFAULT_MAX_QUEUED_JOBS,
            concurrency: ConcurrencyPlan::serial(),
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
        Ok(Self {
            max_queued_jobs,
            concurrency: ConcurrencyPlan::serial(),
        })
    }

    pub const fn max_active_jobs(self) -> usize {
        SERIAL_ACTIVE_JOBS
    }

    pub const fn max_queued_jobs(self) -> usize {
        self.max_queued_jobs
    }

    /// The internal worker and memory plan one active job draws from.
    pub(crate) const fn concurrency(self) -> ConcurrencyPlan {
        self.concurrency
    }

    /// Replace the internal plan.
    ///
    /// ADR-0014 keeps every parallel axis off by default, so no public
    /// constructor can reach this. It exists for the first-party differential
    /// tests that have to drive a non-serial plan through the real
    /// `Application` path rather than through a mirrored constant.
    #[cfg(test)]
    pub(crate) const fn with_concurrency(self, concurrency: ConcurrencyPlan) -> Self {
        Self {
            concurrency,
            ..self
        }
    }

    fn max_admitted_jobs(self) -> usize {
        SERIAL_ACTIVE_JOBS + self.max_queued_jobs
    }
}

impl Default for ExecutionBudget {
    fn default() -> Self {
        Self::product()
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
    allocation: PlanAllocation,
}

impl ApplicationJob {
    /// The permits this job received before any sub-task was scheduled.
    ///
    /// Permits are granted once, up front. Nothing below this job asks for a
    /// second permit, which is what keeps nested pools from deadlocking.
    pub(crate) const fn allocation(&self) -> PlanAllocation {
        self.allocation
    }

    pub fn analyze_file(
        self,
        request: AnalyzeRequest,
        progress: &dyn ProgressSink,
    ) -> Result<AnalysisReport, AnalysisError> {
        let decode = self.allocation().decode();
        self.execute(progress, |control| {
            Analyzer::new(decode).analyze_file_with_control(request, control)
        })
    }

    pub fn run_batch(
        self,
        request: BatchRequest,
        progress: &dyn ProgressSink,
    ) -> Result<BatchReport, AnalysisError> {
        let allocation = self.allocation();
        self.execute(progress, |control| {
            BatchRunner::new(allocation).run(request, control)
        })
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

        // Every permit this job may ever spend is granted here, up front and
        // outside the admission lock. Nothing below it asks for a second one.
        let allocation = self
            .inner
            .budget
            .concurrency()
            .allocate(PRODUCTION_FILE_LANES)?;

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
            allocation,
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

    const TEST_HOST_WORKERS: NonZeroUsize = NonZeroUsize::new(8).unwrap();

    /// A budget built by the production bound derivation with a fixed host
    /// ceiling, so non-serial coverage does not depend on the test runner.
    fn bounded_budget(requested_workers: usize) -> ExecutionBudget {
        ExecutionBudget::serial().with_concurrency(ConcurrencyPlan::bounded_for_test(
            NonZeroUsize::new(requested_workers).unwrap(),
            TEST_HOST_WORKERS,
        ))
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name)
    }

    fn wire_bytes(application: &Application, name: &str) -> Vec<u8> {
        let report = application
            .analyze_file(AnalyzeRequest::new(fixture(name)))
            .unwrap_or_else(|error| panic!("{name} must analyze: {error}"));
        serde_json::to_vec(&crate::WireEnvelope::analysis(report))
            .expect("the wire envelope must serialize")
    }

    fn last_execution() -> macinmeter_codecs::DecodeExecution {
        crate::application::LAST_DECODE_EXECUTION
            .with(std::cell::Cell::get)
            .expect("an analysis must have recorded its decode execution")
    }

    #[test]
    fn a_non_serial_plan_reaches_the_decoder_through_the_application_path() {
        // The fixed host ceiling makes this a deterministic non-serial test of
        // what `Application` actually hands down for a bounded plan.
        let budget = bounded_budget(8);
        let plan_workers = budget.concurrency().total_workers().get();
        let job = Application::with_budget(budget)
            .reserve(&CancellationToken::new())
            .unwrap();
        let allocation = job.allocation();

        assert_eq!(allocation.file_lanes().get(), 1, "batch items stay serial");
        let decode = allocation.decode();
        assert_eq!(plan_workers, TEST_HOST_WORKERS.get());
        assert_eq!(decode.workers().get(), plan_workers);
        assert!(!decode.is_serial());
        assert_eq!(decode.queue_capacity().get(), plan_workers * 4);
        assert_eq!(
            decode.max_in_flight_pcm_bytes(),
            plan_workers as u64 * 4 * 1024 * 1024
        );
    }

    /// The engine a fixture is expected to select once workers are granted.
    ///
    /// Graduated routes are named one by one rather than inferred, which is the
    /// same rule the decoder itself follows.
    fn graduated_engine(name: &str) -> Option<macinmeter_codecs::DecodeEngineKind> {
        if name.starts_with("native-alac-v1/") {
            Some(macinmeter_codecs::DecodeEngineKind::AlacPacketWorkers)
        } else if name.ends_with(".flac") {
            Some(macinmeter_codecs::DecodeEngineKind::FlacPacketWorkers)
        } else {
            None
        }
    }

    #[test]
    fn the_product_default_selects_packet_workers_only_for_graduated_routes() {
        // The point of enabling the default plan is that the graduated routes
        // actually use it. Equality alone would also hold if nothing did.
        let granted = Application::new()
            .budget()
            .concurrency()
            .total_workers()
            .get();
        for name in [
            "native-alac-v1/alac16-stereo-48000-multipacket.m4a",
            "native-pcm-v1/flac-pcm-s16-stereo-multiblock.flac",
            "native-pcm-v1/wav-pcm-s16-stereo.wav",
            "native-pcm-v1/aiff-pcm-s24-stereo.aiff",
        ] {
            let expected = wire_bytes(&Application::with_budget(ExecutionBudget::serial()), name);
            assert_eq!(
                wire_bytes(&Application::new(), name),
                expected,
                "{name} changed under the product default"
            );

            let execution = last_execution();
            match graduated_engine(name).filter(|_| granted > 1) {
                Some(engine) => {
                    assert_eq!(
                        execution.engine(),
                        engine,
                        "{name} did not use packet workers under the product default"
                    );
                    assert_eq!(execution.workers().get(), granted);
                }
                None => {
                    assert_eq!(
                        execution.engine(),
                        macinmeter_codecs::DecodeEngineKind::Serial,
                        "{name} must not start packet workers"
                    );
                    assert_eq!(execution.workers().get(), 1);
                }
            }
        }
    }

    #[test]
    fn the_application_path_reports_identically_under_a_non_serial_plan() {
        // Only a graduated route may let the plan change the engine. The
        // others must be unaffected, which is what keeps enabling the plan from
        // leaking into routes that never graduated.
        let serial = Application::with_budget(ExecutionBudget::serial());
        for name in [
            "native-alac-v1/alac16-stereo-48000-multipacket.m4a",
            "native-alac-v1/alac24-8ch-48000.mp4",
            "native-pcm-v1/wav-pcm-s16-stereo.wav",
            "native-pcm-v1/flac-pcm-s16-stereo-multiblock.flac",
            "native-pcm-v1/aiff-pcm-s24-stereo.aiff",
        ] {
            let expected = wire_bytes(&serial, name);
            let serial_execution = last_execution();
            assert_eq!(
                serial_execution.engine(),
                macinmeter_codecs::DecodeEngineKind::Serial,
                "{name} must use the serial engine under the product plan"
            );
            assert_eq!(
                serial_execution.workers().get(),
                1,
                "{name} must use one worker under the product plan"
            );

            for requested_workers in [2, 4, 8] {
                let budget = bounded_budget(requested_workers);
                let granted = budget.concurrency().total_workers().get();
                assert_eq!(granted, requested_workers);
                let bounded = Application::with_budget(budget);
                assert_eq!(
                    wire_bytes(&bounded, name),
                    expected,
                    "{name} changed under a {requested_workers}-worker plan"
                );

                // Only a graduated route may switch engines. Every other route
                // must keep both its serial engine and its single worker.
                let expected_engine =
                    graduated_engine(name).unwrap_or(macinmeter_codecs::DecodeEngineKind::Serial);
                let expected_workers = if graduated_engine(name).is_some() {
                    granted
                } else {
                    1
                };
                let execution = last_execution();
                assert_eq!(
                    execution.engine(),
                    expected_engine,
                    "{name} selected an unexpected engine on {granted} granted workers"
                );
                assert_eq!(
                    execution.workers().get(),
                    expected_workers,
                    "{name} used an unexpected worker count on {granted} granted workers"
                );
            }
        }
    }

    fn run_test_job(
        job: ApplicationJob,
        operation: impl FnOnce() -> Result<(), AnalysisError>,
    ) -> Result<(), AnalysisError> {
        let progress = NoopProgressSink;
        job.execute(&progress, |_| operation())
    }

    #[test]
    fn the_product_budget_draws_its_plan_from_the_host_within_the_ceiling() {
        let host = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
        let expected = host.min(macinmeter_domain::MAX_DECODE_WORKERS);
        let application = Application::new();
        assert_eq!(
            application.budget().concurrency().total_workers().get(),
            expected,
            "the product plan is the smaller of the host and the fixed ceiling"
        );

        let job = application.reserve(&CancellationToken::new()).unwrap();
        let allocation = job.allocation();
        assert_eq!(
            allocation.file_lanes().get(),
            1,
            "file lanes are ADR-0014 P1 and remain unimplemented"
        );

        let decode = allocation.decode();
        assert_eq!(decode.workers().get(), expected);
        if expected == 1 {
            assert!(
                decode.is_serial(),
                "a single-worker host degrades to serial"
            );
            assert_eq!(decode.max_in_flight_pcm_bytes(), 0);
        } else {
            assert_eq!(decode.queue_capacity().get(), expected * 4);
            assert_eq!(
                decode.max_in_flight_pcm_bytes(),
                expected as u64 * 4 * 1024 * 1024
            );
        }
    }

    #[test]
    fn an_explicit_serial_budget_stays_fully_serial() {
        // The serial plan is the differential reference, so enabling the
        // product default must not turn it into an alias of that default.
        let application = Application::with_budget(ExecutionBudget::serial());
        assert!(application.budget().concurrency().is_serial());
        let decode = application
            .reserve(&CancellationToken::new())
            .unwrap()
            .allocation()
            .decode();
        assert!(decode.is_serial());
        assert_eq!(decode.workers().get(), 1);
        assert_eq!(decode.queue_capacity().get(), 1);
        assert_eq!(
            decode.max_in_flight_pcm_bytes(),
            0,
            "a serial route may never leave decoded PCM waiting on an earlier index"
        );
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
