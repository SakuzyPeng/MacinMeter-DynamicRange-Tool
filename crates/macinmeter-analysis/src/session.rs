use crate::profile::{
    DR_FLOOR_DB, HISTOGRAM_BIN_WIDTH_DB, HISTOGRAM_BINS, LOUD_FRACTION_DENOMINATOR,
    MINIMUM_TAIL_FRAMES, PEAK_KEY_BIN_WIDTH_DB, RMS_HISTOGRAM_MAX_DB, RMS_HISTOGRAM_MIN_DB,
    RMS_SUM_MULTIPLIER, SILENT_CHANNEL_DR_DB, WINDOW_DURATION_COEFFICIENT, descriptor,
};
use macinmeter_domain::{
    AggregateResults, AlgorithmDescriptor, AnalysisError, AnalysisProfile, AnalysisResult,
    AnalysisStage, ChannelMeasurement, ChannelOutcome, ChannelReportMetrics, ChannelResult,
    DecodedDuration, ErrorCode, ExcludedChannel, ExclusionReason, FiniteF32, FiniteF64, StreamSpec,
    TrackAggregate, TrackReportMetrics,
};

/// A one-pass analysis session for a single PCM stream.
///
/// The stream specification and algorithm profile are immutable after
/// construction. Each call to [`Self::push_interleaved`] must contain complete
/// frames for that specification. [`Self::finish`] consumes the session so a
/// tail window cannot accidentally be finalized twice.
#[derive(Debug)]
pub struct AnalyzerSession {
    stream: StreamSpec,
    algorithm: AlgorithmDescriptor,
    window_frames: usize,
    frames_in_window: usize,
    frames_seen: u64,
    channels: Vec<ChannelAccumulator>,
}

impl AnalyzerSession {
    /// Creates an analyzer with the fixed rules of the requested profile.
    pub fn new(stream: StreamSpec, profile: AnalysisProfile) -> Result<Self, AnalysisError> {
        stream.channel_layout.validate(stream.channels)?;

        let window_frames_f64 =
            (stream.sample_rate.get() as f64 * WINDOW_DURATION_COEFFICIENT).floor();
        if window_frames_f64 < 1.0 || window_frames_f64 > usize::MAX as f64 {
            return Err(resource_error(
                "the analysis window length cannot be represented on this platform",
            ));
        }
        let window_frames = window_frames_f64 as usize;

        let channel_count = stream.channels.as_usize();
        let mut channels = Vec::new();
        channels
            .try_reserve_exact(channel_count)
            .map_err(|_| resource_error("unable to allocate per-channel analysis state"))?;
        for _ in 0..channel_count {
            channels.push(ChannelAccumulator::try_new()?);
        }

        Ok(Self {
            stream,
            algorithm: descriptor(profile),
            window_frames,
            frames_in_window: 0,
            frames_seen: 0,
            channels,
        })
    }

    /// Returns the immutable PCM stream specification.
    pub fn stream(&self) -> &StreamSpec {
        &self.stream
    }

    /// Returns the fixed algorithm descriptor recorded in the final result.
    pub fn algorithm(&self) -> &AlgorithmDescriptor {
        &self.algorithm
    }

    /// Returns the number of complete frames accepted so far.
    pub fn frames_seen(&self) -> u64 {
        self.frames_seen
    }

    /// Returns the profile's window length in frames per channel.
    pub fn window_frames(&self) -> usize {
        self.window_frames
    }

    /// Adds finite, frame-aligned, interleaved PCM samples.
    ///
    /// Validation is atomic: an invalid chunk leaves the session unchanged and
    /// may be followed by another valid chunk.
    pub fn push_interleaved(&mut self, samples: &[f64]) -> Result<(), AnalysisError> {
        let channel_count = self.stream.channels.as_usize();
        if !samples.len().is_multiple_of(channel_count) {
            return Err(analysis_error(format!(
                "interleaved PCM chunk contains {} samples, which is not divisible by the configured {} channels",
                samples.len(),
                channel_count
            )));
        }
        if samples.iter().any(|sample| !sample.is_finite()) {
            return Err(analysis_error(
                "interleaved PCM chunk contains a non-finite sample",
            ));
        }

        let chunk_frames = samples.len() / channel_count;
        let chunk_frames_u64 = u64::try_from(chunk_frames)
            .map_err(|_| resource_error("PCM chunk frame count exceeds the supported range"))?;
        let new_frames_seen = self
            .frames_seen
            .checked_add(chunk_frames_u64)
            .ok_or_else(|| resource_error("total PCM frame count exceeds the supported range"))?;

        self.validate_numeric_safety(samples)?;

        for frame in samples.chunks_exact(channel_count) {
            for (channel, sample) in self.channels.iter_mut().zip(frame) {
                channel.add_sample(*sample);
            }

            self.frames_in_window += 1;
            if self.frames_in_window == self.window_frames {
                self.finalize_current_window();
            }
        }

        self.frames_seen = new_frames_seen;
        Ok(())
    }

