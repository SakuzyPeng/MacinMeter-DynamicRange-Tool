#![forbid(unsafe_code)]

use macinmeter::{
    AnalysisEvent, AnalysisProfile, AnalyzeRequest, Analyzer, BatchItemOutcome, BatchRequest,
    BatchRunner, BatchStatus, CancellationToken, ErrorCode, ExecutionControl, NoopProgressSink,
    WIRE_SCHEMA_VERSION, WireEnvelope, WirePayload,
};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

fn run_batch(inputs: Vec<PathBuf>) -> macinmeter::BatchReport {
    let cancellation = CancellationToken::new();
    let progress = NoopProgressSink;
    BatchRunner::new()
        .run(
            BatchRequest::new(inputs, false),
            &ExecutionControl::new(&cancellation, &progress),
        )
        .expect("batch should produce an item outcome for ordinary media failures")
}

#[test]
fn rust_api_analyzes_a_repository_wave_fixture() {
    let path = fixture("tiny_duration.wav");
    let report = Analyzer::new()
        .analyze_file(AnalyzeRequest::new(&path))
        .expect("valid PCM WAV fixture should analyze");

    assert_eq!(report.source.display_path, path.display().to_string());
    assert_eq!(report.source.sample_rate.get(), 44_100);
    assert_eq!(report.source.channels.get(), 2);
    assert_eq!(report.source.expected_frames, Some(441));
    assert_eq!(report.pcm.expected_frames, Some(441));
    assert_eq!(report.analysis.frames_seen, 441);
    assert_eq!(report.analysis.channels.len(), 2);
    assert_eq!(
        report.analysis.algorithm.profile,
        AnalysisProfile::ProvisionalV1
    );
    assert_eq!(report.diagnostics.decoded_frames, 441);
    assert!(report.diagnostics.warnings.is_empty());
}

#[test]
fn rust_api_distinguishes_unsupported_content_and_truncated_media() {
    let analyzer = Analyzer::new();

    let unsupported = analyzer
        .analyze_file(AnalyzeRequest::new(fixture("fake_audio.wav")))
        .expect_err("text with a WAV extension must not pass content probing");
    assert_eq!(unsupported.code, ErrorCode::UnsupportedFormat);

    let truncated = analyzer
        .analyze_file(AnalyzeRequest::new(fixture("truncated.wav")))
        .expect_err("a truncated WAV must not yield a partial report");
    assert_eq!(truncated.code, ErrorCode::MalformedMedia);
    assert_eq!(truncated.stage, macinmeter::AnalysisStage::Probe);
}

#[test]
fn batch_is_serial_and_preserves_explicit_input_order() {
    let first = fixture("full_scale_clipping.wav");
    let second = fixture("tiny_duration.wav");
    let report = run_batch(vec![first.clone(), second.clone()]);

    assert_eq!(report.status, BatchStatus::Succeeded);
    assert_eq!(report.summary.total, 2);
    assert_eq!(report.summary.succeeded, 2);
    assert_eq!(report.summary.failed, 0);
    assert_eq!(report.items[0].display_path, first.display().to_string());
    assert_eq!(report.items[1].display_path, second.display().to_string());
    assert!(
        report
            .items
            .iter()
            .all(|item| matches!(item.outcome, BatchItemOutcome::Success { .. }))
    );
}

#[test]
fn batch_reports_full_partial_and_zero_success_without_short_circuiting() {
    let valid = fixture("tiny_duration.wav");
    let unsupported = fixture("fake_audio.wav");
    let truncated = fixture("truncated.wav");

    let succeeded = run_batch(vec![valid.clone()]);
    assert_eq!(succeeded.status, BatchStatus::Succeeded);
    assert_eq!(
        (succeeded.summary.succeeded, succeeded.summary.failed),
        (1, 0)
    );

    let partial = run_batch(vec![unsupported.clone(), valid]);
    assert_eq!(partial.status, BatchStatus::PartiallySucceeded);
    assert_eq!((partial.summary.succeeded, partial.summary.failed), (1, 1));
    assert!(matches!(
        partial.items[0].outcome,
        BatchItemOutcome::Failure { .. }
    ));
    assert!(matches!(
        partial.items[1].outcome,
        BatchItemOutcome::Success { .. }
    ));

    let failed = run_batch(vec![unsupported, truncated]);
    assert_eq!(failed.status, BatchStatus::Failed);
    assert_eq!((failed.summary.succeeded, failed.summary.failed), (0, 2));
    assert!(
        failed
            .items
            .iter()
            .all(|item| matches!(item.outcome, BatchItemOutcome::Failure { .. }))
    );
}

#[test]
fn batch_rejects_an_empty_request_before_processing() {
    let cancellation = CancellationToken::new();
    let progress = NoopProgressSink;
    let error = BatchRunner::new()
        .run(
            BatchRequest::new(Vec::new(), false),
            &ExecutionControl::new(&cancellation, &progress),
        )
        .expect_err("an empty batch request must not report success");

    assert_eq!(error.code, ErrorCode::NoInputs);
}

