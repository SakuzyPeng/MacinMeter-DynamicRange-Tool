#![forbid(unsafe_code)]

//! Valid stream, source, report and error types for the MacinMeter analyzer.
//!
//! The types here use constructors that refuse invalid state rather than
//! carrying it: a [`StreamSpec`], [`PcmBlock`] or [`FiniteF64`] exists only if
//! its own invariants were already checked. Cross-object contracts remain the
//! caller's responsibility; for example, a block's channel geometry must still
//! match the stream it belongs to.
//!
//! This crate is a dependency of the [`macinmeter`](https://docs.rs/macinmeter)
//! facade and is published so those public types resolve; most callers want the
//! facade instead.

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
