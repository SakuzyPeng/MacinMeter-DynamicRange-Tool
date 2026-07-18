#![forbid(unsafe_code)]

use serde_json::Value;
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
#[cfg(unix)]
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::Stdio,
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
        .canonicalize()
        .expect("repository fixture should exist")
}

fn run<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(env!("CARGO_BIN_EXE_macinmeter"))
        .args(args)
        .output()
        .expect("CLI process should start")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout should be UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr should be UTF-8")
}

fn assert_code(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "unexpected status\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
}

fn parse_stdout_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout should contain only one JSON document: {error}\nstdout:\n{}\nstderr:\n{}",
            stdout(output),
            stderr(output)
        )
    })
}

#[test]
fn analyze_human_keeps_results_on_stdout_and_progress_on_stderr() {
    let input = fixture("tiny_duration.wav");
    let output = run(["analyze".as_ref(), input.as_os_str()]);

    assert_code(&output, 0);
    let stdout = stdout(&output);
    let stderr = stderr(&output);
    assert!(stdout.starts_with("MacinMeter ProvisionalV1 — UNVERIFIED\n"));
    assert!(stdout.contains("PCM: 44100 Hz, 2 channels, 441 frames"));
    assert!(stdout.contains("Aggregate: DR"));
    assert!(!stdout.contains("[0]"));
    assert!(stderr.contains("[0] analyzing"));
    assert!(stderr.contains("[0] ok:"));
    assert!(!stderr.contains("Aggregate:"));
}

#[test]
fn analyze_json_stdout_is_machine_clean_and_schema_versioned() {
    let input = fixture("tiny_duration.wav");
    let output = run([
        "analyze".as_ref(),
        input.as_os_str(),
        "--format".as_ref(),
        "json".as_ref(),
    ]);

    assert_code(&output, 0);
    let value = parse_stdout_json(&output);
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["toolVersion"], "0.2.0");
    assert_eq!(value["kind"], "analysis");
    assert_eq!(
        value["data"]["analysis"]["algorithm"]["profile"],
        "provisional_v1"
    );
    assert_eq!(
        value["data"]["analysis"]["algorithm"]["compatibility"],
        "unverified"
    );
    assert_eq!(value["data"]["analysis"]["framesSeen"], 441);
    let api_report = macinmeter::Analyzer::new()
        .analyze_file(macinmeter::AnalyzeRequest::new(&input))
        .expect("the same fixture should analyze through the Rust API");
    let api_value = serde_json::to_value(macinmeter::WireEnvelope::analysis(api_report))
        .expect("Rust API report should serialize");
    assert_eq!(
        value, api_value,
        "CLI JSON and the Rust API must expose the exact same core report"
    );
    assert!(!stdout(&output).contains("[0] analyzing"));
    assert!(stderr(&output).contains("[0] analyzing"));
}

#[test]
fn analyze_failure_is_exit_one_and_does_not_pollute_stdout() {
    let input = fixture("fake_audio.wav");
    let output = run(["analyze".as_ref(), input.as_os_str()]);

    assert_code(&output, 1);
    assert!(output.stdout.is_empty());
    let stderr = stderr(&output);
    assert!(stderr.contains("[0] analyzing"));
    assert!(stderr.contains("error [unsupported_format]"));
}

#[test]
fn analyze_json_failure_is_a_structured_error_document() {
    let input = fixture("truncated.wav");
    let output = run([
        "analyze".as_ref(),
        input.as_os_str(),
        "--format".as_ref(),
        "json".as_ref(),
    ]);

    assert_code(&output, 1);
    let value = parse_stdout_json(&output);
    assert_eq!(value["kind"], "error");
    assert_eq!(value["data"]["code"], "malformed_media");
    assert_eq!(value["data"]["stage"], "probe");
    assert!(stderr(&output).contains("[0] analyzing"));
    assert!(!stderr(&output).contains("\"kind\""));
}

#[test]
fn batch_exit_codes_cover_all_partial_and_zero_success() {
    let valid = fixture("tiny_duration.wav");
    let unsupported = fixture("fake_audio.wav");
    let truncated = fixture("truncated.wav");

    let all_success = run([
        "batch".as_ref(),
        valid.as_os_str(),
        "--format".as_ref(),
        "json".as_ref(),
    ]);
    assert_code(&all_success, 0);
    let all_success_json = parse_stdout_json(&all_success);
    assert_eq!(all_success_json["data"]["status"], "succeeded");
    assert_eq!(all_success_json["data"]["summary"]["succeeded"], 1);

    let partial = run([
        "batch".as_ref(),
        unsupported.as_os_str(),
        valid.as_os_str(),
        "--format".as_ref(),
        "json".as_ref(),
    ]);
    assert_code(&partial, 3);
    let partial_json = parse_stdout_json(&partial);
    assert_eq!(partial_json["data"]["status"], "partially_succeeded");
    assert_eq!(partial_json["data"]["summary"]["succeeded"], 1);
    assert_eq!(partial_json["data"]["summary"]["failed"], 1);
    assert_eq!(
        partial_json["data"]["items"][0]["outcome"]["status"],
        "failure"
    );
    assert_eq!(
        partial_json["data"]["items"][1]["outcome"]["status"],
        "success"
    );

    let all_failed = run([
        "batch".as_ref(),
        unsupported.as_os_str(),
        truncated.as_os_str(),
        "--format".as_ref(),
        "json".as_ref(),
    ]);
    assert_code(&all_failed, 1);
    let all_failed_json = parse_stdout_json(&all_failed);
    assert_eq!(all_failed_json["data"]["status"], "failed");
    assert_eq!(all_failed_json["data"]["summary"]["succeeded"], 0);
    assert_eq!(all_failed_json["data"]["summary"]["failed"], 2);
}

