#![forbid(unsafe_code)]

use macinmeter_analysis::AnalyzerSession;
use macinmeter_domain::{
    AnalysisProfile, AnalysisStage, ChannelCount, ChannelLayout, ChannelOutcome, ChannelRole,
    CompatibilityStatus, ErrorCode, ExclusionReason, SampleRate, StreamSpec,
};

fn stream(sample_rate: u32, channels: u16, layout: ChannelLayout) -> StreamSpec {
    StreamSpec::new(sample_rate, channels, layout).unwrap()
}

fn analyze(
    stream: StreamSpec,
    chunks: impl IntoIterator<Item = Vec<f32>>,
) -> macinmeter_domain::AnalysisResult {
    let mut session = AnalyzerSession::new(stream, AnalysisProfile::ProvisionalV1).unwrap();
    for chunk in chunks {
        session.push_interleaved(&chunk).unwrap();
    }
    session.finish()
}

fn measurement(
    result: &macinmeter_domain::AnalysisResult,
    channel: usize,
) -> &macinmeter_domain::ChannelMeasurement {
    match &result.channels[channel].outcome {
        ChannelOutcome::Measured { measurement } => measurement,
        outcome => panic!("expected a measured channel, got {outcome:?}"),
    }
}

fn interleave(mono: &[f32], channels: usize) -> Vec<f32> {
    let mut interleaved = Vec::with_capacity(mono.len() * channels);
    for sample in mono {
        for _ in 0..channels {
            interleaved.push(*sample);
        }
    }
    interleaved
}

#[test]
fn records_the_complete_provisional_v1_contract() {
    let stream = stream(44_100, 2, ChannelLayout::Unknown);
    let session = AnalyzerSession::new(stream, AnalysisProfile::ProvisionalV1).unwrap();

    assert_eq!(session.window_frames(), 132_480);
    assert_eq!(session.algorithm().profile, AnalysisProfile::ProvisionalV1);
    assert_eq!(session.algorithm().profile_version, 1);
    assert_eq!(
        session.algorithm().compatibility,
        CompatibilityStatus::Unverified
    );
    let parameters = &session.algorithm().parameters;
    assert_eq!(
        parameters.window_duration_coefficient,
        3.004_081_632_653_061_3
    );
    assert_eq!(parameters.rms_sum_multiplier, 2.0);
    assert_eq!(parameters.histogram_bins, 10_001);
    assert_eq!(parameters.minimum_nonzero_rms_bin, 1);
    assert_eq!(parameters.loud_fraction, 0.2);
    assert_eq!(parameters.minimum_tail_frames, 2);
    assert!(parameters.exact_window_virtual_zero_peak);
}

#[test]
fn analysis_is_invariant_to_frame_aligned_chunk_boundaries() {
    let mono: Vec<f32> = (0..137)
        .map(|frame| {
            let saw = (frame % 19) as f32 / 18.0;
            if frame % 2 == 0 { saw } else { -saw * 0.7 }
        })
        .collect();
    let samples = interleave(&mono, 3);
    let spec = stream(10, 3, ChannelLayout::KnownNoLfe);

    let one_chunk = analyze(spec.clone(), [samples.clone()]);

    let mut chunks = Vec::new();
    let mut frame_offset = 0;
    for frames in [1, 9, 2, 31, 7, 3, 53, 4, 27] {
        if frame_offset == mono.len() {
            break;
        }
        let end = (frame_offset + frames).min(mono.len());
        chunks.push(samples[frame_offset * 3..end * 3].to_vec());
        frame_offset = end;
    }
    if frame_offset < mono.len() {
        chunks.push(samples[frame_offset * 3..].to_vec());
    }
    let partitioned = analyze(spec, chunks);

    assert_eq!(partitioned, one_chunk);
}

#[test]
fn many_pseudorandom_frame_aligned_partitions_are_invariant() {
    let channels = 6;
    let mono: Vec<f32> = (0..1_003)
        .map(|frame| {
            let phase = frame as f32 * 0.071;
            phase.sin() * (0.05 + (frame % 37) as f32 / 50.0)
        })
        .collect();
    let samples = interleave(&mono, channels);
    let spec = stream(10, channels as u16, ChannelLayout::Unknown);
    let baseline = analyze(spec.clone(), [samples.clone()]);

    for seed in 1_u64..=32 {
        let mut state = seed;
        let mut chunks = Vec::new();
        let mut frame_offset = 0;
        while frame_offset < mono.len() {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let frames = usize::try_from((state >> 32) % 97 + 1).unwrap();
            let end = (frame_offset + frames).min(mono.len());
            chunks.push(samples[frame_offset * channels..end * channels].to_vec());
            frame_offset = end;
        }
        assert_eq!(analyze(spec.clone(), chunks), baseline, "seed {seed}");
    }
}

