#![forbid(unsafe_code)]

mod render;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Generator, Shell};
use macinmeter::{
    AnalysisError, AnalysisEvent, AnalysisReport, Application, BatchItemOutcome, BatchReport,
    BatchRequest, BatchStatus, CancellationToken, ChannelOutcome, ErrorCode, ExecutionControl,
    PhaseTimings, ProgressSink, WireEnvelope,
};
use render::{PhaseTimingScope, format_dbfs, format_duration_token, format_elapsed_line};
use std::{
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

#[derive(Debug, Parser)]
#[command(name = "mdrmeter", version, about = "Offline dynamic-range analysis")]
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
        /// Also report how long decode and analysis each occupied. They may
        /// overlap and omit other work, so they do not partition elapsed time.
        #[arg(long)]
        timing: bool,
    },
    /// Analyze files and directories, reporting them in stable input order.
    Batch {
        inputs: Vec<PathBuf>,
        #[arg(long)]
        recursive: bool,
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Also report the decode and analysis totals the lanes accumulated.
        /// Lanes may overlap and the totals omit other work, so they do not
        /// partition elapsed time.
        #[arg(long)]
        timing: bool,
    },
    /// Write a shell completion script to stdout.
    ///
    /// This only prints; installing it is the caller's step, because where a
    /// shell reads completions from is the caller's business and not something
    /// an analyzer should be writing to on its own.
    Completions {
        #[arg(value_enum)]
        shell: Shell,
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
    let application = Application::new();

    let exit_code = match cli.command {
        Command::Analyze {
            file,
            format,
            output,
            timing,
        } => run_analyze(
            &application,
            file,
            format,
            output.as_deref(),
            timing,
            &control,
        ),
        Command::Completions { shell } => run_completions(shell),
        Command::Batch {
            inputs,
            recursive,
            format,
            output,
            timing,
        } => run_batch(
            &application,
            inputs,
            recursive,
            format,
            output.as_deref(),
            timing,
            &control,
        ),
    };
    process::exit(exit_code);
}

fn run_completions(shell: Shell) -> i32 {
    // Generated from the same `Command` the parser uses, so a subcommand or
    // flag cannot exist without the completion knowing about it.
    let mut command = Cli::command();
    let name = command.get_name().to_string();
    command.set_bin_name(name);
    command.build();

    let mut stdout = io::stdout().lock();
    let generated = shell
        .try_generate(&command, &mut stdout)
        .and_then(|()| stdout.flush())
        .map_err(completion_output_error);
    match generated {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("error [{}]: {}", error_code_name(error.code), error);
            1
        }
    }
}

fn run_analyze(
    application: &Application,
    file: PathBuf,
    format: OutputFormat,
    output: Option<&Path>,
    timing: bool,
    control: &ExecutionControl<'_>,
) -> i32 {
    let request = macinmeter::AnalyzeRequest { path: file };

    let started = Instant::now();
    // Two entries rather than a flag, so an ordinary run does not carry even a
    // disabled clock's decision into the analyzer.
    let analyzed = if timing {
        application.analyze_file_timed(request, control)
    } else {
        application
            .analyze_file_with_control(request, control)
            .map(|report| (report, PhaseTimings::default()))
    };
    match analyzed {
        Ok((report, phases)) => {
            let elapsed = started.elapsed();
            let rendered = match format {
                OutputFormat::Human => render_analysis(&report, elapsed, timing.then_some(phases)),
                OutputFormat::Json => render_json(&WireEnvelope::analysis(report)),
            };
            finish_output(rendered, output)
        }
        Err(error) => finish_error(error, format, output),
    }
}

