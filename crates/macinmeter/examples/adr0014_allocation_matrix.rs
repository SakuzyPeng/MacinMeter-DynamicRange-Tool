#![forbid(unsafe_code)]

//! ADR-0014 decode allocation equivalence matrix.
//!
//! ADR-0014's common graduation gate requires the decoded `f64`, the
//! `AnalysisResult` raw bits, and the wire-visible report to be identical
//! across every worker count and every reorder permit on the same input. The
//! committed correctness fixtures are only seconds long, so this drives the
//! same matrix over the untracked long corpus track while verifying that every
//! non-serial cell actually selected that route's packet-worker engine.
//!
//! This is a correctness harness, not a benchmark. It reports fingerprints, not
//! timings, and it drives `codecs` through an explicit allocation, so it says
//! nothing about the `Application` enablement path.

use macinmeter::{
    AggregateResults, AlgorithmDescriptor, AnalysisReport, AnalysisResult, AnalyzerSession,
    ChannelOutcome, ChannelResult, ExclusionReason, FiniteF32, FiniteF64, SourceCodec, StreamSpec,
    TrackReportMetrics, WireEnvelope,
};
use macinmeter_codecs::{DecodeEngineKind, DecoderFactory, ReadOutcome};
use macinmeter_domain::{
    ChannelLayout, DecodeReservation, MAX_DECODE_QUEUE_CAPACITY, MAX_IN_FLIGHT_PCM_BYTES,
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    env,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    process::ExitCode,
};

/// Worker counts the matrix sweeps.
const WORKER_COUNTS: [usize; 4] = [1, 2, 4, 8];

/// Mirrors of the crate-private application plan derivation.
const QUEUE_DEPTH_PER_WORKER: usize = 4;
const IN_FLIGHT_PCM_BYTES_PER_WORKER: u64 = 4 * 1024 * 1024;

fn main() -> ExitCode {
    match run() {
        Ok(output) => match canonical_json(&output) {
            Ok(serialized) => {
                println!("{serialized}");
                ExitCode::SUCCESS
            }
            Err(error) => fail(format!("failed to serialize matrix output: {error}")),
        },
        Err(error) => fail(error),
    }
}

fn fail(message: String) -> ExitCode {
    eprintln!("adr0014 allocation matrix: {message}");
    ExitCode::from(2)
}

fn run() -> Result<Value, String> {
    let mut arguments = env::args_os().skip(1);
    let path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: adr0014_allocation_matrix PATH")?;
    if arguments.next().is_some() {
        return Err("usage: adr0014_allocation_matrix PATH".to_owned());
    }

    let mut cells = Vec::new();
    for workers in WORKER_COUNTS {
        // Minimum legal, plan-derived, and the fixed product maximum. A single
        // worker collapses all three onto the serial route, which is exactly
        // the degradation the matrix has to confirm is result-neutral.
        for capacity in queue_capacities(workers) {
            let reservation = reservation(workers, capacity)?;
            let outcome = analyze_with(&path, reservation)?;
            cells.push(json!({
                "workers": workers,
                "queueCapacity": reservation.queue_capacity().get(),
                "maxInFlightPcmBytes": reservation.max_in_flight_pcm_bytes(),
                "decodedFrames": outcome.frames,
                "decodedBlocks": outcome.blocks,
                "decodedPcmF64LeSha256": outcome.decoded_pcm,
                "analysisResultRawBitsSha256": outcome.analysis_raw_bits,
                "wireReportSha256": outcome.wire_report,
            }));
        }
    }

    let distinct = |key: &str| -> BTreeSet<String> {
        cells
            .iter()
            .filter_map(|cell| cell[key].as_str().map(str::to_owned))
            .collect()
    };
    let pcm = distinct("decodedPcmF64LeSha256");
    let analysis = distinct("analysisResultRawBitsSha256");
    let wire = distinct("wireReportSha256");
    let frames: BTreeSet<u64> = cells
        .iter()
        .filter_map(|cell| cell["decodedFrames"].as_u64())
        .collect();

    // The matrix only means something if every cell agrees. A single differing
    // fingerprint is a correctness failure, not a value to be summarised.
    for (label, values) in [
        ("decoded f64", &pcm),
        ("analysis result raw bits", &analysis),
        ("wire report", &wire),
    ] {
        if values.len() != 1 {
            return Err(format!(
                "{label} differs across the allocation matrix: {values:?}"
            ));
        }
    }
    if frames.len() != 1 {
        return Err(format!("decoded frame count differs: {frames:?}"));
    }

    Ok(json!({
        "kind": "adr0014_allocation_matrix",
        "schemaVersion": 2,
        "path": display_name(&path),
        "cells": cells,
        "decodedPcmF64LeSha256": pcm.iter().next(),
        "analysisResultRawBitsSha256": analysis.iter().next(),
        "wireReportSha256": wire.iter().next(),
        "decodedFrames": frames.iter().next(),
    }))
}

