#![forbid(unsafe_code)]

use macinmeter::{
    AnalysisEvent, AnalyzeRequest, Application, BatchItemOutcome, BatchRequest, BatchStatus,
    CancellationToken, ContainerFormat, DecodeProgress, ErrorCode, ExecutionControl,
    NoopProgressSink, SourceCodec, WIRE_SCHEMA_VERSION, WireEnvelope, WirePayload,
};
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

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

fn run_batch(inputs: Vec<PathBuf>) -> macinmeter::BatchReport {
    let cancellation = CancellationToken::new();
    let progress = NoopProgressSink;
    Application::new()
        .run_batch(
            BatchRequest::new(inputs, false),
            &ExecutionControl::new(&cancellation, &progress),
        )
        .expect("batch should produce an item outcome for ordinary media failures")
}

#[test]
fn timing_is_opt_in_and_leaves_the_report_untouched() {
    let path = fixture("edge_cases.wav");
    let cancellation = CancellationToken::new();
    let progress = NoopProgressSink;
    let control = ExecutionControl::new(&cancellation, &progress);
    let application = Application::new();

    let plain = application
        .analyze_file(AnalyzeRequest::new(&path))
        .expect("the fixture should analyze untimed");
    let (timed, phases) = application
        .analyze_file_timed(AnalyzeRequest::new(&path), &control)
        .expect("the fixture should analyze timed");

    // Observation must not reach the result. Both entries produce the same
    // wire document, so nothing downstream can tell which one ran.
    assert_eq!(
        serde_json::to_value(WireEnvelope::analysis(plain)).unwrap(),
        serde_json::to_value(WireEnvelope::analysis(timed)).unwrap()
    );
    assert!(phases.decode() > std::time::Duration::ZERO, "{phases:?}");
    assert!(phases.analysis() > std::time::Duration::ZERO, "{phases:?}");
    // A role's span is the only thing these measurements partition exactly:
    // it contains that role's own activity by construction. The two spans are
    // not comparable this way, and their intersection is not recoverable.
    assert!(phases.decode_span() >= phases.decode(), "{phases:?}");
    assert!(phases.analysis_span() >= phases.analysis(), "{phases:?}");

    let (_, batch_phases) = application
        .run_batch_timed(BatchRequest::new(vec![path], false), &control)
        .expect("the fixture should batch timed");
    assert!(batch_phases.decode() > std::time::Duration::ZERO);
    assert!(batch_phases.analysis() > std::time::Duration::ZERO);
    assert!(batch_phases.decode_span() >= batch_phases.decode());
    assert!(batch_phases.analysis_span() >= batch_phases.analysis());
}

#[test]
fn timed_batch_keeps_phase_work_from_a_failed_item() {
    let path = fixture("malformed-media-v1/flac-frame-byte-flip.flac");
    let cancellation = CancellationToken::new();
    let progress = NoopProgressSink;
    let control = ExecutionControl::new(&cancellation, &progress);

    let (report, phases) = Application::new()
        .run_batch_timed(BatchRequest::new(vec![path], false), &control)
        .expect("an ordinary decode failure should remain a batch item outcome");

    assert_eq!(report.status, BatchStatus::Failed);
    assert_eq!((report.summary.succeeded, report.summary.failed), (0, 1));
    assert!(matches!(
        report.items[0].outcome,
        BatchItemOutcome::Failure { .. }
    ));
    assert!(
        phases.decode() > std::time::Duration::ZERO,
        "the decoder work before the integrity failure must be retained: {phases:?}"
    );
    assert!(
        phases.analysis() > std::time::Duration::ZERO,
        "the analyzed prefix before the integrity failure must be retained: {phases:?}"
    );
    assert!(phases.decode_span() >= phases.decode(), "{phases:?}");
    assert!(phases.analysis_span() >= phases.analysis(), "{phases:?}");
}

fn assert_only_partial_window_warning(warnings: &[String]) {
    assert_eq!(
        warnings.len(),
        1,
        "unexpected report warnings: {warnings:?}"
    );
    assert!(warnings[0].contains("one partial window"), "{warnings:?}");
}

