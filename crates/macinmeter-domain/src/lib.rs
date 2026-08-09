#![forbid(unsafe_code)]

mod concurrency;
mod error;
mod model;

#[doc(hidden)]
pub use concurrency::{
    DecodeReservation, MAX_DECODE_QUEUE_CAPACITY, MAX_DECODE_WORKERS, MAX_IN_FLIGHT_PCM_BYTES,
};
pub use error::{AnalysisError, AnalysisStage, ErrorCode};
pub use model::{
    AggregateResults, AlgorithmDescriptor, AlgorithmParameters, AnalysisReport, AnalysisResult,
    AnalysisResultView, ChannelCount, ChannelLayout, ChannelMeasurement, ChannelOutcome,
    ChannelReportMetrics, ChannelResult, ChannelRole, ContainerFormat, DecodeDiagnostics,
    DecodeProgress, DecodedDuration, ExcludedChannel, ExclusionReason, FiniteF32, FiniteF64,
    MAX_ANALYSIS_CHANNELS, PcmBlock, PcmStreamInfo, ReportDiagnostics, SampleRate, SourceCodec,
    SourceInfo, StreamSpec, TrackAggregate, TrackReportMetrics,
};