fn queue_capacities(workers: usize) -> Vec<usize> {
    let derived = (workers * QUEUE_DEPTH_PER_WORKER).min(MAX_DECODE_QUEUE_CAPACITY);
    let mut capacities = vec![workers, derived, MAX_DECODE_QUEUE_CAPACITY];
    capacities.sort_unstable();
    capacities.dedup();
    capacities
}

fn reservation(workers: usize, capacity: usize) -> Result<DecodeReservation, String> {
    let workers = NonZeroUsize::new(workers).ok_or("worker count must be greater than zero")?;
    let capacity = NonZeroUsize::new(capacity).ok_or("queue capacity must be greater than zero")?;
    if workers.get() == 1 && capacity.get() == 1 {
        return Ok(DecodeReservation::serial());
    }
    let in_flight = IN_FLIGHT_PCM_BYTES_PER_WORKER
        .saturating_mul(workers.get() as u64)
        .min(MAX_IN_FLIGHT_PCM_BYTES);
    DecodeReservation::new(workers, capacity, in_flight).map_err(|error| error.to_string())
}

struct MatrixOutcome {
    frames: u64,
    blocks: u64,
    decoded_pcm: String,
    analysis_raw_bits: String,
    wire_report: String,
}

fn analyze_with(path: &Path, reservation: DecodeReservation) -> Result<MatrixOutcome, String> {
    let (mut opened, execution) = DecoderFactory::with_application_reservation(reservation)
        .open_with_execution(path)
        .map_err(|error| error.to_string())?;
    // Graduated routes are named, never inferred: a matrix run over a route
    // that falls back to serial would report one fingerprint per cell and prove
    // nothing about allocation.
    let workers_engine = match opened.source.codec {
        SourceCodec::Alac => DecodeEngineKind::AlacPacketWorkers,
        SourceCodec::Flac => DecodeEngineKind::FlacPacketWorkers,
        other => {
            return Err(format!(
                "allocation matrix requires a graduated packet route, found {other:?}"
            ));
        }
    };
    let expected_engine = if reservation.workers().get() == 1 {
        DecodeEngineKind::Serial
    } else {
        workers_engine
    };
    if execution.engine() != expected_engine || execution.workers() != reservation.workers() {
        return Err(format!(
            "requested {} workers but decoder selected {:?} with {} workers",
            reservation.workers(),
            execution.engine(),
            execution.workers()
        ));
    }
    let pcm = opened.reader.stream_info().clone();
    let mut session = AnalyzerSession::new(pcm.spec.clone()).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut frames = 0_u64;
    let mut blocks = 0_u64;

    while let ReadOutcome::Data(block) = opened
        .reader
        .read_block()
        .map_err(|error| error.to_string())?
    {
        for sample in block.samples() {
            digest.update(sample.to_bits().to_le_bytes());
        }
        session
            .push_interleaved(block.samples())
            .map_err(|error| error.to_string())?;
        frames += u64::try_from(block.frames()).map_err(|error| error.to_string())?;
        blocks += 1;
    }

    let analysis = session.finish().map_err(|error| error.to_string())?;
    let analysis_raw_bits = analysis_raw_bits(&analysis);
    let diagnostics = opened.reader.diagnostics().clone();
    // SourceInfo retains the caller's display path for the product. This
    // source-bound harness normalizes that incidental spelling so relative,
    // absolute and parent-containing paths produce the same wire fingerprint.
    opened.source.display_path = display_name(path);
    let report = AnalysisReport::try_new(opened.source, pcm, analysis, diagnostics)
        .map_err(|error| error.to_string())?;
    let wire = serde_json::to_string(&WireEnvelope::analysis(report))
        .map_err(|error| error.to_string())?;

    Ok(MatrixOutcome {
        frames,
        blocks,
        decoded_pcm: hex(digest.finalize().as_slice()),
        analysis_raw_bits,
        wire_report: hex(Sha256::digest(wire.as_bytes()).as_slice()),
    })
}

/// Serialize the committed record with stable key order and four-space indent.
fn canonical_json(value: &Value) -> Result<String, String> {
    let value = sorted_json(value);
    let mut bytes = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, formatter);
    value
        .serialize(&mut serializer)
        .map_err(|error| error.to_string())?;
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

