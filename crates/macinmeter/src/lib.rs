#![forbid(unsafe_code)]

mod album;
mod application;
mod batch;
mod control;
mod wire;

pub use album::{AlbumAggregate, AlbumAggregator, AlbumTrackMetrics, AlbumWeighting};
pub use application::{AnalyzeRequest, Analyzer};
pub use batch::{
    BatchItem, BatchItemOutcome, BatchReport, BatchRequest, BatchRunner, BatchStatus, BatchSummary,
    discover_inputs, discover_inputs_with_control,
};
pub use control::{
    AnalysisEvent, CancellationToken, ExecutionControl, NoopProgressSink, ProgressSink,
};
pub use macinmeter_analysis::AnalyzerSession;
pub use macinmeter_codecs::SUPPORTED_EXTENSIONS;
pub use macinmeter_domain::*;
pub use wire::{WIRE_SCHEMA_VERSION, WireEnvelope, WirePayload};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
