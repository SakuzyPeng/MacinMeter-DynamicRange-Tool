use super::*;
use macinmeter_domain::{AlgorithmParameters, ChannelLayout, ChannelRole};

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionBits {
    stream: StreamSpec,
    algorithm: AlgorithmBits,
    window_frames: usize,
    frames_in_window: usize,
    frames_seen: u64,
    channels: Vec<ChannelAccumulatorBits>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AlgorithmBits {
    profile: AnalysisProfile,
    profile_version: u32,
    compatibility: macinmeter_domain::CompatibilityStatus,
    float_parameters: [u64; 9],
    histogram_bins: usize,
    minimum_tail_frames: usize,
    include_entire_boundary_bin: bool,
    exact_window_virtual_zero_peak: bool,
    includes_lfe_in_track_aggregate: bool,
    result_precision_bits: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChannelAccumulatorBits {
    current_sum_squares: u64,
    sum_window_rms2: u64,
    current_peak: u64,
    saw_nonzero_sample: bool,
    histogram_len: usize,
    histogram_nonzero: Vec<(usize, u64)>,
    valid_windows: u64,
    primary: Option<PeakCandidateBits>,
    secondary: Option<PeakCandidateBits>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PeakCandidateBits {
    amplitude: u64,
    key_centi_db: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StorageShape {
    channels: (usize, usize),
    layout_positions: Option<(usize, usize)>,
    histograms: Vec<(usize, usize)>,
}

impl From<&AnalyzerSession> for SessionBits {
    fn from(session: &AnalyzerSession) -> Self {
        let AnalyzerSession {
            stream,
            algorithm,
            window_frames,
            frames_in_window,
            frames_seen,
            channels,
        } = session;
        let AlgorithmDescriptor {
            profile,
            profile_version,
            compatibility,
            parameters,
        } = algorithm;
        let AlgorithmParameters {
            window_duration_coefficient,
            rms_sum_multiplier,
            histogram_bins,
            rms_histogram_min_db,
            rms_histogram_max_db,
            histogram_bin_width_db,
            peak_key_bin_width_db,
            loud_fraction,
            minimum_tail_frames,
            include_entire_boundary_bin,
            exact_window_virtual_zero_peak,
            dr_floor_db,
            silent_channel_dr_db,
            includes_lfe_in_track_aggregate,
            result_precision_bits,
        } = parameters;

        Self {
            stream: stream.clone(),
            algorithm: AlgorithmBits {
                profile: *profile,
                profile_version: *profile_version,
                compatibility: *compatibility,
                float_parameters: [
                    window_duration_coefficient.get().to_bits(),
                    rms_sum_multiplier.get().to_bits(),
                    rms_histogram_min_db.get().to_bits(),
                    rms_histogram_max_db.get().to_bits(),
                    histogram_bin_width_db.get().to_bits(),
                    peak_key_bin_width_db.get().to_bits(),
                    loud_fraction.get().to_bits(),
                    dr_floor_db.get().to_bits(),
                    silent_channel_dr_db.get().to_bits(),
                ],
                histogram_bins: *histogram_bins,
                minimum_tail_frames: *minimum_tail_frames,
                include_entire_boundary_bin: *include_entire_boundary_bin,
                exact_window_virtual_zero_peak: *exact_window_virtual_zero_peak,
                includes_lfe_in_track_aggregate: *includes_lfe_in_track_aggregate,
                result_precision_bits: *result_precision_bits,
            },
            window_frames: *window_frames,
            frames_in_window: *frames_in_window,
            frames_seen: *frames_seen,
            channels: channels.iter().map(project_accumulator).collect(),
        }
    }
}

fn project_accumulator(channel: &ChannelAccumulator) -> ChannelAccumulatorBits {
    let ChannelAccumulator {
        current_sum_squares,
        sum_window_rms2,
        current_peak,
        saw_nonzero_sample,
        histogram,
        valid_windows,
        peaks,
    } = channel;
    let TopTwoPeaks { primary, secondary } = peaks;

    ChannelAccumulatorBits {
        current_sum_squares: current_sum_squares.to_bits(),
        sum_window_rms2: sum_window_rms2.to_bits(),
        current_peak: current_peak.to_bits(),
        saw_nonzero_sample: *saw_nonzero_sample,
        histogram_len: histogram.len(),
        histogram_nonzero: histogram
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, count)| *count != 0)
            .collect(),
        valid_windows: *valid_windows,
        primary: primary.map(project_peak),
        secondary: secondary.map(project_peak),
    }
}

fn project_peak(peak: PeakCandidate) -> PeakCandidateBits {
    let PeakCandidate {
        amplitude,
        key_centi_db,
    } = peak;
    PeakCandidateBits {
        amplitude: amplitude.to_bits(),
        key_centi_db,
    }
}

impl From<&AnalyzerSession> for StorageShape {
    fn from(session: &AnalyzerSession) -> Self {
        Self {
            channels: (session.channels.len(), session.channels.capacity()),
            layout_positions: match &session.stream.channel_layout {
                ChannelLayout::Known { positions } => Some((positions.len(), positions.capacity())),
                ChannelLayout::Unknown | ChannelLayout::KnownNoLfe => None,
            },
            histograms: session
                .channels
                .iter()
                .map(|channel| (channel.histogram.len(), channel.histogram.capacity()))
                .collect(),
        }
    }
}

fn session(sample_rate: u32, channels: usize, layout: ChannelLayout) -> AnalyzerSession {
    AnalyzerSession::new(
        StreamSpec::new(sample_rate, u16::try_from(channels).unwrap(), layout).unwrap(),
        AnalysisProfile::FooDrMeter108CandidateV1,
    )
    .unwrap()
}

fn matrix_signal(frames: usize, channel_count: usize) -> Vec<f64> {
    let mut samples = Vec::with_capacity(frames * channel_count);
    for frame in 0..frames {
        for channel in 0..channel_count {
            let lane = channel as f64 + 1.0;
            samples.push(match frame % 7 {
                0 if channel % 2 == 0 => 0.0,
                0 => -0.0,
                1 => f64::from_bits(u64::try_from(channel + 1).unwrap()),
                2 => lane / 32.0,
                3 => -lane / 17.0,
                4 => 1.25 + lane / 64.0,
                5 => lane / 19.0,
                _ => -lane / 23.0,
            });
        }
    }
    samples
}

fn chunks_by_pattern(
    samples: &[f64],
    channel_count: usize,
    pattern: &[usize],
    insert_empty: bool,
) -> Vec<Vec<f64>> {
    let total_frames = samples.len() / channel_count;
    let mut chunks = Vec::new();
    let mut offset = 0;
    let mut pattern_index = 0;
    if insert_empty {
        chunks.push(Vec::new());
    }
    while offset < total_frames {
        let end = (offset + pattern[pattern_index % pattern.len()]).min(total_frames);
        chunks.push(samples[offset * channel_count..end * channel_count].to_vec());
        if insert_empty {
            chunks.push(Vec::new());
        }
        offset = end;
        pattern_index += 1;
    }
    if chunks.is_empty() {
        chunks.push(Vec::new());
    }
    chunks
}

fn push_all(session: &mut AnalyzerSession, chunks: impl IntoIterator<Item = Vec<f64>>) {
    for chunk in chunks {
        session.push_interleaved(&chunk).unwrap();
    }
}

fn safe_signal(frames: usize, channels: usize, frame_offset: usize) -> Vec<f64> {
    let mut samples = Vec::with_capacity(frames * channels);
    for frame in frame_offset..frame_offset + frames {
        for channel in 0..channels {
            let magnitude = ((frame * 11 + channel * 7) % 29 + 1) as f64 / 64.0;
            samples.push(if (frame + channel) % 2 == 0 {
                magnitude
            } else {
                -magnitude
            });
        }
    }
    samples
}

fn channel_major_numeric_safety_reference(
    session: &AnalyzerSession,
    samples: &[f64],
) -> NumericSafetyInspection {
    if samples.iter().any(|sample| !sample.is_finite()) {
        return NumericSafetyInspection::NonFinite;
    }

    let channel_count = session.stream.channels.as_usize();
    for (channel_index, channel) in session.channels.iter().enumerate() {
        let mut sum_squares = channel.current_sum_squares;
        let mut sum_window_rms2 = channel.sum_window_rms2;
        let mut frames_in_window = session.frames_in_window;

        for sample in samples.iter().skip(channel_index).step_by(channel_count) {
            let magnitude = sample.abs();
            let square = magnitude * magnitude;
            if !square.is_finite() {
                return NumericSafetyInspection::Overflow {
                    channel_index,
                    failure: NumericSafetyFailure::SampleSquare,
                };
            }

            sum_squares += square;
            if !sum_squares.is_finite() {
                return NumericSafetyInspection::Overflow {
                    channel_index,
                    failure: NumericSafetyFailure::SquareAccumulation,
                };
            }

            frames_in_window += 1;
            let rms2 = window_rms_squared(sum_squares, frames_in_window);
            if !rms2.is_finite() {
                return NumericSafetyInspection::Overflow {
                    channel_index,
                    failure: NumericSafetyFailure::WindowRms,
                };
            }

            if frames_in_window == session.window_frames {
                sum_window_rms2 += rms2;
                if !sum_window_rms2.is_finite() {
                    return NumericSafetyInspection::Overflow {
                        channel_index,
                        failure: NumericSafetyFailure::OverallRmsAccumulation,
                    };
                }
                sum_squares = 0.0;
                frames_in_window = 0;
            }
        }
    }

    NumericSafetyInspection::Valid
}

#[test]
fn chunk_plans_preserve_complete_session_bits_matrix() {
    let window = 3;
    let lengths = [
        0,
        1,
        window - 1,
        window,
        window + 1,
        2 * window - 1,
        2 * window,
        2 * window + 1,
    ];

    for channel_count in [1, 2, 3, 6, 8, 16] {
        for frames in lengths {
            let samples = matrix_signal(frames, channel_count);
            let mut baseline = session(1, channel_count, ChannelLayout::Unknown);
            baseline.push_interleaved(&samples).unwrap();
            let expected = SessionBits::from(&baseline);

            for (variant, chunks) in [
                (
                    "framewise-with-empty",
                    chunks_by_pattern(&samples, channel_count, &[1], true),
                ),
                (
                    "cross-window",
                    chunks_by_pattern(&samples, channel_count, &[2, 3, 1], false),
                ),
                (
                    "irregular",
                    chunks_by_pattern(&samples, channel_count, &[4, 1, 2], true),
                ),
            ] {
                let mut actual = session(1, channel_count, ChannelLayout::Unknown);
                push_all(&mut actual, chunks);
                assert_eq!(
                    SessionBits::from(&actual),
                    expected,
                    "{channel_count} channels, {frames} frames, {variant}"
                );
            }
        }
    }
}

#[test]
fn frame_major_numeric_inspection_matches_the_channel_major_error_contract() {
    let rms_overflow = f64::MAX.sqrt() * 0.75;
    assert!((rms_overflow * rms_overflow).is_finite());

    for channel_count in [1, 2, 3, 4, 5, 6, 8, 16, usize::from(MAX_ANALYSIS_CHANNELS)] {
        for prefix_frames in [0, 1, 2, 3, 4, 7] {
            let mut actual = session(1, channel_count, ChannelLayout::Unknown);
            actual
                .push_interleaved(&safe_signal(prefix_frames, channel_count, 0))
                .unwrap();

            let mut cases = vec![
                ("empty", Vec::new()),
                ("finite", safe_signal(7, channel_count, prefix_frames + 10)),
            ];

            for (label, sample_index, value) in [
                ("NaN first", 0, f64::NAN),
                (
                    "positive infinity middle",
                    channel_count + channel_count / 2,
                    f64::INFINITY,
                ),
                (
                    "negative infinity last",
                    3 * channel_count - 1,
                    f64::NEG_INFINITY,
                ),
                ("square overflow", 2 * channel_count, f64::MAX),
                ("window RMS overflow", channel_count, rms_overflow),
            ] {
                let mut samples = safe_signal(3, channel_count, prefix_frames + 100);
                samples[sample_index] = value;
                cases.push((label, samples));
            }

            let mut non_finite_precedence = safe_signal(3, channel_count, prefix_frames + 200);
            non_finite_precedence[0] = f64::MAX;
            non_finite_precedence[3 * channel_count - 1] = f64::NAN;
            cases.push(("non-finite precedence", non_finite_precedence));

            if channel_count > 1 {
                let mut lower_channel_precedence =
                    safe_signal(3, channel_count, prefix_frames + 300);
                lower_channel_precedence[1] = rms_overflow;
                lower_channel_precedence[2 * channel_count] = f64::MAX;
                cases.push(("lower channel precedence", lower_channel_precedence));
            }

            for (label, samples) in cases {
                assert_eq!(
                    actual.inspect_numeric_safety(&samples),
                    channel_major_numeric_safety_reference(&actual, &samples),
                    "{channel_count} channels, {prefix_frames} prefix frames, {label}"
                );
            }
        }
    }
}

#[test]
fn lane_perturbations_are_local_in_complete_session_state() {
    let channel_count = 6;
    let frames = 7;
    let samples = matrix_signal(frames, channel_count);
    let mut baseline = session(1, channel_count, ChannelLayout::KnownNoLfe);
    baseline.push_interleaved(&samples).unwrap();
    let expected = SessionBits::from(&baseline);

    for (lane, frame) in [(0, 2), (channel_count / 2, 3), (channel_count - 1, 6)] {
        let mut perturbed = samples.clone();
        perturbed[frame * channel_count + lane] = 4.0 + lane as f64;
        let mut actual = session(1, channel_count, ChannelLayout::KnownNoLfe);
        actual.push_interleaved(&perturbed).unwrap();
        let actual = SessionBits::from(&actual);

        assert_ne!(actual.channels[lane], expected.channels[lane]);
        for channel in 0..channel_count {
            if channel != lane {
                assert_eq!(
                    actual.channels[channel], expected.channels[channel],
                    "lane {lane} contaminated lane {channel}"
                );
            }
        }
    }

    let all_positive_zero = vec![0.0; frames * channel_count];
    let alternating_signed_zero = (0..frames * channel_count)
        .map(|index| if index % 2 == 0 { 0.0 } else { -0.0 })
        .collect::<Vec<_>>();
    let mut positive = session(1, channel_count, ChannelLayout::KnownNoLfe);
    positive.push_interleaved(&all_positive_zero).unwrap();
    let mut signed = session(1, channel_count, ChannelLayout::KnownNoLfe);
    signed.push_interleaved(&alternating_signed_zero).unwrap();
    assert_eq!(SessionBits::from(&signed), SessionBits::from(&positive));
}

fn assert_rejected_without_mutation(session: &mut AnalyzerSession, invalid: &[f64], label: &str) {
    let before = SessionBits::from(&*session);
    let storage_before = StorageShape::from(&*session);
    let error = session.push_interleaved(invalid).unwrap_err();
    assert_eq!(error.code, ErrorCode::AnalysisFailed, "{label}");
    assert_eq!(error.stage, AnalysisStage::Analysis, "{label}");
    assert_eq!(SessionBits::from(&*session), before, "{label}");
    assert_eq!(StorageShape::from(&*session), storage_before, "{label}");
}

#[test]
fn invalid_pushes_are_bitwise_transactional() {
    let channel_count = 3;
    let window = 30;
    for (prefix_frames, lane, invalid, label) in [
        (window - 1, 0, f64::NAN, "NaN in first lane before W"),
        (window, 1, f64::INFINITY, "+Inf in middle lane at W"),
        (
            window + 1,
            2,
            f64::NEG_INFINITY,
            "-Inf in last lane after W",
        ),
        (
            2 * window - 1,
            2,
            f64::MAX,
            "square overflow in last lane before 2W",
        ),
    ] {
        let mut actual = session(10, channel_count, ChannelLayout::Unknown);
        actual
            .push_interleaved(&safe_signal(prefix_frames, channel_count, 0))
            .unwrap();
        let mut invalid_chunk = safe_signal(3, channel_count, 10_000);
        invalid_chunk[channel_count + lane] = invalid;
        assert_rejected_without_mutation(&mut actual, &invalid_chunk, label);
    }

    let mut misaligned = session(10, channel_count, ChannelLayout::Unknown);
    misaligned
        .push_interleaved(&safe_signal(window - 1, channel_count, 0))
        .unwrap();
    assert_rejected_without_mutation(
        &mut misaligned,
        &vec![0.125; channel_count + 1],
        "misaligned chunk",
    );

    let near_limit = f64::MAX.sqrt() * 0.49;
    let mut cross_chunk = session(1, channel_count, ChannelLayout::Unknown);
    let mut accepted = vec![0.0; channel_count * 2];
    accepted[1] = near_limit;
    accepted[channel_count + 1] = -near_limit;
    cross_chunk.push_interleaved(&accepted).unwrap();
    let mut accumulation_overflow = vec![0.0; channel_count];
    accumulation_overflow[1] = near_limit;
    assert_rejected_without_mutation(
        &mut cross_chunk,
        &accumulation_overflow,
        "cross-chunk accumulation overflow in middle lane",
    );

    let sample = (f64::MAX * 0.45).sqrt();
    let mut high_window = vec![0.0; channel_count * 3];
    high_window[channel_count - 1] = sample;
    let mut overall = session(1, channel_count, ChannelLayout::Unknown);
    overall.push_interleaved(&high_window.repeat(3)).unwrap();
    assert_rejected_without_mutation(
        &mut overall,
        &high_window,
        "overall RMS accumulation overflow in last lane",
    );
}

#[test]
fn numeric_rejection_preserves_non_finite_and_channel_error_precedence() {
    let channel_count = 3;
    let rms_overflow = f64::MAX.sqrt() * 0.75;

    let mut lower_channel_wins = session(1, channel_count, ChannelLayout::Unknown);
    let mut numeric_failures = safe_signal(3, channel_count, 0);
    numeric_failures[1] = rms_overflow;
    numeric_failures[2 * channel_count] = f64::MAX;
    let error = lower_channel_wins
        .push_interleaved(&numeric_failures)
        .unwrap_err();
    assert_eq!(
        error.message,
        "PCM sample in channel 0 is too large to square without overflow"
    );
    assert_eq!(lower_channel_wins.frames_seen(), 0);

    let mut non_finite_wins = session(1, channel_count, ChannelLayout::Unknown);
    let mut mixed_failures = safe_signal(3, channel_count, 0);
    mixed_failures[0] = f64::MAX;
    mixed_failures[3 * channel_count - 1] = f64::NAN;
    let error = non_finite_wins
        .push_interleaved(&mixed_failures)
        .unwrap_err();
    assert_eq!(
        error.message,
        "interleaved PCM chunk contains a non-finite sample"
    );
    assert_eq!(non_finite_wins.frames_seen(), 0);
}

#[test]
fn storage_shape_is_duration_independent() {
    let channel_count = 16;
    let positions = (0..channel_count)
        .map(|channel| {
            if channel == 3 {
                ChannelRole::Lfe
            } else {
                ChannelRole::Other
            }
        })
        .collect();
    let mut session = session(10, channel_count, ChannelLayout::Known { positions });
    let window = session.window_frames();
    assert_eq!(window, 30);
    let expected = StorageShape::from(&session);
    let mut frames_seen = 0;

    for checkpoint in [
        0,
        window - 1,
        window,
        window + 1,
        10 * window,
        1_000 * window,
    ] {
        let additional_frames = checkpoint - frames_seen;
        session
            .push_interleaved(&vec![0.25; additional_frames * channel_count])
            .unwrap();
        frames_seen = checkpoint;
        assert_eq!(
            StorageShape::from(&session),
            expected,
            "storage changed at {checkpoint} frames"
        );
    }
}

#[test]
fn public_f32_display_rounding_preserves_the_observed_half_boundary() {
    let half = 12.5_f32;
    let before = f32::from_bits(half.to_bits() - 1);
    let after = f32::from_bits(half.to_bits() + 1);

    assert_eq!(rounded_display_dr(before), 12);
    assert_eq!(rounded_display_dr(half), 13);
    assert_eq!(rounded_display_dr(after), 13);
}