#[test]
fn rust_api_analyzes_a_repository_wave_fixture() {
    let path = fixture("tiny_duration.wav");
    let report = Application::new()
        .analyze_file(AnalyzeRequest::new(&path))
        .expect("valid PCM WAV fixture should analyze");

    let source = report.source();
    let pcm = report.pcm();
    let analysis = report.analysis();
    let diagnostics = report.diagnostics();
    assert_eq!(source.display_path, path.display().to_string());
    assert_eq!(source.sample_rate.get(), 44_100);
    assert_eq!(source.channels.get(), 2);
    assert_eq!(source.expected_frames, Some(441));
    assert_eq!(pcm.expected_frames, Some(441));
    assert_eq!(source.channels, pcm.spec.channels);
    assert_eq!(pcm.spec.channels, analysis.stream().channels);
    assert_eq!(analysis.frames_seen(), 441);
    assert_eq!(analysis.channels().len(), 2);
    assert_eq!(analysis.algorithm().parameters.histogram_bins, 10_001);
    assert_eq!(diagnostics.decoded_frames, 441);
    assert_only_partial_window_warning(&diagnostics.warnings);
}

#[test]
fn rust_api_analyzes_the_product_aiff_and_flac_routes() {
    for (relative_path, container, codec, bits_per_sample, sample_rate, channels, frames) in [
        (
            "native-pcm-v1/aiff-pcm-s24-stereo.aiff",
            ContainerFormat::Aiff,
            SourceCodec::PcmInteger,
            24,
            44_100,
            2,
            4,
        ),
        (
            "native-pcm-v1/flac-pcm-s16-stereo-multiblock.flac",
            ContainerFormat::Flac,
            SourceCodec::Flac,
            16,
            8_000,
            2,
            400,
        ),
    ] {
        let path = fixture(relative_path);
        let report = Application::new()
            .analyze_file(AnalyzeRequest::new(&path))
            .unwrap_or_else(|error| panic!("{relative_path} should analyze: {error}"));

        let source = report.source();
        let pcm = report.pcm();
        let analysis = report.analysis();
        let diagnostics = report.diagnostics();
        assert_eq!(source.display_path, path.display().to_string());
        assert_eq!(source.container, container);
        assert_eq!(source.codec, codec);
        assert_eq!(source.bits_per_sample, Some(bits_per_sample));
        assert_eq!(source.sample_rate.get(), sample_rate);
        assert_eq!(source.channels.get(), channels);
        assert_eq!(source.expected_frames, Some(frames));
        assert_eq!(pcm.expected_frames, Some(frames));
        assert_eq!(source.sample_rate, pcm.spec.sample_rate);
        assert_eq!(source.channels, pcm.spec.channels);
        assert_eq!(pcm.spec, *analysis.stream());
        assert_eq!(analysis.frames_seen(), frames);
        assert_eq!(analysis.channels().len(), usize::from(channels));
        assert_eq!(diagnostics.backend, "symphonia");
        assert_eq!(diagnostics.decoded_frames, frames);
        assert_only_partial_window_warning(&diagnostics.warnings);
    }
}

#[test]
fn rust_api_analyzes_representative_m4a_and_mp4_alac_routes() {
    for (relative_path, bits_per_sample, sample_rate, channels, frames) in [
        (
            "native-alac-v1/alac16-stereo-48000-multipacket.m4a",
            16,
            48_000,
            2,
            9_001,
        ),
        (
            "native-alac-v1/alac24-stereo-96000-faststart.mp4",
            24,
            96_000,
            2,
            5_003,
        ),
    ] {
        let path = fixture(relative_path);
        let report = Application::new()
            .analyze_file(AnalyzeRequest::new(&path))
            .unwrap_or_else(|error| panic!("{relative_path} should analyze: {error}"));

        assert_eq!(report.source().container, ContainerFormat::Mp4);
        assert_eq!(report.source().codec, SourceCodec::Alac);
        assert_eq!(report.source().bits_per_sample, Some(bits_per_sample));
        assert_eq!(report.source().sample_rate.get(), sample_rate);
        assert_eq!(report.source().channels.get(), channels);
        assert_eq!(report.source().expected_frames, Some(frames));
        assert_eq!(
            report.pcm().spec.channel_layout,
            macinmeter::ChannelLayout::Unknown
        );
        assert_eq!(report.analysis().frames_seen(), frames);
        assert_eq!(report.diagnostics().decoded_frames, frames);
        assert_only_partial_window_warning(&report.diagnostics().warnings);
    }
}

