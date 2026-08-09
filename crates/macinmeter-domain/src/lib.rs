#![forbid(unsafe_code)]

//! Valid stream, source, report and error types for the MacinMeter analyzer.
//!
//! The types here are constructors that refuse invalid state rather than
//! containers that carry it: a [`StreamSpec`], [`PcmBlock`] or [`FiniteF64`]
//! exists only if it was already checked. Layers above therefore do not
//! re-validate, and an invalid combination has no representation to travel in.
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