    fn validate_numeric_safety(&self, samples: &[f64]) -> Result<(), AnalysisError> {
        let channel_count = self.stream.channels.as_usize();

        for (channel_index, channel) in self.channels.iter().enumerate() {
            let mut sum_squares = channel.current_sum_squares;
            let mut sum_window_rms2 = channel.sum_window_rms2;
            let mut frames_in_window = self.frames_in_window;

            for sample in samples.iter().skip(channel_index).step_by(channel_count) {
                let magnitude = sample.abs();
                let square = magnitude * magnitude;
                if !square.is_finite() {
                    return Err(analysis_error(format!(
                        "PCM sample in channel {channel_index} is too large to square without overflow"
                    )));
                }

                sum_squares += square;
                if !sum_squares.is_finite() {
                    return Err(analysis_error(format!(
                        "PCM square accumulation in channel {channel_index} exceeds the finite f64 range"
                    )));
                }

                frames_in_window += 1;
                let rms2 = window_rms_squared(sum_squares, frames_in_window);
                if !rms2.is_finite() {
                    return Err(analysis_error(format!(
                        "PCM window RMS in channel {channel_index} exceeds the finite f64 range"
                    )));
                }

                if frames_in_window == self.window_frames {
                    sum_window_rms2 += rms2;
                    if !sum_window_rms2.is_finite() {
                        return Err(analysis_error(format!(
                            "overall RMS accumulation in channel {channel_index} exceeds the finite f64 range"
                        )));
                    }
                    sum_squares = 0.0;
                    frames_in_window = 0;
                }
            }
        }

        Ok(())
    }

    /// Finalizes every non-empty tail window and returns the complete analysis.
    pub fn finish(mut self) -> Result<AnalysisResult, AnalysisError> {
        if self.frames_in_window >= MINIMUM_TAIL_FRAMES {
            self.validate_tail_numeric_safety()?;
            self.finalize_current_window();
        }

        let mut channel_results = Vec::with_capacity(self.channels.len());
        let mut aggregate_drs = Vec::with_capacity(self.channels.len());
        for (channel_index, channel) in self.channels.into_iter().enumerate() {
            let finalized = channel.into_outcome(self.frames_seen)?;
            channel_results.push(ChannelResult {
                channel_index,
                report: finalized.report,
                outcome: finalized.outcome,
            });
            aggregate_drs.push(finalized.aggregate_dr_db);
        }

        let track = aggregate(&channel_results, &aggregate_drs);
        let report = track_report(&channel_results, self.frames_seen, self.stream.sample_rate)?;
        Ok(AnalysisResult {
            algorithm: self.algorithm,
            stream: self.stream,
            frames_seen: self.frames_seen,
            channels: channel_results,
            aggregates: AggregateResults { track },
            report,
        })
    }

    fn validate_tail_numeric_safety(&self) -> Result<(), AnalysisError> {
        debug_assert!(self.frames_in_window > 0);
        for (channel_index, channel) in self.channels.iter().enumerate() {
            let rms2 = window_rms_squared(channel.current_sum_squares, self.frames_in_window);
            let total = channel.sum_window_rms2 + rms2;
            if !total.is_finite() {
                return Err(analysis_error(format!(
                    "overall RMS accumulation in channel {channel_index} exceeds the finite f64 range"
                )));
            }
        }
        Ok(())
    }

    fn finalize_current_window(&mut self) {
        debug_assert!(self.frames_in_window > 0);
        let frames = self.frames_in_window;
        for channel in &mut self.channels {
            channel.finalize_window(frames);
        }
        self.frames_in_window = 0;
    }
}

#[derive(Debug)]
struct ChannelAccumulator {
    current_sum_squares: f64,
    sum_window_rms2: f64,
    current_peak: f64,
    saw_nonzero_sample: bool,
    histogram: Vec<u64>,
    valid_windows: u64,
    peaks: TopTwoPeaks,
}

