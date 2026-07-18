use crate::profile::{
    EXACT_WINDOW_VIRTUAL_ZERO_PEAK, HISTOGRAM_BINS, HISTOGRAM_SCALE, LOUD_FRACTION_DENOMINATOR,
    MINIMUM_NONZERO_RMS_BIN, MINIMUM_TAIL_FRAMES, RMS_SUM_MULTIPLIER, WINDOW_DURATION_COEFFICIENT,
    descriptor,
};
use macinmeter_domain::{
    AggregateResults, AlgorithmDescriptor, AnalysisError, AnalysisProfile, AnalysisResult,
    AnalysisStage, ChannelOutcome, ChannelResult, ErrorCode, ExcludedChannel, ExclusionReason,
    StreamSpec, TrackAggregate,
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

    /// Creates an analyzer using [`AnalysisProfile::ProvisionalV1`].
    pub fn provisional_v1(stream: StreamSpec) -> Result<Self, AnalysisError> {
        Self::new(stream, AnalysisProfile::ProvisionalV1)
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
    pub fn push_interleaved(&mut self, samples: &[f32]) -> Result<(), AnalysisError> {
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

    /// Finalizes the optional tail window and returns the complete analysis.
    pub fn finish(mut self) -> AnalysisResult {
        let ended_on_window_boundary = self.frames_seen > 0 && self.frames_in_window == 0;

        if self.frames_in_window >= MINIMUM_TAIL_FRAMES {
            self.finalize_current_window();
        }

        let mut channel_results = Vec::with_capacity(self.channels.len());
        for (channel_index, channel) in self.channels.into_iter().enumerate() {
            let outcome = channel.into_outcome(
                self.frames_seen,
                ended_on_window_boundary && EXACT_WINDOW_VIRTUAL_ZERO_PEAK,
            );
            channel_results.push(ChannelResult {
                channel_index,
                outcome,
            });
        }

        let all_channels = aggregate(&channel_results, &self.stream, false);
        let without_lfe = match &self.stream.channel_layout {
            macinmeter_domain::ChannelLayout::Unknown => None,
            macinmeter_domain::ChannelLayout::KnownNoLfe
            | macinmeter_domain::ChannelLayout::Known { .. } => {
                Some(aggregate(&channel_results, &self.stream, true))
            }
        };

        AnalysisResult {
            algorithm: self.algorithm,
            stream: self.stream,
            frames_seen: self.frames_seen,
            channels: channel_results,
            aggregates: AggregateResults {
                all_channels,
                without_lfe,
            },
        }
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
            current_peak: 0.0,
            saw_nonzero_sample: false,
            histogram,
            valid_windows: 0,
            peaks: TopTwoPeaks::default(),
        })
    }

    fn add_sample(&mut self, sample: f32) {
        let sample = f64::from(sample);
        self.current_sum_squares += sample * sample;
        self.current_peak = self.current_peak.max(sample.abs());
        self.saw_nonzero_sample |= sample != 0.0;
    }

    fn finalize_window(&mut self, frames: usize) {
        let rms = (RMS_SUM_MULTIPLIER * self.current_sum_squares / frames as f64).sqrt();
        let truncated_bin = (rms * HISTOGRAM_SCALE) as usize;
        let bin = if rms == 0.0 {
            0
        } else {
            truncated_bin.clamp(MINIMUM_NONZERO_RMS_BIN, HISTOGRAM_BINS - 1)
        };

        self.histogram[bin] += 1;
        self.valid_windows += 1;
        self.peaks.observe(self.current_peak);
        self.current_sum_squares = 0.0;
        self.current_peak = 0.0;
    }

    fn into_outcome(self, frames: u64, include_virtual_zero_peak: bool) -> ChannelOutcome {
        if self.valid_windows == 0 {
            return ChannelOutcome::InsufficientData { frames };
        }

        let loud_rms = loud_rms(&self.histogram, self.valid_windows);
        let mut peaks = self.peaks;
        if include_virtual_zero_peak {
            peaks.observe(0.0);
        }
        let (primary_peak, secondary_peak) = peaks.values();
        let selected_peak = secondary_peak
            .filter(|peak| *peak > 0.0)
            .unwrap_or(primary_peak);

        if !self.saw_nonzero_sample {
            return ChannelOutcome::Silent {
                frames,
                valid_windows: self.valid_windows,
            };
        }
        if loud_rms == 0.0 || selected_peak == 0.0 {
            return ChannelOutcome::InsufficientData { frames };
        }

        let dr_db = -20.0 * (loud_rms / selected_peak).log10();
        ChannelOutcome::Measured {
            measurement: macinmeter_domain::ChannelMeasurement {
                dr_db,
                rounded_dr: dr_db.round() as i32,
                loud_rms,
                selected_peak,
                primary_peak,
                secondary_peak,
                valid_windows: self.valid_windows,
                frames,
            },
        }
    }
}