fn sorted_json(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(sorted_json).collect()),
        Value::Object(fields) => {
            let mut names: Vec<_> = fields.keys().collect();
            names.sort_unstable();
            let mut sorted = serde_json::Map::new();
            for name in names {
                sorted.insert(name.clone(), sorted_json(&fields[name]));
            }
            Value::Object(sorted)
        }
        value => value.clone(),
    }
}

/// Hash the complete result graph by IEEE-754 bit pattern.
///
/// The wire fingerprint alone would compare rendered decimal text. Walking the
/// exhaustive view and feeding `to_bits` keeps the comparison at raw bits, so a
/// difference below the printed precision cannot pass as equal.
fn analysis_raw_bits(analysis: &AnalysisResult) -> String {
    let view = analysis.view();
    let mut digest = Sha256::new();
    descriptor_bits(&mut digest, view.algorithm);
    stream_bits(&mut digest, view.stream);
    digest.update(view.frames_seen.to_le_bytes());
    digest.update((view.channels.len() as u64).to_le_bytes());
    for channel in view.channels {
        channel_bits(&mut digest, channel);
    }
    aggregate_bits(&mut digest, view.aggregates);
    track_report_bits(&mut digest, view.report);
    hex(digest.finalize().as_slice())
}

fn descriptor_bits(digest: &mut Sha256, descriptor: &AlgorithmDescriptor) {
    let parameters = &descriptor.parameters;
    f64_bits(digest, parameters.window_duration_coefficient);
    f64_bits(digest, parameters.rms_sum_multiplier);
    digest.update((parameters.histogram_bins as u64).to_le_bytes());
    f64_bits(digest, parameters.rms_histogram_min_db);
    f64_bits(digest, parameters.rms_histogram_max_db);
    f64_bits(digest, parameters.histogram_bin_width_db);
    f64_bits(digest, parameters.peak_key_bin_width_db);
    f64_bits(digest, parameters.loud_fraction);
    digest.update((parameters.minimum_tail_frames as u64).to_le_bytes());
    digest.update([u8::from(parameters.include_entire_boundary_bin)]);
    digest.update([u8::from(parameters.exact_window_virtual_zero_peak)]);
    f64_bits(digest, parameters.dr_floor_db);
    f64_bits(digest, parameters.silent_channel_dr_db);
    digest.update([u8::from(parameters.includes_lfe_in_track_aggregate)]);
    digest.update(parameters.result_precision_bits.to_le_bytes());
}

fn stream_bits(digest: &mut Sha256, stream: &StreamSpec) {
    digest.update(stream.sample_rate.get().to_le_bytes());
    digest.update(stream.channels.get().to_le_bytes());
    layout_bits(digest, &stream.channel_layout);
}

fn layout_bits(digest: &mut Sha256, layout: &ChannelLayout) {
    match layout {
        ChannelLayout::Unknown => digest.update([0]),
        ChannelLayout::KnownNoLfe => digest.update([1]),
        ChannelLayout::Known { positions } => {
            digest.update([2]);
            digest.update((positions.len() as u64).to_le_bytes());
            for role in positions {
                digest.update([role_tag(role)]);
            }
        }
    }
}

/// A stable, explicit tag per role.
///
/// Casting the enum would silently follow any future reordering of its
/// variants, which would change this fingerprint without any result changing.
const fn role_tag(role: &macinmeter::ChannelRole) -> u8 {
    use macinmeter::ChannelRole;
    match role {
        ChannelRole::FrontLeft => 0,
        ChannelRole::FrontRight => 1,
        ChannelRole::FrontCenter => 2,
        ChannelRole::Lfe => 3,
        ChannelRole::BackLeft => 4,
        ChannelRole::BackRight => 5,
        ChannelRole::SideLeft => 6,
        ChannelRole::SideRight => 7,
        ChannelRole::Other => 8,
    }
}

fn channel_bits(digest: &mut Sha256, channel: &ChannelResult) {
    digest.update((channel.channel_index as u64).to_le_bytes());
    f32_bits(digest, channel.report.overall_rms_linear);
    optional_f32_bits(digest, channel.report.overall_rms_dbfs);
    f32_bits(digest, channel.report.primary_peak_linear);
    match &channel.outcome {
        ChannelOutcome::Measured { measurement } => {
            digest.update([0]);
            f32_bits(digest, measurement.dr_db);
            digest.update(measurement.rounded_dr.to_le_bytes());
            f64_bits(digest, measurement.loud_window_rms);
            f64_bits(digest, measurement.dr_selected_peak);
            f64_bits(digest, measurement.dr_primary_peak);
            optional_f64_bits(digest, measurement.dr_secondary_peak);
            digest.update(measurement.valid_windows.to_le_bytes());
            digest.update(measurement.frames.to_le_bytes());
        }
        ChannelOutcome::Silent {
            frames,
            valid_windows,
        } => {
            digest.update([1]);
            digest.update(frames.to_le_bytes());
            digest.update(valid_windows.to_le_bytes());
        }
        ChannelOutcome::InsufficientData { frames } => {
            digest.update([2]);
            digest.update(frames.to_le_bytes());
        }
    }
}