impl ChannelAccumulator {
    fn try_new() -> Result<Self, AnalysisError> {
        let mut histogram = Vec::new();
        histogram
            .try_reserve_exact(HISTOGRAM_BINS)
            .map_err(|_| resource_error("unable to allocate an RMS histogram"))?;
        histogram.resize(HISTOGRAM_BINS, 0);

        Ok(Self {
            current_sum_squares: 0.0,
            sum_window_rms2: 0.0,
            current_peak: 0.0,
            saw_nonzero_sample: false,
            histogram,
            valid_windows: 0,
            peaks: TopTwoPeaks::default(),
        })
    }

    fn add_sample(&mut self, sample: f64) {
        let magnitude = sample.abs();
        self.current_sum_squares += magnitude * magnitude;
        self.current_peak = self.current_peak.max(magnitude);
        self.saw_nonzero_sample |= magnitude != 0.0;
    }

    fn finalize_window(&mut self, frames: usize) {
        let rms2 = window_rms_squared(self.current_sum_squares, frames);
        let rms = rms2.sqrt();
        debug_assert!(rms.is_finite());
        self.sum_window_rms2 += rms2;
        debug_assert!(self.sum_window_rms2.is_finite());
        if rms != 0.0 {
            self.histogram[rms_histogram_bin(rms)] += 1;
        }

        self.valid_windows += 1;
        if self.current_peak > 0.0 {
            self.peaks.observe(self.current_peak);
        }
        self.current_sum_squares = 0.0;
        self.current_peak = 0.0;
    }

    fn into_outcome(self, frames: u64) -> Result<FinalizedChannel, AnalysisError> {
        let (primary_peak, secondary_peak) = self.peaks.values();
        let overall_rms = if self.valid_windows == 0 {
            0.0
        } else {
            (self.sum_window_rms2 / self.valid_windows as f64).sqrt()
        };
        let report = channel_report(overall_rms, primary_peak)?;

        if self.valid_windows == 0 {
            return Ok(FinalizedChannel {
                outcome: ChannelOutcome::InsufficientData { frames },
                aggregate_dr_db: None,
                report,
            });
        }
        if !self.saw_nonzero_sample {
            return Ok(FinalizedChannel {
                outcome: ChannelOutcome::Silent {
                    frames,
                    valid_windows: self.valid_windows,
                },
                aggregate_dr_db: Some(SILENT_CHANNEL_DR_DB),
                report,
            });
        }

        let Some(loud_window_rms) = loud_window_rms(&self.histogram, self.valid_windows) else {
            return Ok(FinalizedChannel {
                outcome: ChannelOutcome::InsufficientData { frames },
                aggregate_dr_db: None,
                report,
            });
        };
        let mut selected_peak = secondary_peak
            .filter(|peak| *peak > 0.0)
            .unwrap_or(primary_peak);
        if selected_peak == 0.0 || primary_peak == 0.0 || loud_window_rms == 0.0 {
            return Ok(FinalizedChannel {
                outcome: ChannelOutcome::InsufficientData { frames },
                aggregate_dr_db: None,
                report,
            });
        }

        let mut dr_db = dr_for_peak(loud_window_rms, selected_peak);
        if dr_db < DR_FLOOR_DB {
            selected_peak = primary_peak;
            dr_db = dr_for_peak(loud_window_rms, primary_peak).max(DR_FLOOR_DB);
        }
        let public_dr_db = dr_db as f32;

        Ok(FinalizedChannel {
            outcome: ChannelOutcome::Measured {
                measurement: ChannelMeasurement {
                    dr_db: public_dr_db,
                    rounded_dr: rounded_display_dr(public_dr_db),
                    loud_window_rms,
                    dr_selected_peak: selected_peak,
                    dr_primary_peak: primary_peak,
                    dr_secondary_peak: secondary_peak,
                    valid_windows: self.valid_windows,
                    frames,
                },
            },
            aggregate_dr_db: Some(dr_db),
            report,
        })
    }
}

fn window_rms_squared(sum_squares: f64, frames: usize) -> f64 {
    debug_assert!(frames > 0);
    RMS_SUM_MULTIPLIER * sum_squares / frames as f64
}

#[derive(Debug)]
struct FinalizedChannel {
    outcome: ChannelOutcome,
    aggregate_dr_db: Option<f64>,
    report: ChannelReportMetrics,
}

#[derive(Debug, Clone, Copy)]
struct PeakCandidate {
    amplitude: f64,
    key_centi_db: i32,
}

#[derive(Debug, Default)]
struct TopTwoPeaks {
    primary: Option<PeakCandidate>,
    secondary: Option<PeakCandidate>,
}

