#![forbid(unsafe_code)]

mod error;
mod model;

pub use error::{AnalysisError, AnalysisStage, ErrorCode};
pub use model::{
    AggregateResults, AlgorithmDescriptor, AlgorithmParameters, AnalysisProfile, AnalysisReport,
    AnalysisResult, AnalysisResultView, ChannelCount, ChannelLayout, ChannelMeasurement,
    ChannelOutcome, ChannelReportMetrics, ChannelResult, ChannelRole, CompatibilityStatus,
    ContainerFormat, DecodeDiagnostics, DecodeProgress, DecodedDuration, ExcludedChannel,
    ExclusionReason, FiniteF32, FiniteF64, MAX_ANALYSIS_CHANNELS, PcmBlock, PcmStreamInfo,
    SampleRate, SourceCodec, SourceInfo, StreamSpec, TrackAggregate, TrackReportMetrics,
};
