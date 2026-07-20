#![forbid(unsafe_code)]

//! Decoder-independent worker for the bounded M4 Candidate V1 conformance.
//!
//! This is a reference-side adapter, not a product CLI. It accepts exactly one
//! finite interleaved `f64le` stream on stdin and emits one path-free JSON
//! result on stdout. The suite runner launches a fresh process per input.

use macinmeter_analysis::AnalyzerSession;
use macinmeter_domain::{
    AnalysisProfile, ChannelLayout, ChannelOutcome, MAX_ANALYSIS_CHANNELS, StreamSpec,
};
use serde_json::{Value, json};
use std::io::{self, Read, Write};
use std::process::ExitCode;

const ARGUMENT_COUNT: usize = 5;
const F64_BYTES: usize = 8;
const F64_BYTES_U64: u64 = 8;
const MAX_BLOCK_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug)]
struct Request {
    input_id: String,
    sample_rate_hz: u32,
    channels: u16,
    frames: u64,
    block_frames: usize,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("candidate conformance worker error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let request = parse_request(std::env::args().skip(1).collect())?;
    let spec = StreamSpec::new(
        request.sample_rate_hz,
        request.channels,
        ChannelLayout::Unknown,
    )
    .map_err(|error| error.to_string())?;
    let mut session = AnalyzerSession::new(spec, AnalysisProfile::FooDrMeter108CandidateV1)
        .map_err(|error| error.to_string())?;

    let channel_count = usize::from(request.channels);
    let expected_samples = request
        .frames
        .checked_mul(u64::from(request.channels))
        .ok_or_else(|| "declared PCM sample count overflows u64".to_string())?;
    let expected_bytes = expected_samples
        .checked_mul(F64_BYTES_U64)
        .ok_or_else(|| "declared PCM byte count overflows u64".to_string())?;
    let block_samples = request
        .block_frames
        .checked_mul(channel_count)
        .ok_or_else(|| "block sample count overflows usize".to_string())?;
    let block_bytes = block_samples
        .checked_mul(F64_BYTES)
        .ok_or_else(|| "block byte count overflows usize".to_string())?;
    if block_bytes > MAX_BLOCK_BYTES {
        return Err(format!(
            "block geometry exceeds the worker limit of {MAX_BLOCK_BYTES} bytes"
        ));
    }
    let block_bytes_u64 = u64::try_from(block_bytes)
        .map_err(|_| "block byte count cannot be represented as u64".to_string())?;

    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut remaining = expected_bytes;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(block_bytes)
        .map_err(|_| "cannot allocate the requested PCM byte block".to_string())?;
    bytes.resize(block_bytes, 0);
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(block_samples)
        .map_err(|_| "cannot allocate the requested PCM sample block".to_string())?;

    while remaining > 0 {
        let chunk_bytes = usize::try_from(remaining.min(block_bytes_u64))
            .map_err(|_| "next PCM block cannot be represented on this platform".to_string())?;
        input
            .read_exact(&mut bytes[..chunk_bytes])
            .map_err(|error| format!("stdin ended before the declared PCM length: {error}"))?;

        samples.clear();
        for encoded in bytes[..chunk_bytes].chunks_exact(F64_BYTES) {
            let encoded: [u8; F64_BYTES] = encoded
                .try_into()
                .map_err(|_| "PCM block is not f64-aligned".to_string())?;
            samples.push(f64::from_le_bytes(encoded));
        }
        session
            .push_interleaved(&samples)
            .map_err(|error| error.to_string())?;
        remaining -= u64::try_from(chunk_bytes)
            .map_err(|_| "PCM block length cannot be represented as u64".to_string())?;
    }

    reject_trailing_stdin(&mut input)?;
    let result = session.finish().map_err(|error| error.to_string())?;
    if result.frames_seen() != request.frames {
        return Err("analyzer frame count differs from the declared input".to_string());
    }

    let core_bits = core_bit_projection(&result);
    let record = json!({
        "schemaVersion": 1,
        "kind": "macinmeter_candidate_v1_conformance_result",
        "inputId": request.input_id,
        "input": {
            "sampleRateHz": request.sample_rate_hz,
            "channels": request.channels,
            "frames": request.frames,
            "blockFrames": request.block_frames,
            "sampleEncoding": "f64le-interleaved",
        },
        "algorithm": result.algorithm(),
        "coreBits": core_bits,
        "analysis": result,
        "claims": {
            "scope": "decoder-independent MacinMeter Candidate V1 analysis",
            "compatibility": "unverified",
            "referenceParity": "not_assessed",
        },
    });

    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer_pretty(&mut output, &record)
        .map_err(|error| format!("cannot serialize worker result: {error}"))?;
    output
        .write_all(b"\n")
        .map_err(|error| format!("cannot write worker result: {error}"))
}

fn parse_request(arguments: Vec<String>) -> Result<Request, String> {
    if arguments.len() != ARGUMENT_COUNT {
        return Err("expected INPUT_ID SAMPLE_RATE_HZ CHANNELS FRAMES BLOCK_FRAMES".to_string());
    }
    let input_id = arguments[0].clone();
    if input_id.is_empty()
        || !input_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("input ID must contain only ASCII letters, digits, '-' or '_'".to_string());
    }

