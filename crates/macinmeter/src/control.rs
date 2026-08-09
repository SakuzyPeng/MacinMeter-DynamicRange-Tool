use crate::batch::BatchItem;
use macinmeter_domain::DecodeProgress;
use serde::Serialize;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AnalysisEvent {
    DiscoveryStarted,
    DiscoveryFinished {
        files: usize,
    },
    FileStarted {
        index: usize,
        display_path: String,
    },
    DecodeProgress {
        index: usize,
        display_path: String,
        progress: DecodeProgress,
    },
    FileFinished {
        index: usize,
        display_path: String,
        success: bool,
    },
    /// One completed batch item, published as soon as its lane has produced
    /// the same value that will later appear in the ordered batch report.
    ///
    /// This is a runtime event rather than a wire-envelope field: adapters can
    /// render a large batch incrementally without changing the reproducible
    /// final document.
    BatchItemFinished {
        index: usize,
        item: BatchItem,
    },
    BatchFinished {
        succeeded: usize,
        failed: usize,
    },
}

pub trait ProgressSink: Send + Sync {
    fn emit(&self, event: AnalysisEvent);
}

impl<F> ProgressSink for F
where
    F: Fn(AnalysisEvent) + Send + Sync,
{
    fn emit(&self, event: AnalysisEvent) {
        self(event);
    }
}

#[derive(Debug, Default)]
pub struct NoopProgressSink;

impl ProgressSink for NoopProgressSink {
    fn emit(&self, _event: AnalysisEvent) {}
}

pub struct ExecutionControl<'a> {
    pub cancellation: &'a CancellationToken,
    pub progress: &'a dyn ProgressSink,
}

impl<'a> ExecutionControl<'a> {
    pub fn new(cancellation: &'a CancellationToken, progress: &'a dyn ProgressSink) -> Self {
        Self {
            cancellation,
            progress,
        }
    }
}