#[test]
fn missing_analyze_path_and_implicit_old_style_are_argument_errors() {
    let missing = run(["analyze"]);
    assert_code(&missing, 2);
    assert!(missing.stdout.is_empty());
    assert!(stderr(&missing).contains("required"));

    let implicit_old_style = run([fixture("tiny_duration.wav")]);
    assert_code(&implicit_old_style, 2);
    assert!(implicit_old_style.stdout.is_empty());
}

#[test]
fn batch_with_no_inputs_is_an_operation_failure() {
    let output = run(["batch"]);

    assert_code(&output, 1);
    assert!(output.stdout.is_empty());
    assert!(stderr(&output).contains("error [no_inputs]"));
}

#[cfg(unix)]
#[test]
fn sigint_cancels_an_active_analysis_with_exit_130() {
    let directory = tempfile::tempdir().expect("temporary input directory");
    let input = directory.path().join("long-sparse.wav");
    write_sparse_zero_wave(&input, 256 * 1024 * 1024);

    let mut child = Command::new(env!("CARGO_BIN_EXE_macinmeter"))
        .args(["analyze".as_ref(), input.as_os_str()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("CLI process should start");
    let stderr_pipe = child.stderr.take().expect("stderr should be piped");
    let mut stderr_reader = BufReader::new(stderr_pipe);
    let mut first_line = String::new();
    stderr_reader
        .read_line(&mut first_line)
        .expect("progress line should be readable");
    assert!(first_line.contains("[0] analyzing"));

    let signal_status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("kill command should start");
    assert!(
        signal_status.success(),
        "SIGINT should reach the CLI process"
    );

    let status = child.wait().expect("CLI process should exit");
    let mut remaining_stderr = String::new();
    stderr_reader
        .read_to_string(&mut remaining_stderr)
        .expect("remaining stderr should be readable");
    let mut stdout = Vec::new();
    child
        .stdout
        .take()
        .expect("stdout should be piped")
        .read_to_end(&mut stdout)
        .expect("stdout should be readable");

    assert_eq!(
        status.code(),
        Some(130),
        "stderr:\n{first_line}{remaining_stderr}"
    );
    assert!(stdout.is_empty());
    assert!(remaining_stderr.contains("error [cancelled]"));
}

#[test]
fn explicit_output_is_atomic_and_suppresses_stdout() {
    let directory = tempfile::tempdir().expect("temporary output directory");
    let output_path = directory.path().join("report.json");
    fs::write(&output_path, b"old report").expect("seed existing output");
    let input = fixture("tiny_duration.wav");
    let output = run([
        "analyze".as_ref(),
        input.as_os_str(),
        "--format".as_ref(),
        "json".as_ref(),
        "--output".as_ref(),
        output_path.as_os_str(),
    ]);

    assert_code(&output, 0);
    assert!(output.stdout.is_empty());
    let value: Value =
        serde_json::from_slice(&fs::read(&output_path).expect("output file should exist"))
            .expect("output file should be valid JSON");
    assert_eq!(value["kind"], "analysis");

    let entries = fs::read_dir(directory.path())
        .expect("temporary directory should be readable")
        .collect::<Result<Vec<_>, _>>()
        .expect("directory entries should be readable");
    assert_eq!(entries.len(), 1, "atomic temporary file should be removed");
    assert_eq!(entries[0].path(), output_path);
}

#[test]
fn output_failure_is_exit_one_and_leaves_no_temporary_file() {
    let directory = tempfile::tempdir().expect("temporary output directory");
    let output_path = directory.path().join("missing").join("report.json");
    let input = fixture("tiny_duration.wav");
    let output = run([
        "analyze".as_ref(),
        input.as_os_str(),
        "--output".as_ref(),
        output_path.as_os_str(),
    ]);

    assert_code(&output, 1);
    assert!(output.stdout.is_empty());
    assert!(stderr(&output).contains("error [output_failed]"));
    assert_directory_empty(directory.path());
}

#[test]
fn stdout_mode_does_not_create_an_implicit_report_file() {
    let directory = tempfile::tempdir().expect("temporary working directory");
    let input = fixture("tiny_duration.wav");
    let output = Command::new(env!("CARGO_BIN_EXE_macinmeter"))
        .current_dir(directory.path())
        .args(["analyze".as_ref(), input.as_os_str()])
        .output()
        .expect("CLI process should start");

    assert_code(&output, 0);
    assert!(!output.stdout.is_empty());
    assert_directory_empty(directory.path());
}

fn assert_directory_empty(path: &Path) {
    let mut entries = fs::read_dir(path).expect("temporary directory should be readable");
    assert!(
        entries.next().is_none(),
        "CLI unexpectedly created a file in {}",
        path.display()
    );
}

#[cfg(unix)]
fn write_sparse_zero_wave(path: &Path, data_size: u32) {
    let sample_rate = 44_100_u32;
    let channels = 1_u16;
    let bits_per_sample = 16_u16;
    let block_align = channels * (bits_per_sample / 8);
    let byte_rate = sample_rate * u32::from(block_align);
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36 + data_size).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16_u32.to_le_bytes());
    header.extend_from_slice(&1_u16.to_le_bytes());
    header.extend_from_slice(&channels.to_le_bytes());
    header.extend_from_slice(&sample_rate.to_le_bytes());
    header.extend_from_slice(&byte_rate.to_le_bytes());
    header.extend_from_slice(&block_align.to_le_bytes());
    header.extend_from_slice(&bits_per_sample.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_size.to_le_bytes());

    let mut file = fs::File::create(path).expect("sparse WAV should be created");
    file.write_all(&header)
        .expect("sparse WAV header should be written");
    file.set_len(u64::from(data_size) + 44)
        .expect("sparse WAV length should be set");
}