#[test]
fn replicated_channels_have_identical_measurements_for_common_layout_sizes() {
    let mono: Vec<f32> = (0..95)
        .map(|frame| {
            let phase = frame as f32 * 0.173;
            phase.sin() * (0.2 + (frame % 11) as f32 / 20.0)
        })
        .collect();

    for channels in [1_u16, 2, 3, 6, 8, 16] {
        let result = analyze(
            stream(10, channels, ChannelLayout::Unknown),
            [interleave(&mono, usize::from(channels))],
        );
        let first = measurement(&result, 0);
        for channel in 1..usize::from(channels) {
            assert_eq!(measurement(&result, channel), first);
        }
        assert_eq!(result.frames_seen, mono.len() as u64);
    }
}

#[test]
fn rejects_bad_chunks_atomically_and_accepts_empty_chunks() {
    let spec = stream(1, 2, ChannelLayout::KnownNoLfe);
    let first = vec![0.25, 0.5, -0.25, -0.5];
    let second = vec![0.75, 0.125, -0.75, -0.125];

    let mut session = AnalyzerSession::provisional_v1(spec.clone()).unwrap();
    session.push_interleaved(&first).unwrap();
    session.push_interleaved(&[]).unwrap();

    let alignment_error = session.push_interleaved(&[0.5]).unwrap_err();
    assert_eq!(alignment_error.code, ErrorCode::AnalysisFailed);
    assert_eq!(alignment_error.stage, AnalysisStage::Analysis);

    let finite_error = session.push_interleaved(&[f32::NAN, 0.0]).unwrap_err();
    assert_eq!(finite_error.code, ErrorCode::AnalysisFailed);
    assert_eq!(finite_error.stage, AnalysisStage::Analysis);
    assert_eq!(session.frames_seen(), 2);

    session.push_interleaved(&second).unwrap();
    let after_errors = session.finish();

    let mut combined = first;
    combined.extend(second);
    let baseline = analyze(spec, [combined]);
    assert_eq!(after_errors, baseline);
}

#[test]
fn constructor_revalidates_public_stream_spec_fields() {
    let malformed = StreamSpec {
        sample_rate: SampleRate::new(44_100).unwrap(),
        channels: ChannelCount::new(2).unwrap(),
        channel_layout: ChannelLayout::Known {
            positions: vec![ChannelRole::FrontLeft],
        },
    };

    let error = AnalyzerSession::new(malformed, AnalysisProfile::ProvisionalV1).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(error.stage, AnalysisStage::Validation);
}

#[test]
fn tail_requires_two_frames_and_is_finalized_only_once() {
    let empty = analyze(stream(1, 1, ChannelLayout::KnownNoLfe), []);
    assert_eq!(
        empty.channels[0].outcome,
        ChannelOutcome::InsufficientData { frames: 0 }
    );

    let one_frame = analyze(stream(1, 1, ChannelLayout::KnownNoLfe), [vec![0.5]]);
    assert_eq!(
        one_frame.channels[0].outcome,
        ChannelOutcome::InsufficientData { frames: 1 }
    );

    let two_frames = analyze(stream(1, 1, ChannelLayout::KnownNoLfe), [vec![0.5, 0.5]]);
    let tail = measurement(&two_frames, 0);
    assert_eq!(tail.frames, 2);
    assert_eq!(tail.valid_windows, 1);
    assert_eq!(tail.primary_peak, 0.5);
    assert_eq!(tail.secondary_peak, None);

    let full_plus_one = analyze(stream(1, 1, ChannelLayout::KnownNoLfe), [vec![0.5; 4]]);
    let measurement = measurement(&full_plus_one, 0);
    assert_eq!(measurement.frames, 4);
    assert_eq!(measurement.valid_windows, 1);
}

#[test]
fn window_boundary_matrix_has_the_expected_submitted_window_counts() {
    let window = 3;
    for (frames, expected_windows) in [
        (window - 1, 1),
        (window, 1),
        (window + 1, 1),
        (2 * window - 1, 2),
        (2 * window, 2),
        (2 * window + 1, 2),
    ] {
        let result = analyze(stream(1, 1, ChannelLayout::KnownNoLfe), [vec![0.5; frames]]);
        assert_eq!(
            measurement(&result, 0).valid_windows,
            expected_windows,
            "{frames} frames"
        );
    }
}

#[test]
fn exact_window_uses_virtual_zero_and_online_top_two_preserves_duplicates() {
    let one_window = analyze(stream(1, 1, ChannelLayout::KnownNoLfe), [vec![0.5; 3]]);
    let one = measurement(&one_window, 0);
    assert_eq!(one.primary_peak, 0.5);
    assert_eq!(one.secondary_peak, Some(0.0));
    assert_eq!(one.selected_peak, 0.5);

    let duplicate_windows = analyze(stream(1, 1, ChannelLayout::KnownNoLfe), [vec![0.75; 6]]);
    let duplicate = measurement(&duplicate_windows, 0);
    assert_eq!(duplicate.primary_peak, 0.75);
    assert_eq!(duplicate.secondary_peak, Some(0.75));
    assert_eq!(duplicate.selected_peak, 0.75);
}