#[test]
fn request_cancellation_stops_batch_before_a_result_is_published() {
    let cancellation = CancellationToken::new();
    let token_for_sink = cancellation.clone();
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_for_sink = Arc::clone(&events);
    let progress = move |event: AnalysisEvent| {
        if matches!(event, AnalysisEvent::FileStarted { .. }) {
            token_for_sink.cancel();
        }
        events_for_sink
            .lock()
            .expect("event lock should not be poisoned")
            .push(event);
    };
    let control = ExecutionControl::new(&cancellation, &progress);

    let error = BatchRunner::new()
        .run(
            BatchRequest::new(vec![fixture("edge_cases.wav")], false),
            &control,
        )
        .expect_err("cancellation requested at file start must abort the batch");

    assert_eq!(error.code, ErrorCode::Cancelled);
    let events = events.lock().expect("event lock should not be poisoned");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AnalysisEvent::FileStarted { index: 0, .. }))
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AnalysisEvent::BatchFinished { .. }))
    );
}

#[test]
fn cancellation_requested_at_discovery_start_stops_before_walking_inputs() {
    let cancellation = CancellationToken::new();
    let token_for_sink = cancellation.clone();
    let progress = move |event: AnalysisEvent| {
        if matches!(event, AnalysisEvent::DiscoveryStarted) {
            token_for_sink.cancel();
        }
    };

    let error = BatchRunner::new()
        .run(
            BatchRequest::new(vec![fixture("tiny_duration.wav")], false),
            &ExecutionControl::new(&cancellation, &progress),
        )
        .expect_err("cancellation at discovery start must stop the batch");

    assert_eq!(error.code, ErrorCode::Cancelled);
}

#[test]
fn analysis_emits_a_terminal_eof_progress_event() {
    let cancellation = CancellationToken::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_for_sink = Arc::clone(&events);
    let progress = move |event: AnalysisEvent| {
        events_for_sink.lock().unwrap().push(event);
    };

    Analyzer::new()
        .analyze_file_with_control(
            AnalyzeRequest::new(fixture("tiny_duration.wav")),
            &ExecutionControl::new(&cancellation, &progress),
        )
        .unwrap();

    let events = events.lock().unwrap();
    assert!(events.iter().any(|event| matches!(
        event,
        AnalysisEvent::DecodeProgress {
            progress: macinmeter::DecodeProgress { eof: true, .. },
            ..
        }
    )));
}

#[test]
fn wire_envelopes_have_a_stable_finite_timestamp_free_schema() {
    let report = Analyzer::new()
        .analyze_file(AnalyzeRequest::new(fixture("tiny_duration.wav")))
        .expect("valid fixture should analyze");
    let envelope = WireEnvelope::analysis(report);

    assert_eq!(envelope.schema_version, WIRE_SCHEMA_VERSION);
    assert_eq!(envelope.tool_version, macinmeter::VERSION);
    assert!(matches!(envelope.payload, WirePayload::Analysis(_)));

    let value = serde_json::to_value(&envelope).expect("wire report should serialize");
    assert_eq!(value["schemaVersion"], WIRE_SCHEMA_VERSION);
    assert_eq!(value["toolVersion"], macinmeter::VERSION);
    assert_eq!(value["kind"], "analysis");
    assert!(value.get("data").is_some());
    assert_json_contract(&value);

    let batch = run_batch(vec![
        fixture("tiny_duration.wav"),
        fixture("fake_audio.wav"),
    ]);
    let batch_value =
        serde_json::to_value(WireEnvelope::batch(batch)).expect("wire batch should serialize");
    let items = batch_value["data"]["items"]
        .as_array()
        .expect("batch items should be an array");
    assert_eq!(items[0]["outcome"]["status"], "success");
    assert!(items[0]["outcome"].get("report").is_some());
    assert!(items[0]["outcome"].get("error").is_none());
    assert_eq!(items[1]["outcome"]["status"], "failure");
    assert!(items[1]["outcome"].get("error").is_some());
    assert!(items[1]["outcome"].get("report").is_none());
    assert_json_contract(&batch_value);

    let event = serde_json::to_value(AnalysisEvent::FileStarted {
        index: 7,
        display_path: "track.wav".to_string(),
    })
    .unwrap();
    assert_eq!(event["type"], "file_started");
    assert_eq!(event["displayPath"], "track.wav");
    assert!(event.get("display_path").is_none());

    let silent = serde_json::to_value(macinmeter::ChannelOutcome::Silent {
        frames: 12,
        valid_windows: 3,
    })
    .unwrap();
    assert_eq!(silent["status"], "silent");
    assert_eq!(silent["validWindows"], 3);
    assert!(silent.get("valid_windows").is_none());
}

fn assert_json_contract(value: &Value) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                assert!(
                    !key.to_ascii_lowercase().contains("timestamp"),
                    "wire schema unexpectedly contains timestamp-like field {key}"
                );
                assert_json_contract(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                assert_json_contract(child);
            }
        }
        Value::Number(number) => {
            if let Some(value) = number.as_f64() {
                assert!(value.is_finite(), "wire schema contains non-finite number");
            }
        }
        Value::Null | Value::Bool(_) | Value::String(_) => {}
    }
}
