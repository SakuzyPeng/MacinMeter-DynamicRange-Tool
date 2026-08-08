#![forbid(unsafe_code)]

use macinmeter::{
    AnalysisResult, AnalyzerSession, Application, BatchItemOutcome, BatchRequest,
    CancellationToken, ChannelLayout, ExecutionBudget, ExecutionControl, NoopProgressSink,
    StreamSpec, WireEnvelope,
};
#[cfg(feature = "performance-probes")]
use macinmeter_codecs::PacketPipelineProbeSnapshot;
use macinmeter_codecs::{DecodeExecution, DecoderFactory, OpenedAudio, ReadOutcome};
use macinmeter_domain::{DecodeReservation, MAX_DECODE_QUEUE_CAPACITY, MAX_IN_FLIGHT_PCM_BYTES};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env,
    ffi::OsString,
    fmt::Write as _,
    hint::black_box,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    process::ExitCode,
    time::{Duration, Instant},
};

const WORKER_SCHEMA_VERSION: u32 = 1;

fn main() -> ExitCode {
    match run() {
        Ok(output) => match serde_json::to_string(&output) {
            Ok(serialized) => {
                println!("{serialized}");
                ExitCode::SUCCESS
            }
            Err(error) => fail(format!("failed to serialize worker output: {error}")),
        },
        Err(error) => fail(error),
    }
}

fn fail(message: String) -> ExitCode {
    eprintln!("m6 baseline worker: {message}");
    ExitCode::from(2)
}

fn run() -> Result<Value, String> {
    let mut arguments = env::args_os().skip(1);
    let mode = required_utf8(arguments.next(), "mode")?;
    let remaining: Vec<OsString> = arguments.collect();

    match mode.as_str() {
        "analysis" => run_analysis(&remaining),
        "decode" => run_decode(&remaining),
        "decode-phases" => run_decode_phases(&remaining),
        #[cfg(feature = "performance-probes")]
        "decode-pipeline" => run_decode_pipeline(&remaining),
        "application" => run_application(&remaining),
        "batch" => run_batch(&remaining),
        "discovery" => run_discovery(&remaining),
        "render-json" => run_render_json(&remaining),
        _ => Err(format!("unknown mode {mode:?}\n{}", usage())),
    }
}

fn usage() -> &'static str {
    "usage:
  m6_baseline_worker analysis CHANNELS SAMPLE_RATE FRAMES BLOCK_FRAMES
  m6_baseline_worker decode PATH ITERATIONS [DECODE_WORKERS [QUEUE_CAPACITY]]
  m6_baseline_worker decode-phases PATH ITERATIONS DECODE_WORKERS
  m6_baseline_worker decode-pipeline PATH ITERATIONS DECODE_WORKERS
  m6_baseline_worker application PATH ITERATIONS [DECODE_WORKERS] [DEPTH] [BATCH]
  m6_baseline_worker batch DIRECTORY ITERATIONS [FILE_LANES]
  m6_baseline_worker discovery DIRECTORY ITERATIONS
  m6_baseline_worker render-json PATH ITERATIONS"
}

fn run_analysis(arguments: &[OsString]) -> Result<Value, String> {
    require_len(arguments, 4, "analysis")?;
    let channels = parse_number::<u16>(&arguments[0], "channels")?;
    let sample_rate = parse_number::<u32>(&arguments[1], "sample rate")?;
    let frames = parse_number::<u64>(&arguments[2], "frames")?;
    let block_frames = parse_number::<usize>(&arguments[3], "block frames")?;
    if block_frames == 0 {
        return Err("block frames must be greater than zero".to_owned());
    }

    let channel_count = usize::from(channels);
    let block_samples = block_frames
        .checked_mul(channel_count)
        .ok_or_else(|| "analysis block sample count overflowed usize".to_owned())?;
    let block = deterministic_block(block_samples, channel_count);
    let stream = StreamSpec::new(sample_rate, channels, ChannelLayout::Unknown)
        .map_err(|error| error.to_string())?;

    let (result, elapsed) = timed_analysis_workload(stream, &block, frames, block_frames)?;

    if result.frames_seen() != frames {
        return Err(format!(
            "analysis accepted {} frames, expected {frames}",
            result.frames_seen()
        ));
    }
    let (fingerprint, result_bytes) = fingerprint(&result)?;
    workload_output(
        "analysis",
        elapsed,
        WorkUnits::audio(frames, channels, sample_rate, 1)?,
        fingerprint,
        result_bytes,
        json!({
            "blockFrames": block_frames,
            "pattern": "deterministic_dense_v1",
        }),
    )
}