#[test]
fn loud_rms_uses_the_loudest_floor_twenty_percent_of_quantized_windows() {
    let mut samples = Vec::new();
    for amplitude_step in 1..=10 {
        samples.extend([amplitude_step as f32 * 0.05; 3]);
    }
    let result = analyze(stream(1, 1, ChannelLayout::KnownNoLfe), [samples]);
    let measurement = measurement(&result, 0);

    let expected = ((6_363_f64.powi(2) + 7_071_f64.powi(2)) * 1e-8 / 2.0).sqrt();
    assert!((measurement.loud_rms - expected).abs() < 1e-12);
    assert_eq!(measurement.valid_windows, 10);
    assert_eq!(measurement.primary_peak, 0.5);
    let expected_secondary_peak = (9_f32 * 0.05) as f64;
    assert_eq!(measurement.secondary_peak, Some(expected_secondary_peak));
    assert_eq!(measurement.selected_peak, expected_secondary_peak);
}

#[test]
fn silence_and_lfe_are_excluded_with_explicit_reasons() {
    let layout = ChannelLayout::Known {
        positions: vec![
            ChannelRole::FrontLeft,
            ChannelRole::Lfe,
            ChannelRole::FrontRight,
        ],
    };
    let samples = vec![0.5, 0.25, 0.0, 0.5, 0.25, 0.0, 0.5, 0.25, 0.0];
    let result = analyze(stream(1, 3, layout), [samples]);

    assert!(matches!(
        result.channels[2].outcome,
        ChannelOutcome::Silent {
            frames: 3,
            valid_windows: 1
        }
    ));

    let all = &result.aggregates.all_channels;
    assert_eq!(all.included_channels, vec![0, 1]);
    assert_eq!(all.excluded_channels.len(), 1);
    assert_eq!(all.excluded_channels[0].channel_index, 2);
    assert_eq!(all.excluded_channels[0].reason, ExclusionReason::Silent);

    let without_lfe = result.aggregates.without_lfe.as_ref().unwrap();
    assert_eq!(without_lfe.included_channels, vec![0]);
    assert_eq!(without_lfe.excluded_channels.len(), 2);
    assert_eq!(without_lfe.excluded_channels[0].channel_index, 1);
    assert_eq!(
        without_lfe.excluded_channels[0].reason,
        ExclusionReason::Lfe
    );
    assert_eq!(without_lfe.excluded_channels[1].channel_index, 2);
    assert_eq!(
        without_lfe.excluded_channels[1].reason,
        ExclusionReason::Silent
    );
}

#[test]
fn lfe_free_aggregate_requires_reliable_layout_metadata() {
    let samples = vec![0.5; 6];
    let unknown = analyze(stream(1, 2, ChannelLayout::Unknown), [samples.clone()]);
    assert!(unknown.aggregates.all_channels.precise_dr_db.is_some());
    assert!(unknown.aggregates.without_lfe.is_none());

    let known_no_lfe = analyze(stream(1, 2, ChannelLayout::KnownNoLfe), [samples]);
    assert_eq!(
        known_no_lfe.aggregates.without_lfe.as_ref(),
        Some(&known_no_lfe.aggregates.all_channels)
    );
}

#[test]
fn aggregates_preserve_exclusions_when_no_channel_can_be_included() {
    let lfe_only = analyze(
        stream(
            1,
            1,
            ChannelLayout::Known {
                positions: vec![ChannelRole::Lfe],
            },
        ),
        [vec![0.5; 3]],
    );
    let without_lfe = lfe_only.aggregates.without_lfe.as_ref().unwrap();
    assert_eq!(without_lfe.precise_dr_db, None);
    assert_eq!(without_lfe.rounded_dr, None);
    assert!(without_lfe.included_channels.is_empty());
    assert_eq!(
        without_lfe.excluded_channels,
        vec![macinmeter_domain::ExcludedChannel {
            channel_index: 0,
            reason: ExclusionReason::Lfe,
        }]
    );

    let silent = analyze(stream(1, 1, ChannelLayout::KnownNoLfe), [vec![0.0; 3]]);
    assert_eq!(silent.aggregates.all_channels.precise_dr_db, None);
    assert_eq!(silent.aggregates.all_channels.rounded_dr, None);
    assert_eq!(
        silent.aggregates.all_channels.excluded_channels[0].reason,
        ExclusionReason::Silent
    );
}

#[test]
fn tiny_nonzero_signal_is_measured_and_serializes_as_finite_json() {
    let result = analyze(
        stream(1, 1, ChannelLayout::KnownNoLfe),
        [vec![f32::MIN_POSITIVE; 3]],
    );
    let measurement = measurement(&result, 0);

    assert_eq!(measurement.loud_rms, 0.0001);
    assert!(measurement.dr_db.is_finite());
    assert!(measurement.selected_peak.is_finite());
    serde_json::to_string(&result).unwrap();
}

#[test]
fn ignored_nonzero_single_frame_tail_is_insufficient_instead_of_silent() {
    let mut samples = vec![0.0; 3];
    samples.push(0.25);
    let result = analyze(stream(1, 1, ChannelLayout::KnownNoLfe), [samples]);

    assert_eq!(
        result.channels[0].outcome,
        ChannelOutcome::InsufficientData { frames: 4 }
    );
}