impl TopTwoPeaks {
    fn observe(&mut self, amplitude: f64) {
        debug_assert!(amplitude > 0.0);
        let candidate = PeakCandidate {
            amplitude,
            key_centi_db: centi_db_key(amplitude),
        };

        match self.primary {
            None => self.primary = Some(candidate),
            Some(primary) if candidate.key_centi_db > primary.key_centi_db => {
                self.secondary = Some(primary);
                self.primary = Some(candidate);
            }
            Some(_) => match self.secondary {
                None => self.secondary = Some(candidate),
                Some(secondary) if candidate.key_centi_db > secondary.key_centi_db => {
                    self.secondary = Some(candidate);
                }
                Some(_) => {}
            },
        }
    }

    fn values(&self) -> (f64, Option<f64>) {
        (
            self.primary.map_or(0.0, |peak| peak.amplitude),
            self.secondary.map(|peak| peak.amplitude),
        )
    }
}

fn centi_db_key(amplitude: f64) -> i32 {
    debug_assert_eq!(PEAK_KEY_BIN_WIDTH_DB, 0.01);
    (2_000.0 * amplitude.log10()).round() as i32
}

fn rms_histogram_bin(rms: f64) -> usize {
    debug_assert_eq!(HISTOGRAM_BIN_WIDTH_DB, 0.01);
    debug_assert_eq!(RMS_HISTOGRAM_MIN_DB, -100.0);
    debug_assert_eq!(RMS_HISTOGRAM_MAX_DB, 0.0);
    let key_centi_db = centi_db_key(rms).clamp(-10_000, 0);
    usize::try_from(key_centi_db + 10_000).unwrap()
}

fn loud_window_rms(histogram: &[u64], valid_windows: u64) -> Option<f64> {
    debug_assert_eq!(histogram.len(), HISTOGRAM_BINS);
    debug_assert!(valid_windows > 0);

    let target = (valid_windows / LOUD_FRACTION_DENOMINATOR).max(1);
    let mut selected_count = 0_u64;
    let mut selected_power = 0.0;

    for (bin, count) in histogram.iter().copied().enumerate().rev() {
        if count == 0 {
            continue;
        }

        let bin_db = RMS_HISTOGRAM_MIN_DB + bin as f64 * HISTOGRAM_BIN_WIDTH_DB;
        selected_count += count;
        selected_power += 10.0_f64.powf(bin_db / 10.0) * count as f64;
        if selected_count >= target {
            break;
        }
    }

    (selected_count > 0).then(|| (selected_power / selected_count as f64).sqrt())
}

fn dr_for_peak(loud_window_rms: f64, peak: f64) -> f64 {
    -20.0 * (loud_window_rms / peak).log10()
}

fn rounded_display_dr(dr_db: f32) -> u32 {
    (dr_db + 0.5).trunc() as u32
}

fn channel_report(
    overall_rms: f64,
    primary_peak: f64,
) -> Result<ChannelReportMetrics, AnalysisError> {
    let overall_rms_linear = finite_f32_narrow(overall_rms, "channel overall RMS")?;
    let primary_peak_linear = finite_f32_narrow(primary_peak, "channel primary peak")?;
    Ok(ChannelReportMetrics {
        overall_rms_linear,
        overall_rms_dbfs: report_dbfs(
            f64::from(overall_rms_linear.get()),
            "channel overall RMS dBFS",
        )?,
        primary_peak_linear,
    })
}

fn track_report(
    channels: &[ChannelResult],
    decoded_frames: u64,
    sample_rate: macinmeter_domain::SampleRate,
) -> Result<TrackReportMetrics, AnalysisError> {
    if channels.is_empty() {
        return Err(analysis_error(
            "track report requires at least one configured channel",
        ));
    }

    let mut rms_power_sum = 0.0_f64;
    let mut primary_peak = 0.0_f32;
    for channel in channels {
        let channel_rms = channel.report.overall_rms_linear.get();
        let channel_rms_squared = channel_rms * channel_rms;
        if !channel_rms_squared.is_finite() {
            return Err(analysis_error(format!(
                "public overall RMS square in channel {} exceeds the finite f32 range",
                channel.channel_index
            )));
        }
        rms_power_sum += f64::from(channel_rms_squared);
        if !rms_power_sum.is_finite() {
            return Err(analysis_error(
                "track overall RMS accumulation exceeds the finite f64 range",
            ));
        }
        primary_peak = primary_peak.max(channel.report.primary_peak_linear.get());
    }

    let overall_rms = (rms_power_sum / channels.len() as f64).sqrt();
    let overall_rms_linear = finite_f64(overall_rms, "track overall RMS")?;
    let primary_peak_linear = FiniteF32::new(primary_peak)
        .map_err(|_| analysis_error("track primary peak is not finite"))?;

    Ok(TrackReportMetrics {
        overall_rms_linear,
        overall_rms_dbfs: report_dbfs(overall_rms, "track overall RMS dBFS")?,
        primary_peak_linear,
        primary_peak_dbfs: report_dbfs(
            f64::from(primary_peak_linear.get()),
            "track primary peak dBFS",
        )?,
        duration: DecodedDuration::new(decoded_frames, sample_rate),
    })
}

