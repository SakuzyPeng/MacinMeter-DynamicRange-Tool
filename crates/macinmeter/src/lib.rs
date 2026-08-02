#![forbid(unsafe_code)]

mod album;
mod application;
mod batch;
mod capability;
mod concurrency;
mod control;
mod execution;
mod wire;

pub use album::{AlbumAggregate, AlbumAggregator, AlbumTrackMetrics, AlbumWeighting};
pub use application::AnalyzeRequest;
pub use batch::{
    BatchItem, BatchItemOutcome, BatchReport, BatchRequest, BatchStatus, BatchSummary,
};
pub use capability::{
    CapabilityRoute, CapabilitySnapshot, CapabilityStatus, NativeRouteCapability, capabilities,
};
pub use control::{
    AnalysisEvent, CancellationToken, ExecutionControl, NoopProgressSink, ProgressSink,
};
pub use execution::{Application, ApplicationJob, ExecutionBudget};
pub use macinmeter_analysis::AnalyzerSession;
pub use macinmeter_domain::{
    AggregateResults, AlgorithmDescriptor, AlgorithmParameters, AnalysisError, AnalysisReport,
    AnalysisResult, AnalysisResultView, AnalysisStage, ChannelCount, ChannelLayout,
    ChannelMeasurement, ChannelOutcome, ChannelReportMetrics, ChannelResult, ChannelRole,
    ContainerFormat, DecodeDiagnostics, DecodeProgress, DecodedDuration, ErrorCode,
    ExcludedChannel, ExclusionReason, FiniteF32, FiniteF64, MAX_ANALYSIS_CHANNELS, PcmBlock,
    PcmStreamInfo, SampleRate, SourceCodec, SourceInfo, StreamSpec, TrackAggregate,
    TrackReportMetrics,
};
pub use wire::{WIRE_SCHEMA_VERSION, WireEnvelope, WirePayload};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
