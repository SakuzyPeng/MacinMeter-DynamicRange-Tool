#![forbid(unsafe_code)]

mod error;
mod model;

pub use error::{AnalysisError, AnalysisStage, ErrorCode};
pub use model::{
    AggregateResults, AlgorithmDescriptor, AlgorithmParameters, AnalysisProfile, AnalysisReport,
    AnalysisResult, ChannelCount, ChannelLayout, ChannelMeasurement, ChannelOutcome, ChannelResult,
    ChannelRole, CompatibilityStatus, ContainerFormat, DecodeDiagnostics, DecodeProgress,
    ExcludedChannel, ExclusionReason, PcmBlock, PcmStreamInfo, SampleRate, SourceCodec, SourceInfo,
    StreamSpec, TrackAggregate,
};