#[test]
fn alac_and_wave_twins_have_identical_analysis_and_normalized_reports() {
    let corpus = fixture("native-alac-v1");
    let manifest: Value = serde_json::from_slice(
        &fs::read(corpus.join("manifest.json")).expect("ALAC manifest must exist"),
    )
    .expect("ALAC manifest must be valid JSON");
    let fixtures = manifest["fixtures"]
        .as_array()
        .expect("ALAC manifest must contain fixtures");

    for alac in fixtures.iter().filter(|fixture| fixture["kind"] == "alac") {
        let alac_name = alac["path"].as_str().expect("ALAC path");
        let wave_name = alac["twin"].as_str().expect("WAV twin path");
        let alac_report = Application::new()
            .analyze_file(AnalyzeRequest::new(corpus.join(alac_name)))
            .unwrap_or_else(|error| panic!("{alac_name} failed: {error}"));
        let wave_report = Application::new()
            .analyze_file(AnalyzeRequest::new(corpus.join(wave_name)))
            .unwrap_or_else(|error| panic!("{wave_name} failed: {error}"));

        assert_eq!(
            alac_report.analysis(),
            wave_report.analysis(),
            "{alac_name}"
        );
        assert_eq!(alac_report.pcm(), wave_report.pcm(), "{alac_name}");
        let mut alac_wire = serde_json::to_value(WireEnvelope::analysis(alac_report)).unwrap();
        let mut wave_wire = serde_json::to_value(WireEnvelope::analysis(wave_report)).unwrap();
        for wire in [&mut alac_wire, &mut wave_wire] {
            wire["data"]["source"]["displayPath"] = Value::String("<twin>".to_owned());
            wire["data"]["source"]["container"] = Value::String("<container>".to_owned());
            wire["data"]["source"]["codec"] = Value::String("<codec>".to_owned());
        }
        assert_eq!(alac_wire, wave_wire, "{alac_name}");
    }
}

#[test]
fn alac_content_probe_ignores_extensions_and_rejects_fake_mp4_content() {
    let directory = tempfile::tempdir().unwrap();
    let disguised_alac = directory.path().join("alac-with-wav-extension.wav");
    fs::copy(
        fixture("native-alac-v1/alac16-mono-44100.m4a"),
        &disguised_alac,
    )
    .unwrap();
    let report = Application::new()
        .analyze_file(AnalyzeRequest::new(&disguised_alac))
        .expect("ALAC content must route independently of its extension");
    assert_eq!(report.source().container, ContainerFormat::Mp4);
    assert_eq!(report.source().codec, SourceCodec::Alac);

    let fake_mp4 = directory.path().join("fake.m4a");
    fs::write(&fake_mp4, b"not ISO BMFF").unwrap();
    let error = Application::new()
        .analyze_file(AnalyzeRequest::new(&fake_mp4))
        .expect_err("an M4A extension must not bypass content probing");
    assert_eq!(error.code, ErrorCode::UnsupportedFormat);
    assert_eq!(error.stage, macinmeter::AnalysisStage::Probe);
}

