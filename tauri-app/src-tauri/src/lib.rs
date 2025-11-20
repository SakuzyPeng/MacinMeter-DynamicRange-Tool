use macinmeter_dr_tool::{
    analyze_file,
    audio::UniversalDecoder,
    error::{AudioError, ErrorCategory},
    processing::{EdgeTrimReport, SilenceFilterReport},
    tools::{self, constants::defaults, formatter},
    AppConfig, AudioFormat, DrResult,
};
use serde::{Deserialize, Serialize};
use rayon::prelude::*;
use std::{
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct UiAnalyzeOptions {
    parallel_decoding: bool,
    exclude_lfe: bool,
    show_rms_peak: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalyzeResponse {
    source_path: String,
    format: AudioFormatView,
    dr_results: Vec<DrChannelResultView>,
    aggregates: AggregatesView,
    trim_report: Option<TrimReportView>,
    silence_report: Option<UiSilenceReport>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DrChannelResultView {
    channel: usize,
    dr_value: f64,
    dr_value_rounded: i32,
    rms: f64,
    peak: f64,
    primary_peak: f64,
    secondary_peak: f64,
    sample_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AudioFormatView {
    sample_rate: u32,
    channels: u16,
    bits_per_sample: u16,
    sample_count: u64,
    duration_seconds: f64,
    codec: Option<String>,
    processed_sample_rate: Option<u32>,
    dsd_native_rate_hz: Option<u32>,
    dsd_multiple_of_44k: Option<u32>,
    has_channel_layout_metadata: bool,
    lfe_indices: Vec<usize>,
    partial_analysis: bool,
    skipped_packets: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrimReportView {
    enabled: bool,
    threshold_db: f64,
    min_run_ms: f64,
    hysteresis_ms: f64,
    leading_seconds: f64,
    trailing_seconds: f64,
    total_seconds: f64,
    total_samples_trimmed: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UiSilenceReport {
    threshold_db: f64,
    channels: Vec<UiSilenceChannel>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UiSilenceChannel {
    channel_index: usize,
    valid_windows: usize,
    filtered_windows: usize,
    total_windows: usize,
    filtered_percent: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BoundaryWarningView {
    level: String,
    direction: String,
    distance_db: f64,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AggregateView {
    official_dr: Option<i32>,
    precise_dr: Option<f64>,
    excluded_channels: usize,
    excluded_lfe: usize,
    boundary_warning: Option<BoundaryWarningView>,
    warning_text: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AggregatesView {
    include_lfe: AggregateView,
    exclude_lfe: AggregateView,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DirectoryAnalysisEntry {
    path: String,
    file_name: String,
    analysis: Option<AnalyzeResponse>,
    error: Option<AnalyzeCommandError>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DirectoryAnalysisResponse {
    directory: String,
    files: Vec<DirectoryAnalysisEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanResult {
    directory: String,
    files: Vec<ScannedFile>,
    supported_formats: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScannedFile {
    file_name: String,
    path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MetadataResponse {
    supported_formats: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalyzeCommandError {
    message: String,
    suggestion: Option<String>,
    category: String,
    supported_formats: Option<Vec<String>>,
}

impl fmt::Display for AnalyzeCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for AnalyzeCommandError {}

impl AnalyzeCommandError {
    fn from_audio_error(error: AudioError) -> Self {
        let category = ErrorCategory::from_audio_error(&error);
        let suggestion = Some(error_suggestion(&error).to_string());
        let supported_formats = if matches!(category, ErrorCategory::Format) {
            Some(supported_formats_list())
        } else {
            None
        };
        Self {
            message: error.to_string(),
            suggestion,
            category: format!("{category:?}"),
            supported_formats,
        }
    }

    fn from_scan_error(error: AudioError, directory: &Path) -> Self {
        let mut err = Self::from_audio_error(error);
        err.message = format!("扫描目录 {} 时失败: {}", directory.display(), err.message);
        err
    }

    fn internal(message: String) -> Self {
        Self {
            message,
            suggestion: None,
            category: "Internal".to_string(),
            supported_formats: None,
        }
    }
}

#[tauri::command]
async fn analyze_audio(
    path: PathBuf,
    options: UiAnalyzeOptions,
) -> Result<AnalyzeResponse, AnalyzeCommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = options.to_app_config(path);
        let analysis_target = config.input_path.clone();
        let (results, format, trim_report, silence_report) =
            analyze_file(&analysis_target, &config).map_err(AnalyzeCommandError::from_audio_error)?;

        Ok(build_analyze_response(
            &config,
            &analysis_target,
            results,
            format,
            trim_report,
            silence_report,
        ))
    })
    .await
    .map_err(|err| AnalyzeCommandError::internal(format!("分析线程调度失败: {err}")))?
}

#[tauri::command]
async fn scan_audio_directory(path: PathBuf) -> Result<ScanResult, AnalyzeCommandError> {
    let directory = path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let files =
            tools::scan_audio_files(&directory).map_err(|e| AnalyzeCommandError::from_scan_error(e, &directory))?;
        Ok(ScanResult {
            directory: directory.display().to_string(),
            files: files
                .into_iter()
                .map(|p| {
                    let file_name = p
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| p.to_string_lossy().into_owned());
                    ScannedFile {
                        file_name,
                        path: p.to_string_lossy().into_owned(),
                    }
                })
                .collect(),
            supported_formats: supported_formats_list(),
        })
    })
    .await
    .map_err(|err| AnalyzeCommandError::internal(format!("扫描线程调度失败: {err}")))?
}

#[tauri::command]
fn load_app_metadata() -> MetadataResponse {
    MetadataResponse {
        supported_formats: supported_formats_list(),
    }
}

#[tauri::command]
fn set_ffmpeg_override(path: Option<String>) -> Result<(), AnalyzeCommandError> {
    if let Some(p) = path.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        unsafe { std::env::set_var("MACINMETER_FFMPEG_PATH", &p); }
    } else {
        unsafe { std::env::remove_var("MACINMETER_FFMPEG_PATH"); }
    }
    Ok(())
}

#[tauri::command]
fn path_is_directory(path: PathBuf) -> Result<bool, AnalyzeCommandError> {
    std::fs::metadata(&path)
        .map(|m| m.is_dir())
        .map_err(|e| AnalyzeCommandError::from_audio_error(AudioError::IoError(e)))
}

#[tauri::command]
async fn analyze_directory(
    path: PathBuf,
    options: UiAnalyzeOptions,
) -> Result<DirectoryAnalysisResponse, AnalyzeCommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let directory = path.clone();
        let files =
            tools::scan_audio_files(&directory).map_err(|e| AnalyzeCommandError::from_scan_error(e, &directory))?;
        if files.is_empty() {
            return Ok(DirectoryAnalysisResponse {
                directory: directory.display().to_string(),
                files: Vec::new(),
            });
        }

        let options = Arc::new(options);
        let entries: Vec<DirectoryAnalysisEntry> = files
            .into_par_iter()
            .map(|file| analyze_single_file(file, &options))
            .collect();

        Ok(DirectoryAnalysisResponse {
            directory: directory.display().to_string(),
            files: entries,
        })
    })
    .await
    .map_err(|err| AnalyzeCommandError::internal(format!("批量分析线程调度失败: {err}")))?
}

#[tauri::command]
async fn analyze_files(
    paths: Vec<String>,
    options: UiAnalyzeOptions,
) -> Result<DirectoryAnalysisResponse, AnalyzeCommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        if paths.is_empty() {
            return Ok(DirectoryAnalysisResponse {
                directory: "selected-files".to_string(),
                files: Vec::new(),
            });
        }
        let options = Arc::new(options);
        let entries: Vec<DirectoryAnalysisEntry> = paths
            .into_par_iter()
            .map(|p| PathBuf::from(p))
            .map(|file| analyze_single_file(file, &options))
            .collect();
        Ok(DirectoryAnalysisResponse {
            directory: "selected-files".to_string(),
            files: entries,
        })
    })
    .await
    .map_err(|err| AnalyzeCommandError::internal(format!("多文件分析线程调度失败: {err}")))?
}

fn build_analyze_response(
    _config: &AppConfig,
    source_path: &Path,
    dr_results: Vec<DrResult>,
    format: AudioFormat,
    trim_report: Option<EdgeTrimReport>,
    silence_report: Option<SilenceFilterReport>,
) -> AnalyzeResponse {
    let include_aggregate = build_aggregate_view(&dr_results, &format, false);
    let exclude_aggregate = build_aggregate_view(&dr_results, &format, true);

    AnalyzeResponse {
        source_path: source_path.to_string_lossy().into_owned(),
        format: AudioFormatView {
            sample_rate: format.sample_rate,
            channels: format.channels,
            bits_per_sample: format.bits_per_sample,
            sample_count: format.sample_count,
            duration_seconds: format.duration_seconds(),
            codec: format.codec_type.map(|c| format!("{c:?}")),
            processed_sample_rate: format.processed_sample_rate,
            dsd_native_rate_hz: format.dsd_native_rate_hz,
            dsd_multiple_of_44k: format.dsd_multiple_of_44k,
            has_channel_layout_metadata: format.has_channel_layout_metadata,
            lfe_indices: format.lfe_indices.clone(),
            partial_analysis: format.is_partial(),
            skipped_packets: format.skipped_packets(),
        },
        dr_results: dr_results
            .iter()
            .map(|dr| DrChannelResultView {
                channel: dr.channel,
                dr_value: dr.dr_value,
                dr_value_rounded: dr.dr_value_rounded(),
                rms: dr.rms,
                peak: dr.peak,
                primary_peak: dr.primary_peak,
                secondary_peak: dr.secondary_peak,
                sample_count: dr.sample_count,
            })
            .collect(),
        aggregates: AggregatesView {
            include_lfe: include_aggregate,
            exclude_lfe: exclude_aggregate,
        },
        trim_report: trim_report.map(|report| {
            let channels = format.channels as usize;
            TrimReportView {
                enabled: report.config.enabled,
                threshold_db: report.config.threshold_db,
                min_run_ms: report.config.min_run_ms,
                hysteresis_ms: report.config.hysteresis_ms,
                leading_seconds: report.leading_duration_sec(format.sample_rate, channels),
                trailing_seconds: report.trailing_duration_sec(format.sample_rate, channels),
                total_seconds: report.total_duration_sec(format.sample_rate, channels),
                total_samples_trimmed: report.total_samples_trimmed(),
            }
        }),
        silence_report: silence_report.map(|r| UiSilenceReport {
            threshold_db: r.threshold_db,
            channels: r
                .channels
                .into_iter()
                .map(|c| UiSilenceChannel {
                    channel_index: c.channel_index,
                    valid_windows: c.valid_windows,
                    filtered_windows: c.filtered_windows,
                    total_windows: c.total_windows,
                    filtered_percent: c.filtered_percent(),
                })
                .collect(),
        }),
    }
}

fn build_aggregate_view(
    dr_results: &[DrResult],
    format: &AudioFormat,
    exclude_lfe: bool,
) -> AggregateView {
    let official_info = formatter::compute_official_precise_dr(dr_results, format, exclude_lfe);
    let (official_dr, precise_dr, excluded_channels, excluded_lfe) = match official_info {
        Some((official, precise, excluded, excluded_lfe)) => {
            (Some(official), Some(precise), excluded, excluded_lfe)
        }
        None => (None, None, 0, 0),
    };

    let (boundary_warning, warning_text) = match (official_dr, precise_dr) {
        (Some(official), Some(precise)) => {
            let text = formatter::detect_dr_boundary_warning(official, precise);
            let view = formatter::detect_boundary_risk_level(official, precise).map(
                |(level, direction, distance)| BoundaryWarningView {
                    level: format!("{level:?}"),
                    direction: match direction {
                        formatter::BoundaryDirection::Upper => "Upper".to_string(),
                        formatter::BoundaryDirection::Lower => "Lower".to_string(),
                    },
                    distance_db: (distance * 100.0).round() / 100.0,
                    message: text.clone().unwrap_or_default(),
                },
            );
            (view, text)
        }
        _ => (None, None),
    };

    AggregateView {
        official_dr,
        precise_dr,
        excluded_channels,
        excluded_lfe,
        boundary_warning,
        warning_text,
    }
}

fn analyze_single_file(file: PathBuf, options: &UiAnalyzeOptions) -> DirectoryAnalysisEntry {
    let file_name = file
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| file.to_string_lossy().into_owned());
    let path_display = file.to_string_lossy().into_owned();
    let config = options.to_app_config(file.clone());

    match analyze_file(&file, &config) {
        Ok((results, format, trim_report, silence_report)) => DirectoryAnalysisEntry {
            path: path_display,
            file_name,
            analysis: Some(build_analyze_response(
                &config,
                &file,
                results,
                format,
                trim_report,
                silence_report,
            )),
            error: None,
        },
        Err(err) => DirectoryAnalysisEntry {
            path: path_display,
            file_name,
            analysis: None,
            error: Some(AnalyzeCommandError::from_audio_error(err)),
        },
    }
}

impl UiAnalyzeOptions {
    fn to_app_config(&self, input_path: PathBuf) -> AppConfig {
        AppConfig {
            input_path,
            verbose: false,
            output_path: None,
            parallel_decoding: self.parallel_decoding,
            parallel_batch_size: defaults::PARALLEL_BATCH_SIZE,
            parallel_threads: defaults::PARALLEL_THREADS,
            parallel_files: None,
            silence_filter_threshold_db: None,
            edge_trim_threshold_db: None,
            edge_trim_min_run_ms: None,
            exclude_lfe: self.exclude_lfe,
            show_rms_peak: self.show_rms_peak,
            dsd_pcm_rate: Some(352_800),
            dsd_gain_db: 6.0,
            dsd_filter: "teac".to_string(),
        }
    }
}

fn supported_formats_list() -> Vec<String> {
    let decoder = UniversalDecoder::new();
    decoder
        .supported_formats()
        .extensions
        .iter()
        .map(|ext| ext.to_uppercase())
        .collect()
}

fn error_suggestion(error: &AudioError) -> &'static str {
    match error {
        AudioError::InvalidInput(_) => {
            "检查命令行参数是否正确，或确保输入文件路径有效。"
        }
        AudioError::ResourceError(_) => {
            "资源不可用，请检查系统资源或降低并发度后重试。"
        }
        AudioError::OutOfMemory => {
            "内存不足，可尝试禁用并行解码或一次仅分析单个文件。"
        }
        _ => match ErrorCategory::from_audio_error(error) {
            ErrorCategory::Io => "检查文件路径是否存在，并确认读写权限。",
            ErrorCategory::Format => "确保输入文件是受支持的音频格式。",
            ErrorCategory::Decoding => "文件可能损坏或使用了不受支持的编码。",
            ErrorCategory::Calculation => "计算过程出现异常，请确认音频数据有效。",
            ErrorCategory::Other => "请检查输入文件及参数设置。",
        },
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    ensure_default_path();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            analyze_audio,
            scan_audio_directory,
            path_is_directory,
            analyze_directory,
            analyze_files,
            set_ffmpeg_override,
            load_app_metadata
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn ensure_default_path() {
    #[cfg(target_os = "macos")]
    {
        use std::env;
        const DEFAULT_SEGMENTS: &str = "/usr/local/bin:/opt/homebrew/bin";
        if let Ok(current) = env::var("PATH") {
            if !current.contains("/opt/homebrew/bin") && !current.contains("/usr/local/bin") {
                let new_path = format!("{DEFAULT_SEGMENTS}:{current}");
                unsafe { env::set_var("PATH", new_path); }
            }
        } else {
            unsafe { env::set_var("PATH", DEFAULT_SEGMENTS); }
        }
    }
}