fn aggregate_bits(digest: &mut Sha256, aggregates: &AggregateResults) {
    let track = &aggregates.track;
    optional_f32_bits(digest, track.dr_db);
    match track.rounded_dr {
        Some(value) => {
            digest.update([1]);
            digest.update(value.to_le_bytes());
        }
        None => digest.update([0]),
    }
    digest.update((track.contributing_channels.len() as u64).to_le_bytes());
    for index in &track.contributing_channels {
        digest.update((*index as u64).to_le_bytes());
    }
    digest.update((track.excluded_channels.len() as u64).to_le_bytes());
    for excluded in &track.excluded_channels {
        digest.update((excluded.channel_index as u64).to_le_bytes());
        match excluded.reason {
            ExclusionReason::InsufficientData => digest.update([0]),
        }
    }
}

fn track_report_bits(digest: &mut Sha256, report: &TrackReportMetrics) {
    f64_bits(digest, report.overall_rms_linear);
    optional_f32_bits(digest, report.overall_rms_dbfs);
    f32_bits(digest, report.primary_peak_linear);
    optional_f32_bits(digest, report.primary_peak_dbfs);
    digest.update(report.duration.decoded_frames.to_le_bytes());
    digest.update(report.duration.sample_rate.get().to_le_bytes());
}

fn f64_bits(digest: &mut Sha256, value: FiniteF64) {
    digest.update(value.get().to_bits().to_le_bytes());
}

fn f32_bits(digest: &mut Sha256, value: FiniteF32) {
    digest.update(value.get().to_bits().to_le_bytes());
}

fn optional_f64_bits(digest: &mut Sha256, value: Option<FiniteF64>) {
    match value {
        Some(value) => {
            digest.update([1]);
            f64_bits(digest, value);
        }
        None => digest.update([0]),
    }
}

fn optional_f32_bits(digest: &mut Sha256, value: Option<FiniteF32>) {
    match value {
        Some(value) => {
            digest.update([1]);
            f32_bits(digest, value);
        }
        None => digest.update([0]),
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let packaged = manifest.join("package-fixtures");
        let fixtures = if packaged.is_dir() {
            packaged
        } else {
            manifest.join("../../tests/fixtures")
        };
        fixtures.join(name)
    }

    #[test]
    fn matrix_rejects_routes_that_cannot_start_packet_workers() {
        let error = match analyze_with(
            &fixture("native-pcm-v1/wav-pcm-s32-stereo.wav"),
            DecodeReservation::serial(),
        ) {
            Ok(_) => panic!("a WAV source must not claim packet allocation equivalence"),
            Err(error) => error,
        };
        assert!(
            error.contains("requires a graduated packet route"),
            "{error}"
        );
    }

    #[test]
    fn wire_fingerprint_is_independent_of_input_path_spelling() {
        let path = fixture("native-alac-v1/alac16-stereo-48000-multipacket.m4a");
        let canonical = path.canonicalize().unwrap();
        let parent_spelling = path
            .parent()
            .unwrap()
            .join("..")
            .join("native-alac-v1/alac16-stereo-48000-multipacket.m4a");

        let canonical = analyze_with(&canonical, DecodeReservation::serial()).unwrap();
        let parent_spelling = analyze_with(&parent_spelling, DecodeReservation::serial()).unwrap();
        assert_eq!(canonical.decoded_pcm, parent_spelling.decoded_pcm);
        assert_eq!(
            canonical.analysis_raw_bits,
            parent_spelling.analysis_raw_bits
        );
        assert_eq!(canonical.wire_report, parent_spelling.wire_report);
    }

    #[test]
    fn canonical_output_is_sorted_and_uses_four_space_indent() {
        let value = json!({"z": 1, "a": {"d": 2, "b": 3}});
        assert_eq!(
            canonical_json(&value).unwrap(),
            "{\n    \"a\": {\n        \"b\": 3,\n        \"d\": 2\n    },\n    \"z\": 1\n}"
        );
    }
}