#[test]
fn extensible_and_classic_twins_have_identical_analysis_and_report_projection() {
    let corpus = fixture("native-pcm-extensible-v1");
    let manifest: Value = serde_json::from_slice(
        &fs::read(corpus.join("manifest.json")).expect("Extensible manifest must exist"),
    )
    .expect("Extensible manifest must be valid JSON");
    let fixtures = manifest["fixtures"]
        .as_array()
        .expect("Extensible manifest must contain fixtures");

    for extensible in fixtures
        .iter()
        .filter(|fixture| fixture["encapsulation"] == "wave_format_extensible")
    {
        let twin_id = extensible["twinId"].as_str().expect("twin id");
        let classic = fixtures
            .iter()
            .find(|fixture| {
                fixture["twinId"] == twin_id && fixture["encapsulation"] == "classic_wave_format"
            })
            .unwrap_or_else(|| panic!("classic twin missing for {twin_id}"));
        let extensible_path = corpus.join(extensible["path"].as_str().unwrap());
        let classic_path = corpus.join(classic["path"].as_str().unwrap());
        let extensible_report = Application::new()
            .analyze_file(AnalyzeRequest::new(&extensible_path))
            .unwrap_or_else(|error| panic!("Extensible twin {twin_id} failed: {error}"));
        let classic_report = Application::new()
            .analyze_file(AnalyzeRequest::new(&classic_path))
            .unwrap_or_else(|error| panic!("classic twin {twin_id} failed: {error}"));

        assert_eq!(
            extensible_report.analysis(),
            classic_report.analysis(),
            "complete AnalysisResult differs for {twin_id}"
        );
        assert_eq!(
            extensible_report.pcm().spec.channel_layout,
            macinmeter::ChannelLayout::Unknown,
            "Extensible channel masks must not create a known layout for {twin_id}"
        );

        let mut extensible_wire = serde_json::to_value(WireEnvelope::analysis(extensible_report))
            .expect("Extensible report must serialize");
        let mut classic_wire = serde_json::to_value(WireEnvelope::analysis(classic_report))
            .expect("classic report must serialize");
        extensible_wire["data"]["source"]["displayPath"] = Value::String("<twin>".to_owned());
        classic_wire["data"]["source"]["displayPath"] = Value::String("<twin>".to_owned());
        assert_eq!(extensible_wire["schemaVersion"], WIRE_SCHEMA_VERSION);
        assert_eq!(
            extensible_wire, classic_wire,
            "shared report projection differs for {twin_id}"
        );
    }
}

#[test]
fn rust_api_distinguishes_unsupported_content_and_truncated_media() {
    let application = Application::new();

    let unsupported = application
        .analyze_file(AnalyzeRequest::new(fixture("fake_audio.wav")))
        .expect_err("text with a WAV extension must not pass content probing");
    assert_eq!(unsupported.code, ErrorCode::UnsupportedFormat);

    let truncated = application
        .analyze_file(AnalyzeRequest::new(fixture("truncated.wav")))
        .expect_err("a truncated WAV must not yield a partial report");
    assert_eq!(truncated.code, ErrorCode::MalformedMedia);
    assert_eq!(truncated.stage, macinmeter::AnalysisStage::Probe);
}