fn run_batch(
    application: &Application,
    inputs: Vec<PathBuf>,
    recursive: bool,
    format: OutputFormat,
    output: Option<&Path>,
    timing: bool,
    control: &ExecutionControl<'_>,
) -> i32 {
    let request = BatchRequest { inputs, recursive };
    let started = Instant::now();
    let ran = if timing {
        application.run_batch_timed(request, control)
    } else {
        application
            .run_batch(request, control)
            .map(|report| (report, PhaseTimings::default()))
    };
    match ran {
        Ok((report, phases)) => {
            let elapsed = started.elapsed();
            let status = report.status;
            let rendered = match format {
                OutputFormat::Human => render_batch(&report, elapsed, timing.then_some(phases)),
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

fn render_analysis(
    report: &AnalysisReport,
    elapsed: Duration,
    phases: Option<PhaseTimings>,
) -> Result<String, AnalysisError> {
    let analysis = report.analysis();
    let mut output = String::new();
    output.push_str("MacinMeter\n");
    output.push_str(&format!("Source: {}\n", report.source().display_path));
    output.push_str(&format!(
        "PCM: {} Hz, {} channels, {} frames\nDuration: {}\n\n",
        report.pcm().spec.sample_rate.get(),
        report.pcm().spec.channels.get(),
        analysis.frames_seen(),
        format_duration_token(analysis.report().duration)?
    ));

    for channel in analysis.channels() {
        match &channel.outcome {
            ChannelOutcome::Measured { measurement } => output.push_str(&format!(
                "CH {}: DR{} ({:.4} dB), overall RMS {} dBFS, selected DR peak {:.8}\n",
                channel.channel_index + 1,
                measurement.rounded_dr,
                measurement.dr_db.get(),
                format_dbfs(channel.report.overall_rms_dbfs),
                measurement.dr_selected_peak.get()
            )),
            ChannelOutcome::Silent {
                frames,
                valid_windows,
            } => output.push_str(&format!(
                "CH {}: silent — DR0 contribution, overall RMS -inf dBFS ({frames} frames, {valid_windows} windows)\n",
                channel.channel_index + 1,
            )),
            ChannelOutcome::InsufficientData { frames } => output.push_str(&format!(
                "CH {}: insufficient data ({frames} frames)\n",
                channel.channel_index + 1
            )),
        }
    }

    let aggregate = &analysis.aggregates().track;
    let report_metrics = analysis.report();
    if let (Some(rounded_dr), Some(dr_db)) = (aggregate.rounded_dr, aggregate.dr_db) {
        output.push_str(&format!(
            "\nTrack aggregate: DR{} ({:.4} dB; {} contributing channels)\nReport levels: peak {} dBFS, RMS {} dBFS\n",
            rounded_dr,
            dr_db.get(),
            aggregate.contributing_channels.len(),
            format_dbfs(report_metrics.primary_peak_dbfs),
            format_dbfs(report_metrics.overall_rms_dbfs),
        ));
    } else {
        output.push_str("\nTrack aggregate: unavailable\n");
    }
    if !report.diagnostics().warnings.is_empty() {
        output.push_str("\nWarnings:\n");
        for warning in &report.diagnostics().warnings {
            output.push_str(&format!("- {warning}\n"));
        }
    }
    output.push_str(&format_elapsed_line(
        elapsed,
        report_metrics.duration.seconds(),
        phases,
        PhaseTimingScope::SingleFile,
    ));
    Ok(output)
}

fn render_batch(
    report: &BatchReport,
    elapsed: Duration,
    phases: Option<PhaseTimings>,
) -> Result<String, AnalysisError> {
    let mut output = String::from("MacinMeter batch\n\n");
    for item in &report.items {
        match &item.outcome {
            BatchItemOutcome::Success { report } => {
                let aggregate = &report.analysis().aggregates().track;
                let aggregate = match (aggregate.rounded_dr, aggregate.dr_db) {
                    (Some(rounded_dr), Some(dr_db)) => {
                        format!("DR{rounded_dr} ({:.4} dB)", dr_db.get())
                    }
                    _ => "unavailable".to_string(),
                };
                output.push_str(&format!("OK   {} — {aggregate}\n", item.display_path));
                for warning in &report.diagnostics().warnings {
                    output.push_str(&format!("     warning: {warning}\n"));
                }
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
    // Only analyzed audio counts toward the multiple. A batch that spent its
    // time rejecting unsupported files really did get through less audio per
    // second, and hiding that would flatter the number.
    let audio_seconds = report
        .items
        .iter()
        .filter_map(|item| match &item.outcome {
            BatchItemOutcome::Success { report } => {
                Some(report.analysis().report().duration.seconds())
            }
            BatchItemOutcome::Failure { .. } => None,
        })
        .sum();
    output.push_str(&format_elapsed_line(
        elapsed,
        audio_seconds,
        phases,
        PhaseTimingScope::Batch,
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

fn completion_output_error(error: io::Error) -> AnalysisError {
    AnalysisError::new(
        ErrorCode::OutputFailed,
        macinmeter::AnalysisStage::Output,
        "failed to write shell completion output",
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
