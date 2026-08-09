#![forbid(unsafe_code)]

//! Offline audio dynamic-range (DR) analysis.
//!
//! [`Application`] is the only public entry point for analyzing a file, running
//! a batch, and discovering inputs. Clones share one execution domain, so a
//! process runs one job at a time with a bounded queue behind it rather than
//! spawning work per call.
//!
//! ```no_run
//! use macinmeter::{AnalyzeRequest, Application};
//!
//! # fn main() -> Result<(), macinmeter::AnalysisError> {
//! let report = Application::new().analyze_file(AnalyzeRequest::new("track.flac"))?;
//! let track = &report.analysis().aggregates().track;
//! println!("{:?}", track.rounded_dr);
//! # Ok(())
//! # }
//! ```
//!
//! The analysis algorithm is fixed and reconstructed from one recorded target;
//! there is no profile to select and no tolerance to configure. A report states
//! its own numeric parameters. Results do not depend on how many workers or
//! lanes the host granted, so a report is not evidence of which internal engine
//! produced it.
//!
//! Unsupported input is reported as such rather than guessed at: no external
//! decoder is invoked, and nothing is resampled or preprocessed. See
//! [`capabilities`] for the routes this build actually accepts.

mod album;
mod application;
mod batch;
mod capability;
mod concurrency;
mod control;
mod execution;
mod wire;

pub use album::{AlbumAggregate, AlbumAggregator, AlbumTrackMetrics, AlbumWeighting};
#[cfg(feature = "performance-probes")]
#[doc(hidden)]
pub use application::ApplicationPerformanceProbe;
pub use application::{AnalyzeRequest, PhaseTimings};
#[cfg(feature = "performance-probes")]
#[doc(hidden)]
pub use batch::BatchPerformanceProbe;
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
    PcmStreamInfo, ReportDiagnostics, SampleRate, SourceCodec, SourceInfo, StreamSpec,
    TrackAggregate, TrackReportMetrics,
};
pub use wire::{WIRE_SCHEMA_VERSION, WireEnvelope, WirePayload};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