    let sample_rate_hz = parse_positive(&arguments[1], "sample rate")?;
    let channels = parse_positive(&arguments[2], "channel count")?;
    if channels > MAX_ANALYSIS_CHANNELS {
        return Err(format!(
            "channel count exceeds the product maximum of {MAX_ANALYSIS_CHANNELS}"
        ));
    }
    let frames = arguments[3]
        .parse::<u64>()
        .map_err(|_| "frames must be an unsigned 64-bit integer".to_string())?;
    let block_frames = parse_positive(&arguments[4], "block frames")?;

    Ok(Request {
        input_id,
        sample_rate_hz,
        channels,
        frames,
        block_frames,
    })
}

fn parse_positive<T>(value: &str, label: &str) -> Result<T, String>
where
    T: std::str::FromStr + Default + PartialEq,
{
    let parsed = value
        .parse::<T>()
        .map_err(|_| format!("{label} is not a valid positive integer"))?;
    if parsed == T::default() {
        return Err(format!("{label} must be greater than zero"));
    }
    Ok(parsed)
}

fn reject_trailing_stdin(input: &mut impl Read) -> Result<(), String> {
    let mut trailing = [0_u8; 1];
    loop {
        match input.read(&mut trailing) {
            Ok(0) => return Ok(()),
            Ok(_) => return Err("stdin contains bytes after the declared PCM length".to_string()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(format!("cannot check stdin termination: {error}")),
        }
    }
}

fn f32_bits(value: f32) -> String {
    format!("{:08x}", value.to_bits())
}

fn core_bit_projection(result: &macinmeter_domain::AnalysisResult) -> Value {
    let channels = result
        .channels()
        .iter()
        .map(|channel| {
            let (outcome, dr_bits) = match &channel.outcome {
                ChannelOutcome::Measured { measurement } => {
                    ("measured", Some(f32_bits(measurement.dr_db.get())))
                }
                ChannelOutcome::Silent { .. } => ("silent", Some(f32_bits(0.0))),
                ChannelOutcome::InsufficientData { .. } => ("insufficient_data", None),
            };
            json!({
                "index": channel.channel_index,
                "outcome": outcome,
                "drBits": dr_bits,
                "rmsBits": f32_bits(channel.report.overall_rms_linear.get()),
                "peakBits": f32_bits(channel.report.primary_peak_linear.get()),
            })
        })
        .collect::<Vec<_>>();

    json!({
        "trackDrBits": result
            .aggregates()
            .track
            .dr_db
            .map(|value| f32_bits(value.get())),
        "channelResults": channels,
    })
}
