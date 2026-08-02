#![forbid(unsafe_code)]

use macinmeter::{
    AnalysisResult, AnalyzerSession, Application, BatchItemOutcome, BatchRequest,
    CancellationToken, ChannelLayout, ExecutionControl, NoopProgressSink, StreamSpec, WireEnvelope,
};
use macinmeter_codecs::{DecoderFactory, ReadOutcome};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env,
    ffi::OsString,
    fmt::Write as _,
    hint::black_box,
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
  m6_baseline_worker decode PATH ITERATIONS
  m6_baseline_worker application PATH ITERATIONS
  m6_baseline_worker batch DIRECTORY ITERATIONS
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

fn run_decode(arguments: &[OsString]) -> Result<Value, String> {
    require_len(arguments, 2, "decode")?;
    let path = PathBuf::from(&arguments[0]);
    let iterations = positive_iterations(&arguments[1])?;

    let (timed_summary, elapsed) = timed_decode_workload(&path, iterations)?;

    // Full PCM hashing is deliberately outside the measured interval. It is a
    // correctness oracle for the timed pass, not part of product decoding.
    let verified = decode_once(&path, true)?;
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
            "pcmF64LeSha256": pcm_sha256,
            "verificationHashOutsideTimedRegion": true,
        }),
    )
}

#[inline(never)]
fn timed_decode_workload(
    path: &Path,
    iterations: u32,
) -> Result<(DecodeSummary, Duration), String> {
    let started = Instant::now();
    let mut timed_summary = None;
    for _ in 0..iterations {
        let summary = decode_once(path, false)?;
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

fn decode_once(path: &Path, hash_pcm: bool) -> Result<DecodeSummary, String> {
    let mut opened = DecoderFactory::new()
        .open(path)
        .map_err(|error| error.to_string())?;
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
    require_len(arguments, 2, "application")?;
    let path = PathBuf::from(&arguments[0]);
    let iterations = positive_iterations(&arguments[1])?;
    let application = Application::new();
    let mut report = None;

    let started = Instant::now();
    for _ in 0..iterations {
        report = Some(
            application
                .analyze_file(macinmeter::AnalyzeRequest::new(&path))
                .map_err(|error| error.to_string())?,
        );
    }
    let elapsed = started.elapsed();
    let report = report
        .as_ref()
        .ok_or_else(|| "application produced no report".to_owned())?;
    let analysis_fingerprint = fingerprint(report.analysis())?.0;
    let (fingerprint, result_bytes) = fingerprint(report)?;
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
        json!({
            "path": display_name(&path),
            "backend": report.diagnostics().backend,
            "decodedFramesPerIteration": report.analysis().frames_seen(),
            "analysisResultFingerprintSha256": analysis_fingerprint,
        }),
    )
}

fn run_batch(arguments: &[OsString]) -> Result<Value, String> {
    require_len(arguments, 2, "batch")?;
    let directory = PathBuf::from(&arguments[0]);
    let iterations = positive_iterations(&arguments[1])?;
    let application = Application::new();
    let cancellation = CancellationToken::new();
    let progress = NoopProgressSink;
    let control = ExecutionControl::new(&cancellation, &progress);
    let mut report = None;

    let started = Instant::now();
    for _ in 0..iterations {
        report = Some(
            application
                .run_batch(BatchRequest::new(vec![directory.clone()], true), &control)
                .map_err(|error| error.to_string())?,
        );
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
    workload_output(
        "batch",
        elapsed,
        work,
        fingerprint,
        result_bytes,
        json!({
            "directory": display_name(&directory),
            "filesPerIteration": report.summary.total,
            "serialApplicationBudget": true,
        }),
    )
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
        "workerElapsedNs": u64::try_from(elapsed.as_nanos())
            .map_err(|_| "worker elapsed time overflowed u64 nanoseconds".to_owned())?,
        "work": work,
        "resultFingerprintSha256": result_fingerprint,
        "resultBytes": result_bytes,
        "details": details,
    }))
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
