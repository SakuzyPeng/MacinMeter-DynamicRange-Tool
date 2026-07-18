#![forbid(unsafe_code)]

use macinmeter::{
    AnalysisError, AnalysisEvent, AnalysisProfile, AnalysisStage, AnalyzeRequest, Analyzer,
    BatchRequest, BatchRunner, CancellationToken, ErrorCode, ExecutionControl, NoopProgressSink,
    WireEnvelope, discover_inputs_with_control as discover_paths,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::Emitter;

#[derive(Debug, Clone, Default)]
struct JobRegistry {
    jobs: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl JobRegistry {
    fn register(&self, id: &str) -> Result<ActiveJob, AnalysisError> {
        let token = CancellationToken::new();
        self.insert(id, token.clone())?;
        Ok(ActiveJob {
            id: id.to_string(),
            token,
            registry: self.clone(),
        })
    }

    fn insert(&self, id: &str, token: CancellationToken) -> Result<(), AnalysisError> {
        let mut jobs = self
            .jobs
            .lock()
            .map_err(|_| internal_error("job registry is poisoned"))?;
        if jobs.contains_key(id) {
            return Err(AnalysisError::new(
                ErrorCode::InvalidRequest,
                AnalysisStage::Validation,
                "job id is already active",
            ));
        }
        jobs.insert(id.to_string(), token);
        Ok(())
    }

    fn remove(&self, id: &str) {
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.remove(id);
        }
    }

    fn cancel(&self, id: &str) -> Result<bool, AnalysisError> {
        let token = self
            .jobs
            .lock()
            .map_err(|_| internal_error("job registry is poisoned"))?
            .get(id)
            .cloned();
        if let Some(token) = token {
            token.cancel();
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[derive(Debug)]
struct ActiveJob {
    id: String,
    token: CancellationToken,
    registry: JobRegistry,
}

impl Drop for ActiveJob {
    fn drop(&mut self) {
        self.registry.remove(&self.id);
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunAnalysisRequest {
    job_id: String,
    path: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunBatchRequest {
    job_id: String,
    inputs: Vec<PathBuf>,
    recursive: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiscoverRequest {
    job_id: String,
    inputs: Vec<PathBuf>,
    recursive: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiscoveryResponse {
    files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct JobEvent {
    job_id: String,
    event: AnalysisEvent,
}

#[tauri::command]
async fn run_analysis(
    window: tauri::Window,
    registry: tauri::State<'_, JobRegistry>,
    request: RunAnalysisRequest,
) -> Result<WireEnvelope, AnalysisError> {
    if let Err(error) = validate_job_id(&request.job_id) {
        return Ok(WireEnvelope::error(error));
    }
    let registry = registry.inner().clone();
    let active_job = match registry.register(&request.job_id) {
        Ok(job) => job,
        Err(error) => return Ok(WireEnvelope::error(error)),
    };

    let job_id = request.job_id;
    let envelope = match tauri::async_runtime::spawn_blocking(move || {
        let event_window = window.clone();
        let event_job_id = job_id.clone();
        let sink = move |event: AnalysisEvent| {
            let _ = event_window.emit(
                "analysis-event",
                JobEvent {
                    job_id: event_job_id.clone(),
                    event,
                },
            );
        };
        let control = ExecutionControl::new(&active_job.token, &sink);
        execute_analysis(request.path, &control)
    })
    .await
    {
        Ok(envelope) => envelope,
        Err(error) => WireEnvelope::error(internal_error(format!(
            "analysis task failed to join: {error}"
        ))),
    };
    Ok(envelope)
}

fn execute_analysis(path: PathBuf, control: &ExecutionControl<'_>) -> WireEnvelope {
    Analyzer::new()
        .analyze_file_with_control(AnalyzeRequest::new(path), control)
        .map(WireEnvelope::analysis)
        .unwrap_or_else(WireEnvelope::error)
}

#[tauri::command]
async fn run_batch(
    window: tauri::Window,
    registry: tauri::State<'_, JobRegistry>,
    request: RunBatchRequest,
) -> Result<WireEnvelope, AnalysisError> {
    if let Err(error) = validate_job_id(&request.job_id) {
        return Ok(WireEnvelope::error(error));
    }
    let registry = registry.inner().clone();
    let active_job = match registry.register(&request.job_id) {
        Ok(job) => job,
        Err(error) => return Ok(WireEnvelope::error(error)),
    };

    let job_id = request.job_id;
    let envelope = match tauri::async_runtime::spawn_blocking(move || {
        let event_window = window.clone();
        let event_job_id = job_id.clone();
        let sink = move |event: AnalysisEvent| {
            let _ = event_window.emit(
                "analysis-event",
                JobEvent {
                    job_id: event_job_id.clone(),
                    event,
                },
            );
        };
        let control = ExecutionControl::new(&active_job.token, &sink);
        let batch_request = BatchRequest {
            inputs: request.inputs,
            recursive: request.recursive,
            profile: AnalysisProfile::ProvisionalV1,
        };
        BatchRunner::new()
            .run(batch_request, &control)
            .map(WireEnvelope::batch)
            .unwrap_or_else(WireEnvelope::error)
    })
    .await
    {
        Ok(envelope) => envelope,
        Err(error) => WireEnvelope::error(internal_error(format!(
            "batch task failed to join: {error}"
        ))),
    };
    Ok(envelope)
}

#[tauri::command]
async fn discover_inputs(
    registry: tauri::State<'_, JobRegistry>,
    request: DiscoverRequest,
) -> Result<DiscoveryResponse, AnalysisError> {
    validate_job_id(&request.job_id)?;
    let registry = registry.inner().clone();
    let active_job = registry.register(&request.job_id)?;

    tauri::async_runtime::spawn_blocking(move || {
        let progress = NoopProgressSink;
        let control = ExecutionControl::new(&active_job.token, &progress);
        let files = discover_paths(&request.inputs, request.recursive, &control)?
            .into_iter()
            .map(|path| path.display().to_string())
            .collect();
        Ok(DiscoveryResponse { files })
    })
    .await
    .map_err(|error| internal_error(format!("discovery task failed to join: {error}")))?
}

#[tauri::command]
fn cancel_job(
    registry: tauri::State<'_, JobRegistry>,
    job_id: String,
) -> Result<bool, AnalysisError> {
    validate_job_id(&job_id)?;
    registry.cancel(&job_id)
}

fn validate_job_id(id: &str) -> Result<(), AnalysisError> {
    if id.trim().is_empty() || id.len() > 128 {
        Err(AnalysisError::new(
            ErrorCode::InvalidRequest,
            AnalysisStage::Validation,
            "job id must contain between 1 and 128 characters",
        ))
    } else {
        Ok(())
    }
}

fn internal_error(message: impl Into<String>) -> AnalysisError {
    AnalysisError::new(ErrorCode::Internal, AnalysisStage::Internal, message)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::<tauri::Wry>::default()
        .manage(JobRegistry::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            run_analysis,
            run_batch,
            discover_inputs,
            cancel_job
        ])
        .run(tauri::generate_context!());
    if let Err(error) = result {
        eprintln!("failed to run MacinMeter GUI: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_is_scoped_to_one_job() {
        let registry = JobRegistry::default();
        let first = CancellationToken::new();
        let second = CancellationToken::new();
        registry.insert("first", first.clone()).unwrap();
        registry.insert("second", second.clone()).unwrap();

        assert!(registry.cancel("first").unwrap());
        assert!(first.is_cancelled());
        assert!(!second.is_cancelled());
    }

    #[test]
    fn active_job_releases_its_id_when_dropped() {
        let registry = JobRegistry::default();
        {
            let _active = registry.register("job").unwrap();
            assert!(registry.register("job").is_err());
        }
        assert!(registry.register("job").is_ok());
    }

    #[test]
    fn tauri_analysis_path_returns_the_shared_application_report() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/tiny_duration.wav");
        let cancellation = CancellationToken::new();
        let progress = macinmeter::NoopProgressSink;
        let control = ExecutionControl::new(&cancellation, &progress);

        let from_tauri_adapter = execute_analysis(path.clone(), &control);
        let from_application = Analyzer::new()
            .analyze_file(AnalyzeRequest::new(path))
            .map(WireEnvelope::analysis)
            .unwrap_or_else(WireEnvelope::error);

        assert_eq!(from_tauri_adapter, from_application);
    }
}
