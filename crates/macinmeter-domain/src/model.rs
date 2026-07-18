use crate::{AnalysisError, AnalysisStage, ErrorCode};
use serde::{Deserialize, Serialize};
use std::num::{NonZeroU16, NonZeroU32};

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCodec {
    PcmInteger,
    PcmFloat,
    Flac,
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

#[derive(Debug, Clone, PartialEq)]
pub struct PcmBlock {
    samples: Vec<f32>,
    frames: usize,
}

impl PcmBlock {
    pub fn new(samples: Vec<f32>, channels: ChannelCount) -> Result<Self, AnalysisError> {
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
        Ok(Self { samples, frames })
    }

    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    pub const fn frames(&self) -> usize {
        self.frames
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodeProgress {
    pub decoded_frames: u64,
    pub expected_frames: Option<u64>,
    pub fraction: Option<f64>,
    pub eof: bool,
}

impl DecodeProgress {
    pub fn new(decoded_frames: u64, expected_frames: Option<u64>, eof: bool) -> Self {
        let fraction = expected_frames
            .filter(|expected| *expected > 0)
            .map(|expected| (decoded_frames as f64 / expected as f64).clamp(0.0, 1.0));
        Self {
            decoded_frames,
            expected_frames,
            fraction,
            eof,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodeDiagnostics {
    pub backend: String,
    pub decoded_frames: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisProfile {
    ProvisionalV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityStatus {
    Unverified,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlgorithmParameters {
    pub window_duration_coefficient: f64,
    pub rms_sum_multiplier: f64,
    pub histogram_bins: usize,
    pub minimum_nonzero_rms_bin: usize,
    pub loud_fraction: f64,
    pub minimum_tail_frames: usize,
    pub exact_window_virtual_zero_peak: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlgorithmDescriptor {
    pub profile: AnalysisProfile,
    pub profile_version: u32,
    pub compatibility: CompatibilityStatus,
    pub parameters: AlgorithmParameters,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMeasurement {
    pub dr_db: f64,
    pub rounded_dr: i32,
    pub loud_rms: f64,
    pub selected_peak: f64,
    pub primary_peak: f64,
    pub secondary_peak: Option<f64>,
    pub valid_windows: u64,
    pub frames: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelResult {
    pub channel_index: usize,
    pub outcome: ChannelOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    Silent,
    InsufficientData,
    Lfe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcludedChannel {
    pub channel_index: usize,
    pub reason: ExclusionReason,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackAggregate {
    pub precise_dr_db: Option<f64>,
    pub rounded_dr: Option<i32>,
    pub included_channels: Vec<usize>,
    pub excluded_channels: Vec<ExcludedChannel>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateResults {
    pub all_channels: TrackAggregate,
    pub without_lfe: Option<TrackAggregate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResult {
    pub algorithm: AlgorithmDescriptor,
    pub stream: StreamSpec,
    pub frames_seen: u64,
    pub channels: Vec<ChannelResult>,
    pub aggregates: AggregateResults,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisReport {
    pub source: SourceInfo,
    pub pcm: PcmStreamInfo,
    pub analysis: AnalysisResult,
    pub diagnostics: DecodeDiagnostics,
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn pcm_block_requires_finite_complete_frames() {
        let channels = ChannelCount::new(2).unwrap();
        assert!(PcmBlock::new(Vec::new(), channels).is_err());
        assert!(PcmBlock::new(vec![0.0], channels).is_err());
        assert!(PcmBlock::new(vec![0.0, f32::NAN], channels).is_err());
        assert!(PcmBlock::new(vec![f32::INFINITY, 0.0], channels).is_err());
        assert!(PcmBlock::new(vec![0.0, f32::NEG_INFINITY], channels).is_err());
        assert_eq!(
            PcmBlock::new(vec![0.0, 0.0, 0.5, -0.5], channels)
                .unwrap()
                .frames(),
            2
        );
    }
}
