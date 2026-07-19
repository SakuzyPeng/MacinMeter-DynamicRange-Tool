#![forbid(unsafe_code)]

use macinmeter_analysis::AnalyzerSession;
use macinmeter_domain::{
    AnalysisProfile, AnalysisStage, ChannelCount, ChannelLayout, ChannelMeasurement,
    ChannelOutcome, ChannelRole, CompatibilityStatus, ErrorCode, ExclusionReason,
    MAX_ANALYSIS_CHANNELS, SampleRate, StreamSpec,
};

const SAMPLE_RATE: u32 = 8_000;
const WINDOW_FRAMES: usize = 24_032;

fn stream(sample_rate: u32, channels: u16, layout: ChannelLayout) -> StreamSpec {
    StreamSpec::new(sample_rate, channels, layout).unwrap()
}

fn analyze(
    stream: StreamSpec,
    chunks: impl IntoIterator<Item = Vec<f64>>,
) -> macinmeter_domain::AnalysisResult {
    let mut session =
        AnalyzerSession::new(stream, AnalysisProfile::FooDrMeter108CandidateV1).unwrap();
    for chunk in chunks {
        session.push_interleaved(&chunk).unwrap();
    }
    session.finish().unwrap()
}

fn measurement(result: &macinmeter_domain::AnalysisResult, channel: usize) -> &ChannelMeasurement {
    match &result.channels[channel].outcome {
        ChannelOutcome::Measured { measurement } => measurement,
        outcome => panic!("expected a measured channel, got {outcome:?}"),
    }
}

fn shaped_window(frames: usize, rms: f64, peak: f64) -> Vec<f64> {
    assert!(frames >= 2);
    let required_sum_squares = rms * rms * frames as f64 / 2.0;
    let remainder = required_sum_squares - peak * peak;
    assert!(remainder >= 0.0);
    let floor = (remainder / (frames - 1) as f64).sqrt();

    let mut output = Vec::with_capacity(frames);
    output.push(peak);
    output.extend((1..frames).map(|frame| if frame % 2 == 1 { floor } else { -floor }));
    output
}

fn append(output: &mut Vec<f64>, part: &[f64]) {
    output.extend_from_slice(part);
}

fn repeated_shaped_windows(count: usize, rms: f64, peak: f64) -> Vec<f64> {
    let window = shaped_window(WINDOW_FRAMES, rms, peak);
    window.repeat(count)
}

fn interleave(channels: &[Vec<f64>]) -> Vec<f64> {
    assert!(!channels.is_empty());
    let frames = channels[0].len();
    assert!(channels.iter().all(|channel| channel.len() == frames));

    let mut output = Vec::with_capacity(frames * channels.len());
    for frame in 0..frames {
        for channel in channels {
            output.push(channel[frame]);
        }
    }
    output
}

fn analyze_mono(samples: Vec<f64>) -> macinmeter_domain::AnalysisResult {
    analyze(stream(SAMPLE_RATE, 1, ChannelLayout::KnownNoLfe), [samples])
}

#[test]
fn records_the_complete_candidate_contract() {
    let stream = stream(44_100, 2, ChannelLayout::Unknown);
    let session = AnalyzerSession::new(stream, AnalysisProfile::FooDrMeter108CandidateV1).unwrap();

    assert_eq!(session.window_frames(), 132_480);
    assert_eq!(
        session.algorithm().profile,
        AnalysisProfile::FooDrMeter108CandidateV1
    );
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
    assert_eq!(
        parameters.window_duration_coefficient.to_bits(),
        0x4008_085b_f376_12cf
    );
    assert_eq!(parameters.rms_sum_multiplier, 2.0);
    assert_eq!(parameters.histogram_bins, 10_001);
    assert_eq!(parameters.rms_histogram_min_db, -100.0);
    assert_eq!(parameters.rms_histogram_max_db, 0.0);
    assert_eq!(parameters.histogram_bin_width_db, 0.01);
    assert_eq!(parameters.peak_key_bin_width_db, 0.01);
    assert_eq!(parameters.loud_fraction, 0.2);
    assert_eq!(parameters.minimum_tail_frames, 1);
    assert!(parameters.include_entire_boundary_bin);
    assert!(!parameters.exact_window_virtual_zero_peak);
    assert_eq!(parameters.dr_floor_db, 0.0);
    assert_eq!(parameters.silent_channel_dr_db, 0.0);
    assert!(parameters.includes_lfe_in_track_aggregate);
    assert_eq!(parameters.result_precision_bits, 32);
}

