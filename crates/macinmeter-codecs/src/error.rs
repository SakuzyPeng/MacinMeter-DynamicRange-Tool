use macinmeter_domain::{AnalysisError, AnalysisStage, ErrorCode};
use std::{io, path::Path};
use symphonia::core::errors::Error as SymphoniaError;

pub(crate) const BACKEND: &str = "symphonia";

pub(crate) fn file_open_error(path: &Path, error: io::Error) -> AnalysisError {
    let (code, stage, message) = match error.kind() {
        io::ErrorKind::NotFound => (
            ErrorCode::InputNotFound,
            AnalysisStage::Discovery,
            "input file does not exist",
        ),
        io::ErrorKind::PermissionDenied => (
            ErrorCode::PermissionDenied,
            AnalysisStage::Discovery,
            "permission denied while opening input file",
        ),
        _ => (
            ErrorCode::DecodeFailed,
            AnalysisStage::Probe,
            "failed to open input file",
        ),
    };
    analysis_error(path, code, stage, message, Some(error.to_string()))
}

pub(crate) fn io_analysis_error(
    path: &Path,
    stage: AnalysisStage,
    error: io::Error,
) -> AnalysisError {
    let code = if error.kind() == io::ErrorKind::PermissionDenied {
        ErrorCode::PermissionDenied
    } else {
        ErrorCode::DecodeFailed
    };
    analysis_error(
        path,
        code,
        stage,
        "input file I/O failed",
        Some(error.to_string()),
    )
}

pub(crate) fn probe_error(path: &Path, error: SymphoniaError) -> AnalysisError {
    let (code, message) = match &error {
        SymphoniaError::Unsupported(_) => (
            ErrorCode::UnsupportedFormat,
            "container uses an unsupported feature",
        ),
        SymphoniaError::LimitError(_) => (
            ErrorCode::ResourceExhausted,
            "container exceeded a decoder resource limit",
        ),
        SymphoniaError::IoError(io_error) if io_error.kind() == io::ErrorKind::PermissionDenied => {
            (
                ErrorCode::PermissionDenied,
                "permission denied while probing the input",
            )
        }
        _ => (
            ErrorCode::MalformedMedia,
            "failed to parse the supported container",
        ),
    };
    analysis_error(
        path,
        code,
        AnalysisStage::Probe,
        message,
        Some(error.to_string()),
    )
}

pub(crate) fn decoder_creation_error(path: &Path, error: SymphoniaError) -> AnalysisError {
    let code = match error {
        SymphoniaError::Unsupported(_) => ErrorCode::UnsupportedFormat,
        SymphoniaError::LimitError(_) => ErrorCode::ResourceExhausted,
        _ => ErrorCode::MalformedMedia,
    };
    analysis_error(
        path,
        code,
        AnalysisStage::Probe,
        "failed to create the audio decoder",
        Some(error.to_string()),
    )
}

pub(crate) fn runtime_error(path: &Path, message: &str, error: SymphoniaError) -> AnalysisError {
    let code = match &error {
        SymphoniaError::LimitError(_) => ErrorCode::ResourceExhausted,
        SymphoniaError::IoError(io_error) if io_error.kind() == io::ErrorKind::PermissionDenied => {
            ErrorCode::PermissionDenied
        }
        _ => ErrorCode::DecodeFailed,
    };
    analysis_error(
        path,
        code,
        AnalysisStage::Decode,
        message,
        Some(error.to_string()),
    )
}

pub(crate) fn analysis_error(
    path: &Path,
    code: ErrorCode,
    stage: AnalysisStage,
    message: impl Into<String>,
    details: Option<String>,
) -> AnalysisError {
    let mut error = AnalysisError::new(code, stage, message)
        .with_display_path(path.display().to_string())
        .with_backend(BACKEND);
    if let Some(details) = details {
        error = error.with_details(details);
    }
    error
}