fn finite_f32_narrow(value: f64, label: &str) -> Result<FiniteF32, AnalysisError> {
    if !value.is_finite() {
        return Err(analysis_error(format!("{label} is not finite")));
    }
    FiniteF32::new(value as f32)
        .map_err(|_| analysis_error(format!("{label} cannot be represented as finite f32")))
}

fn finite_f64(value: f64, label: &str) -> Result<FiniteF64, AnalysisError> {
    FiniteF64::new(value).map_err(|_| analysis_error(format!("{label} is not finite")))
}

fn report_dbfs(linear: f64, label: &str) -> Result<Option<FiniteF32>, AnalysisError> {
    if !linear.is_finite() || linear < 0.0 {
        return Err(analysis_error(format!(
            "{label} requires a finite non-negative linear value"
        )));
    }
    if linear == 0.0 {
        return Ok(None);
    }
    finite_f32_narrow(20.0 * linear.log10(), label).map(Some)
}

fn aggregate(channels: &[ChannelResult], aggregate_drs: &[Option<f64>]) -> TrackAggregate {
    debug_assert_eq!(channels.len(), aggregate_drs.len());
    let mut contributing_channels = Vec::new();
    let mut excluded_channels = Vec::new();
    let mut dr_sum = 0.0;

    for (channel, aggregate_dr_db) in channels.iter().zip(aggregate_drs) {
        match aggregate_dr_db {
            Some(dr_db) => {
                contributing_channels.push(channel.channel_index);
                dr_sum += dr_db;
            }
            None => {
                debug_assert!(matches!(
                    channel.outcome,
                    ChannelOutcome::InsufficientData { .. }
                ));
                excluded_channels.push(ExcludedChannel {
                    channel_index: channel.channel_index,
                    reason: ExclusionReason::InsufficientData,
                });
            }
        }
    }

    let dr_db = (!contributing_channels.is_empty())
        .then(|| (dr_sum / contributing_channels.len() as f64) as f32);
    TrackAggregate {
        dr_db,
        rounded_dr: dr_db.map(rounded_display_dr),
        contributing_channels,
        excluded_channels,
    }
}

fn analysis_error(message: impl Into<String>) -> AnalysisError {
    AnalysisError::new(ErrorCode::AnalysisFailed, AnalysisStage::Analysis, message)
}