#[test]
fn analysis_is_invariant_to_frame_aligned_chunk_boundaries() {
    let mono: Vec<f64> = (0..137)
        .map(|frame| {
            let saw = (frame % 19) as f64 / 18.0;
            if frame % 2 == 0 { saw } else { -saw * 0.7 }
        })
        .collect();
    let samples = interleave(&[mono.clone(), mono.clone(), mono.clone()]);
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

    assert_eq!(analyze(spec, chunks), one_chunk);
}

#[test]
fn many_pseudorandom_frame_aligned_partitions_are_invariant() {
    let channel_count = 6;
    let mono: Vec<f64> = (0..1_003)
        .map(|frame| {
            let phase = frame as f64 * 0.071;
            phase.sin() * (0.05 + (frame % 37) as f64 / 50.0)
        })
        .collect();
    let channels = vec![mono.clone(); channel_count];
    let samples = interleave(&channels);
    let spec = stream(10, channel_count as u16, ChannelLayout::Unknown);
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
            chunks.push(samples[frame_offset * channel_count..end * channel_count].to_vec());
            frame_offset = end;
        }
        assert_eq!(analyze(spec.clone(), chunks), baseline, "seed {seed}");
    }
}

#[test]
fn replicated_channels_have_identical_measurements_for_common_layout_sizes() {
    let mono: Vec<f64> = (0..95)
        .map(|frame| {
            let phase = frame as f64 * 0.173;
            phase.sin() * (0.2 + (frame % 11) as f64 / 20.0)
        })
        .collect();

    for channel_count in [1_u16, 2, 3, 6, 8, 16] {
        let channels = vec![mono.clone(); usize::from(channel_count)];
        let result = analyze(
            stream(10, channel_count, ChannelLayout::Unknown),
            [interleave(&channels)],
        );
        let first = measurement(&result, 0);
        for channel in 1..usize::from(channel_count) {
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

    let mut session =
        AnalyzerSession::new(spec.clone(), AnalysisProfile::FooDrMeter108CandidateV1).unwrap();
    session.push_interleaved(&first).unwrap();
    session.push_interleaved(&[]).unwrap();

    let alignment_error = session.push_interleaved(&[0.5]).unwrap_err();
    assert_eq!(alignment_error.code, ErrorCode::AnalysisFailed);
    assert_eq!(alignment_error.stage, AnalysisStage::Analysis);

    let finite_error = session.push_interleaved(&[f64::NAN, 0.0]).unwrap_err();
    assert_eq!(finite_error.code, ErrorCode::AnalysisFailed);
    assert_eq!(finite_error.stage, AnalysisStage::Analysis);
    assert_eq!(session.frames_seen(), 2);

    session.push_interleaved(&second).unwrap();
    let after_errors = session.finish().unwrap();

    let mut combined = first;
    combined.extend(second);
    assert_eq!(after_errors, analyze(spec, [combined]));
}

#[test]
fn rejects_finite_samples_that_overflow_f64_analysis_atomically() {
    let spec = stream(1, 1, ChannelLayout::KnownNoLfe);
    let valid = vec![0.25, -0.5, 0.0];
    let mut session =
        AnalyzerSession::new(spec.clone(), AnalysisProfile::FooDrMeter108CandidateV1).unwrap();

    let square_error = session.push_interleaved(&[f64::MAX]).unwrap_err();
    assert_eq!(square_error.code, ErrorCode::AnalysisFailed);
    assert_eq!(square_error.stage, AnalysisStage::Analysis);
    assert!(square_error.message.contains("square"));
    assert_eq!(session.frames_seen(), 0);

    let rms_overflow = f64::MAX.sqrt() * 0.75;
    assert!((rms_overflow * rms_overflow).is_finite());
    assert!((2.0 * rms_overflow * rms_overflow).is_infinite());
    let rms_error = session.push_interleaved(&[rms_overflow]).unwrap_err();
    assert_eq!(rms_error.code, ErrorCode::AnalysisFailed);
    assert_eq!(rms_error.stage, AnalysisStage::Analysis);
    assert!(rms_error.message.contains("RMS"));
    assert_eq!(session.frames_seen(), 0);

    session.push_interleaved(&valid).unwrap();
    assert_eq!(session.finish().unwrap(), analyze(spec, [valid]));
}

#[test]
fn rejects_cross_chunk_square_accumulation_overflow_without_losing_prior_state() {
    let spec = stream(1, 1, ChannelLayout::KnownNoLfe);
    let near_accumulation_limit = f64::MAX.sqrt() * 0.49;
    let square = near_accumulation_limit * near_accumulation_limit;
    assert!(square.is_finite());
    assert!((2.0 * (square + square)).is_finite());
    assert!((2.0 * (square + square + square)).is_infinite());

    let mut session =
        AnalyzerSession::new(spec.clone(), AnalysisProfile::FooDrMeter108CandidateV1).unwrap();
    session
        .push_interleaved(&[near_accumulation_limit])
        .unwrap();
    session
        .push_interleaved(&[-near_accumulation_limit])
        .unwrap();

    let error = session
        .push_interleaved(&[near_accumulation_limit])
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::AnalysisFailed);
    assert_eq!(error.stage, AnalysisStage::Analysis);
    assert!(error.message.contains("RMS"));
    assert_eq!(session.frames_seen(), 2);

    session.push_interleaved(&[0.0]).unwrap();
    let after_error = session.finish().unwrap_err();

    let mut baseline =
        AnalyzerSession::new(spec, AnalysisProfile::FooDrMeter108CandidateV1).unwrap();
    baseline
        .push_interleaved(&[near_accumulation_limit, -near_accumulation_limit, 0.0])
        .unwrap();
    assert_eq!(after_error, baseline.finish().unwrap_err());
}

#[test]
fn rejects_completed_window_overall_rms_overflow_before_mutating_the_session() {
    let spec = stream(1, 1, ChannelLayout::KnownNoLfe);
    let sample = (f64::MAX * 0.45).sqrt();
    let rms2 = 2.0 * (sample * sample) / 3.0;
    assert!(rms2.is_finite());
    assert!((3.0 * rms2).is_finite());
    assert!((4.0 * rms2).is_infinite());

    let high_window = [sample, 0.0, 0.0];
    let mut accepted = high_window.repeat(3);
    let mut session =
        AnalyzerSession::new(spec.clone(), AnalysisProfile::FooDrMeter108CandidateV1).unwrap();
    session.push_interleaved(&accepted).unwrap();

    let error = session.push_interleaved(&high_window).unwrap_err();
    assert_eq!(error.code, ErrorCode::AnalysisFailed);
    assert_eq!(error.stage, AnalysisStage::Analysis);
    assert!(error.message.contains("overall RMS accumulation"));
    assert_eq!(session.frames_seen(), 9);

    session.push_interleaved(&[0.0; 3]).unwrap();
    let after_error = session.finish().unwrap_err();

    accepted.extend_from_slice(&[0.0; 3]);
    let mut baseline =
        AnalyzerSession::new(spec, AnalysisProfile::FooDrMeter108CandidateV1).unwrap();
    baseline.push_interleaved(&accepted).unwrap();
    assert_eq!(after_error, baseline.finish().unwrap_err());
}

#[test]
fn finish_reports_tail_overall_rms_overflow_as_a_structured_error() {
    let spec = stream(1, 1, ChannelLayout::KnownNoLfe);
    let sample = (f64::MAX * 0.45).sqrt();
    let high_window = [sample, 0.0, 0.0];
    let mut session =
        AnalyzerSession::new(spec, AnalysisProfile::FooDrMeter108CandidateV1).unwrap();
    session.push_interleaved(&high_window.repeat(3)).unwrap();
    session.push_interleaved(&[sample]).unwrap();

    let error = session.finish().unwrap_err();
    assert_eq!(error.code, ErrorCode::AnalysisFailed);
    assert_eq!(error.stage, AnalysisStage::Analysis);
    assert!(error.message.contains("overall RMS accumulation"));
}

#[test]
fn finish_rejects_report_values_that_cannot_be_narrowed_to_finite_f32() {
    let sample = f64::from(f32::MAX) * 2.0;
    let spec = stream(1, 1, ChannelLayout::KnownNoLfe);
    let mut session =
        AnalyzerSession::new(spec, AnalysisProfile::FooDrMeter108CandidateV1).unwrap();
    session.push_interleaved(&[sample; 3]).unwrap();

    let error = session.finish().unwrap_err();
    assert_eq!(error.code, ErrorCode::AnalysisFailed);
    assert_eq!(error.stage, AnalysisStage::Analysis);
    assert!(error.message.contains("finite f32"));
}

#[test]
fn accepts_moderately_overfull_finite_pcm_without_clamping() {
    let result = analyze_mono(vec![2.0, -4.0, 3.0]);
    let channel = measurement(&result, 0);

    assert_eq!(channel.dr_primary_peak, 4.0);
    assert_eq!(channel.dr_selected_peak, 4.0);
}

#[test]
fn preserves_f64_pcm_without_narrowing_before_accumulation() {
    let sample = 0.5 + f64::EPSILON;
    assert_ne!(sample, f64::from(sample as f32));

    let result = analyze_mono(vec![sample]);
    let channel = measurement(&result, 0);

    assert_eq!(channel.dr_primary_peak, sample);
    assert_eq!(channel.dr_selected_peak, sample);
    assert_eq!(
        result.channels[0].report.primary_peak_linear.get(),
        sample as f32
    );
    assert_eq!(
        result.channels[0].report.overall_rms_linear.get(),
        (2.0_f64.sqrt() * sample) as f32
    );
    assert_ne!(
        f64::from(result.channels[0].report.primary_peak_linear.get()),
        channel.dr_primary_peak
    );
}

#[test]
fn overall_rms_is_the_equal_weighted_mean_of_unquantized_window_power() {
    let result = analyze(
        stream(1, 1, ChannelLayout::KnownNoLfe),
        [vec![0.25, 0.25, 0.25, 0.5]],
    );
    let expected_channel_rms = ((0.125_f64 + 0.5) / 2.0).sqrt() as f32;

    assert_eq!(
        result.channels[0].report.overall_rms_linear.get(),
        expected_channel_rms
    );
    assert_eq!(
        result.channels[0].report.overall_rms_dbfs.unwrap().get(),
        (20.0 * f64::from(expected_channel_rms).log10()) as f32
    );
    assert_eq!(
        result.report.overall_rms_linear.get().to_bits(),
        f64::from(expected_channel_rms * expected_channel_rms)
            .sqrt()
            .to_bits()
    );
    assert_eq!(result.report.duration.decoded_frames, 4);
    assert_eq!(result.report.duration.seconds(), 4.0);
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

    let error =
        AnalyzerSession::new(malformed, AnalysisProfile::FooDrMeter108CandidateV1).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(error.stage, AnalysisStage::Validation);
}

#[test]
fn constructor_enforces_the_product_channel_limit_before_allocation() {
    let at_limit = AnalyzerSession::new(
        stream(48_000, MAX_ANALYSIS_CHANNELS, ChannelLayout::Unknown),
        AnalysisProfile::FooDrMeter108CandidateV1,
    )
    .expect("the documented maximum channel count should be accepted");
    assert_eq!(at_limit.stream().channels.get(), MAX_ANALYSIS_CHANNELS);
    let at_limit_result = at_limit
        .finish()
        .expect("an empty session at the channel limit should finish");
    assert_eq!(
        at_limit_result.channels.len(),
        usize::from(MAX_ANALYSIS_CHANNELS)
    );

    for channels in [MAX_ANALYSIS_CHANNELS + 1, u16::MAX] {
        let error = AnalyzerSession::new(
            stream(48_000, channels, ChannelLayout::Unknown),
            AnalysisProfile::FooDrMeter108CandidateV1,
        )
        .expect_err("over-limit sessions must fail before allocating per-channel state");
        assert_eq!(error.code, ErrorCode::ResourceExhausted);
        assert_eq!(error.stage, AnalysisStage::Analysis);
        assert!(error.message.contains(&channels.to_string()));
        assert!(error.message.contains(&MAX_ANALYSIS_CHANNELS.to_string()));
    }
}

#[test]
fn every_nonempty_tail_is_submitted_and_no_virtual_window_is_added() {
    let empty = analyze(stream(1, 1, ChannelLayout::KnownNoLfe), []);
    assert_eq!(
        empty.channels[0].outcome,
        ChannelOutcome::InsufficientData { frames: 0 }
    );

    let one_frame = analyze(stream(1, 1, ChannelLayout::KnownNoLfe), [vec![0.5]]);
    let one = measurement(&one_frame, 0);
    assert_eq!(one.frames, 1);
    assert_eq!(one.valid_windows, 1);
    assert_eq!(one.rounded_dr, 0);

    let exact_window = analyze(stream(1, 1, ChannelLayout::KnownNoLfe), [vec![0.5; 3]]);
    let exact = measurement(&exact_window, 0);
    assert_eq!(exact.valid_windows, 1);
    assert_eq!(exact.dr_primary_peak, 0.5);
    assert_eq!(exact.dr_secondary_peak, None);

    let exact_plus_one = analyze(stream(1, 1, ChannelLayout::KnownNoLfe), [vec![0.5; 4]]);
    assert_eq!(measurement(&exact_plus_one, 0).valid_windows, 2);
}

#[test]
fn window_boundary_matrix_has_the_expected_submitted_window_counts() {
    let window = 3;
    for (frames, expected_windows) in [
        (window - 1, 1),
        (window, 1),
        (window + 1, 2),
        (2 * window - 1, 2),
        (2 * window, 2),
        (2 * window + 1, 3),
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
fn fixtures_103_and_104_distinguish_the_one_frame_tail() {
    let mut base = shaped_window(WINDOW_FRAMES, 0.1, 1.0);
    append(&mut base, &shaped_window(WINDOW_FRAMES, 0.09, 0.9));
    let base_result = analyze_mono(base.clone());
    assert_eq!(measurement(&base_result, 0).valid_windows, 2);
    assert_eq!(measurement(&base_result, 0).rounded_dr, 19, "fixture 103");

    base.push(0.5);
    let plus_one_result = analyze_mono(base);
    assert_eq!(measurement(&plus_one_result, 0).valid_windows, 3);
    assert_eq!(
        measurement(&plus_one_result, 0).rounded_dr,
        2,
        "fixture 104"
    );
}

#[test]
fn fixture_105_recomputes_negative_dr_with_the_primary_peak() {
    let mut samples = shaped_window(WINDOW_FRAMES, 0.8, 1.0);
    append(&mut samples, &shaped_window(WINDOW_FRAMES, 0.05, 0.1));
    let result = analyze_mono(samples);
    let channel = measurement(&result, 0);

    assert_eq!(channel.dr_primary_peak, 1.0);
    assert_eq!(channel.dr_secondary_peak, Some(0.1));
    assert_eq!(channel.dr_selected_peak, channel.dr_primary_peak);
    assert!(channel.dr_db > 1.9 && channel.dr_db < 2.0);
    assert_eq!(channel.rounded_dr, 2);
}

#[test]
fn fixture_110_quantizes_rms_in_centi_db_bins() {
    let mut samples = shaped_window(WINDOW_FRAMES, 0.00109, 0.1);
    let quieter = shaped_window(WINDOW_FRAMES, 0.00101, 0.1);
    for _ in 0..4 {
        append(&mut samples, &quieter);
    }
    let result = analyze_mono(samples);
    let channel = measurement(&result, 0);

    assert_eq!(channel.rounded_dr, 39);
    assert!((channel.loud_window_rms - 10.0_f64.powf(-59.25 / 20.0)).abs() < 1e-15);
}

#[test]
fn fixture_111_includes_the_complete_loud_boundary_bin() {
    let mut samples = shaped_window(WINDOW_FRAMES, 0.2, 0.5);
    let boundary = shaped_window(WINDOW_FRAMES, 0.1, 0.5);
    let quiet = shaped_window(WINDOW_FRAMES, 0.02, 0.5);
    for _ in 0..4 {
        append(&mut samples, &boundary);
    }
    for _ in 0..5 {
        append(&mut samples, &quiet);
    }
    let result = analyze_mono(samples);
    let channel = measurement(&result, 0);

    let expected_loud_rms =
        ((10.0_f64.powf(-13.98 / 10.0) + 4.0 * 10.0_f64.powf(-20.0 / 10.0)) / 5.0).sqrt();
    assert!((channel.loud_window_rms - expected_loud_rms).abs() < 1e-15);
    assert_eq!(channel.rounded_dr, 12);
}

fn peak_order_case(first_peak_db: f64, second_peak_db: f64) -> Vec<f64> {
    let rms = 10.0_f64.powf(-14.5 / 20.0);
    let mut samples = Vec::new();
    for peak_db in [
        first_peak_db,
        second_peak_db,
        -3.098_039_199_714_863_7,
        -3.098_039_199_714_863_7,
        -3.098_039_199_714_863_7,
    ] {
        append(
            &mut samples,
            &shaped_window(WINDOW_FRAMES, rms, 10.0_f64.powf(peak_db / 20.0)),
        );
    }
    samples
}

#[test]
fn fixtures_120_and_121_preserve_quantized_peak_arrival_order() {
    let low_peak_db = -2.0035;
    let high_peak_db = -1.9965;
    let low_peak = 10.0_f64.powf(low_peak_db / 20.0);
    let high_peak = 10.0_f64.powf(high_peak_db / 20.0);

    let low_then_high = analyze_mono(peak_order_case(low_peak_db, high_peak_db));
    let low_first = measurement(&low_then_high, 0);
    assert_eq!(low_first.dr_primary_peak, low_peak);
    assert_eq!(low_first.dr_secondary_peak, Some(high_peak));
    assert_eq!(low_first.rounded_dr, 13, "fixture 120");

    let high_then_low = analyze_mono(peak_order_case(high_peak_db, low_peak_db));
    let high_first = measurement(&high_then_low, 0);
    assert_eq!(high_first.dr_primary_peak, high_peak);
    assert_eq!(high_first.dr_secondary_peak, Some(low_peak));
    assert_eq!(high_first.rounded_dr, 12, "fixture 121");
}

#[test]
fn fixtures_201_to_203_cover_short_and_silent_inputs() {
    let one_frame = analyze_mono(vec![0.5]);
    assert_eq!(measurement(&one_frame, 0).rounded_dr, 0, "fixture 201");

    let two_frames = analyze_mono(vec![0.5, 0.5]);
    assert_eq!(measurement(&two_frames, 0).rounded_dr, 0, "fixture 202");

    let silent = analyze_mono(vec![0.0; 2 * WINDOW_FRAMES]);
    assert!(matches!(
        silent.channels[0].outcome,
        ChannelOutcome::Silent {
            frames: 48_064,
            valid_windows: 2
        }
    ));
    assert_eq!(silent.aggregates.track.dr_db, Some(0.0));
    assert_eq!(silent.aggregates.track.rounded_dr, Some(0));
    assert_eq!(silent.aggregates.track.contributing_channels, vec![0]);
    assert!(silent.aggregates.track.excluded_channels.is_empty());
    assert_eq!(silent.channels[0].report.overall_rms_linear.get(), 0.0);
    assert_eq!(silent.channels[0].report.overall_rms_dbfs, None);
    assert_eq!(silent.channels[0].report.primary_peak_linear.get(), 0.0);
    assert_eq!(silent.report.overall_rms_linear.get(), 0.0);
    assert_eq!(silent.report.overall_rms_dbfs, None);
    assert_eq!(silent.report.primary_peak_linear.get(), 0.0);
    assert_eq!(silent.report.primary_peak_dbfs, None);
}

#[test]
fn fixture_301_includes_a_silent_channel_as_numeric_zero() {
    let measured = repeated_shaped_windows(10, 10.0_f64.powf(-12.0 / 20.0), 1.0);
    let silent = vec![0.0; 10 * WINDOW_FRAMES];
    let result = analyze(
        stream(SAMPLE_RATE, 2, ChannelLayout::KnownNoLfe),
        [interleave(&[measured, silent])],
    );

    assert_eq!(measurement(&result, 0).rounded_dr, 12);
    assert!(matches!(
        result.channels[1].outcome,
        ChannelOutcome::Silent { .. }
    ));
    assert_eq!(result.aggregates.track.rounded_dr, Some(6));
    assert_eq!(result.aggregates.track.contributing_channels, vec![0, 1]);
}

#[test]
fn fixture_302_uses_the_unweighted_channel_arithmetic_mean() {
    let channels: Vec<_> = [10.0, 20.0, 30.0]
        .into_iter()
        .map(|dr_db| repeated_shaped_windows(10, 10.0_f64.powf(-dr_db / 20.0), 1.0))
        .collect();
    let result = analyze(
        stream(
            SAMPLE_RATE,
            3,
            ChannelLayout::Known {
                positions: vec![
                    ChannelRole::FrontLeft,
                    ChannelRole::FrontRight,
                    ChannelRole::FrontCenter,
                ],
            },
        ),
        [interleave(&channels)],
    );

    assert_eq!(
        result
            .channels
            .iter()
            .map(|channel| match &channel.outcome {
                ChannelOutcome::Measured { measurement } => measurement.rounded_dr,
                outcome => panic!("unexpected outcome: {outcome:?}"),
            })
            .collect::<Vec<_>>(),
        vec![10, 20, 30]
    );
    assert_eq!(result.aggregates.track.rounded_dr, Some(20));
}

#[test]
fn fixture_303_includes_lfe_in_the_reference_default_track_aggregate() {
    let target_drs = [6.0, 9.0, 12.0, 30.0, 15.0, 18.0];
    let channels: Vec<_> = target_drs
        .into_iter()
        .map(|dr_db| repeated_shaped_windows(10, 10.0_f64.powf(-dr_db / 20.0), 1.0))
        .collect();
    let result = analyze(
        stream(
            SAMPLE_RATE,
            6,
            ChannelLayout::Known {
                positions: vec![
                    ChannelRole::FrontLeft,
                    ChannelRole::FrontRight,
                    ChannelRole::FrontCenter,
                    ChannelRole::Lfe,
                    ChannelRole::BackLeft,
                    ChannelRole::BackRight,
                ],
            },
        ),
        [interleave(&channels)],
    );

    assert_eq!(measurement(&result, 3).rounded_dr, 30);
    assert_eq!(result.aggregates.track.rounded_dr, Some(15));
    assert_eq!(
        result.aggregates.track.contributing_channels,
        vec![0, 1, 2, 3, 4, 5]
    );
    assert!(result.aggregates.track.excluded_channels.is_empty());
}

#[test]
fn aggregate_preserves_insufficient_data_exclusions() {
    let result = analyze(stream(1, 2, ChannelLayout::Unknown), []);
    assert_eq!(result.aggregates.track.dr_db, None);
    assert_eq!(result.aggregates.track.rounded_dr, None);
    assert!(result.aggregates.track.contributing_channels.is_empty());
    assert_eq!(
        result.aggregates.track.excluded_channels,
        vec![
            macinmeter_domain::ExcludedChannel {
                channel_index: 0,
                reason: ExclusionReason::InsufficientData,
            },
            macinmeter_domain::ExcludedChannel {
                channel_index: 1,
                reason: ExclusionReason::InsufficientData,
            },
        ]
    );
}

#[test]
fn tiny_nonzero_signal_is_measured_and_serializes_as_finite_json() {
    let result = analyze(
        stream(1, 1, ChannelLayout::KnownNoLfe),
        [vec![f64::from(f32::MIN_POSITIVE); 3]],
    );
    let channel = measurement(&result, 0);

    assert_eq!(channel.loud_window_rms, 0.00001);
    assert_eq!(channel.rounded_dr, 0);
    assert!(channel.dr_db.is_finite());
    assert!(channel.dr_selected_peak.is_finite());
    let json = serde_json::to_string(&result).unwrap();
    let round_trip: macinmeter_domain::AnalysisResult = serde_json::from_str(&json).unwrap();
    assert_eq!(round_trip, result);
}
