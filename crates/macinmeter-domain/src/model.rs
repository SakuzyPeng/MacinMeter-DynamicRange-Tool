use crate::{AnalysisError, AnalysisStage, ErrorCode};
use serde::{Deserialize, Deserializer, Serialize};
use std::num::{NonZeroU16, NonZeroU32};

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct FiniteF32(f32);

impl FiniteF32 {
    pub fn new(value: f32) -> Result<Self, AnalysisError> {
        if value.is_finite() {
            Ok(Self(value))
        } else {
            Err(AnalysisError::invalid(
                "finite f32 value cannot be NaN or infinity",
            ))
        }
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for FiniteF32 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = f32::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct FiniteF64(f64);

impl FiniteF64 {
    pub fn new(value: f64) -> Result<Self, AnalysisError> {
        if value.is_finite() {
            Ok(Self(value))
        } else {
            Err(AnalysisError::invalid(
                "finite f64 value cannot be NaN or infinity",
            ))
        }
    }

    pub const fn get(self) -> f64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for FiniteF64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = f64::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SampleRate(NonZeroU32);

impl SampleRate {
    pub fn new(value: u32) -> Result<Self, AnalysisError> {
        NonZeroU32::new(value)
            .map(Self)
            .ok_or_else(|| AnalysisError::invalid("sample rate must be greater than zero"))
    }

    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

/// Maximum channel count accepted by the product analysis pipeline.
///
/// [`ChannelCount`] remains capable of representing larger source geometries so
/// codecs can report and reject them without losing the declared channel count.
pub const MAX_ANALYSIS_CHANNELS: u16 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelCount(NonZeroU16);

impl ChannelCount {
    pub fn new(value: u16) -> Result<Self, AnalysisError> {
        NonZeroU16::new(value)
            .map(Self)
            .ok_or_else(|| AnalysisError::invalid("channel count must be greater than zero"))
    }

    pub const fn get(self) -> u16 {
        self.0.get()
    }

    pub const fn as_usize(self) -> usize {
        self.0.get() as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelRole {
    FrontLeft,
    FrontRight,
    FrontCenter,
    Lfe,
    BackLeft,
    BackRight,
    SideLeft,
    SideRight,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ChannelLayout {
    Unknown,
    KnownNoLfe,
    Known { positions: Vec<ChannelRole> },
}

impl ChannelLayout {
    pub fn validate(&self, channels: ChannelCount) -> Result<(), AnalysisError> {
        if let Self::Known { positions } = self
            && positions.len() != channels.as_usize()
        {
            return Err(AnalysisError::invalid(format!(
                "channel layout contains {} positions for {} channels",
                positions.len(),
                channels.get()
            )));
        }
        Ok(())
    }

    pub fn is_lfe(&self, index: usize) -> Option<bool> {
        match self {
            Self::Unknown => None,
            Self::KnownNoLfe => Some(false),
            Self::Known { positions } => positions
                .get(index)
                .map(|role| matches!(role, ChannelRole::Lfe)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamSpec {
    pub sample_rate: SampleRate,
    pub channels: ChannelCount,
    pub channel_layout: ChannelLayout,
}

impl StreamSpec {
    pub fn new(
        sample_rate: u32,
        channels: u16,
        channel_layout: ChannelLayout,
    ) -> Result<Self, AnalysisError> {
        let sample_rate = SampleRate::new(sample_rate)?;
        let channels = ChannelCount::new(channels)?;
        channel_layout.validate(channels)?;
        Ok(Self {
            sample_rate,
            channels,
            channel_layout,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerFormat {
    Wave,
    Flac,
    Aiff,
    Mp4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCodec {
    PcmInteger,
    PcmFloat,
    Flac,
    Alac,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub display_path: String,
    pub container: ContainerFormat,
    pub codec: SourceCodec,
    pub sample_rate: SampleRate,
    pub channels: ChannelCount,
    pub bits_per_sample: Option<u32>,
    pub expected_frames: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PcmStreamInfo {
    pub spec: StreamSpec,
    pub expected_frames: Option<u64>,
}

/// A non-empty block of finite, frame-aligned interleaved PCM samples.
///
/// The retained channel count is the geometry used to interpret the samples;
/// semantic channel layout remains a stream-level property.
#[derive(Debug, Clone, PartialEq)]
pub struct PcmBlock {
    samples: Vec<f64>,
    frames: usize,
    channels: ChannelCount,
}

impl PcmBlock {
    pub fn new(samples: Vec<f64>, channels: ChannelCount) -> Result<Self, AnalysisError> {
        if samples.is_empty() {
            return Err(AnalysisError::new(
                ErrorCode::DecodeFailed,
                AnalysisStage::Decode,
                "decoder produced an empty PCM block",
            ));
        }
        if !samples.len().is_multiple_of(channels.as_usize()) {
            return Err(AnalysisError::new(
                ErrorCode::DecodeFailed,
                AnalysisStage::Decode,
                "decoder produced a block that is not frame-aligned",
            ));
        }
        if samples.iter().any(|sample| !sample.is_finite()) {
            return Err(AnalysisError::new(
                ErrorCode::DecodeFailed,
                AnalysisStage::Decode,
                "decoder produced a non-finite PCM sample",
            ));
        }
        let frames = samples.len() / channels.as_usize();
        Ok(Self {
            samples,
            frames,
            channels,
        })
    }

    pub fn samples(&self) -> &[f64] {
        &self.samples
    }

    pub const fn frames(&self) -> usize {
        self.frames
    }

    /// Return the channel geometry used when this block was constructed.
    pub const fn channels(&self) -> ChannelCount {
        self.channels
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodeProgress {
    decoded_frames: u64,
    expected_frames: Option<u64>,
    fraction: Option<FiniteF64>,
    eof: bool,
}

impl DecodeProgress {
    pub fn new(decoded_frames: u64, expected_frames: Option<u64>, eof: bool) -> Self {
        let fraction = expected_frames
            .filter(|expected| *expected > 0)
            .map(|expected| {
                let value = (decoded_frames as f64 / expected as f64).clamp(0.0, 1.0);
                // A ratio of two u64 values is finite, and clamp preserves finiteness.
                FiniteF64(value)
            });
        Self {
            decoded_frames,
            expected_frames,
            fraction,
            eof,
        }
    }

    pub const fn decoded_frames(&self) -> u64 {
        self.decoded_frames
    }

    pub const fn expected_frames(&self) -> Option<u64> {
        self.expected_frames
    }

    pub fn fraction(&self) -> Option<f64> {
        self.fraction.map(FiniteF64::get)
    }

    pub const fn is_eof(&self) -> bool {
        self.eof
    }
}

/// Diagnostic state owned and updated by a decoder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodeDiagnostics {
    pub backend: String,
    pub decoded_frames: u64,
    pub warnings: Vec<String>,
}

/// Diagnostics attached to a completed analysis report.
///
/// This is deliberately distinct from [`DecodeDiagnostics`]: report assembly
/// may add interpretation warnings after the decoder has reached its terminal
/// state, without retyping those warnings as decoder output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportDiagnostics {
    pub backend: String,
    pub decoded_frames: u64,
    pub warnings: Vec<String>,
}

impl ReportDiagnostics {
    fn from_decode(
        diagnostics: DecodeDiagnostics,
        report_warnings: Vec<String>,
    ) -> ReportDiagnostics {
        let DecodeDiagnostics {
            backend,
            decoded_frames,
            mut warnings,
        } = diagnostics;
        warnings.extend(report_warnings);
        Self {
            backend,
            decoded_frames,
            warnings,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlgorithmParameters {
    pub window_duration_coefficient: FiniteF64,
    pub rms_sum_multiplier: FiniteF64,
    pub histogram_bins: usize,
    pub rms_histogram_min_db: FiniteF64,
    pub rms_histogram_max_db: FiniteF64,
    pub histogram_bin_width_db: FiniteF64,
    pub peak_key_bin_width_db: FiniteF64,
    pub loud_fraction: FiniteF64,
    pub minimum_tail_frames: usize,
    pub include_entire_boundary_bin: bool,
    pub exact_window_virtual_zero_peak: bool,
    pub dr_floor_db: FiniteF64,
    pub silent_channel_dr_db: FiniteF64,
    pub includes_lfe_in_track_aggregate: bool,
    pub result_precision_bits: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlgorithmDescriptor {
    pub parameters: AlgorithmParameters,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMeasurement {
    pub dr_db: FiniteF32,
    pub rounded_dr: u32,
    pub loud_window_rms: FiniteF64,
    pub dr_selected_peak: FiniteF64,
    pub dr_primary_peak: FiniteF64,
    pub dr_secondary_peak: Option<FiniteF64>,
    pub valid_windows: u64,
    pub frames: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelReportMetrics {
    pub overall_rms_linear: FiniteF32,
    pub overall_rms_dbfs: Option<FiniteF32>,
    pub primary_peak_linear: FiniteF32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ChannelOutcome {
    Measured { measurement: ChannelMeasurement },
    Silent { frames: u64, valid_windows: u64 },
    InsufficientData { frames: u64 },
}

impl ChannelOutcome {
    pub const fn frames(&self) -> u64 {
        match self {
            Self::Measured { measurement } => measurement.frames,
            Self::Silent { frames, .. } | Self::InsufficientData { frames } => *frames,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelResult {
    pub channel_index: usize,
    pub report: ChannelReportMetrics,
    pub outcome: ChannelOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    InsufficientData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcludedChannel {
    pub channel_index: usize,
    pub reason: ExclusionReason,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackAggregate {
    pub dr_db: Option<FiniteF32>,
    pub rounded_dr: Option<u32>,
    pub contributing_channels: Vec<usize>,
    pub excluded_channels: Vec<ExcludedChannel>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateResults {
    pub track: TrackAggregate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodedDuration {
    pub decoded_frames: u64,
    pub sample_rate: SampleRate,
}

impl DecodedDuration {
    pub const fn new(decoded_frames: u64, sample_rate: SampleRate) -> Self {
        Self {
            decoded_frames,
            sample_rate,
        }
    }

    pub fn seconds(self) -> f64 {
        self.decoded_frames as f64 / f64::from(self.sample_rate.get())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackReportMetrics {
    pub overall_rms_linear: FiniteF64,
    pub overall_rms_dbfs: Option<FiniteF32>,
    pub primary_peak_linear: FiniteF32,
    pub primary_peak_dbfs: Option<FiniteF32>,
    pub duration: DecodedDuration,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResult {
    algorithm: AlgorithmDescriptor,
    stream: StreamSpec,
    frames_seen: u64,
    channels: Vec<ChannelResult>,
    aggregates: AggregateResults,
    report: TrackReportMetrics,
}

/// A read-only, exhaustive view of a valid [`AnalysisResult`].
///
/// The result owns all state privately so its cross-field invariants cannot be
/// invalidated after construction. This view preserves explicit access to the
/// complete result graph without exposing mutation.
#[derive(Debug, Clone, Copy)]
pub struct AnalysisResultView<'a> {
    pub algorithm: &'a AlgorithmDescriptor,
    pub stream: &'a StreamSpec,
    pub frames_seen: u64,
    pub channels: &'a [ChannelResult],
    pub aggregates: &'a AggregateResults,
    pub report: &'a TrackReportMetrics,
}

impl AnalysisResult {
    pub fn try_new(
        algorithm: AlgorithmDescriptor,
        stream: StreamSpec,
        frames_seen: u64,
        channels: Vec<ChannelResult>,
        aggregates: AggregateResults,
        report: TrackReportMetrics,
    ) -> Result<Self, AnalysisError> {
        stream
            .channel_layout
            .validate(stream.channels)
            .map_err(|error| analysis_result_error(error.message))?;
        if channels.len() != stream.channels.as_usize() {
            return Err(analysis_result_error(format!(
                "analysis result contains {} channel results for a {}-channel stream",
                channels.len(),
                stream.channels.get()
            )));
        }
        for (expected_index, channel) in channels.iter().enumerate() {
            if channel.channel_index != expected_index {
                return Err(analysis_result_error(format!(
                    "analysis channel index {} is not the expected contiguous index {expected_index}",
                    channel.channel_index
                )));
            }
            if channel.outcome.frames() != frames_seen {
                return Err(analysis_result_error(format!(
                    "analysis channel {expected_index} records {} frames, expected {frames_seen}",
                    channel.outcome.frames()
                )));
            }
        }
        if report.duration.decoded_frames != frames_seen {
            return Err(analysis_result_error(format!(
                "analysis report duration records {} frames, expected {frames_seen}",
                report.duration.decoded_frames
            )));
        }
        if report.duration.sample_rate != stream.sample_rate {
            return Err(analysis_result_error(format!(
                "analysis report duration uses {} Hz, expected {} Hz",
                report.duration.sample_rate.get(),
                stream.sample_rate.get()
            )));
        }

        Ok(Self {
            algorithm,
            stream,
            frames_seen,
            channels,
            aggregates,
            report,
        })
    }

    pub fn view(&self) -> AnalysisResultView<'_> {
        let Self {
            algorithm,
            stream,
            frames_seen,
            channels,
            aggregates,
            report,
        } = self;
        AnalysisResultView {
            algorithm,
            stream,
            frames_seen: *frames_seen,
            channels,
            aggregates,
            report,
        }
    }

    pub fn algorithm(&self) -> &AlgorithmDescriptor {
        &self.algorithm
    }

    pub fn stream(&self) -> &StreamSpec {
        &self.stream
    }

    pub const fn frames_seen(&self) -> u64 {
        self.frames_seen
    }

    pub fn channels(&self) -> &[ChannelResult] {
        &self.channels
    }

    pub fn aggregates(&self) -> &AggregateResults {
        &self.aggregates
    }

    pub fn report(&self) -> &TrackReportMetrics {
        &self.report
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisReport {
    source: SourceInfo,
    pcm: PcmStreamInfo,
    analysis: AnalysisResult,
    diagnostics: ReportDiagnostics,
}

impl AnalysisReport {
    pub fn try_new(
        source: SourceInfo,
        pcm: PcmStreamInfo,
        analysis: AnalysisResult,
        diagnostics: DecodeDiagnostics,
    ) -> Result<Self, AnalysisError> {
        Self::try_new_with_report_warnings(source, pcm, analysis, diagnostics, Vec::new())
    }

    pub fn try_new_with_report_warnings(
        source: SourceInfo,
        pcm: PcmStreamInfo,
        analysis: AnalysisResult,
        diagnostics: DecodeDiagnostics,
        report_warnings: Vec<String>,
    ) -> Result<Self, AnalysisError> {
        if pcm.spec != *analysis.stream() {
            return Err(analysis_report_error(
                "PCM stream specification does not match the analysis stream",
            ));
        }
        if diagnostics.decoded_frames != analysis.frames_seen() {
            return Err(analysis_report_error(format!(
                "decode diagnostics record {} frames, expected {} analysis frames",
                diagnostics.decoded_frames,
                analysis.frames_seen()
            )));
        }

        Ok(Self {
            source,
            pcm,
            analysis,
            diagnostics: ReportDiagnostics::from_decode(diagnostics, report_warnings),
        })
    }

    pub fn source(&self) -> &SourceInfo {
        &self.source
    }

    pub fn pcm(&self) -> &PcmStreamInfo {
        &self.pcm
    }

    pub fn analysis(&self) -> &AnalysisResult {
        &self.analysis
    }

    pub fn diagnostics(&self) -> &ReportDiagnostics {
        &self.diagnostics
    }
}

fn analysis_result_error(message: impl Into<String>) -> AnalysisError {
    AnalysisError::new(ErrorCode::AnalysisFailed, AnalysisStage::Analysis, message)
}

fn analysis_report_error(message: impl Into<String>) -> AnalysisError {
    AnalysisError::new(ErrorCode::DecodeFailed, AnalysisStage::Decode, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct AnalysisResultParts {
        algorithm: AlgorithmDescriptor,
        stream: StreamSpec,
        frames_seen: u64,
        channels: Vec<ChannelResult>,
        aggregates: AggregateResults,
        report: TrackReportMetrics,
    }

    impl AnalysisResultParts {
        fn try_build(self) -> Result<AnalysisResult, AnalysisError> {
            AnalysisResult::try_new(
                self.algorithm,
                self.stream,
                self.frames_seen,
                self.channels,
                self.aggregates,
                self.report,
            )
        }
    }

    fn finite32(value: f32) -> FiniteF32 {
        FiniteF32::new(value).unwrap()
    }

    fn finite64(value: f64) -> FiniteF64 {
        FiniteF64::new(value).unwrap()
    }

    fn algorithm_descriptor() -> AlgorithmDescriptor {
        AlgorithmDescriptor {
            parameters: AlgorithmParameters {
                window_duration_coefficient: finite64(3.0),
                rms_sum_multiplier: finite64(2.0),
                histogram_bins: 10_001,
                rms_histogram_min_db: finite64(-100.0),
                rms_histogram_max_db: finite64(0.0),
                histogram_bin_width_db: finite64(0.01),
                peak_key_bin_width_db: finite64(0.01),
                loud_fraction: finite64(0.2),
                minimum_tail_frames: 1,
                include_entire_boundary_bin: true,
                exact_window_virtual_zero_peak: false,
                dr_floor_db: finite64(0.0),
                silent_channel_dr_db: finite64(0.0),
                includes_lfe_in_track_aggregate: true,
                result_precision_bits: 32,
            },
        }
    }

    fn channel_report() -> ChannelReportMetrics {
        ChannelReportMetrics {
            overall_rms_linear: finite32(0.0),
            overall_rms_dbfs: None,
            primary_peak_linear: finite32(0.0),
        }
    }

    fn measured(frames: u64) -> ChannelOutcome {
        ChannelOutcome::Measured {
            measurement: ChannelMeasurement {
                dr_db: finite32(1.0),
                rounded_dr: 1,
                loud_window_rms: finite64(0.1),
                dr_selected_peak: finite64(0.2),
                dr_primary_peak: finite64(0.2),
                dr_secondary_peak: None,
                valid_windows: 1,
                frames,
            },
        }
    }

    fn result_parts(
        stream: StreamSpec,
        frames_seen: u64,
        outcomes: Vec<ChannelOutcome>,
    ) -> AnalysisResultParts {
        let mut contributing_channels = Vec::new();
        let mut excluded_channels = Vec::new();
        let channels = outcomes
            .into_iter()
            .enumerate()
            .map(|(channel_index, outcome)| {
                if matches!(outcome, ChannelOutcome::InsufficientData { .. }) {
                    excluded_channels.push(ExcludedChannel {
                        channel_index,
                        reason: ExclusionReason::InsufficientData,
                    });
                } else {
                    contributing_channels.push(channel_index);
                }
                ChannelResult {
                    channel_index,
                    report: channel_report(),
                    outcome,
                }
            })
            .collect();
        let has_contributors = !contributing_channels.is_empty();

        AnalysisResultParts {
            algorithm: algorithm_descriptor(),
            report: TrackReportMetrics {
                overall_rms_linear: finite64(0.0),
                overall_rms_dbfs: None,
                primary_peak_linear: finite32(0.0),
                primary_peak_dbfs: None,
                duration: DecodedDuration::new(frames_seen, stream.sample_rate),
            },
            stream,
            frames_seen,
            channels,
            aggregates: AggregateResults {
                track: TrackAggregate {
                    dr_db: has_contributors.then(|| finite32(0.0)),
                    rounded_dr: has_contributors.then_some(0),
                    contributing_channels,
                    excluded_channels,
                },
            },
        }
    }

    fn assert_result_error(error: AnalysisError, message_fragment: &str) {
        assert_eq!(error.code, ErrorCode::AnalysisFailed);
        assert_eq!(error.stage, AnalysisStage::Analysis);
        assert!(error.message.contains(message_fragment), "{error}");
    }

    #[test]
    fn rejects_invalid_stream_spec() {
        assert!(StreamSpec::new(0, 2, ChannelLayout::Unknown).is_err());
        assert!(StreamSpec::new(44_100, 0, ChannelLayout::Unknown).is_err());
        assert!(
            StreamSpec::new(
                44_100,
                2,
                ChannelLayout::Known {
                    positions: vec![ChannelRole::FrontLeft]
                }
            )
            .is_err()
        );
    }

    #[test]
    fn analysis_limit_does_not_narrow_source_channel_geometry() {
        assert_eq!(MAX_ANALYSIS_CHANNELS, 64);
        assert_eq!(
            ChannelCount::new(MAX_ANALYSIS_CHANNELS + 1).unwrap().get(),
            65
        );
        assert_eq!(ChannelCount::new(u16::MAX).unwrap().get(), u16::MAX);
        assert_eq!(
            StreamSpec::new(48_000, MAX_ANALYSIS_CHANNELS + 1, ChannelLayout::Unknown)
                .unwrap()
                .channels
                .get(),
            65
        );
    }

    #[test]
    fn pcm_block_requires_finite_complete_frames() {
        let channels = ChannelCount::new(2).unwrap();
        for error in [
            PcmBlock::new(Vec::new(), channels).unwrap_err(),
            PcmBlock::new(vec![0.0], channels).unwrap_err(),
            PcmBlock::new(vec![0.0, f64::NAN], channels).unwrap_err(),
            PcmBlock::new(vec![f64::INFINITY, 0.0], channels).unwrap_err(),
            PcmBlock::new(vec![0.0, f64::NEG_INFINITY], channels).unwrap_err(),
        ] {
            assert_eq!(error.code, ErrorCode::DecodeFailed);
            assert_eq!(error.stage, AnalysisStage::Decode);
        }

        let samples = vec![0.0, 0.0, 0.5, -0.5];
        let block = PcmBlock::new(samples.clone(), channels).unwrap();
        assert_eq!(block.samples(), samples);
        assert_eq!(block.frames(), 2);
        assert_eq!(block.channels(), channels);
    }

    #[test]
    fn pcm_block_preserves_the_geometry_used_for_construction() {
        let samples = vec![0.0; 6];
        let stereo = PcmBlock::new(samples.clone(), ChannelCount::new(2).unwrap()).unwrap();
        let three_channel = PcmBlock::new(samples, ChannelCount::new(3).unwrap()).unwrap();

        assert_eq!(stereo.channels().get(), 2);
        assert_eq!(stereo.frames(), 3);
        assert_eq!(three_channel.channels().get(), 3);
        assert_eq!(three_channel.frames(), 2);
    }

    #[test]
    fn finite_floats_validate_construction_and_deserialization() {
        assert_eq!(FiniteF32::new(-1.25).unwrap().get(), -1.25);
        assert_eq!(FiniteF64::new(2.5).unwrap().get(), 2.5);
        assert!(FiniteF32::new(f32::NAN).is_err());
        assert!(FiniteF32::new(f32::INFINITY).is_err());
        assert!(FiniteF64::new(f64::NEG_INFINITY).is_err());

        let finite_f32: FiniteF32 = serde_json::from_str("0.5").unwrap();
        let finite_f64: FiniteF64 = serde_json::from_str("-12.75").unwrap();
        assert_eq!(finite_f32.get(), 0.5);
        assert_eq!(finite_f64.get(), -12.75);
        assert!(serde_json::from_str::<FiniteF32>("1e100").is_err());
        assert!(serde_json::from_str::<FiniteF64>("1e999").is_err());

        let nan = serde::de::value::F32Deserializer::<serde::de::value::Error>::new(f32::NAN);
        let infinity =
            serde::de::value::F64Deserializer::<serde::de::value::Error>::new(f64::INFINITY);
        assert!(FiniteF32::deserialize(nan).is_err());
        assert!(FiniteF64::deserialize(infinity).is_err());
    }

    #[test]
    fn decoded_duration_uses_the_actual_pcm_rate() {
        let duration = DecodedDuration::new(96_000, SampleRate::new(48_000).unwrap());
        assert_eq!(duration.seconds(), 2.0);
    }

    #[test]
    fn decode_progress_is_derived_finite_and_bounded() {
        for (decoded, expected, eof, expected_fraction) in [
            (0, None, false, None),
            (0, Some(0), false, None),
            (0, Some(10), false, Some(0.0)),
            (5, Some(10), false, Some(0.5)),
            (10, Some(10), true, Some(1.0)),
            (11, Some(10), false, Some(1.0)),
            (u64::MAX, Some(1), false, Some(1.0)),
        ] {
            let progress = DecodeProgress::new(decoded, expected, eof);
            assert_eq!(progress.decoded_frames(), decoded);
            assert_eq!(progress.expected_frames(), expected);
            assert_eq!(progress.fraction(), expected_fraction);
            assert_eq!(progress.is_eof(), eof);
            assert!(
                progress
                    .fraction()
                    .is_none_or(|fraction| fraction.is_finite() && (0.0..=1.0).contains(&fraction))
            );
        }
    }

    #[test]
    fn analysis_result_accepts_all_outcomes_when_cross_field_relations_match() {
        let parts = result_parts(
            StreamSpec::new(48_000, 3, ChannelLayout::KnownNoLfe).unwrap(),
            9,
            vec![
                measured(9),
                ChannelOutcome::Silent {
                    frames: 9,
                    valid_windows: 1,
                },
                ChannelOutcome::InsufficientData { frames: 9 },
            ],
        );

        let result = parts.try_build().unwrap();
        assert_eq!(result.frames_seen(), 9);
        assert_eq!(result.channels().len(), 3);
        assert_eq!(result.report().duration.decoded_frames, 9);
    }

    #[test]
    fn analysis_result_rejects_channel_count_and_index_mismatches() {
        let valid = result_parts(
            StreamSpec::new(48_000, 3, ChannelLayout::KnownNoLfe).unwrap(),
            9,
            vec![measured(9), measured(9), measured(9)],
        );

        let mut too_few = valid.clone();
        too_few.channels.pop();
        assert_result_error(too_few.try_build().unwrap_err(), "channel results");

        let mut too_many = valid.clone();
        too_many.channels.push(ChannelResult {
            channel_index: 3,
            report: channel_report(),
            outcome: measured(9),
        });
        assert_result_error(too_many.try_build().unwrap_err(), "channel results");

        let mut duplicate = valid.clone();
        duplicate.channels[2].channel_index = 1;
        assert_result_error(duplicate.try_build().unwrap_err(), "contiguous index 2");

        let mut out_of_order = valid;
        out_of_order.channels[1].channel_index = 2;
        out_of_order.channels[2].channel_index = 1;
        assert_result_error(out_of_order.try_build().unwrap_err(), "contiguous index 1");
    }

    #[test]
    fn analysis_result_rejects_each_outcome_frame_mismatch() {
        let valid = result_parts(
            StreamSpec::new(48_000, 3, ChannelLayout::KnownNoLfe).unwrap(),
            9,
            vec![
                measured(9),
                ChannelOutcome::Silent {
                    frames: 9,
                    valid_windows: 1,
                },
                ChannelOutcome::InsufficientData { frames: 9 },
            ],
        );

        for (channel_index, replacement) in [
            (0, measured(8)),
            (
                1,
                ChannelOutcome::Silent {
                    frames: 10,
                    valid_windows: 1,
                },
            ),
            (2, ChannelOutcome::InsufficientData { frames: 8 }),
        ] {
            let mut mismatched = valid.clone();
            mismatched.channels[channel_index].outcome = replacement;
            assert_result_error(
                mismatched.try_build().unwrap_err(),
                &format!("channel {channel_index}"),
            );
        }
    }

    #[test]
    fn analysis_result_rejects_duration_frame_and_rate_mismatches() {
        let valid = result_parts(
            StreamSpec::new(48_000, 1, ChannelLayout::KnownNoLfe).unwrap(),
            9,
            vec![measured(9)],
        );

        let mut wrong_frames = valid.clone();
        wrong_frames.report.duration.decoded_frames = 8;
        assert_result_error(wrong_frames.try_build().unwrap_err(), "duration records");

        let mut wrong_rate = valid;
        wrong_rate.report.duration.sample_rate = SampleRate::new(44_100).unwrap();
        assert_result_error(wrong_rate.try_build().unwrap_err(), "duration uses");
    }

    #[test]
    fn analysis_report_rejects_pcm_and_diagnostic_mismatches_only() {
        let pcm_spec = StreamSpec::new(48_000, 1, ChannelLayout::Unknown).unwrap();
        let analysis = result_parts(pcm_spec.clone(), 9, vec![measured(9)])
            .try_build()
            .unwrap();
        let source = SourceInfo {
            display_path: "metadata-may-differ.wav".to_owned(),
            container: ContainerFormat::Wave,
            codec: SourceCodec::PcmFloat,
            sample_rate: SampleRate::new(96_000).unwrap(),
            channels: ChannelCount::new(2).unwrap(),
            bits_per_sample: Some(64),
            expected_frames: Some(999),
        };
        let pcm = PcmStreamInfo {
            spec: pcm_spec,
            expected_frames: None,
        };
        let diagnostics = DecodeDiagnostics {
            backend: "domain-test".to_owned(),
            decoded_frames: 9,
            warnings: Vec::new(),
        };

        let valid =
            AnalysisReport::try_new(source.clone(), pcm.clone(), analysis.clone(), diagnostics)
                .unwrap();
        assert_eq!(valid.source().sample_rate.get(), 96_000);
        assert_eq!(valid.pcm().spec.sample_rate.get(), 48_000);
        assert_eq!(valid.source().expected_frames, Some(999));
        assert_eq!(valid.pcm().expected_frames, None);

        let decode_diagnostics = DecodeDiagnostics {
            backend: "domain-test".to_owned(),
            decoded_frames: 9,
            warnings: vec!["decoder warning".to_owned()],
        };
        let with_report_warning = AnalysisReport::try_new_with_report_warnings(
            source.clone(),
            pcm.clone(),
            analysis.clone(),
            decode_diagnostics.clone(),
            vec!["analysis interpretation warning".to_owned()],
        )
        .unwrap();
        assert_eq!(decode_diagnostics.warnings, ["decoder warning"]);
        assert_eq!(
            with_report_warning.diagnostics().warnings,
            ["decoder warning", "analysis interpretation warning"]
        );
        let serialized = serde_json::to_value(&with_report_warning).unwrap();
        assert_eq!(serialized["diagnostics"]["backend"], "domain-test");
        assert_eq!(serialized["diagnostics"]["decodedFrames"], 9);
        assert_eq!(
            serialized["diagnostics"]["warnings"],
            serde_json::json!(["decoder warning", "analysis interpretation warning"])
        );

        for mismatched_spec in [
            StreamSpec::new(44_100, 1, ChannelLayout::Unknown).unwrap(),
            StreamSpec::new(48_000, 2, ChannelLayout::Unknown).unwrap(),
            StreamSpec::new(48_000, 1, ChannelLayout::KnownNoLfe).unwrap(),
        ] {
            let error = AnalysisReport::try_new(
                source.clone(),
                PcmStreamInfo {
                    spec: mismatched_spec,
                    expected_frames: None,
                },
                analysis.clone(),
                DecodeDiagnostics {
                    backend: "domain-test".to_owned(),
                    decoded_frames: 9,
                    warnings: Vec::new(),
                },
            )
            .unwrap_err();
            assert_eq!(error.code, ErrorCode::DecodeFailed);
            assert_eq!(error.stage, AnalysisStage::Decode);
            assert!(error.message.contains("specification"));
        }

        for decoded_frames in [8, 10] {
            let error = AnalysisReport::try_new(
                source.clone(),
                pcm.clone(),
                analysis.clone(),
                DecodeDiagnostics {
                    backend: "domain-test".to_owned(),
                    decoded_frames,
                    warnings: Vec::new(),
                },
            )
            .unwrap_err();
            assert_eq!(error.code, ErrorCode::DecodeFailed);
            assert_eq!(error.stage, AnalysisStage::Decode);
            assert!(error.message.contains("diagnostics"));
        }
    }
}