fn resource_error(message: impl Into<String>) -> AnalysisError {
    AnalysisError::new(
        ErrorCode::ResourceExhausted,
        AnalysisStage::Analysis,
        message,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate_session(stream: StreamSpec) -> AnalyzerSession {
        AnalyzerSession::new(stream, AnalysisProfile::FooDrMeter108CandidateV1).unwrap()
    }

    fn report_only_channel(index: usize, overall_rms: f32, primary_peak: f32) -> ChannelResult {
        ChannelResult {
            channel_index: index,
            report: ChannelReportMetrics {
                overall_rms_linear: FiniteF32::new(overall_rms).unwrap(),
                overall_rms_dbfs: report_dbfs(f64::from(overall_rms), "test channel overall RMS")
                    .unwrap(),
                primary_peak_linear: FiniteF32::new(primary_peak).unwrap(),
            },
            outcome: ChannelOutcome::InsufficientData { frames: 0 },
        }
    }

    #[test]
    fn histogram_shape_is_fixed_per_channel() {
        let stream = StreamSpec::new(48_000, 6, macinmeter_domain::ChannelLayout::Unknown).unwrap();
        let session = candidate_session(stream);

        assert_eq!(session.channels.len(), 6);
        assert!(
            session
                .channels
                .iter()
                .all(|channel| channel.histogram.len() == HISTOGRAM_BINS)
        );
    }

    #[test]
    fn strict_quantized_keys_preserve_first_arrival_for_ties() {
        let low = 10.0_f64.powf(-2.0035 / 20.0);
        let high = 10.0_f64.powf(-1.9965 / 20.0);
        let third = 10.0_f64.powf(-3.1 / 20.0);
        assert_eq!(centi_db_key(low), -200);
        assert_eq!(centi_db_key(high), -200);
        assert_eq!(centi_db_key(third), -310);

        let mut low_then_high = TopTwoPeaks::default();
        low_then_high.observe(low);
        low_then_high.observe(high);
        low_then_high.observe(third);
        assert_eq!(low_then_high.values(), (low, Some(high)));

        let mut high_then_low = TopTwoPeaks::default();
        high_then_low.observe(high);
        high_then_low.observe(low);
        high_then_low.observe(third);
        assert_eq!(high_then_low.values(), (high, Some(low)));
    }

    #[test]
    fn duplicate_peak_keys_fill_but_do_not_replace_secondary() {
        let mut peaks = TopTwoPeaks::default();
        peaks.observe(0.75);
        peaks.observe(0.75);
        peaks.observe(0.75);
        peaks.observe(0.5);

        assert_eq!(peaks.values(), (0.75, Some(0.75)));
    }

    #[test]
    fn track_aggregate_uses_internal_f64_channel_values() {
        let report = ChannelReportMetrics {
            overall_rms_linear: FiniteF32::new(0.1).unwrap(),
            overall_rms_dbfs: FiniteF32::new(-20.0).ok(),
            primary_peak_linear: FiniteF32::new(1.0).unwrap(),
        };
        let channels = [
            ChannelResult {
                channel_index: 0,
                report: report.clone(),
                outcome: ChannelOutcome::Measured {
                    measurement: ChannelMeasurement {
                        dr_db: 1.0,
                        rounded_dr: 1,
                        loud_window_rms: 0.1,
                        dr_selected_peak: 1.0,
                        dr_primary_peak: 1.0,
                        dr_secondary_peak: None,
                        valid_windows: 1,
                        frames: 3,
                    },
                },
            },
            ChannelResult {
                channel_index: 1,
                report,
                outcome: ChannelOutcome::Measured {
                    measurement: ChannelMeasurement {
                        dr_db: 1.0,
                        rounded_dr: 1,
                        loud_window_rms: 0.1,
                        dr_selected_peak: 1.0,
                        dr_primary_peak: 1.0,
                        dr_secondary_peak: None,
                        valid_windows: 1,
                        frames: 3,
                    },
                },
            },
        ];

        let track = aggregate(&channels, &[Some(10.0), Some(20.0)]);
        assert_eq!(track.dr_db, Some(15.0));
        assert_eq!(track.rounded_dr, Some(15));
    }

    #[test]
    fn track_report_squares_public_f32_rms_before_widening() {
        let channels = [
            report_only_channel(0, 0.1, 0.25),
            report_only_channel(1, 0.3, 0.75),
        ];
        let sample_rate = macinmeter_domain::SampleRate::new(48_000).unwrap();
        let report = track_report(&channels, 96_000, sample_rate).unwrap();

        let expected = ((f64::from(0.1_f32 * 0.1_f32) + f64::from(0.3_f32 * 0.3_f32)) / 2.0).sqrt();
        let square_after_widening =
            ((f64::from(0.1_f32).powi(2) + f64::from(0.3_f32).powi(2)) / 2.0).sqrt();

        assert_ne!(expected.to_bits(), square_after_widening.to_bits());
        assert_eq!(
            report.overall_rms_linear.get().to_bits(),
            expected.to_bits()
        );
        assert_eq!(report.primary_peak_linear.get(), 0.75);
        assert_eq!(report.duration.seconds(), 2.0);
    }

    #[test]
    fn long_stream_does_not_grow_per_channel_storage() {
        let stream = StreamSpec::new(1, 8, macinmeter_domain::ChannelLayout::Unknown).unwrap();
        let mut session = candidate_session(stream);
        let histogram_capacities: Vec<_> = session
            .channels
            .iter()
            .map(|channel| channel.histogram.capacity())
            .collect();
        let chunk = vec![0.25; 8 * 997];

        for _ in 0..2_000 {
            session.push_interleaved(&chunk).unwrap();
        }

        assert_eq!(
            session
                .channels
                .iter()
                .map(|channel| channel.histogram.capacity())
                .collect::<Vec<_>>(),
            histogram_capacities
        );
    }
}
