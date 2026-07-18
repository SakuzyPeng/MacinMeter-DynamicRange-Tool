use crate::{AnalysisError, AnalysisReport, BatchReport};
use serde::{Deserialize, Serialize};

pub const WIRE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum WirePayload {
    Analysis(Box<AnalysisReport>),
    Batch(Box<BatchReport>),
    Error(AnalysisError),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireEnvelope {
    pub schema_version: u32,
    pub tool_version: String,
    #[serde(flatten)]
    pub payload: WirePayload,
}

impl WireEnvelope {
    pub fn analysis(report: AnalysisReport) -> Self {
        Self::new(WirePayload::Analysis(Box::new(report)))
    }

    pub fn batch(report: BatchReport) -> Self {
        Self::new(WirePayload::Batch(Box::new(report)))
    }

    pub fn error(error: AnalysisError) -> Self {
        Self::new(WirePayload::Error(error))
    }

    fn new(payload: WirePayload) -> Self {
        Self {
            schema_version: WIRE_SCHEMA_VERSION,
            tool_version: crate::VERSION.to_string(),
            payload,
        }
    }
}
