#![forbid(unsafe_code)]

use macinmeter_analysis::AnalyzerSession;
use macinmeter_domain::{
    AggregateResults, AlgorithmDescriptor, AlgorithmParameters, AnalysisProfile, AnalysisResult,
    AnalysisStage, ChannelLayout, ChannelMeasurement, ChannelOutcome, ChannelReportMetrics,
    ChannelResult, ChannelRole, CompatibilityStatus, DecodedDuration, ErrorCode, ExcludedChannel,
    StreamSpec, TrackAggregate, TrackReportMetrics,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawAnalysisProjection {
    algorithm: RawAlgorithmProjection,
    stream: StreamSpec,
    frames_seen: u64,
    channels: Vec<RawChannelProjection>,
    track: RawTrackAggregateProjection,
    report: RawTrackReportProjection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawAlgorithmProjection {
    profile: AnalysisProfile,
    profile_version: u32,
    compatibility: CompatibilityStatus,
    parameters: RawAlgorithmParametersProjection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawAlgorithmParametersProjection {
    window_duration_coefficient: u64,
    rms_sum_multiplier: u64,
    histogram_bins: usize,
    rms_histogram_min_db: u64,
    rms_histogram_max_db: u64,
    histogram_bin_width_db: u64,
    peak_key_bin_width_db: u64,
    loud_fraction: u64,
    minimum_tail_frames: usize,
    include_entire_boundary_bin: bool,
    exact_window_virtual_zero_peak: bool,
    dr_floor_db: u64,
    silent_channel_dr_db: u64,
    includes_lfe_in_track_aggregate: bool,
    result_precision_bits: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawChannelProjection {
    channel_index: usize,
    report: RawChannelReportProjection,
    outcome: RawChannelOutcomeProjection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawChannelReportProjection {
    overall_rms_linear: u32,
    overall_rms_dbfs: Option<u32>,
    primary_peak_linear: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RawChannelOutcomeProjection {
    Measured {
        dr_db: u32,
        rounded_dr: u32,
        loud_window_rms: u64,
        dr_selected_peak: u64,
        dr_primary_peak: u64,
        dr_secondary_peak: Option<u64>,
        valid_windows: u64,
        frames: u64,
    },
    Silent {
        frames: u64,
        valid_windows: u64,
    },
    InsufficientData {
        frames: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawTrackAggregateProjection {
    dr_db: Option<u32>,
    rounded_dr: Option<u32>,
    contributing_channels: Vec<usize>,
    excluded_channels: Vec<ExcludedChannel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawTrackReportProjection {
    overall_rms_linear: u64,
    overall_rms_dbfs: Option<u32>,
    primary_peak_linear: u32,
    primary_peak_dbfs: Option<u32>,
    duration: DecodedDuration,
}

impl From<&AnalysisResult> for RawAnalysisProjection {
    fn from(result: &AnalysisResult) -> Self {
        let AnalysisResult {
            algorithm,
            stream,
            frames_seen,
            channels,
            aggregates,
            report,
        } = result;
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
        let AggregateResults { track } = aggregates;
        let TrackAggregate {
            dr_db,
            rounded_dr,
            contributing_channels,
            excluded_channels,
        } = track;
        let TrackReportMetrics {
            overall_rms_linear,
            overall_rms_dbfs,
            primary_peak_linear,
            primary_peak_dbfs,
            duration,
        } = report;

        Self {
            algorithm: RawAlgorithmProjection {
                profile: *profile,
                profile_version: *profile_version,
                compatibility: *compatibility,
                parameters: RawAlgorithmParametersProjection {
                    window_duration_coefficient: window_duration_coefficient.to_bits(),
                    rms_sum_multiplier: rms_sum_multiplier.to_bits(),
                    histogram_bins: *histogram_bins,
                    rms_histogram_min_db: rms_histogram_min_db.to_bits(),
                    rms_histogram_max_db: rms_histogram_max_db.to_bits(),
                    histogram_bin_width_db: histogram_bin_width_db.to_bits(),
                    peak_key_bin_width_db: peak_key_bin_width_db.to_bits(),
                    loud_fraction: loud_fraction.to_bits(),
                    minimum_tail_frames: *minimum_tail_frames,
                    include_entire_boundary_bin: *include_entire_boundary_bin,
                    exact_window_virtual_zero_peak: *exact_window_virtual_zero_peak,
                    dr_floor_db: dr_floor_db.to_bits(),
                    silent_channel_dr_db: silent_channel_dr_db.to_bits(),
                    includes_lfe_in_track_aggregate: *includes_lfe_in_track_aggregate,
                    result_precision_bits: *result_precision_bits,
                },
            },
            stream: stream.clone(),
            frames_seen: *frames_seen,
            channels: channels.iter().map(project_channel).collect(),
            track: RawTrackAggregateProjection {
                dr_db: dr_db.map(f32::to_bits),
                rounded_dr: *rounded_dr,
                contributing_channels: contributing_channels.clone(),
                excluded_channels: excluded_channels.clone(),
            },
            report: RawTrackReportProjection {
                overall_rms_linear: overall_rms_linear.get().to_bits(),
                overall_rms_dbfs: overall_rms_dbfs.map(|value| value.get().to_bits()),
                primary_peak_linear: primary_peak_linear.get().to_bits(),
                primary_peak_dbfs: primary_peak_dbfs.map(|value| value.get().to_bits()),
                duration: *duration,
            },
        }
    }
}

fn project_channel(channel: &ChannelResult) -> RawChannelProjection {
    let ChannelResult {
        channel_index,
        report,
        outcome,
    } = channel;
    let ChannelReportMetrics {
        overall_rms_linear,
        overall_rms_dbfs,
        primary_peak_linear,
    } = report;
    let outcome = match outcome {
        ChannelOutcome::Measured { measurement } => {
            let ChannelMeasurement {
                dr_db,
                rounded_dr,
                loud_window_rms,
                dr_selected_peak,
                dr_primary_peak,
                dr_secondary_peak,
                valid_windows,
                frames,
            } = measurement;
            RawChannelOutcomeProjection::Measured {
                dr_db: dr_db.to_bits(),
                rounded_dr: *rounded_dr,
                loud_window_rms: loud_window_rms.to_bits(),
                dr_selected_peak: dr_selected_peak.to_bits(),
                dr_primary_peak: dr_primary_peak.to_bits(),
                dr_secondary_peak: dr_secondary_peak.map(f64::to_bits),
                valid_windows: *valid_windows,
                frames: *frames,
            }
        }
        ChannelOutcome::Silent {
            frames,
            valid_windows,
        } => RawChannelOutcomeProjection::Silent {
            frames: *frames,
            valid_windows: *valid_windows,
        },
        ChannelOutcome::InsufficientData { frames } => {
            RawChannelOutcomeProjection::InsufficientData { frames: *frames }
        }
    };

    RawChannelProjection {
        channel_index: *channel_index,
        report: RawChannelReportProjection {
            overall_rms_linear: overall_rms_linear.get().to_bits(),
            overall_rms_dbfs: overall_rms_dbfs.map(|value| value.get().to_bits()),
            primary_peak_linear: primary_peak_linear.get().to_bits(),
        },
        outcome,
    }
}

fn stream(sample_rate: u32, channels: usize, layout: ChannelLayout) -> StreamSpec {
    StreamSpec::new(sample_rate, u16::try_from(channels).unwrap(), layout).unwrap()
}

fn analyze(spec: &StreamSpec, chunks: impl IntoIterator<Item = Vec<f64>>) -> AnalysisResult {
    let mut session =
        AnalyzerSession::new(spec.clone(), AnalysisProfile::FooDrMeter108CandidateV1).unwrap();
    for chunk in chunks {
        session.push_interleaved(&chunk).unwrap();
    }
    session.finish().unwrap()
}

fn raw(result: &AnalysisResult) -> RawAnalysisProjection {
    RawAnalysisProjection::from(result)
}

fn interleave(channels: &[Vec<f64>]) -> Vec<f64> {
    assert!(!channels.is_empty());
    let frames = channels[0].len();
    assert!(channels.iter().all(|channel| channel.len() == frames));

    let mut samples = Vec::with_capacity(frames * channels.len());
    for frame in 0..frames {
        for channel in channels {
            samples.push(channel[frame]);
        }
    }
    samples
}

fn chunks_by_pattern(
    samples: &[f64],
    channel_count: usize,
    frame_pattern: &[usize],
    insert_empty: bool,
) -> Vec<Vec<f64>> {
    assert!(!frame_pattern.is_empty());
    assert!(frame_pattern.iter().all(|frames| *frames > 0));
    let total_frames = samples.len() / channel_count;
    let mut chunks = Vec::new();
    let mut frame_offset = 0;
    let mut pattern_index = 0;
    if insert_empty {
        chunks.push(Vec::new());
    }
    while frame_offset < total_frames {
        let frames = frame_pattern[pattern_index % frame_pattern.len()];
        let end = (frame_offset + frames).min(total_frames);
        chunks.push(samples[frame_offset * channel_count..end * channel_count].to_vec());
        if insert_empty {
            chunks.push(Vec::new());
        }
        frame_offset = end;
        pattern_index += 1;
    }
    if chunks.is_empty() {
        chunks.push(Vec::new());
    }
    chunks
}

fn pseudorandom_chunks(samples: &[f64], channel_count: usize, seed: u64) -> Vec<Vec<f64>> {
    let total_frames = samples.len() / channel_count;
    let mut chunks = Vec::new();
    let mut frame_offset = 0;
    let mut state = seed;
    while frame_offset < total_frames {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let frames = usize::try_from((state >> 32) % 5 + 1).unwrap();
        let end = (frame_offset + frames).min(total_frames);
        chunks.push(samples[frame_offset * channel_count..end * channel_count].to_vec());
        if state & 1 == 0 {
            chunks.push(Vec::new());
        }
        frame_offset = end;
    }
    if chunks.is_empty() {
        chunks.push(Vec::new());
    }
    chunks
}

fn matrix_signal(frames: usize, channel_count: usize) -> Vec<f64> {
    let mut samples = Vec::with_capacity(frames * channel_count);
    for frame in 0..frames {
        for channel in 0..channel_count {
            let lane = channel as f64 + 1.0;
            let sample = match frame % 7 {
                0 if channel % 2 == 0 => 0.0,
                0 => -0.0,
                1 => f64::from_bits(u64::try_from(channel + 1).unwrap()),
                2 => lane / 32.0,
                3 => -lane / 17.0,
                4 => 1.25 + lane / 64.0,
                5 => lane / 19.0,
                _ => -lane / 23.0,
            };
            samples.push(sample);
        }
    }
    samples
}

fn safe_signal(frames: usize, channel_count: usize, frame_offset: usize) -> Vec<f64> {
    let mut samples = Vec::with_capacity(frames * channel_count);
    for frame in frame_offset..frame_offset + frames {
        for channel in 0..channel_count {
            let numerator = ((frame * 11 + channel * 7) % 29 + 1) as f64;
            let sample = numerator / 64.0;
            samples.push(if (frame + channel) % 2 == 0 {
                sample
            } else {
                -sample
            });
        }
    }
    samples
}

#[test]
fn raw_projector_distinguishes_signed_zero_that_partial_eq_cannot_see() {
    let spec = stream(1, 1, ChannelLayout::KnownNoLfe);
    let positive_zero = analyze(&spec, [vec![0.5; 6]]);
    let mut negative_zero = positive_zero.clone();
    let ChannelOutcome::Measured { measurement } = &mut negative_zero.channels[0].outcome else {
        panic!("the signed-zero projector fixture must be measured");
    };
    assert_eq!(measurement.dr_db.to_bits(), 0.0_f32.to_bits());
    measurement.dr_db = -0.0;

    assert_eq!(positive_zero, negative_zero);
    assert_ne!(raw(&positive_zero), raw(&negative_zero));
}

#[test]
fn signed_zero_input_signs_normalize_to_identical_result_bits() {
    let spec = stream(1, 3, ChannelLayout::KnownNoLfe);
    let positive = vec![0.0; 21];
    let signed = (0..21)
        .map(|index| if index % 2 == 0 { 0.0 } else { -0.0 })
        .collect::<Vec<_>>();

    assert_eq!(
        raw(&analyze(&spec, [signed])),
        raw(&analyze(&spec, [positive]))
    );
}

#[test]
fn declared_chunk_and_window_matrix_is_bit_exact_for_complete_results() {
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
        let spec = stream(1, channel_count, ChannelLayout::Unknown);
        let probe =
            AnalyzerSession::new(spec.clone(), AnalysisProfile::FooDrMeter108CandidateV1).unwrap();
        assert_eq!(probe.window_frames(), window);

        for frames in lengths {
            let samples = matrix_signal(frames, channel_count);
            let expected = raw(&analyze(&spec, [samples.clone()]));
            let variants = [
                (
                    "framewise-with-empty",
                    chunks_by_pattern(&samples, channel_count, &[1], true),
                ),
                (
                    "cross-window",
                    chunks_by_pattern(&samples, channel_count, &[2, 3, 1], false),
                ),
                (
                    "fixed-seed-1",
                    pseudorandom_chunks(&samples, channel_count, 1),
                ),
                (
                    "fixed-seed-2",
                    pseudorandom_chunks(&samples, channel_count, 0xfeed_beef),
                ),
            ];

            for (variant, chunks) in variants {
                assert_eq!(
                    raw(&analyze(&spec, chunks)),
                    expected,
                    "{channel_count} channels, {frames} frames, {variant}"
                );
            }
        }
    }
}

#[test]
fn signed_zero_subnormal_histogram_peak_and_overfull_signals_are_chunk_invariant() {
    let channel_count = 6;
    let window = 3;
    let rms_db = [-101.0_f64, -100.0, -99.0, -1.0, 0.0, 1.0];
    let peak_db = [-2.0035_f64, -1.9965, -3.1, -3.1, -3.1, -3.1];
    let frames = rms_db.len() * window;
    let mut channels = (0..channel_count)
        .map(|_| Vec::with_capacity(frames))
        .collect::<Vec<_>>();
    for frame in 0..frames {
        let window_index = frame / window;
        channels[0].push(if frame % 2 == 0 { 0.0 } else { -0.0 });
        channels[1].push(if frame % 2 == 0 {
            f64::from_bits(1)
        } else {
            -f64::from_bits(1)
        });
        channels[2]
            .push(10.0_f64.powf(rms_db[window_index] / 20.0) * std::f64::consts::FRAC_1_SQRT_2);
        let peak = 10.0_f64.powf(peak_db[window_index] / 20.0);
        channels[3].push(if frame % window == 0 {
            peak
        } else {
            -peak / 4.0
        });
        channels[4].push([2.0, -4.0, 3.0][frame % window]);
        channels[5].push(((frame * 13 % 31) as f64 - 15.0) / 32.0);
    }

    let samples = interleave(&channels);
    let spec = stream(1, channel_count, ChannelLayout::KnownNoLfe);
    let expected = raw(&analyze(&spec, [samples.clone()]));
    for (variant, chunks) in [
        (
            "framewise",
            chunks_by_pattern(&samples, channel_count, &[1], false),
        ),
        (
            "cross-window-with-empty",
            chunks_by_pattern(&samples, channel_count, &[2, 5, 1], true),
        ),
        (
            "fixed-seed",
            pseudorandom_chunks(&samples, channel_count, 0x1234_5678),
        ),
    ] {
        assert_eq!(raw(&analyze(&spec, chunks)), expected, "{variant}");
    }
}

#[test]
fn perturbing_one_lane_cannot_change_any_other_channel_bits() {
    let channel_count = 6;
    let frames = 7;
    let mut channels = Vec::new();
    for channel in 0..channel_count {
        channels.push(
            (0..frames)
                .map(|frame| {
                    let magnitude = (channel as f64 + 1.0) / 16.0;
                    if frame % 2 == 0 {
                        magnitude
                    } else {
                        -magnitude * 0.75
                    }
                })
                .collect::<Vec<_>>(),
        );
    }
    let spec = stream(1, channel_count, ChannelLayout::KnownNoLfe);
    let baseline = raw(&analyze(&spec, [interleave(&channels)]));

    for perturbed_lane in 0..channel_count {
        let mut perturbed = channels.clone();
        perturbed[perturbed_lane][3] = 2.0 + perturbed_lane as f64;
        let actual = raw(&analyze(&spec, [interleave(&perturbed)]));

        assert_ne!(
            actual.channels[perturbed_lane], baseline.channels[perturbed_lane],
            "lane {perturbed_lane} perturbation must be observable"
        );
        for channel in 0..channel_count {
            if channel != perturbed_lane {
                assert_eq!(
                    actual.channels[channel], baseline.channels[channel],
                    "lane {perturbed_lane} contaminated lane {channel}"
                );
            }
        }
    }
}

#[test]
fn every_multichannel_lane_matches_its_independent_mono_analysis() {
    let frames = 7;
    for channel_count in [1, 2, 3, 6, 8, 16] {
        let channels = (0..channel_count)
            .map(|channel| {
                (0..frames)
                    .map(|frame| {
                        let magnitude = (channel as f64 + 1.0) / 24.0;
                        match frame % 4 {
                            0 => magnitude * 1.5,
                            1 => -magnitude,
                            2 => magnitude * 0.5,
                            _ => -magnitude * 0.25,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let multichannel_spec = stream(1, channel_count, ChannelLayout::KnownNoLfe);
        let multichannel = raw(&analyze(&multichannel_spec, [interleave(&channels)]));

        for (channel_index, channel_samples) in channels.into_iter().enumerate() {
            let mono_spec = stream(1, 1, ChannelLayout::KnownNoLfe);
            let mono = raw(&analyze(&mono_spec, [channel_samples]));
            let mut mono_channel = mono.channels[0].clone();
            mono_channel.channel_index = channel_index;
            assert_eq!(
                multichannel.channels[channel_index], mono_channel,
                "{channel_count} channels: lane {channel_index} differs from mono"
            );
        }
    }
}

fn roles(channel_count: usize) -> Vec<ChannelRole> {
    let roles = [
        ChannelRole::FrontLeft,
        ChannelRole::FrontRight,
        ChannelRole::FrontCenter,
        ChannelRole::Lfe,
        ChannelRole::BackLeft,
        ChannelRole::BackRight,
        ChannelRole::SideLeft,
        ChannelRole::SideRight,
        ChannelRole::Other,
    ];
    (0..channel_count)
        .map(|channel| roles[channel % roles.len()].clone())
        .collect()
}

#[test]
fn channel_permutations_are_exactly_reversible_at_the_per_channel_boundary() {
    for channel_count in [2, 3, 6, 8, 16] {
        let frames = 7;
        let mut channels = Vec::new();
        for channel in 0..channel_count {
            let samples = match channel {
                0 => (0..frames)
                    .map(|frame| if frame % 2 == 0 { 0.0 } else { -0.0 })
                    .collect(),
                1 => (0..frames)
                    .map(|frame| {
                        if frame % 2 == 0 {
                            f64::from_bits(1)
                        } else {
                            -f64::from_bits(1)
                        }
                    })
                    .collect(),
                _ => (0..frames)
                    .map(|frame| {
                        let magnitude = (channel as f64 + 1.0) / 20.0;
                        if frame == 0 {
                            magnitude * 1.5
                        } else if frame % 2 == 0 {
                            magnitude
                        } else {
                            -magnitude * 0.5
                        }
                    })
                    .collect(),
            };
            channels.push(samples);
        }

        let original_roles = roles(channel_count);
        let original_spec = stream(
            1,
            channel_count,
            ChannelLayout::Known {
                positions: original_roles.clone(),
            },
        );
        let original = raw(&analyze(&original_spec, [interleave(&channels)]));

        let permutation: Vec<usize> = (0..channel_count).rev().collect();
        let permuted_channels: Vec<Vec<f64>> = permutation
            .iter()
            .map(|old_index| channels[*old_index].clone())
            .collect();
        let permuted_roles: Vec<ChannelRole> = permutation
            .iter()
            .map(|old_index| original_roles[*old_index].clone())
            .collect();
        let permuted_spec = stream(
            1,
            channel_count,
            ChannelLayout::Known {
                positions: permuted_roles.clone(),
            },
        );
        let permuted = raw(&analyze(&permuted_spec, [interleave(&permuted_channels)]));

        let ChannelLayout::Known {
            positions: result_roles,
        } = &permuted.stream.channel_layout
        else {
            panic!("permuted result lost its known channel layout");
        };
        assert_eq!(result_roles, &permuted_roles);

        for (new_index, old_index) in permutation.iter().copied().enumerate() {
            let mut remapped = permuted.channels[new_index].clone();
            remapped.channel_index = old_index;
            assert_eq!(
                remapped, original.channels[old_index],
                "{channel_count} channels: new lane {new_index} did not map to old lane {old_index}"
            );
            assert_eq!(result_roles[new_index], original_roles[old_index]);
        }

        let mut original_contributors = original.track.contributing_channels.clone();
        let mut remapped_contributors: Vec<usize> = permuted
            .track
            .contributing_channels
            .iter()
            .map(|new_index| permutation[*new_index])
            .collect();
        original_contributors.sort_unstable();
        remapped_contributors.sort_unstable();
        assert_eq!(remapped_contributors, original_contributors);

        let mut original_exclusions: Vec<_> = original
            .track
            .excluded_channels
            .iter()
            .map(|excluded| (excluded.channel_index, excluded.reason))
            .collect();
        let mut remapped_exclusions: Vec<_> = permuted
            .track
            .excluded_channels
            .iter()
            .map(|excluded| (permutation[excluded.channel_index], excluded.reason))
            .collect();
        original_exclusions.sort_by_key(|(channel, _)| *channel);
        remapped_exclusions.sort_by_key(|(channel, _)| *channel);
        assert_eq!(remapped_exclusions, original_exclusions);
    }
}

#[derive(Debug, Clone, Copy)]
enum InvalidChunk {
    Misaligned,
    Nan,
    PositiveInfinity,
    NegativeInfinity,
    SquareOverflow,
    AccumulationOverflow,
}

fn invalid_chunk(kind: InvalidChunk, channel_count: usize, target_lane: usize) -> Vec<f64> {
    match kind {
        InvalidChunk::Misaligned => vec![0.125; channel_count + 1],
        InvalidChunk::AccumulationOverflow => {
            let near_limit = f64::MAX.sqrt() * 0.49;
            let mut samples = vec![0.0; channel_count * 5];
            for frame in 0..5 {
                samples[frame * channel_count + target_lane] = near_limit;
            }
            samples
        }
        InvalidChunk::Nan
        | InvalidChunk::PositiveInfinity
        | InvalidChunk::NegativeInfinity
        | InvalidChunk::SquareOverflow => {
            let invalid = match kind {
                InvalidChunk::Nan => f64::NAN,
                InvalidChunk::PositiveInfinity => f64::INFINITY,
                InvalidChunk::NegativeInfinity => f64::NEG_INFINITY,
                InvalidChunk::SquareOverflow => f64::MAX,
                InvalidChunk::Misaligned | InvalidChunk::AccumulationOverflow => unreachable!(),
            };
            let mut samples = safe_signal(3, channel_count, 10_000);
            samples[channel_count + target_lane] = invalid;
            samples
        }
    }
}

#[test]
fn rejected_chunks_leave_no_partial_mutation_before_continued_bit_exact_analysis() {
    let channel_count = 5;
    let spec = stream(10, channel_count, ChannelLayout::Unknown);
    let probe =
        AnalyzerSession::new(spec.clone(), AnalysisProfile::FooDrMeter108CandidateV1).unwrap();
    let window = probe.window_frames();
    assert_eq!(window, 30);

    for prefix_frames in [window - 1, window, window + 1] {
        let prefix = safe_signal(prefix_frames, channel_count, 0);
        let suffix = safe_signal(window + 2, channel_count, prefix_frames);
        let mut accepted = prefix.clone();
        accepted.extend_from_slice(&suffix);
        let expected = raw(&analyze(&spec, [accepted]));

        for kind in [
            InvalidChunk::Misaligned,
            InvalidChunk::Nan,
            InvalidChunk::PositiveInfinity,
            InvalidChunk::NegativeInfinity,
            InvalidChunk::SquareOverflow,
            InvalidChunk::AccumulationOverflow,
        ] {
            let target_lanes: &[usize] = if matches!(kind, InvalidChunk::Misaligned) {
                &[0]
            } else {
                &[0, channel_count / 2, channel_count - 1]
            };
            for target_lane in target_lanes {
                let mut session =
                    AnalyzerSession::new(spec.clone(), AnalysisProfile::FooDrMeter108CandidateV1)
                        .unwrap();
                session.push_interleaved(&prefix).unwrap();

                let error = session
                    .push_interleaved(&invalid_chunk(kind, channel_count, *target_lane))
                    .unwrap_err();
                assert_eq!(error.code, ErrorCode::AnalysisFailed, "{kind:?}");
                assert_eq!(error.stage, AnalysisStage::Analysis, "{kind:?}");
                assert_eq!(
                    session.frames_seen(),
                    u64::try_from(prefix_frames).unwrap(),
                    "{kind:?}, prefix={prefix_frames}, lane={target_lane}"
                );

                session.push_interleaved(&suffix).unwrap();
                let actual = session.finish().unwrap();
                assert_eq!(
                    raw(&actual),
                    expected,
                    "{kind:?}, prefix={prefix_frames}, lane={target_lane}"
                );
            }
        }
    }
}