#[test]
fn product_batch_preserves_explicit_input_order_across_file_lanes() {
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
fn batch_publishes_each_final_item_before_the_terminal_event() {
    let inputs = vec![fixture("tiny_duration.wav"), fixture("fake_audio.wav")];
    let cancellation = CancellationToken::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_for_sink = Arc::clone(&events);
    let progress = move |event: AnalysisEvent| {
        events_for_sink.lock().unwrap().push(event);
    };
    let report = Application::new()
        .run_batch(
            BatchRequest::new(inputs, false),
            &ExecutionControl::new(&cancellation, &progress),
        )
        .expect("ordinary item failures must still produce a batch report");

    let events = events.lock().unwrap();
    let terminal = events
        .iter()
        .position(|event| matches!(event, AnalysisEvent::BatchFinished { .. }))
        .expect("a completed batch must publish its terminal event");
    let mut published = events
        .iter()
        .enumerate()
        .filter_map(|(position, event)| match event {
            AnalysisEvent::BatchItemFinished { index, item } => {
                Some((position, *index, item.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    published.sort_by_key(|(_, index, _)| *index);

    assert_eq!(published.len(), report.items.len());
    for (position, index, item) in published {
        assert!(
            position < terminal,
            "item {index} was published after the batch terminal event"
        );
        assert_eq!(item, report.items[index]);
    }
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
    let error = Application::new()
        .run_batch(
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

    let error = Application::new()
        .run_batch(
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
    assert!(events.iter().any(|event| matches!(
        event,
        AnalysisEvent::FileFinished {
            index: 0,
            success: false,
            ..
        }
    )));
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

    let error = Application::new()
        .run_batch(
            BatchRequest::new(vec![fixture("tiny_duration.wav")], false),
            &ExecutionControl::new(&cancellation, &progress),
        )
        .expect_err("cancellation at discovery start must stop the batch");

    assert_eq!(error.code, ErrorCode::Cancelled);
}

#[test]
fn analysis_emits_ordered_file_and_terminal_progress_events() {
    let path = fixture("tiny_duration.wav");
    let display_path = path.display().to_string();
    let cancellation = CancellationToken::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_for_sink = Arc::clone(&events);
    let progress = move |event: AnalysisEvent| {
        events_for_sink.lock().unwrap().push(event);
    };

    Application::new()
        .analyze_file_with_control(
            AnalyzeRequest::new(path),
            &ExecutionControl::new(&cancellation, &progress),
        )
        .unwrap();

    let events = events.lock().unwrap();
    assert!(matches!(
        events.first(),
        Some(AnalysisEvent::FileStarted { index: 0, .. })
    ));
    assert!(matches!(
        events.last(),
        Some(AnalysisEvent::FileFinished {
            index: 0,
            success: true,
            ..
        })
    ));

    let progress_events = &events[1..events.len() - 1];
    assert!(!progress_events.is_empty());
    let mut previous_decoded_frames = 0;
    let mut eof_events = 0;
    for (position, event) in progress_events.iter().enumerate() {
        let AnalysisEvent::DecodeProgress {
            index,
            display_path: event_path,
            progress,
        } = event
        else {
            panic!("only decode progress may occur between file lifecycle events");
        };
        assert_eq!(*index, 0);
        assert_eq!(event_path, &display_path);
        assert!(progress.decoded_frames() >= previous_decoded_frames);
        previous_decoded_frames = progress.decoded_frames();
        if progress.is_eof() {
            eof_events += 1;
            assert_eq!(position, progress_events.len() - 1);
        }
    }
    assert_eq!(eof_events, 1);
}

#[test]
fn probe_failure_emits_started_then_finished_without_decode_progress() {
    let cancellation = CancellationToken::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_for_sink = Arc::clone(&events);
    let progress = move |event: AnalysisEvent| {
        events_for_sink.lock().unwrap().push(event);
    };

    let error = Application::new()
        .analyze_file_with_control(
            AnalyzeRequest::new(fixture("fake_audio.wav")),
            &ExecutionControl::new(&cancellation, &progress),
        )
        .expect_err("unsupported content must fail probing");

    assert_eq!(error.code, ErrorCode::UnsupportedFormat);
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0],
        AnalysisEvent::FileStarted { index: 0, .. }
    ));
    assert!(matches!(
        &events[1],
        AnalysisEvent::FileFinished {
            index: 0,
            success: false,
            ..
        }
    ));
}

#[test]
fn wire_envelopes_have_a_stable_finite_timestamp_free_schema() {
    let report = Application::new()
        .analyze_file(AnalyzeRequest::new(fixture("tiny_duration.wav")))
        .expect("valid fixture should analyze");
    let envelope = WireEnvelope::analysis(report);

    assert_eq!(WIRE_SCHEMA_VERSION, 4);
    assert_eq!(envelope.schema_version, WIRE_SCHEMA_VERSION);
    assert_eq!(envelope.tool_version, macinmeter::VERSION);
    assert!(matches!(envelope.payload, WirePayload::Analysis(_)));

    let value = serde_json::to_value(&envelope).expect("wire report should serialize");
    assert_eq!(value["schemaVersion"], WIRE_SCHEMA_VERSION);
    assert_eq!(value["toolVersion"], macinmeter::VERSION);
    assert_eq!(value["kind"], "analysis");
    assert!(value.get("data").is_some());
    let algorithm = &value["data"]["analysis"]["algorithm"];
    assert!(algorithm.get("profile").is_none());
    assert!(algorithm.get("profileVersion").is_none());
    assert!(algorithm.get("compatibility").is_none());
    assert_eq!(algorithm["parameters"]["histogramBins"], 10_001);
    let measurement = &value["data"]["analysis"]["channels"][0]["outcome"]["measurement"];
    assert!(measurement.get("loudWindowRms").is_some());
    assert!(measurement.get("loudRms").is_none());
    assert!(measurement.get("drSelectedPeak").is_some());
    assert!(measurement.get("selectedPeak").is_none());
    let channel_report = &value["data"]["analysis"]["channels"][0]["report"];
    assert!(channel_report["overallRmsLinear"].is_number());
    assert!(
        channel_report["overallRmsDbfs"].is_number() || channel_report["overallRmsDbfs"].is_null()
    );
    assert!(channel_report["primaryPeakLinear"].is_number());
    let aggregates = &value["data"]["analysis"]["aggregates"];
    assert!(aggregates.get("track").is_some());
    assert!(aggregates["track"].get("drDb").is_some());
    assert!(aggregates["track"].get("contributingChannels").is_some());
    assert!(aggregates.get("allChannels").is_none());
    assert!(aggregates.get("withoutLfe").is_none());
    let report_metrics = &value["data"]["analysis"]["report"];
    assert!(report_metrics["overallRmsLinear"].is_number());
    assert!(report_metrics["primaryPeakLinear"].is_number());
    assert_eq!(report_metrics["duration"]["decodedFrames"], 441);
    assert_eq!(report_metrics["duration"]["sampleRate"], 44_100);
    assert_json_contract(&value);

    let batch = run_batch(vec![
        fixture("tiny_duration.wav"),
        fixture("fake_audio.wav"),
    ]);
    let published_item = batch.items[0].clone();
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

    let progress_event = serde_json::to_value(AnalysisEvent::DecodeProgress {
        index: 7,
        display_path: "track.wav".to_string(),
        progress: DecodeProgress::new(5, Some(10), false),
    })
    .unwrap();
    assert_eq!(progress_event["type"], "decode_progress");
    assert_eq!(progress_event["progress"]["decodedFrames"], 5);
    assert_eq!(progress_event["progress"]["expectedFrames"], 10);
    assert_eq!(progress_event["progress"]["fraction"], 0.5);
    assert_eq!(progress_event["progress"]["eof"], false);

    let item_event = serde_json::to_value(AnalysisEvent::BatchItemFinished {
        index: 3,
        item: published_item,
    })
    .unwrap();
    assert_eq!(item_event["type"], "batch_item_finished");
    assert_eq!(item_event["index"], 3);
    assert_eq!(item_event["item"]["outcome"]["status"], "success");

    let silent = serde_json::to_value(macinmeter::ChannelOutcome::Silent {
        frames: 12,
        valid_windows: 3,
    })
    .unwrap();
    assert_eq!(silent["status"], "silent");
    assert_eq!(silent["validWindows"], 3);
    assert!(silent.get("valid_windows").is_none());
}

#[test]
fn every_stable_route_reaches_its_exact_frame_count_through_the_commit_buffer() {
    // The serial route now commits through the shared in-order packet buffer.
    // These multi-packet fixtures therefore exercise repeated accept/commit
    // cycles on all four stable routes, and each must still land on its exact
    // declared frame count rather than a truncated or padded one.
    for name in [
        "native-pcm-v1/wav-pcm-s16-stereo.wav",
        "native-pcm-v1/flac-pcm-s16-stereo-multiblock.flac",
        "native-pcm-v1/aiff-pcm-s16-stereo.aiff",
        "native-alac-v1/alac16-stereo-48000-multipacket.m4a",
    ] {
        let report = Application::new()
            .analyze_file(AnalyzeRequest::new(fixture(name)))
            .unwrap_or_else(|error| panic!("{name} must analyze: {error}"));
        let declared = report
            .source()
            .expected_frames
            .unwrap_or_else(|| panic!("{name} must declare a total frame count"));
        assert!(declared > 0, "{name} declared an empty stream");
        assert_eq!(
            report.diagnostics().decoded_frames,
            declared,
            "{name} committed a different frame count than it declared"
        );
    }
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