#[derive(Debug, Default)]
struct TopTwoPeaks {
    primary: Option<f64>,
    secondary: Option<f64>,
}

impl TopTwoPeaks {
    fn observe(&mut self, peak: f64) {
        match self.primary {
            None => self.primary = Some(peak),
            Some(primary) if peak >= primary => {
                self.secondary = Some(primary);
                self.primary = Some(peak);
            }
            Some(_) => match self.secondary {
                None => self.secondary = Some(peak),
                Some(secondary) if peak > secondary => self.secondary = Some(peak),
                Some(_) => {}
            },
        }
    }

    fn values(self) -> (f64, Option<f64>) {
        let primary = self.primary.unwrap_or(0.0);
        (primary, self.secondary)
    }
}

fn loud_rms(histogram: &[u64], valid_windows: u64) -> f64 {
    debug_assert_eq!(histogram.len(), HISTOGRAM_BINS);
    debug_assert!(valid_windows > 0);

    let target = (valid_windows / LOUD_FRACTION_DENOMINATOR).max(1);
    let mut remaining = target;
    let mut sum_squared_bins = 0_u128;

    for (bin, count) in histogram.iter().copied().enumerate().rev() {
        let take = count.min(remaining);
        if take == 0 {
            continue;
        }
        let bin = bin as u128;
        sum_squared_bins += u128::from(take) * bin * bin;
        remaining -= take;
        if remaining == 0 {
            break;
        }
    }

    debug_assert_eq!(remaining, 0);
    ((sum_squared_bins as f64 / target as f64) / (HISTOGRAM_SCALE * HISTOGRAM_SCALE)).sqrt()
}

fn aggregate(channels: &[ChannelResult], stream: &StreamSpec, exclude_lfe: bool) -> TrackAggregate {
    let mut included_channels = Vec::new();
    let mut excluded_channels = Vec::new();
    let mut dr_sum = 0.0;

    for channel in channels {
        match &channel.outcome {
            ChannelOutcome::Measured { measurement } => {
                let is_lfe = exclude_lfe
                    && stream
                        .channel_layout
                        .is_lfe(channel.channel_index)
                        .unwrap_or(false);
                if is_lfe {
                    excluded_channels.push(ExcludedChannel {
                        channel_index: channel.channel_index,
                        reason: ExclusionReason::Lfe,
                    });
                } else {
                    included_channels.push(channel.channel_index);
                    dr_sum += measurement.dr_db;
                }
            }
            ChannelOutcome::Silent { .. } => {
                excluded_channels.push(ExcludedChannel {
                    channel_index: channel.channel_index,
                    reason: ExclusionReason::Silent,
                });
            }
            ChannelOutcome::InsufficientData { .. } => {
                excluded_channels.push(ExcludedChannel {
                    channel_index: channel.channel_index,
                    reason: ExclusionReason::InsufficientData,
                });
            }
        }
    }

    let precise_dr_db =
        (!included_channels.is_empty()).then(|| dr_sum / included_channels.len() as f64);
    TrackAggregate {
        precise_dr_db,
        rounded_dr: precise_dr_db.map(|value| value.round() as i32),
        included_channels,
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

    #[test]
    fn histogram_shape_is_fixed_per_channel() {
        let stream = StreamSpec::new(48_000, 6, macinmeter_domain::ChannelLayout::Unknown).unwrap();
        let session = AnalyzerSession::provisional_v1(stream).unwrap();

        assert_eq!(session.channels.len(), 6);
        assert!(
            session
                .channels
                .iter()
                .all(|channel| channel.histogram.len() == HISTOGRAM_BINS)
        );
    }

    #[test]
    fn duplicate_peaks_are_preserved_as_order_statistics() {
        let mut peaks = TopTwoPeaks::default();
        peaks.observe(0.25);
        peaks.observe(0.75);
        peaks.observe(0.75);
        peaks.observe(0.5);

        assert_eq!(peaks.values(), (0.75, Some(0.75)));
    }

    #[test]
    fn long_stream_does_not_grow_per_channel_storage() {
        let stream = StreamSpec::new(1, 8, macinmeter_domain::ChannelLayout::Unknown).unwrap();
        let mut session = AnalyzerSession::provisional_v1(stream).unwrap();
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
