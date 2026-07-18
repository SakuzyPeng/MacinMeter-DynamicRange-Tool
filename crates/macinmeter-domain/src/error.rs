use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    InputNotFound,
    PermissionDenied,
    NoInputs,
    UnsupportedFormat,
    MalformedMedia,
    DecodeFailed,
    AnalysisFailed,
    ResourceExhausted,
    OutputFailed,
    Cancelled,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisStage {
    Validation,
    Discovery,
    Probe,
    Decode,
    Analysis,
    Output,
    Cancellation,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[error("{message}")]
#[serde(rename_all = "camelCase")]
pub struct AnalysisError {
    pub code: ErrorCode,
    pub stage: AnalysisStage,
    pub message: String,
    pub display_path: Option<String>,
    pub backend: Option<String>,
    pub recoverable: bool,
    pub details: Option<String>,
}

impl AnalysisError {
    pub fn new(code: ErrorCode, stage: AnalysisStage, message: impl Into<String>) -> Self {
        Self {
            code,
            stage,
            message: message.into(),
            display_path: None,
            backend: None,
            recoverable: false,
            details: None,
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::InvalidRequest,
            AnalysisStage::Validation,
            message,
        )
    }

    pub fn cancelled() -> Self {
        Self::new(
            ErrorCode::Cancelled,
            AnalysisStage::Cancellation,
            "analysis was cancelled",
        )
    }

    pub fn with_display_path(mut self, display_path: impl Into<String>) -> Self {
        self.display_path = Some(display_path.into());
        self
    }

    pub fn with_backend(mut self, backend: impl Into<String>) -> Self {
        self.backend = Some(backend.into());
        self
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    pub fn recoverable(mut self, recoverable: bool) -> Self {
        self.recoverable = recoverable;
        self
    }
}