#[inline(never)]
fn timed_analysis_workload(
    stream: StreamSpec,
    block: &[f64],
    frames: u64,
    block_frames: usize,
) -> Result<(AnalysisResult, Duration), String> {
    let channel_count = stream.channels.as_usize();
    let started = Instant::now();
    let mut session = AnalyzerSession::new(stream).map_err(|error| error.to_string())?;
    let full_blocks = frames / u64::try_from(block_frames).map_err(|error| error.to_string())?;
    for _ in 0..full_blocks {
        session
            .push_interleaved(black_box(block))
            .map_err(|error| error.to_string())?;
    }
    let tail_frames =
        usize::try_from(frames % u64::try_from(block_frames).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    if tail_frames > 0 {
        let tail_samples = tail_frames
            .checked_mul(channel_count)
            .ok_or_else(|| "analysis tail sample count overflowed usize".to_owned())?;
        session
            .push_interleaved(black_box(&block[..tail_samples]))
            .map_err(|error| error.to_string())?;
    }
    let result = session.finish().map_err(|error| error.to_string())?;
    let elapsed = started.elapsed();
    Ok((result, elapsed))
}

fn deterministic_block(sample_count: usize, channels: usize) -> Vec<f64> {
    let mut samples = Vec::with_capacity(sample_count);
    for index in 0..sample_count {
        let frame = index / channels;
        let channel = index % channels;
        let mixed = (u64::try_from(frame).unwrap_or(u64::MAX))
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(
                (u64::try_from(channel).unwrap_or(u64::MAX) + 1)
                    .wrapping_mul(1_442_695_040_888_963_407),
            );
        let unit = ((mixed >> 11) as f64) * (1.0 / ((1_u64 << 53) as f64));
        let channel_scale = 0.92 - (channel % 11) as f64 * 0.025;
        samples.push((unit * 2.0 - 1.0) * channel_scale);
    }
    samples
}

/// Mirror of the application plan's fixed per-worker derivation.
///
/// `ConcurrencyPlan` is crate-private, so this harness reproduces its
/// derivation instead of widening the product surface to benchmark it. The
/// plan's own unit tests pin the same numbers; the two must move together, and
/// every granted bound is written into the case details so a recorded run can
/// be checked against the plan after the fact.
fn decode_reservation(
    workers: usize,
    queue_override: Option<usize>,
) -> Result<DecodeReservation, String> {
    const QUEUE_DEPTH_PER_WORKER: usize = 4;
    const IN_FLIGHT_PCM_BYTES_PER_WORKER: u64 = 4 * 1024 * 1024;

    let workers = NonZeroUsize::new(workers).ok_or("decode workers must be greater than zero")?;
    if workers.get() == 1 && queue_override.is_none() {
        return Ok(DecodeReservation::serial());
    }
    // An explicit capacity sweeps the queue dimension the plan does not vary.
    // Only the queue bound moves; the in-flight PCM permit stays on the plan's
    // derivation so the two dimensions are not confounded.
    let queue = queue_override.unwrap_or_else(|| {
        workers
            .get()
            .saturating_mul(QUEUE_DEPTH_PER_WORKER)
            .min(MAX_DECODE_QUEUE_CAPACITY)
    });
    let queue_capacity =
        NonZeroUsize::new(queue).ok_or("derived decode queue capacity was empty")?;
    let in_flight = IN_FLIGHT_PCM_BYTES_PER_WORKER
        .saturating_mul(workers.get() as u64)
        .min(MAX_IN_FLIGHT_PCM_BYTES);
    DecodeReservation::new(workers, queue_capacity, in_flight).map_err(|error| error.to_string())
}

fn run_decode(arguments: &[OsString]) -> Result<Value, String> {
    if !(2..=4).contains(&arguments.len()) {
        return Err(format!(
            "decode expects 2 to 4 arguments, received {}\n{}",
            arguments.len(),
            usage()
        ));
    }
    let path = PathBuf::from(&arguments[0]);
    let iterations = positive_iterations(&arguments[1])?;
    // ADR-0014 packet workers are a decode allocation, not a separate binary,
    // so the worker count is a case argument that the runner interleaves like
    // any other. One worker is the serial route.
    let decode_workers = match arguments.get(2) {
        Some(value) => parse_number::<usize>(value, "decode workers")?,
        None => 1,
    };
    let decode_queue_capacity = match arguments.get(3) {
        Some(value) => Some(parse_number::<usize>(value, "decode queue capacity")?),
        None => None,
    };
    let reservation = decode_reservation(decode_workers, decode_queue_capacity)?;

    let (timed_summary, elapsed) = timed_decode_workload(&path, iterations, reservation)?;

    // Full PCM hashing is deliberately outside the measured interval. It is a
    // correctness oracle for the timed pass, not part of product decoding.
    // It runs on the same allocation, so the runner's oracle comparison is a
    // real differential against the corpus rather than a serial re-run.
    let verified = decode_once(&path, true, reservation)?;
    ensure_same_decode_geometry(&timed_summary, &verified)?;
    let pcm_sha256 = verified
        .pcm_sha256
        .ok_or_else(|| "verification decode did not produce a PCM fingerprint".to_owned())?;
    let (fingerprint, result_bytes) = fingerprint(&json!({
        "stream": verified.stream,
        "frames": verified.frames,
        "blocks": verified.blocks,
        "pcmF64LeSha256": pcm_sha256,
    }))?;

    workload_output(
        "decode",
        elapsed,
        WorkUnits::audio(
            timed_summary.frames,
            timed_summary.channels,
            timed_summary.sample_rate,
            iterations,
        )?,
        fingerprint,
        result_bytes,
        json!({
            "path": display_name(&path),
            "blocksPerIteration": timed_summary.blocks,
            "decodeWorkers": decode_workers,
            "decodeQueueCapacity": reservation.queue_capacity().get(),
            "decodeMaxInFlightPcmBytes": reservation.max_in_flight_pcm_bytes(),
            "pcmF64LeSha256": pcm_sha256,
            "verificationHashOutsideTimedRegion": true,
        }),
    )
}

/// Attribute the complete product decode workload between source construction
/// and draining without changing either path.
///
/// The earlier sequential-floor probe starts after backend open, whereas the
/// formal decode workload starts before first-party container inspection,
/// backend probe, decoder construction, and thread creation. Keeping these
/// timers in the existing worker makes that previously omitted interval
/// source-bound and directly comparable with the same corpus/allocation sweep.
fn run_decode_phases(arguments: &[OsString]) -> Result<Value, String> {
    require_len(arguments, 3, "decode-phases")?;
    let path = PathBuf::from(&arguments[0]);
    let iterations = positive_iterations(&arguments[1])?;
    let decode_workers = parse_number::<usize>(&arguments[2], "decode workers")?;
    let reservation = decode_reservation(decode_workers, None)?;

    let (timed, execution, elapsed, open_elapsed, drain_elapsed) =
        timed_decode_phase_workload(&path, iterations, reservation)?;

    // The attribution pass consumes blocks without hashing so its phase
    // boundaries stay on the product path. A second pass on the identical
    // allocation remains outside those timers and supplies the corpus oracle.
    let verified = decode_once(&path, true, reservation)?;
    ensure_same_decode_geometry(&timed, &verified)?;
    let pcm_sha256 = verified
        .pcm_sha256
        .ok_or_else(|| "verification decode did not produce a PCM fingerprint".to_owned())?;
    let (fingerprint, result_bytes) = fingerprint(&json!({
        "stream": verified.stream,
        "frames": verified.frames,
        "blocks": verified.blocks,
        "pcmF64LeSha256": pcm_sha256,
    }))?;
    let accounted = open_elapsed.saturating_add(drain_elapsed);

    let mut output = workload_output(
        "decode_phases",
        elapsed,
        WorkUnits::audio(timed.frames, timed.channels, timed.sample_rate, iterations)?,
        fingerprint,
        result_bytes,
        json!({
            "path": display_name(&path),
            "blocksPerIteration": timed.blocks,
            "decodeWorkers": decode_workers,
            "decodeQueueCapacity": reservation.queue_capacity().get(),
            "decodeMaxInFlightPcmBytes": reservation.max_in_flight_pcm_bytes(),
            "selectedEngine": format!("{:?}", execution.engine()),
            "selectedTotalWorkers": execution.workers().get(),
            "selectedDecoderWorkers": execution.decoder_workers().get(),
            "selectedHasherWorkers": execution.hasher_workers(),
            "phaseBoundary": "open includes first-party inspection, backend probe, decoder construction, and owned-thread start; drain begins after DecoderFactory returns",
            "pcmF64LeSha256": pcm_sha256,
            "verificationHashOutsideTimedRegion": true,
        }),
    )?;
    output
        .as_object_mut()
        .ok_or_else(|| "worker output was not a JSON object".to_owned())?
        .insert(
            "measurements".to_owned(),
            json!({
                "openElapsedNs": duration_ns(open_elapsed)?,
                "drainElapsedNs": duration_ns(drain_elapsed)?,
                "unattributedElapsedNs": duration_ns(elapsed.saturating_sub(accounted))?,
            }),
        );
    Ok(output)
}

#[inline(never)]
fn timed_decode_phase_workload(
    path: &Path,
    iterations: u32,
    reservation: DecodeReservation,
) -> Result<(DecodeSummary, DecodeExecution, Duration, Duration, Duration), String> {
    let started = Instant::now();
    let mut timed_summary = None;
    let mut selected_execution = None;
    let mut open_elapsed = Duration::ZERO;
    let mut drain_elapsed = Duration::ZERO;

    for _ in 0..iterations {
        let open_started = Instant::now();
        let (opened, execution) = DecoderFactory::with_application_reservation(reservation)
            .open_with_execution(path)
            .map_err(|error| error.to_string())?;
        open_elapsed = open_elapsed.saturating_add(open_started.elapsed());

        let drain_started = Instant::now();
        let summary = drain_opened(opened, false)?;
        drain_elapsed = drain_elapsed.saturating_add(drain_started.elapsed());

        if let Some(previous) = &timed_summary {
            ensure_same_decode_geometry(previous, &summary)?;
        } else {
            timed_summary = Some(summary);
        }
        if selected_execution.is_some_and(|previous| previous != execution) {
            return Err("decode execution topology changed between iterations".to_owned());
        }
        selected_execution = Some(execution);
    }

    let elapsed = started.elapsed();
    Ok((
        timed_summary.ok_or_else(|| "decode produced no iteration".to_owned())?,
        selected_execution.ok_or_else(|| "decode selected no execution".to_owned())?,
        elapsed,
        open_elapsed,
        drain_elapsed,
    ))
}

/// Attribute the caller critical path and every source-owned thread inside one
/// complete decode. This mode only exists in an explicit performance-probe
/// build; the same reservation still chooses the product engine and bounds.
#[cfg(feature = "performance-probes")]
fn run_decode_pipeline(arguments: &[OsString]) -> Result<Value, String> {
    require_len(arguments, 3, "decode-pipeline")?;
    let path = PathBuf::from(&arguments[0]);
    let iterations = positive_iterations(&arguments[1])?;
    if iterations != 1 {
        return Err("decode-pipeline requires exactly one source iteration".to_owned());
    }
    let decode_workers = parse_number::<usize>(&arguments[2], "decode workers")?;
    let reservation = decode_reservation(decode_workers, None)?;

    let started = Instant::now();
    let open_started = Instant::now();
    let (opened, execution, probe) = DecoderFactory::with_application_reservation(reservation)
        .open_with_performance_probe(&path)
        .map_err(|error| error.to_string())?;
    let open_elapsed = open_started.elapsed();
    let drain_started = Instant::now();
    let timed = drain_opened(opened, false)?;
    let drain_elapsed = drain_started.elapsed();
    let elapsed = started.elapsed();
    let snapshot = probe.snapshot();

    // Keep the full PCM oracle outside the observed source. Hashing blocks on
    // the caller between reads would back-pressure the result queue and change
    // the pipeline this mode is meant to measure.
    let verified = decode_once(&path, true, reservation)?;
    ensure_same_decode_geometry(&timed, &verified)?;
    let pcm_sha256 = verified
        .pcm_sha256
        .ok_or_else(|| "verification decode did not produce a PCM fingerprint".to_owned())?;
    let (fingerprint, result_bytes) = fingerprint(&json!({
        "stream": verified.stream,
        "frames": verified.frames,
        "blocks": verified.blocks,
        "pcmF64LeSha256": pcm_sha256,
    }))?;

    let mut output = workload_output(
        "decode_pipeline",
        elapsed,
        WorkUnits::audio(timed.frames, timed.channels, timed.sample_rate, iterations)?,
        fingerprint,
        result_bytes,
        json!({
            "path": display_name(&path),
            "blocksPerIteration": timed.blocks,
            "decodeWorkers": decode_workers,
            "decodeQueueCapacity": reservation.queue_capacity().get(),
            "decodeMaxInFlightPcmBytes": reservation.max_in_flight_pcm_bytes(),
            "selectedEngine": format!("{:?}", execution.engine()),
            "selectedTotalWorkers": execution.workers().get(),
            "selectedDecoderWorkers": execution.decoder_workers().get(),
            "selectedHasherWorkers": execution.hasher_workers(),
            "probeDecoderWorkers": snapshot.decoder_workers,
            "probeBoundary": "per-packet timings are source-owned wall intervals; worker totals overlap each other and are not an additive partition of caller elapsed time",
            "pcmF64LeSha256": pcm_sha256,
            "verificationHashOutsideTimedRegion": true,
        }),
    )?;
    output
        .as_object_mut()
        .ok_or_else(|| "worker output was not a JSON object".to_owned())?
        .insert(
            "measurements".to_owned(),
            pipeline_measurements(snapshot, elapsed, open_elapsed, drain_elapsed)?,
        );
    Ok(output)
}

#[cfg(feature = "performance-probes")]
fn pipeline_measurements(
    snapshot: PacketPipelineProbeSnapshot,
    elapsed: Duration,
    open_elapsed: Duration,
    drain_elapsed: Duration,
) -> Result<Value, String> {
    let mut values = serde_json::Map::new();
    let mut insert = |key: &str, value: u64| {
        values.insert(key.to_owned(), Value::from(value));
    };
    let elapsed_ns = duration_ns(elapsed)?;
    let open_ns = duration_ns(open_elapsed)?;
    let drain_ns = duration_ns(drain_elapsed)?;
    insert("openElapsedNs", open_ns);
    insert("drainElapsedNs", drain_ns);
    insert(
        "unattributedElapsedNs",
        elapsed_ns.saturating_sub(open_ns.saturating_add(drain_ns)),
    );
    insert("fileIdentifyNs", snapshot.file_identify_ns);
    insert("containerInspectionNs", snapshot.container_inspection_ns);
    insert("backendProbeNs", snapshot.backend_probe_ns);
    insert("routeSetupNs", snapshot.route_setup_ns);
    insert(
        "openUnattributedNs",
        open_ns.saturating_sub(
            snapshot
                .file_identify_ns
                .saturating_add(snapshot.container_inspection_ns)
                .saturating_add(snapshot.backend_probe_ns)
                .saturating_add(snapshot.route_setup_ns),
        ),
    );
    insert("demuxPacketReadNs", snapshot.demux_packet_read_ns);
    insert("demuxDispatchWaitNs", snapshot.demux_dispatch_wait_ns);
    insert("callerResultWaitNs", snapshot.caller_result_wait_ns);
    insert("callerCommitNs", snapshot.caller_commit_ns);
    insert("callerFinishNs", snapshot.caller_finish_ns);
    insert(
        "callerOtherNs",
        drain_ns.saturating_sub(
            snapshot
                .caller_result_wait_ns
                .saturating_add(snapshot.caller_commit_ns)
                .saturating_add(snapshot.caller_finish_ns),
        ),
    );
    insert("reorderStalls", snapshot.reorder_stalls);
    insert(
        "peakReorderPackets",
        u64::try_from(snapshot.peak_reorder_packets).unwrap_or(u64::MAX),
    );
    insert("peakReorderBytes", snapshot.peak_reorder_bytes);
    insert("hasherPackets", snapshot.hasher_packets);
    insert("hasherReceiveWaitNs", snapshot.hasher_receive_wait_ns);
    insert("hasherActiveNs", snapshot.hasher_active_ns);
    insert("hasherSendWaitNs", snapshot.hasher_send_wait_ns);
    insert("hasherLifetimeNs", snapshot.hasher_lifetime_ns);
    for worker in snapshot.workers {
        let prefix = format!("worker{}", worker.slot);
        insert(&format!("{prefix}Packets"), worker.packets);
        insert(
            &format!("{prefix}BackendDecodeNs"),
            worker.backend_decode_ns,
        );
        insert(
            &format!("{prefix}IntegrityConversionNs"),
            worker.integrity_conversion_ns,
        );
        insert(
            &format!("{prefix}PcmConversionNs"),
            worker.pcm_conversion_ns,
        );
        insert(&format!("{prefix}InboxWaitNs"), worker.inbox_wait_ns);
        insert(
            &format!("{prefix}ResultSendWaitNs"),
            worker.result_send_wait_ns,
        );
        insert(&format!("{prefix}LifetimeNs"), worker.lifetime_ns);
    }
    Ok(Value::Object(values))
}

#[inline(never)]
fn timed_decode_workload(
    path: &Path,
    iterations: u32,
    reservation: DecodeReservation,
) -> Result<(DecodeSummary, Duration), String> {
    let started = Instant::now();
    let mut timed_summary = None;
    for _ in 0..iterations {
        let summary = decode_once(path, false, reservation)?;
        if let Some(previous) = &timed_summary {
            ensure_same_decode_geometry(previous, &summary)?;
        } else {
            timed_summary = Some(summary);
        }
    }
    let elapsed = started.elapsed();
    let timed_summary = timed_summary.ok_or_else(|| "decode produced no iteration".to_owned())?;
    Ok((timed_summary, elapsed))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct DecodeStream {
    sample_rate: u32,
    channels: u16,
    expected_frames: Option<u64>,
}

#[derive(Debug, Clone)]
struct DecodeSummary {
    stream: DecodeStream,
    sample_rate: u32,
    channels: u16,
    frames: u64,
    blocks: u64,
    pcm_sha256: Option<String>,
}

fn decode_once(
    path: &Path,
    hash_pcm: bool,
    reservation: DecodeReservation,
) -> Result<DecodeSummary, String> {
    let opened = DecoderFactory::with_application_reservation(reservation)
        .open(path)
        .map_err(|error| error.to_string())?;
    drain_opened(opened, hash_pcm)
}

fn drain_opened(mut opened: OpenedAudio, hash_pcm: bool) -> Result<DecodeSummary, String> {
    let info = opened.reader.stream_info().clone();
    let stream = DecodeStream {
        sample_rate: info.spec.sample_rate.get(),
        channels: info.spec.channels.get(),
        expected_frames: info.expected_frames,
    };
    let mut frames = 0_u64;
    let mut blocks = 0_u64;
    let mut hasher = hash_pcm.then(Sha256::new);

    while let ReadOutcome::Data(block) = opened
        .reader
        .read_block()
        .map_err(|error| error.to_string())?
    {
        frames = frames
            .checked_add(u64::try_from(block.frames()).map_err(|error| error.to_string())?)
            .ok_or_else(|| "decoded frame count overflowed u64".to_owned())?;
        blocks = blocks
            .checked_add(1)
            .ok_or_else(|| "decoded block count overflowed u64".to_owned())?;
        if let Some(digest) = hasher.as_mut() {
            for sample in block.samples() {
                digest.update(sample.to_bits().to_le_bytes());
            }
        } else {
            black_box(block.samples());
        }
    }

    if opened.reader.diagnostics().decoded_frames != frames {
        return Err(format!(
            "decoder diagnostics record {} frames, counted {frames}",
            opened.reader.diagnostics().decoded_frames
        ));
    }
    if info
        .expected_frames
        .is_some_and(|expected| expected != frames)
    {
        return Err(format!(
            "decoder produced {frames} frames, expected {:?}",
            info.expected_frames
        ));
    }

    Ok(DecodeSummary {
        sample_rate: stream.sample_rate,
        channels: stream.channels,
        stream,
        frames,
        blocks,
        pcm_sha256: hasher.map(|digest| hex_digest(&digest.finalize())),
    })
}

fn ensure_same_decode_geometry(left: &DecodeSummary, right: &DecodeSummary) -> Result<(), String> {
    if left.stream != right.stream || left.frames != right.frames || left.blocks != right.blocks {
        return Err("decode geometry changed between benchmark iterations".to_owned());
    }
    Ok(())
}

fn run_application(arguments: &[OsString]) -> Result<Value, String> {
    if !(2..=5).contains(&arguments.len()) {
        return Err(format!(
            "application expects 2 to 5 argument(s), received {}\n{}",
            arguments.len(),
            usage()
        ));
    }
    let path = PathBuf::from(&arguments[0]);
    let iterations = positive_iterations(&arguments[1])?;
    // The graduation gates compare every axis across 1/2/4/8 workers. For
    // decode-analysis overlap that sweep is not a scale but a boundary: a
    // one-worker plan leaves a serial route no spare permit, so the overlap
    // cannot engage. Absent the argument this stays the product plan.
    let requested_workers = match arguments.get(2) {
        Some(value) => Some(parse_number::<usize>(value, "decode workers")?),
        None => None,
    };
    // The hand-off shape is the other measurement input on the same account:
    // depth and batch only trade retained PCM for synchronisation, and the
    // application refuses a shape its in-flight allowance cannot hold rather
    // than overspending. Absent both arguments this stays the graduated shape.
    let overlap_shape = match (arguments.get(3), arguments.get(4)) {
        (None, None) => None,
        (depth, batch) => {
            if requested_workers.is_none() {
                // A shape only means something against a stated plan: whether
                // the overlap engages at all is decided by the permits the
                // route leaves, so a shape without a worker count would time
                // an unknown topology.
                return Err(
                    "an overlap hand-off shape requires an explicit decode worker count".to_owned(),
                );
            }
            let parse = |value: Option<&OsString>, label| match value {
                Some(value) => parse_number::<usize>(value, label),
                None => Ok(1),
            };
            Some((
                parse(depth, "overlap channel depth")?,
                parse(batch, "overlap batch blocks")?,
            ))
        }
    };
    let application = match requested_workers {
        None => Application::new(),
        Some(workers) => {
            let workers = NonZeroUsize::new(workers)
                .ok_or_else(|| "decode workers must be greater than zero".to_owned())?;
            // Same rule as the lane width: a measurement surface reachable only
            // in an explicit probe build, and a default build refuses a value it
            // cannot honour rather than silently ignoring it.
            #[cfg(feature = "performance-probes")]
            {
                let mut budget = ExecutionBudget::product().with_decode_workers(workers);
                if let Some((depth, batch)) = overlap_shape {
                    let depth = NonZeroUsize::new(depth).ok_or_else(|| {
                        "overlap channel depth must be greater than zero".to_owned()
                    })?;
                    let batch = NonZeroUsize::new(batch).ok_or_else(|| {
                        "overlap batch blocks must be greater than zero".to_owned()
                    })?;
                    budget = budget.with_overlap_shape(depth, batch);
                }
                Application::with_budget(budget)
            }
            #[cfg(not(feature = "performance-probes"))]
            {
                let _ = workers;
                let _ = overlap_shape;
                return Err(
                    "decode worker counts and hand-off shapes require a worker built with the \
                     performance-probes feature"
                        .to_owned(),
                );
            }
        }
    };
    let mut report = None;
    #[cfg(feature = "performance-probes")]
    let mut execution_probe = None;

    let started = Instant::now();
    for _ in 0..iterations {
        #[cfg(feature = "performance-probes")]
        let (next_report, next_probe) = application
            .analyze_file_with_performance_probe(macinmeter::AnalyzeRequest::new(&path))
            .map_err(|error| error.to_string())?;
        #[cfg(not(feature = "performance-probes"))]
        let next_report = application
            .analyze_file(macinmeter::AnalyzeRequest::new(&path))
            .map_err(|error| error.to_string())?;
        #[cfg(feature = "performance-probes")]
        {
            if execution_probe.is_some_and(|previous| previous != next_probe) {
                return Err("application execution topology changed between iterations".to_owned());
            }
            execution_probe = Some(next_probe);
        }
        report = Some(next_report);
    }
    let elapsed = started.elapsed();
    let report = report
        .as_ref()
        .ok_or_else(|| "application produced no report".to_owned())?;
    let analysis_fingerprint = fingerprint(report.analysis())?.0;
    let (fingerprint, result_bytes) = fingerprint(report)?;
    let details = json!({
        "path": display_name(&path),
        "backend": report.diagnostics().backend,
        "requestedDecodeWorkers": requested_workers,
        "decodedFramesPerIteration": report.analysis().frames_seen(),
        "analysisResultFingerprintSha256": analysis_fingerprint,
    });
    #[cfg(feature = "performance-probes")]
    let details = {
        let mut details = details;
        let probe =
            execution_probe.ok_or_else(|| "application produced no execution probe".to_owned())?;
        let object = details
            .as_object_mut()
            .ok_or_else(|| "application details were not a JSON object".to_owned())?;
        object.insert(
            "grantedDecodeWorkers".to_owned(),
            json!(probe.granted_decode_workers()),
        );
        object.insert("selectedEngine".to_owned(), json!(probe.selected_engine()));
        object.insert(
            "selectedTotalWorkers".to_owned(),
            json!(probe.selected_total_workers()),
        );
        object.insert(
            "selectedDecoderWorkers".to_owned(),
            json!(probe.selected_decoder_workers()),
        );
        object.insert(
            "selectedHasherWorkers".to_owned(),
            json!(probe.selected_hasher_workers()),
        );
        object.insert(
            "decodeAnalysisOverlapped".to_owned(),
            json!(probe.decode_analysis_overlapped()),
        );
        object.insert(
            "requestedOverlapChannelDepth".to_owned(),
            json!(probe.requested_overlap_channel_depth()),
        );
        object.insert(
            "requestedOverlapBatchBlocks".to_owned(),
            json!(probe.requested_overlap_batch_blocks()),
        );
        object.insert(
            "appliedOverlapChannelDepth".to_owned(),
            json!(probe.applied_overlap_channel_depth()),
        );
        object.insert(
            "appliedOverlapBatchBlocks".to_owned(),
            json!(probe.applied_overlap_batch_blocks()),
        );
        object.insert(
            "decodedBlocksPerIteration".to_owned(),
            json!(probe.decoded_blocks()),
        );
        object.insert(
            "finalBlockFrames".to_owned(),
            json!(probe.final_block_frames()),
        );
        details
    };
    workload_output(
        "application",
        elapsed,
        WorkUnits::audio(
            report.analysis().frames_seen(),
            report.pcm().spec.channels.get(),
            report.pcm().spec.sample_rate.get(),
            iterations,
        )?,
        fingerprint,
        result_bytes,
        details,
    )
}

fn run_batch(arguments: &[OsString]) -> Result<Value, String> {
    if arguments.len() != 2 && arguments.len() != 3 {
        return Err(format!(
            "batch expects 2 or 3 argument(s), received {}\n{}",
            arguments.len(),
            usage()
        ));
    }
    let directory = PathBuf::from(&arguments[0]);
    let iterations = positive_iterations(&arguments[1])?;
    // Absent the argument this is the product plan's own derived width. The
    // sweep needs the other widths in the same binary, and the plan still
    // clamps the request and narrows each lane's decoder to pay for it.
    let file_lanes = match arguments.get(2) {
        Some(value) => Some(
            NonZeroUsize::new(parse_number::<usize>(value, "file lanes")?)
                .ok_or_else(|| "file lanes must be greater than zero".to_owned())?,
        ),
        None => None,
    };
    // Requesting lanes is a measurement surface, not a product one, so it is
    // reachable only in the same explicit probe build as the pipeline probes. A
    // default build accepts the argument's product value and nothing else,
    // rather than silently ignoring a width it cannot honour.
    let budget = match file_lanes {
        None => ExecutionBudget::product(),
        Some(lanes) => {
            #[cfg(feature = "performance-probes")]
            {
                ExecutionBudget::product().with_file_lanes(lanes)
            }
            #[cfg(not(feature = "performance-probes"))]
            {
                let _ = lanes;
                return Err(
                    "explicit file lanes require a worker built with the performance-probes \
                     feature"
                        .to_owned(),
                );
            }
        }
    };
    let application = Application::with_budget(budget);
    let cancellation = CancellationToken::new();
    let progress = NoopProgressSink;
    let control = ExecutionControl::new(&cancellation, &progress);
    let mut report = None;
    #[cfg(feature = "performance-probes")]
    let mut execution_probe = None;

    let started = Instant::now();
    for _ in 0..iterations {
        #[cfg(feature = "performance-probes")]
        let (next_report, next_probe) = application
            .run_batch_with_performance_probe(
                BatchRequest::new(vec![directory.clone()], true),
                &control,
            )
            .map_err(|error| error.to_string())?;
        #[cfg(not(feature = "performance-probes"))]
        let next_report = application
            .run_batch(BatchRequest::new(vec![directory.clone()], true), &control)
            .map_err(|error| error.to_string())?;
        #[cfg(feature = "performance-probes")]
        {
            if execution_probe.is_some_and(|previous| previous != next_probe) {
                return Err("batch execution topology changed between iterations".to_owned());
            }
            execution_probe = Some(next_probe);
        }
        report = Some(next_report);
    }
    let elapsed = started.elapsed();
    let report = report
        .as_ref()
        .ok_or_else(|| "batch produced no report".to_owned())?;
    if report.summary.failed != 0 {
        return Err(format!(
            "batch baseline contains {} failed item(s)",
            report.summary.failed
        ));
    }
    let mut frames_per_iteration = 0_u64;
    let mut interleaved_samples_per_iteration = 0_u64;
    let mut audio_seconds_per_iteration = 0.0_f64;
    for item in &report.items {
        let BatchItemOutcome::Success { report } = &item.outcome else {
            return Err("batch baseline contains a failed item".to_owned());
        };
        let frames = report.analysis().frames_seen();
        let channels = u64::from(report.pcm().spec.channels.get());
        frames_per_iteration = frames_per_iteration
            .checked_add(frames)
            .ok_or_else(|| "batch frame count overflowed u64".to_owned())?;
        interleaved_samples_per_iteration = interleaved_samples_per_iteration
            .checked_add(
                frames
                    .checked_mul(channels)
                    .ok_or_else(|| "batch sample count overflowed u64".to_owned())?,
            )
            .ok_or_else(|| "batch sample count overflowed u64".to_owned())?;
        audio_seconds_per_iteration +=
            frames as f64 / f64::from(report.pcm().spec.sample_rate.get());
    }
    let (fingerprint, result_bytes) = fingerprint(report)?;
    let mut work = WorkUnits::aggregate_audio(
        frames_per_iteration,
        interleaved_samples_per_iteration,
        audio_seconds_per_iteration,
        iterations,
    )?;
    work.logical_items = u64::try_from(report.summary.total)
        .map_err(|error| error.to_string())?
        .checked_mul(u64::from(iterations))
        .ok_or_else(|| "batch logical item count overflowed u64".to_owned())?;
    let details = json!({
        "directory": display_name(&directory),
        "filesPerIteration": report.summary.total,
        "requestedFileLanes": budget.requested_file_lanes(),
    });
    #[cfg(feature = "performance-probes")]
    let details = {
        let mut details = details;
        let probe =
            execution_probe.ok_or_else(|| "batch produced no execution probe".to_owned())?;
        let object = details
            .as_object_mut()
            .ok_or_else(|| "batch details were not a JSON object".to_owned())?;
        object.insert(
            "grantedPlanWorkers".to_owned(),
            json!(probe.granted_plan_workers()),
        );
        object.insert(
            "allocatedFileLanes".to_owned(),
            json!(probe.allocated_file_lanes()),
        );
        object.insert(
            "decoderWorkersPerLane".to_owned(),
            json!(probe.decoder_workers_per_lane()),
        );
        details
    };
    workload_output("batch", elapsed, work, fingerprint, result_bytes, details)
}

fn run_discovery(arguments: &[OsString]) -> Result<Value, String> {
    require_len(arguments, 2, "discovery")?;
    let directory = PathBuf::from(&arguments[0]);
    let iterations = positive_iterations(&arguments[1])?;
    let application = Application::new();
    let mut paths = None;

    let started = Instant::now();
    for _ in 0..iterations {
        paths = Some(
            application
                .discover_inputs(std::slice::from_ref(&directory), true)
                .map_err(|error| error.to_string())?,
        );
    }
    let elapsed = started.elapsed();
    let paths = paths
        .as_ref()
        .ok_or_else(|| "discovery produced no output".to_owned())?;
    let relative_paths: Vec<String> = paths
        .iter()
        .map(|path| {
            path.strip_prefix(&directory)
                .unwrap_or(path)
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    let (fingerprint, result_bytes) = fingerprint(&relative_paths)?;
    workload_output(
        "discovery",
        elapsed,
        WorkUnits::operations(iterations, paths.len())?,
        fingerprint,
        result_bytes,
        json!({
            "directory": display_name(&directory),
            "filesPerIteration": paths.len(),
            "recursive": true,
            "cacheState": "warm_or_os_managed",
        }),
    )
}

fn run_render_json(arguments: &[OsString]) -> Result<Value, String> {
    require_len(arguments, 2, "render-json")?;
    let path = PathBuf::from(&arguments[0]);
    let iterations = positive_iterations(&arguments[1])?;
    let report = Application::new()
        .analyze_file(macinmeter::AnalyzeRequest::new(&path))
        .map_err(|error| error.to_string())?;
    let mut last = String::new();

    let started = Instant::now();
    for _ in 0..iterations {
        last = serde_json::to_string_pretty(&WireEnvelope::analysis(report.clone()))
            .map_err(|error| error.to_string())?;
        black_box(&last);
    }
    let elapsed = started.elapsed();
    let fingerprint = sha256(last.as_bytes());
    workload_output(
        "render_json",
        elapsed,
        WorkUnits::operations(iterations, 1)?,
        fingerprint,
        last.len(),
        json!({
            "path": display_name(&path),
            "format": "wire_schema_v4_pretty_json",
            "bytesPerIteration": last.len(),
            "analysisOutsideTimedRegion": true,
        }),
    )
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkUnits {
    iterations: u32,
    audio_frames: u64,
    interleaved_samples: u64,
    audio_seconds: f64,
    logical_items: u64,
}

impl WorkUnits {
    fn audio(
        frames_per_iteration: u64,
        channels: u16,
        sample_rate: u32,
        iterations: u32,
    ) -> Result<Self, String> {
        let samples_per_iteration = frames_per_iteration
            .checked_mul(u64::from(channels))
            .ok_or_else(|| "interleaved sample count overflowed u64".to_owned())?;
        Self::aggregate_audio(
            frames_per_iteration,
            samples_per_iteration,
            frames_per_iteration as f64 / f64::from(sample_rate),
            iterations,
        )
    }

    fn aggregate_audio(
        frames_per_iteration: u64,
        samples_per_iteration: u64,
        seconds_per_iteration: f64,
        iterations: u32,
    ) -> Result<Self, String> {
        let multiplier = u64::from(iterations);
        Ok(Self {
            iterations,
            audio_frames: frames_per_iteration
                .checked_mul(multiplier)
                .ok_or_else(|| "total audio frame count overflowed u64".to_owned())?,
            interleaved_samples: samples_per_iteration
                .checked_mul(multiplier)
                .ok_or_else(|| "total sample count overflowed u64".to_owned())?,
            audio_seconds: seconds_per_iteration * f64::from(iterations),
            logical_items: multiplier,
        })
    }

    fn operations(iterations: u32, items_per_iteration: usize) -> Result<Self, String> {
        Ok(Self {
            iterations,
            audio_frames: 0,
            interleaved_samples: 0,
            audio_seconds: 0.0,
            logical_items: u64::try_from(items_per_iteration)
                .map_err(|error| error.to_string())?
                .checked_mul(u64::from(iterations))
                .ok_or_else(|| "logical item count overflowed u64".to_owned())?,
        })
    }
}

fn workload_output(
    mode: &str,
    elapsed: Duration,
    work: WorkUnits,
    result_fingerprint: String,
    result_bytes: usize,
    details: Value,
) -> Result<Value, String> {
    Ok(json!({
        "schemaVersion": WORKER_SCHEMA_VERSION,
        "mode": mode,
        "workerElapsedNs": duration_ns(elapsed)?,
        "work": work,
        "resultFingerprintSha256": result_fingerprint,
        "resultBytes": result_bytes,
        "details": details,
    }))
}

fn duration_ns(duration: Duration) -> Result<u64, String> {
    u64::try_from(duration.as_nanos())
        .map_err(|_| "worker elapsed time overflowed u64 nanoseconds".to_owned())
}

fn fingerprint<T: Serialize>(value: &T) -> Result<(String, usize), String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    Ok((sha256(&bytes), bytes.len()))
}

fn sha256(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    hex_digest(&digest.finalize())
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn required_utf8(value: Option<OsString>, label: &str) -> Result<String, String> {
    value
        .ok_or_else(|| format!("missing {label}\n{}", usage()))?
        .into_string()
        .map_err(|_| format!("{label} is not valid UTF-8"))
}

fn require_len(arguments: &[OsString], expected: usize, mode: &str) -> Result<(), String> {
    if arguments.len() != expected {
        return Err(format!(
            "{mode} expects {expected} argument(s), received {}\n{}",
            arguments.len(),
            usage()
        ));
    }
    Ok(())
}

fn parse_number<T>(value: &OsString, label: &str) -> Result<T, String>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .to_str()
        .ok_or_else(|| format!("{label} is not valid UTF-8"))?
        .parse()
        .map_err(|error| format!("invalid {label}: {error}"))
}

fn positive_iterations(value: &OsString) -> Result<u32, String> {
    let iterations = parse_number::<u32>(value, "iterations")?;
    if iterations == 0 {
        return Err("iterations must be greater than zero".to_owned());
    }
    Ok(iterations)
}
