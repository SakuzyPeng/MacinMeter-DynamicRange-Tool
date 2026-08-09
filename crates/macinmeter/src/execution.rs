#[cfg(feature = "performance-probes")]
use crate::application::ApplicationPerformanceProbe;
#[cfg(feature = "performance-probes")]
use crate::batch::BatchPerformanceProbe;
use crate::{
    AnalysisError, AnalysisReport, AnalysisStage, AnalyzeRequest, BatchReport, BatchRequest,
    CancellationToken, ErrorCode, ExecutionControl, NoopProgressSink, ProgressSink,
    application::{Analyzer, OverlapShape, PhaseTimings},
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

/// A fully serial plan has nothing to split, so it asks for one lane.
///
/// The product plan derives its width from the plan instead; see
/// [`ConcurrencyPlan::saturating_file_lanes`].
const SERIAL_FILE_LANES: NonZeroUsize = NonZeroUsize::MIN;

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
    file_lanes: NonZeroUsize,
    overlap_shape: OverlapShape,
}

impl ExecutionBudget {
    /// The product default: one active job, at most 64 queued jobs, and a
    /// bounded internal plan drawn from the host.
    ///
    /// The plan only changes which engine a graduated route may select; it
    /// never changes a result. Routes that have not graduated, and hosts that
    /// grant a single worker, stay on the serial engine.
    pub fn product() -> Self {
        let concurrency = ConcurrencyPlan::bounded(PRODUCTION_DECODE_WORKERS);
        Self {
            max_queued_jobs: DEFAULT_MAX_QUEUED_JOBS,
            concurrency,
            // ADR-0014 P1 graduated at the width the plan derives for itself:
            // the widest lane split that still grants each lane a packet pool.
            // A batch narrower than this asks for fewer lanes, and a single
            // file asks for one, so no operation pays for lanes it cannot use.
            file_lanes: concurrency.saturating_file_lanes(),
            overlap_shape: OverlapShape::DEFAULT,
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
            file_lanes: SERIAL_FILE_LANES,
            overlap_shape: OverlapShape::DEFAULT,
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
            file_lanes: SERIAL_FILE_LANES,
            overlap_shape: OverlapShape::DEFAULT,
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

    /// File lanes one admitted batch may request from that plan.
    pub(crate) const fn file_lanes(self) -> NonZeroUsize {
        self.file_lanes
    }

    /// The lane width this budget requests, as a number.
    ///
    /// Read-only, like the queue accessors above. The product derives this
    /// request from the host-bound plan; a batch may allocate fewer lanes after
    /// discovery, which the non-default batch performance probe records
    /// separately.
    pub const fn requested_file_lanes(self) -> usize {
        self.file_lanes.get()
    }

    /// Override the product-derived batch lane width for measurement.
    ///
    /// The lane count is a measurement input, not a product tuning knob:
    /// exposing it under the same non-default feature as the pipeline probes
    /// keeps the override out of the ordinary product surface. The plan still
    /// clamps the request and narrows each lane's decoder to pay for it.
    #[cfg(any(test, feature = "performance-probes"))]
    pub const fn with_file_lanes(self, file_lanes: NonZeroUsize) -> Self {
        Self { file_lanes, ..self }
    }

    /// How the decode-analysis hand-off is shaped.
    pub(crate) const fn overlap_shape(self) -> OverlapShape {
        self.overlap_shape
    }

    /// Override the graduated hand-off depth for measurement.
    ///
    /// Depth does not change the block sequence the analyzer sees, so this
    /// cannot move a result; it only trades retained PCM for how often the
    /// producer parks, and the retention it asks for is priced against the same
    /// in-flight allowance the route already holds. A depth that does not fit
    /// leaves the stream serial rather than overspending, so this is a
    /// measurement input under the non-default feature, not a product knob.
    #[cfg(any(test, feature = "performance-probes"))]
    pub fn with_overlap_shape(self, channel_depth: NonZeroUsize) -> Self {
        Self {
            overlap_shape: OverlapShape::new(channel_depth),
            ..self
        }
    }

    /// Request a decode worker count for the internal plan.
    ///
    /// The graduation gates require every axis to be compared across 1/2/4/8
    /// workers, and for decode-analysis overlap the worker count is not merely
    /// a scale: a one-worker plan leaves a serial route no spare permit, so the
    /// overlap cannot engage at all. Measuring that boundary needs the real
    /// `Application` path rather than a mirrored constant. The plan still
    /// clamps the request to the product ceiling and the host.
    #[cfg(any(test, feature = "performance-probes"))]
    pub fn with_decode_workers(self, requested: NonZeroUsize) -> Self {
        Self {
            concurrency: ConcurrencyPlan::bounded(requested),
            ..self
        }
    }

    /// Replace the internal plan.
    ///
    /// It exists for first-party differential tests that have to drive a fixed
    /// non-serial plan through the real `Application` path rather than depend on
    /// the test runner's host or a mirrored constant.
    #[cfg(test)]
    pub(crate) const fn with_concurrency(self, concurrency: ConcurrencyPlan) -> Self {
        Self {
            concurrency,
            // Re-derive the width with the plan. A budget whose lane count came
            // from a different plan than its own would not be a configuration
            // the product can produce.
            file_lanes: concurrency.saturating_file_lanes(),
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

    /// Analyze one file and return the exact non-default execution topology.
    ///
    /// This measurement entry is absent from ordinary product builds and does
    /// not add any field to `AnalysisReport` or the wire schema.
    #[cfg(feature = "performance-probes")]
    #[doc(hidden)]
    pub fn analyze_file_with_performance_probe(
        &self,
        request: AnalyzeRequest,
    ) -> Result<(AnalysisReport, ApplicationPerformanceProbe), AnalysisError> {
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;
        self.reserve(&cancellation)?
            .analyze_file_with_performance_probe(request, &progress)
    }

    pub fn analyze_file_with_control(
        &self,
        request: AnalyzeRequest,
        control: &ExecutionControl<'_>,
    ) -> Result<AnalysisReport, AnalysisError> {
        self.reserve(control.cancellation)?
            .analyze_file(request, control.progress)
    }

    /// Analyze one file and also report how long decode and analysis occupied.
    ///
    /// Timing is opt-in per call because every measured interval reads the
    /// clock at its start and stop: an ordinary data block costs two reads for
    /// decode and two for analysis. An ordinary `analyze_file_with_control`
    /// reads no phase clock, which keeps measurement runs observing the product
    /// rather than the observation. The report itself is identical either way:
    /// [`PhaseTimings`] never enters it or the wire schema.
    pub fn analyze_file_timed(
        &self,
        request: AnalyzeRequest,
        control: &ExecutionControl<'_>,
    ) -> Result<(AnalysisReport, PhaseTimings), AnalysisError> {
        self.reserve(control.cancellation)?
            .analyze_file_timed(request, control.progress)
    }

    pub fn run_batch(
        &self,
        request: BatchRequest,
        control: &ExecutionControl<'_>,
    ) -> Result<BatchReport, AnalysisError> {
        self.reserve(control.cancellation)?
            .run_batch(request, control.progress)
    }

    /// Run one batch and also report the totals its lanes accumulated.
    ///
    /// File lanes and decode/analysis roles may overlap when the granted plan
    /// and selected routes permit it. The totals also omit other work, so they
    /// do not partition the batch's own elapsed time.
    pub fn run_batch_timed(
        &self,
        request: BatchRequest,
        control: &ExecutionControl<'_>,
    ) -> Result<(BatchReport, PhaseTimings), AnalysisError> {
        self.reserve(control.cancellation)?
            .run_batch_timed(request, control.progress)
    }

    /// Run one batch and return the exact non-default allocation topology.
    ///
    /// This measurement entry is absent from ordinary product builds and does
    /// not add any field to `BatchReport` or the wire schema.
    #[cfg(feature = "performance-probes")]
    #[doc(hidden)]
    pub fn run_batch_with_performance_probe(
        &self,
        request: BatchRequest,
        control: &ExecutionControl<'_>,
    ) -> Result<(BatchReport, BatchPerformanceProbe), AnalysisError> {
        self.reserve(control.cancellation)?
            .run_batch_with_performance_probe(request, control.progress)
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
    /// The whole plan this job holds exclusively for its lifetime.
    ///
    /// The job carries the plan rather than an already-split allocation because
    /// the split depends on how many files the operation turns out to have, and
    /// that is not known at admission. Splitting is still done exactly once per
    /// operation, by the plan, before any thread is created.
    plan: ConcurrencyPlan,
    requested_file_lanes: NonZeroUsize,
    overlap_shape: OverlapShape,
}

impl ApplicationJob {
    /// Split this job's plan for an operation over `lanes` file lanes.
    ///
    /// `allocate` is arithmetic over a total this job already holds
    /// exclusively: it never acquires from a shared pool, never blocks and
    /// cannot fail for want of a resource. Admission happened before this job
    /// existed, so no worker below ever waits for a second grant and nested
    /// pools still cannot deadlock.
    #[cfg(test)]
    pub(crate) fn allocation(&self) -> Result<PlanAllocation, AnalysisError> {
        self.plan.allocate(self.requested_file_lanes)
    }

    /// The split a single-file operation takes: one lane, whole decoder.
    fn single_file_allocation(&self) -> Result<PlanAllocation, AnalysisError> {
        self.plan.allocate(NonZeroUsize::MIN)
    }

    pub fn analyze_file(
        self,
        request: AnalyzeRequest,
        progress: &dyn ProgressSink,
    ) -> Result<AnalysisReport, AnalysisError> {
        let decode = match self.single_file_allocation() {
            Ok(allocation) => allocation.decode(),
            Err(error) => return Err(error),
        };
        let shape = self.overlap_shape;
        self.execute(progress, |control| {
            Analyzer::with_overlap_shape(decode, shape).analyze_file_with_control(request, control)
        })
    }

    pub fn analyze_file_timed(
        self,
        request: AnalyzeRequest,
        progress: &dyn ProgressSink,
    ) -> Result<(AnalysisReport, PhaseTimings), AnalysisError> {
        let decode = match self.single_file_allocation() {
            Ok(allocation) => allocation.decode(),
            Err(error) => return Err(error),
        };
        let shape = self.overlap_shape;
        self.execute(progress, |control| {
            Analyzer::with_overlap_shape(decode, shape)
                .collecting_timings()
                .analyze_file_timed(request, control)
        })
    }

    #[cfg(feature = "performance-probes")]
    #[doc(hidden)]
    pub fn analyze_file_with_performance_probe(
        self,
        request: AnalyzeRequest,
        progress: &dyn ProgressSink,
    ) -> Result<(AnalysisReport, ApplicationPerformanceProbe), AnalysisError> {
        let decode = match self.single_file_allocation() {
            Ok(allocation) => allocation.decode(),
            Err(error) => return Err(error),
        };
        let shape = self.overlap_shape;
        self.execute(progress, |control| {
            Analyzer::with_overlap_shape(decode, shape)
                .analyze_file_with_performance_probe(request, control)
        })
    }

    pub fn run_batch(
        self,
        request: BatchRequest,
        progress: &dyn ProgressSink,
    ) -> Result<BatchReport, AnalysisError> {
        let (plan, lanes, shape) = (self.plan, self.requested_file_lanes, self.overlap_shape);
        self.execute(progress, |control| {
            BatchRunner::with_overlap_shape(plan, lanes, shape).run(request, control)
        })
    }

    pub fn run_batch_timed(
        self,
        request: BatchRequest,
        progress: &dyn ProgressSink,
    ) -> Result<(BatchReport, PhaseTimings), AnalysisError> {
        let (plan, lanes, shape) = (self.plan, self.requested_file_lanes, self.overlap_shape);
        self.execute(progress, |control| {
            BatchRunner::with_overlap_shape(plan, lanes, shape)
                .collecting_timings()
                .run_timed(request, control)
        })
    }

    #[cfg(feature = "performance-probes")]
    #[doc(hidden)]
    pub fn run_batch_with_performance_probe(
        self,
        request: BatchRequest,
        progress: &dyn ProgressSink,
    ) -> Result<(BatchReport, BatchPerformanceProbe), AnalysisError> {
        let (plan, lanes, shape) = (self.plan, self.requested_file_lanes, self.overlap_shape);
        self.execute(progress, |control| {
            BatchRunner::with_overlap_shape(plan, lanes, shape)
                .run_with_performance_probe(request, control)
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

        // Reject a plan this budget could never split before admitting the job,
        // so an impossible allocation fails at reserve rather than mid-operation.
        self.inner
            .budget
            .concurrency()
            .allocate(self.inner.budget.file_lanes())?;

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
            plan: self.inner.budget.concurrency(),
            requested_file_lanes: self.inner.budget.file_lanes(),
            overlap_shape: self.inner.budget.overlap_shape(),
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

    fn last_analysis_overlapped() -> bool {
        crate::application::LAST_ANALYSIS_OVERLAP.with(std::cell::Cell::get)
    }

    #[test]
    fn a_serial_route_overlaps_decode_and_analysis_in_an_ordinary_build() {
        let application = Application::with_budget(bounded_budget(8));
        wire_bytes(&application, "native-pcm-v1/wav-pcm-s16-stereo.wav");

        assert!(
            last_analysis_overlapped(),
            "a graduated overlap must reach the ordinary product build"
        );
    }

    #[test]
    fn a_one_worker_plan_leaves_no_permit_to_overlap_with() {
        // The serial route spends the only permit, so there is nothing left to
        // put an analysis thread on. This is the boundary the whole admission
        // rests on, and it must hold in the product build, not just under the
        // measurement feature.
        let application = Application::with_budget(ExecutionBudget::serial());
        wire_bytes(&application, "native-pcm-v1/wav-pcm-s16-stereo.wav");

        assert!(
            !last_analysis_overlapped(),
            "a plan with no spare permit may never start an analysis thread"
        );
    }

    #[test]
    fn a_packet_route_that_spent_every_permit_does_not_also_overlap() {
        // ALAC takes all eight permits for its packet workers, so the overlap
        // has nothing to spend and the two axes cannot stack.
        let application = Application::with_budget(bounded_budget(8));
        wire_bytes(
            &application,
            "native-alac-v1/alac16-stereo-48000-multipacket.m4a",
        );

        assert_eq!(last_execution().workers().get(), 8);
        assert!(
            !last_analysis_overlapped(),
            "a route that spent every permit may not also claim an overlap thread"
        );
    }

    #[cfg(feature = "performance-probes")]
    #[test]
    fn performance_probe_builds_can_measure_decode_analysis_overlap() {
        let application = Application::with_budget(bounded_budget(8));
        let (report, probe) = application
            .analyze_file_with_performance_probe(AnalyzeRequest::new(fixture(
                "native-pcm-v1/wav-pcm-s16-stereo.wav",
            )))
            .expect("the explicit performance path must analyze the WAV fixture");

        assert!(
            last_analysis_overlapped(),
            "the non-default measurement build must reach the candidate"
        );
        assert_eq!(probe.granted_decode_workers(), 8);
        assert_eq!(probe.selected_engine(), "Serial");
        assert_eq!(probe.selected_total_workers(), 1);
        assert_eq!(probe.selected_decoder_workers(), 1);
        assert_eq!(probe.selected_hasher_workers(), 0);
        assert!(probe.decode_analysis_overlapped());
        let shipped = OverlapShape::DEFAULT.channel_depth();
        assert_eq!(probe.requested_overlap_channel_depth(), shipped);
        assert_eq!(probe.applied_overlap_channel_depth(), Some(shipped));
        assert!(probe.decoded_blocks() > 0);
        assert!(probe.final_block_frames() > 0);
        assert_eq!(
            report.analysis().frames_seen(),
            report.diagnostics().decoded_frames
        );
    }

    #[cfg(feature = "performance-probes")]
    #[test]
    fn performance_probe_distinguishes_a_refused_depth_from_the_applied_handoff() {
        let oversized = NonZeroUsize::new(4_096).unwrap();
        let application = Application::with_budget(bounded_budget(8).with_overlap_shape(oversized));
        let (_, probe) = application
            .analyze_file_with_performance_probe(AnalyzeRequest::new(fixture(
                "native-pcm-v1/wav-pcm-s16-stereo.wav",
            )))
            .expect("a refused measurement shape must fall back to serial analysis");

        assert!(!probe.decode_analysis_overlapped());
        assert_eq!(probe.requested_overlap_channel_depth(), 4_096);
        assert_eq!(probe.applied_overlap_channel_depth(), None);
    }

    #[cfg(feature = "performance-probes")]
    #[test]
    fn performance_probe_builds_bind_the_batch_allocation_after_discovery() {
        let application = Application::with_budget(bounded_budget(8));
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;
        let (report, probe) = application
            .run_batch_with_performance_probe(
                BatchRequest::new(
                    vec![
                        fixture("tiny_duration.wav"),
                        fixture("full_scale_clipping.wav"),
                        fixture("native-alac-v1/alac16-mono-44100.m4a"),
                    ],
                    false,
                ),
                &ExecutionControl::new(&cancellation, &progress),
            )
            .expect("the explicit performance path must run the three-file batch");

        assert_eq!(report.summary.total, 3);
        assert_eq!(report.summary.failed, 0);
        assert_eq!(probe.granted_plan_workers(), 8);
        assert_eq!(probe.allocated_file_lanes(), 3);
        assert_eq!(probe.decoder_workers_per_lane(), 2);
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
        // The single-file split, which is what a decoder actually receives when
        // no lane is opened.
        let allocation = job.single_file_allocation().unwrap();

        assert_eq!(allocation.file_lanes().get(), 1);
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
                    if engine == macinmeter_codecs::DecodeEngineKind::FlacPacketWorkers
                        && granted == TEST_HOST_WORKERS.get()
                    {
                        assert_eq!(execution.decoder_workers().get(), granted - 1);
                        assert_eq!(execution.hasher_workers(), 1);
                    } else {
                        assert_eq!(execution.decoder_workers().get(), granted);
                        assert_eq!(execution.hasher_workers(), 0);
                    }
                }
                None => {
                    assert_eq!(
                        execution.engine(),
                        macinmeter_codecs::DecodeEngineKind::Serial,
                        "{name} must not start packet workers"
                    );
                    assert_eq!(execution.workers().get(), 1);
                    assert_eq!(execution.decoder_workers().get(), 1);
                    assert_eq!(execution.hasher_workers(), 0);
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
            assert_eq!(serial_execution.decoder_workers().get(), 1);
            assert_eq!(serial_execution.hasher_workers(), 0);

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
                if expected_engine == macinmeter_codecs::DecodeEngineKind::FlacPacketWorkers
                    && granted == TEST_HOST_WORKERS.get()
                {
                    assert_eq!(execution.decoder_workers().get(), granted - 1);
                    assert_eq!(execution.hasher_workers(), 1);
                } else {
                    assert_eq!(execution.decoder_workers().get(), expected_workers);
                    assert_eq!(execution.hasher_workers(), 0);
                }
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
        // A single-file operation never opens a lane, so it always takes the
        // whole decoder however many lanes the batch path would request.
        let allocation = job.single_file_allocation().unwrap();
        assert_eq!(allocation.file_lanes().get(), 1);

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
    fn a_single_file_operation_keeps_the_whole_decoder_though_the_product_asks_for_lanes() {
        // The regression this guards: lane width and decoder width are two
        // halves of one split, so requesting lanes for the product would have
        // narrowed a single file's decoder from the whole plan to one lane's
        // share had the split stayed at admission.
        let application = Application::with_budget(bounded_budget(8));
        assert!(
            application.budget().requested_file_lanes() > 1,
            "this test is vacuous unless the product budget requests lanes"
        );

        let job = application.reserve(&CancellationToken::new()).unwrap();
        let decode = job.single_file_allocation().unwrap().decode();
        assert_eq!(
            decode.workers().get(),
            TEST_HOST_WORKERS.get(),
            "a single file must decode inside the whole plan, not one lane's share"
        );
    }

    #[test]
    fn a_batch_of_one_file_also_keeps_the_whole_decoder() {
        // The batch path splits after discovery, so a one-item batch asks for
        // one lane and keeps the full decoder just as a single file does.
        let application = Application::with_budget(bounded_budget(8));
        let cancellation = CancellationToken::new();
        let progress = NoopProgressSink;
        let report = application
            .run_batch(
                BatchRequest::new(
                    vec![fixture(
                        "native-alac-v1/alac16-stereo-48000-multipacket.m4a",
                    )],
                    false,
                ),
                &ExecutionControl::new(&cancellation, &progress),
            )
            .expect("a one-item batch must analyze its file");

        assert_eq!(report.summary.total, 1);
        assert_eq!(
            last_execution().workers().get(),
            TEST_HOST_WORKERS.get(),
            "a one-item batch may not narrow its only decoder to pay for lanes"
        );
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
            .unwrap()
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
