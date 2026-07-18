#![forbid(unsafe_code)]

use clap::{Parser, Subcommand, ValueEnum};
use macinmeter::{
    AnalysisError, AnalysisEvent, AnalysisProfile, AnalysisReport, BatchItemOutcome, BatchReport,
    BatchRequest, BatchRunner, BatchStatus, CancellationToken, ChannelOutcome, ErrorCode,
    ExecutionControl, ProgressSink, WireEnvelope,
};
use std::{
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Debug, Parser)]
#[command(
    name = "macinmeter",
    version,
    about = "Offline dynamic-range analysis (ProvisionalV1, unverified)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Analyze one audio file.
    Analyze {
        file: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Analyze files and directories serially.
    Batch {
        inputs: Vec<PathBuf>,
        #[arg(long)]
        recursive: bool,
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
}

struct StderrProgress;

impl ProgressSink for StderrProgress {
    fn emit(&self, event: AnalysisEvent) {
        match event {
            AnalysisEvent::FileStarted {
                index,
                display_path,
            } => eprintln!("[{index}] analyzing {display_path}"),
            AnalysisEvent::FileFinished {
                index,
                display_path,
                success,
            } => {
                let status = if success { "ok" } else { "failed" };
                eprintln!("[{index}] {status}: {display_path}");
            }
            AnalysisEvent::BatchFinished { succeeded, failed } => {
                eprintln!("batch complete: {succeeded} succeeded, {failed} failed");
            }
            _ => {}
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let cancellation = CancellationToken::new();
    let signal_token = cancellation.clone();
    if let Err(error) = ctrlc::set_handler(move || signal_token.cancel()) {
        eprintln!("warning: failed to install cancellation handler: {error}");
    }
    let progress = StderrProgress;
    let control = ExecutionControl::new(&cancellation, &progress);

    let exit_code = match cli.command {
        Command::Analyze {
            file,
            format,
            output,
        } => run_analyze(file, format, output.as_deref(), &control),
        Command::Batch {
            inputs,
            recursive,
            format,
            output,
        } => run_batch(inputs, recursive, format, output.as_deref(), &control),
    };
    process::exit(exit_code);
}

fn run_analyze(
    file: PathBuf,
    format: OutputFormat,
    output: Option<&Path>,
    control: &ExecutionControl<'_>,
) -> i32 {
    let analyzer = macinmeter::Analyzer::new();
    let request = macinmeter::AnalyzeRequest {
        path: file,
        profile: AnalysisProfile::ProvisionalV1,
    };

    match analyzer.analyze_file_with_control(request, control) {
        Ok(report) => {
            let rendered = match format {
                OutputFormat::Human => render_analysis(&report),
                OutputFormat::Json => render_json(&WireEnvelope::analysis(report)),
            };
            finish_output(rendered, output)
        }
        Err(error) => finish_error(error, format, output),
    }
}

fn run_batch(
    inputs: Vec<PathBuf>,
    recursive: bool,
    format: OutputFormat,
    output: Option<&Path>,
    control: &ExecutionControl<'_>,
) -> i32 {
    let runner = BatchRunner::new();
    let request = BatchRequest {
        inputs,
        recursive,
        profile: AnalysisProfile::ProvisionalV1,
    };
    match runner.run(request, control) {
        Ok(report) => {
            let status = report.status;
            let rendered = match format {
                OutputFormat::Human => render_batch(&report),
                OutputFormat::Json => render_json(&WireEnvelope::batch(report)),
            };
            let output_code = finish_output(rendered, output);
            if output_code != 0 {
                output_code
            } else {
                match status {
                    BatchStatus::Succeeded => 0,
                    BatchStatus::PartiallySucceeded => 3,
                    BatchStatus::Failed => 1,
                }
            }
        }
        Err(error) => finish_error(error, format, output),
    }
}

fn finish_error(error: AnalysisError, format: OutputFormat, output: Option<&Path>) -> i32 {
    let cancelled = error.code == ErrorCode::Cancelled;
    match format {
        OutputFormat::Json => {
            let rendered = render_json(&WireEnvelope::error(error));
            let output_code = finish_output(rendered, output);
            if output_code != 0 {
                return output_code;
            }
        }
        OutputFormat::Human => eprintln!("error [{}]: {}", error_code_name(error.code), error),
    }
    if cancelled { 130 } else { 1 }
}

fn finish_output(rendered: Result<String, AnalysisError>, output: Option<&Path>) -> i32 {
    match rendered.and_then(|text| write_output(&text, output)) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("error [{}]: {}", error_code_name(error.code), error);
            1
        }
    }
}

fn render_json(envelope: &WireEnvelope) -> Result<String, AnalysisError> {
    serde_json::to_string_pretty(envelope).map_err(|error| {
        AnalysisError::new(
            ErrorCode::Internal,
            macinmeter::AnalysisStage::Output,
            "failed to serialize JSON output",
        )
        .with_details(error.to_string())
    })
}

fn render_analysis(report: &AnalysisReport) -> Result<String, AnalysisError> {
    let mut output = String::new();
    output.push_str("MacinMeter ProvisionalV1 — UNVERIFIED\n");
    output.push_str(&format!("Source: {}\n", report.source.display_path));
    output.push_str(&format!(
        "PCM: {} Hz, {} channels, {} frames\n\n",
        report.pcm.spec.sample_rate.get(),
        report.pcm.spec.channels.get(),
        report.analysis.frames_seen
    ));

    for channel in &report.analysis.channels {
        match &channel.outcome {
            ChannelOutcome::Measured { measurement } => output.push_str(&format!(
                "CH {}: DR{} ({:.4} dB), RMS {:.8}, peak {:.8}\n",
                channel.channel_index + 1,
                measurement.rounded_dr,
                measurement.dr_db,
                measurement.loud_rms,
                measurement.selected_peak
            )),
            ChannelOutcome::Silent {
                frames,
                valid_windows,
            } => output.push_str(&format!(
                "CH {}: silent ({frames} frames, {valid_windows} windows)\n",
                channel.channel_index + 1
            )),
            ChannelOutcome::InsufficientData { frames } => output.push_str(&format!(
                "CH {}: insufficient data ({frames} frames)\n",
                channel.channel_index + 1
            )),
        }
    }

    let aggregate = &report.analysis.aggregates.all_channels;
    if let (Some(rounded_dr), Some(precise_dr_db)) = (aggregate.rounded_dr, aggregate.precise_dr_db)
    {
        output.push_str(&format!(
            "\nAggregate: DR{} ({:.4} dB; {} measured channels)\n",
            rounded_dr,
            precise_dr_db,
            aggregate.included_channels.len()
        ));
    } else {
        output.push_str("\nAggregate: unavailable\n");
    }
    Ok(output)
}

fn render_batch(report: &BatchReport) -> Result<String, AnalysisError> {
    let mut output = String::from("MacinMeter ProvisionalV1 batch — UNVERIFIED\n\n");
    for item in &report.items {
        match &item.outcome {
            BatchItemOutcome::Success { report } => {
                let aggregate = &report.analysis.aggregates.all_channels;
                let aggregate = match (aggregate.rounded_dr, aggregate.precise_dr_db) {
                    (Some(rounded_dr), Some(precise_dr_db)) => {
                        format!("DR{rounded_dr} ({precise_dr_db:.4} dB)")
                    }
                    _ => "unavailable".to_string(),
                };
                output.push_str(&format!("OK   {} — {aggregate}\n", item.display_path));
            }
            BatchItemOutcome::Failure { error } => {
                output.push_str(&format!(
                    "FAIL {} — [{}] {}\n",
                    item.display_path,
                    error_code_name(error.code),
                    error.message
                ));
            }
        }
    }
    output.push_str(&format!(
        "\nTotal {}, succeeded {}, failed {}\n",
        report.summary.total, report.summary.succeeded, report.summary.failed
    ));
    Ok(output)
}

fn write_output(text: &str, output: Option<&Path>) -> Result<(), AnalysisError> {
    match output {
        None => {
            let mut stdout = io::stdout().lock();
            stdout.write_all(text.as_bytes()).map_err(output_error)?;
            stdout.write_all(b"\n").map_err(output_error)?;
            stdout.flush().map_err(output_error)
        }
        Some(path) => atomic_write(path, text.as_bytes()),
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), AnalysisError> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("macinmeter-output");
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".{file_name}.{}.{}.tmp", process::id(), sequence));

    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(output_error)?;
        file.write_all(bytes).map_err(output_error)?;
        file.sync_all().map_err(output_error)?;
        std::fs::rename(&temporary, path).map_err(output_error)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn output_error(error: io::Error) -> AnalysisError {
    AnalysisError::new(
        ErrorCode::OutputFailed,
        macinmeter::AnalysisStage::Output,
        "failed to write analysis output",
    )
    .with_details(error.to_string())
}

fn error_code_name(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::InvalidRequest => "invalid_request",
        ErrorCode::InputNotFound => "input_not_found",
        ErrorCode::PermissionDenied => "permission_denied",
        ErrorCode::NoInputs => "no_inputs",
        ErrorCode::UnsupportedFormat => "unsupported_format",
        ErrorCode::MalformedMedia => "malformed_media",
        ErrorCode::DecodeFailed => "decode_failed",
        ErrorCode::AnalysisFailed => "analysis_failed",
        ErrorCode::ResourceExhausted => "resource_exhausted",
        ErrorCode::OutputFailed => "output_failed",
        ErrorCode::Cancelled => "cancelled",
        ErrorCode::Internal => "internal",
    }
}
