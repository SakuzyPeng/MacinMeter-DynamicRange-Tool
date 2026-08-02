#![forbid(unsafe_code)]

use macinmeter::{
    AnalysisError, AnalysisEvent, AnalysisStage, AnalyzeRequest, Application, ApplicationJob,
    BatchRequest, CancellationToken, CapabilitySnapshot, ErrorCode, NoopProgressSink, WireEnvelope,
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
    application: tauri::State<'_, Application>,
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
    let application_job = match application.reserve(&active_job.token) {
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
        let envelope = execute_analysis(application_job, request.path, &sink);
        drop(active_job);
        envelope
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

fn execute_analysis(
    job: ApplicationJob,
    path: PathBuf,
    progress: &dyn macinmeter::ProgressSink,
) -> WireEnvelope {
    job.analyze_file(AnalyzeRequest::new(path), progress)
        .map(WireEnvelope::analysis)
        .unwrap_or_else(WireEnvelope::error)
}

#[tauri::command]
async fn run_batch(
    window: tauri::Window,
    registry: tauri::State<'_, JobRegistry>,
    application: tauri::State<'_, Application>,
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
    let application_job = match application.reserve(&active_job.token) {
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
        let batch_request = BatchRequest {
            inputs: request.inputs,
            recursive: request.recursive,
        };
        let envelope = application_job
            .run_batch(batch_request, &sink)
            .map(WireEnvelope::batch)
            .unwrap_or_else(WireEnvelope::error);
        drop(active_job);
        envelope
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
    application: tauri::State<'_, Application>,
    request: DiscoverRequest,
) -> Result<DiscoveryResponse, AnalysisError> {
    validate_job_id(&request.job_id)?;
    let registry = registry.inner().clone();
    let active_job = registry.register(&request.job_id)?;
    let application_job = application.reserve(&active_job.token)?;

    tauri::async_runtime::spawn_blocking(move || {
        let progress = NoopProgressSink;
        let files = application_job
            .discover_inputs(&request.inputs, request.recursive, &progress)?
            .into_iter()
            .map(|path| path.display().to_string())
            .collect();
        drop(active_job);
        Ok(DiscoveryResponse { files })
    })
    .await
    .map_err(|error| internal_error(format!("discovery task failed to join: {error}")))?
}

#[tauri::command]
fn get_capabilities() -> CapabilitySnapshot {
    macinmeter::capabilities()
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
        .manage(Application::new())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            run_analysis,
            run_batch,
            discover_inputs,
            cancel_job,
            get_capabilities
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
    use macinmeter::WirePayload;
    use std::{
        sync::{Condvar, mpsc},
        thread,
        time::Duration,
    };

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
    fn capability_command_returns_the_shared_application_snapshot() {
        let snapshot = get_capabilities();
        assert_eq!(snapshot, macinmeter::capabilities());
        assert_eq!(
            snapshot.stable_discovery_extensions,
            ["aif", "aiff", "flac", "m4a", "mp4", "wav", "wave"]
        );
        assert_eq!(
            snapshot
                .routes
                .iter()
                .filter(|route| route.status == "stable")
                .count(),
            5
        );
        let alac = snapshot
            .routes
            .iter()
            .find(|route| route.container == "mp4" && route.codec == "alac")
            .expect("Tauri capability snapshot must expose ALAC");
        assert_eq!(alac.status, "stable");
        assert_eq!(alac.discovery_extensions, ["m4a", "mp4"]);
    }

    #[test]
    fn tauri_analysis_path_returns_the_shared_application_report() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/native-alac-v1/alac16-mono-44100.m4a");
        let cancellation = CancellationToken::new();
        let progress = macinmeter::NoopProgressSink;
        let application = Application::new();
        let job = application.reserve(&cancellation).unwrap();

        let from_tauri_adapter = execute_analysis(job, path.clone(), &progress);
        let from_application = Application::new()
            .analyze_file(AnalyzeRequest::new(path))
            .map(WireEnvelope::analysis)
            .unwrap_or_else(WireEnvelope::error);

        assert_eq!(from_tauri_adapter.schema_version, 4);
        let WirePayload::Analysis(report) = &from_tauri_adapter.payload else {
            panic!("real ALAC fixture must return an analysis envelope");
        };
        assert_eq!(report.source().container, macinmeter::ContainerFormat::Mp4);
        assert_eq!(report.source().codec, macinmeter::SourceCodec::Alac);
        assert_eq!(from_tauri_adapter, from_application);
    }

    #[test]
    fn shared_application_serializes_jobs_and_queued_cancellation_is_isolated() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/tiny_duration.wav");
        let application = Application::new();
        let registry = JobRegistry::default();
        let first_active = registry.register("first").unwrap();
        let second_active = registry.register("second").unwrap();
        let first_token = first_active.token.clone();
        let first_job = application.reserve(&first_active.token).unwrap();
        let second_job = application.reserve(&second_active.token).unwrap();
        let (first_started_tx, first_started_rx) = mpsc::channel();
        let (second_started_tx, second_started_rx) = mpsc::channel();
        let first_gate = Arc::new((Mutex::new(false), Condvar::new()));
        let first_gate_for_sink = Arc::clone(&first_gate);
        let first_path = path.clone();

        let first_thread = thread::spawn(move || {
            let sink = move |event: AnalysisEvent| {
                if matches!(event, AnalysisEvent::FileStarted { .. }) {
                    first_started_tx.send(()).unwrap();
                    let (released, changed) = &*first_gate_for_sink;
                    let guard = released.lock().unwrap();
                    drop(changed.wait_while(guard, |released| !*released).unwrap());
                }
            };
            let envelope = execute_analysis(first_job, first_path, &sink);
            drop(first_active);
            envelope
        });
        first_started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("the first Tauri job should enter analysis");

        let second_thread = thread::spawn(move || {
            let sink = move |event: AnalysisEvent| {
                if matches!(event, AnalysisEvent::FileStarted { .. }) {
                    second_started_tx.send(()).unwrap();
                }
            };
            let envelope = execute_analysis(second_job, path, &sink);
            drop(second_active);
            envelope
        });
        assert!(
            second_started_rx
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "the second Tauri job must remain queued while the first is active"
        );

        assert!(registry.cancel("second").unwrap());
        let second_envelope = second_thread.join().unwrap();
        assert!(matches!(
            second_envelope.payload,
            WirePayload::Error(AnalysisError {
                code: ErrorCode::Cancelled,
                ..
            })
        ));
        assert!(
            !first_token.is_cancelled(),
            "cancelling the queued job must not affect the active job"
        );

        let (released, changed) = &*first_gate;
        *released.lock().unwrap() = true;
        changed.notify_all();
        let first_envelope = first_thread.join().unwrap();
        assert!(matches!(first_envelope.payload, WirePayload::Analysis(_)));
    }
}
