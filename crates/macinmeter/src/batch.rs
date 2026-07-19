use crate::{
    AnalysisError, AnalysisEvent, AnalysisProfile, AnalysisReport, AnalysisStage, AnalyzeRequest,
    CancellationToken, ErrorCode, ExecutionControl, application::Analyzer,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchRequest {
    pub inputs: Vec<PathBuf>,
    pub recursive: bool,
    pub profile: AnalysisProfile,
}

impl BatchRequest {
    pub fn new(inputs: Vec<PathBuf>, recursive: bool) -> Self {
        Self {
            inputs,
            recursive,
            profile: AnalysisProfile::FooDrMeter108CandidateV1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    Succeeded,
    PartiallySucceeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BatchItemOutcome {
    Success { report: Box<AnalysisReport> },
    Failure { error: AnalysisError },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchItem {
    pub display_path: String,
    pub outcome: BatchItemOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSummary {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchReport {
    pub status: BatchStatus,
    pub items: Vec<BatchItem>,
    pub summary: BatchSummary,
}

#[derive(Debug, Default)]
pub(crate) struct BatchRunner {
    analyzer: Analyzer,
}

impl BatchRunner {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn run(
        &self,
        request: BatchRequest,
        control: &ExecutionControl<'_>,
    ) -> Result<BatchReport, AnalysisError> {
        if control.cancellation.is_cancelled() {
            return Err(AnalysisError::cancelled());
        }
        let files = discover_inputs_with_control(&request.inputs, request.recursive, control)?;

        let mut items = Vec::with_capacity(files.len());
        let mut succeeded = 0;
        let mut failed = 0;

        for (index, path) in files.into_iter().enumerate() {
            if control.cancellation.is_cancelled() {
                return Err(AnalysisError::cancelled());
            }
            let analyze_request = AnalyzeRequest {
                path: path.clone(),
                profile: request.profile,
            };
            let outcome = match self
                .analyzer
                .analyze_file_at(analyze_request, index, control)
            {
                Ok(report) => {
                    succeeded += 1;
                    BatchItemOutcome::Success {
                        report: Box::new(report),
                    }
                }
                Err(error) if error.code == ErrorCode::Cancelled => return Err(error),
                Err(error) => {
                    failed += 1;
                    BatchItemOutcome::Failure { error }
                }
            };
            items.push(BatchItem {
                display_path: path.display().to_string(),
                outcome,
            });
        }

        let status = match (succeeded, failed) {
            (_, 0) => BatchStatus::Succeeded,
            (0, _) => BatchStatus::Failed,
            _ => BatchStatus::PartiallySucceeded,
        };
        let summary = BatchSummary {
            total: items.len(),
            succeeded,
            failed,
        };
        control
            .progress
            .emit(AnalysisEvent::BatchFinished { succeeded, failed });
        Ok(BatchReport {
            status,
            items,
            summary,
        })
    }
}

pub(crate) fn discover_inputs_with_control(
    inputs: &[PathBuf],
    recursive: bool,
    control: &ExecutionControl<'_>,
) -> Result<Vec<PathBuf>, AnalysisError> {
    if control.cancellation.is_cancelled() {
        return Err(AnalysisError::cancelled());
    }
    control.progress.emit(AnalysisEvent::DiscoveryStarted);
    let files = discover_inputs_with_cancellation(inputs, recursive, Some(control.cancellation))?;
    control
        .progress
        .emit(AnalysisEvent::DiscoveryFinished { files: files.len() });
    Ok(files)
}

fn discover_inputs_with_cancellation(
    inputs: &[PathBuf],
    recursive: bool,
    cancellation: Option<&CancellationToken>,
) -> Result<Vec<PathBuf>, AnalysisError> {
    if inputs.is_empty() {
        return Err(AnalysisError::new(
            ErrorCode::NoInputs,
            AnalysisStage::Discovery,
            "no input paths were provided",
        ));
    }

    let mut discovered = Vec::new();
    let mut seen = HashSet::new();

    for input in inputs {
        ensure_discovery_not_cancelled(cancellation)?;
        if input.is_dir() {
            let max_depth = if recursive { usize::MAX } else { 1 };
            let mut directory_files = Vec::new();
            for entry in WalkDir::new(input)
                .follow_links(false)
                .min_depth(1)
                .max_depth(max_depth)
            {
                ensure_discovery_not_cancelled(cancellation)?;
                let entry = entry.map_err(|error| {
                    AnalysisError::new(
                        ErrorCode::PermissionDenied,
                        AnalysisStage::Discovery,
                        "failed to scan an input directory",
                    )
                    .with_display_path(input.display().to_string())
                    .with_details(error.to_string())
                })?;
                if entry.file_type().is_file() && is_discoverable(entry.path()) {
                    directory_files.push(entry.into_path());
                }
            }
            directory_files.sort();
            for file in directory_files {
                if seen.insert(file.clone()) {
                    discovered.push(file);
                }
            }
        } else if seen.insert(input.clone()) {
            discovered.push(input.clone());
        }
    }

    if discovered.is_empty() {
        return Err(AnalysisError::new(
            ErrorCode::NoInputs,
            AnalysisStage::Discovery,
            "no supported audio inputs were found",
        ));
    }
    Ok(discovered)
}

fn ensure_discovery_not_cancelled(
    cancellation: Option<&CancellationToken>,
) -> Result<(), AnalysisError> {
    if cancellation.is_some_and(CancellationToken::is_cancelled) {
        Err(AnalysisError::cancelled())
    } else {
        Ok(())
    }
}

fn is_discoverable(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(crate::capability::is_stable_discovery_extension)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_is_sorted_deduplicated_and_non_recursive_by_default() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("b.wav"), b"x").unwrap();
        std::fs::write(root.path().join("a.flac"), b"x").unwrap();
        std::fs::write(root.path().join("ignored.mp3"), b"x").unwrap();
        std::fs::create_dir(root.path().join("nested")).unwrap();
        std::fs::write(root.path().join("nested/c.aiff"), b"x").unwrap();

        let direct = root.path().join("b.wav");
        let files =
            discover_inputs_with_cancellation(&[root.path().to_path_buf(), direct], false, None)
                .unwrap();
        assert_eq!(files.len(), 2);
        assert!(files[0].ends_with("a.flac"));
        assert!(files[1].ends_with("b.wav"));

        let recursive =
            discover_inputs_with_cancellation(&[root.path().to_path_buf()], true, None).unwrap();
        assert_eq!(recursive.len(), 3);
    }
}
