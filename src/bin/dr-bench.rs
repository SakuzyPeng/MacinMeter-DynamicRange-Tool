//! dr-bench - 跨平台性能基准工具
//!
//! 统一替代 macOS bash 和 Windows PowerShell benchmark 脚本
//! 支持无参数自动运行、Markdown 输出、A/B 对比

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::Local;
use clap::{Parser, Subcommand};
use comfy_table::{Cell, CellAlignment, ContentArrangement, Table, presets::UTF8_FULL};
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, System};
use walkdir::WalkDir;

// ============================================================================
// 常量定义
// ============================================================================

// 默认采样间隔（毫秒）
const DEFAULT_SAMPLE_INTERVAL_MS: u64 = 100;

// 默认运行次数
const DEFAULT_RUNS: usize = 10;

// 可执行文件名（按优先级）
const EXECUTABLE_NAMES: &[&str] = &[
    "MacinMeter-DynamicRange-Tool-foo_dr",
    "MacinMeter-DynamicRange-Tool-foo_dr.exe",
];

// 测试数据目录（按优先级）
const TEST_DATA_DIRS: &[&str] = &["audio", "scripts", "test-data"];

// 支持的音频扩展名
const AUDIO_EXTENSIONS: &[&str] = &[
    "flac", "wav", "mp3", "m4a", "aac", "ogg", "opus", "aiff", "aif", "dsf", "dff", "wv", "ape",
];

// ============================================================================
// CLI 定义
// ============================================================================

#[derive(Parser)]
#[command(name = "dr-bench")]
#[command(about = "跨平台性能基准工具 / Cross-platform benchmark tool")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// 可执行文件路径（默认自动发现）
    /// Executable path (auto-discovered by default)
    #[arg(long, short = 'e')]
    exe: Option<PathBuf>,

    /// 测试数据路径（默认自动发现）
    /// Test data path (auto-discovered by default)
    #[arg(long, short = 'p')]
    path: Option<PathBuf>,

    /// 运行次数（默认10）
    /// Number of runs (default: 10)
    #[arg(long, short = 'n', default_value_t = DEFAULT_RUNS)]
    runs: usize,

    /// 额外参数传递给被测程序
    /// Extra arguments to pass to the target executable
    #[arg(long, short = 'a')]
    args: Option<String>,

    /// 采样间隔（毫秒，默认100）
    /// Sampling interval in ms (default: 100)
    #[arg(long, default_value_t = DEFAULT_SAMPLE_INTERVAL_MS)]
    sample_interval: u64,

    /// 输出格式：markdown, json, table（默认markdown）
    /// Output format: markdown, json, table (default: markdown)
    #[arg(long, short = 'f', default_value = "markdown")]
    format: OutputFormat,
}

#[derive(Subcommand)]
enum Commands {
    /// A/B 对比两个版本
    /// Compare two versions A/B
    Compare {
        /// 基准版本可执行文件
        /// Baseline executable
        #[arg(long, short = 'b')]
        baseline: PathBuf,

        /// 候选版本可执行文件
        /// Candidate executable
        #[arg(long, short = 'c')]
        candidate: PathBuf,

        /// 测试数据路径
        /// Test data path
        #[arg(long, short = 'p')]
        path: Option<PathBuf>,

        /// 运行次数
        /// Number of runs
        #[arg(long, short = 'n', default_value_t = DEFAULT_RUNS)]
        runs: usize,

        /// 额外参数
        /// Extra arguments
        #[arg(long, short = 'a')]
        args: Option<String>,

        /// 输出格式
        /// Output format
        #[arg(long, short = 'f', default_value = "markdown")]
        format: OutputFormat,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum OutputFormat {
    #[default]
    Markdown,
    Json,
    Table,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "markdown" | "md" => Ok(OutputFormat::Markdown),
            "json" => Ok(OutputFormat::Json),
            "table" => Ok(OutputFormat::Table),
            _ => Err(format!("Unknown format: {s}")),
        }
    }
}

// ============================================================================
// 数据结构
// ============================================================================

/// 单次采样结果
#[derive(Clone, Debug)]
struct Sample {
    memory_kb: u64,
    cpu_percent: f32,
}

/// 单次运行结果
#[derive(Clone, Debug, Serialize, Deserialize)]
struct RunResult {
    elapsed_ms: f64,
    peak_memory_kb: u64,
    avg_memory_kb: u64,
    peak_cpu_percent: f64,
    avg_cpu_percent: f64,
    throughput_mb_per_sec: f64,
}

/// 统计结果
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Statistics {
    median: f64,
    average: f64,
    stddev: f64,
    min: f64,
    max: f64,
}

/// 完整报告
#[derive(Clone, Debug, Serialize, Deserialize)]
struct BenchmarkReport {
    executable: String,
    target: String,
    file_count: usize,
    total_size_mb: f64,
    runs: usize,
    timestamp: String,
    time: Statistics,
    peak_memory_mb: Statistics,
    avg_memory_mb: Statistics,
    throughput: Statistics,
    cpu_peak: Statistics,
    cpu_avg: Statistics,
}

/// A/B 对比报告
#[derive(Clone, Debug, Serialize, Deserialize)]
struct CompareReport {
    baseline: BenchmarkReport,
    candidate: BenchmarkReport,
    timestamp: String,
}

// ============================================================================
// 自动发现
// ============================================================================

/// 查找项目根目录
fn find_project_root() -> Option<PathBuf> {
    let mut current = env::current_dir().ok()?;
    loop {
        if current.join("Cargo.toml").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// 自动发现可执行文件
fn auto_discover_executable() -> Option<PathBuf> {
    let root = find_project_root()?;

    // 优先查找 target/release
    for name in EXECUTABLE_NAMES {
        let path = root.join("target/release").join(name);
        if path.exists() && path.is_file() {
            return Some(path);
        }
    }

    // 其次查找 target/debug
    for name in EXECUTABLE_NAMES {
        let path = root.join("target/debug").join(name);
        if path.exists() && path.is_file() {
            return Some(path);
        }
    }

    None
}

/// 自动发现测试数据目录
fn auto_discover_test_data() -> Option<PathBuf> {
    let root = find_project_root()?;

    for dir in TEST_DATA_DIRS {
        let path = root.join(dir);
        if path.exists() && path.is_dir() {
            // 检查是否包含音频文件
            if has_audio_files(&path) {
                return Some(path);
            }
        }
    }

    None
}

/// 检查目录是否包含音频文件
fn has_audio_files(dir: &Path) -> bool {
    WalkDir::new(dir)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
        .any(|e| {
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
                .unwrap_or(false)
        })
}

/// 统计目录中的音频文件
fn scan_audio_files(dir: &Path) -> (usize, u64) {
    let mut count = 0;
    let mut total_bytes = 0u64;

    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let ext_match = entry
            .path()
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
            .unwrap_or(false);

        if ext_match {
            count += 1;
            if let Ok(meta) = entry.metadata() {
                total_bytes += meta.len();
            }
        }
    }

    (count, total_bytes)
}

// ============================================================================
// 采样器
// ============================================================================

/// 在后台采样进程资源使用
fn sample_process(pid: u32, stop: Arc<AtomicBool>, interval_ms: u64) -> Vec<Sample> {
    let mut samples = Vec::new();
    let mut system = System::new();
    let pid = Pid::from_u32(pid);
    let interval = Duration::from_millis(interval_ms);

    // 首次刷新以建立基线
    system.refresh_process(pid);
    thread::sleep(interval);

    while !stop.load(Ordering::Relaxed) {
        system.refresh_process(pid);
        if let Some(process) = system.process(pid) {
            samples.push(Sample {
                memory_kb: process.memory() / 1024,
                cpu_percent: process.cpu_usage(),
            });
        } else {
            // 进程已退出
            break;
        }
        thread::sleep(interval);
    }

    samples
}

// ============================================================================
// 执行引擎
// ============================================================================

/// 运行单次测试
fn run_single(
    exe: &Path,
    target: &Path,
    extra_args: &Option<String>,
    sample_interval: u64,
) -> Result<RunResult> {
    let (_file_count, total_bytes) = if target.is_dir() {
        scan_audio_files(target)
    } else {
        let size = fs::metadata(target).map(|m| m.len()).unwrap_or(0);
        (1, size)
    };

    let total_mb = total_bytes as f64 / (1024.0 * 1024.0);

    // 构建命令
    let mut cmd = Command::new(exe);
    cmd.arg(target);
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());

    // 添加额外参数
    if let Some(args) = extra_args {
        for arg in args.split_whitespace() {
            cmd.arg(arg);
        }
    }

    // 启动进程
    let start = Instant::now();
    let mut child: Child = cmd
        .spawn()
        .context("Failed to spawn process / 无法启动进程")?;

    let pid = child.id();

    // 启动采样线程
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    let sampler_handle = thread::spawn(move || sample_process(pid, stop_clone, sample_interval));

    // 等待进程完成
    let status = child
        .wait()
        .context("Failed to wait for process / 等待进程失败")?;
    let elapsed = start.elapsed();

    // 停止采样
    stop.store(true, Ordering::Relaxed);
    let samples = sampler_handle.join().expect("Sampler thread panicked");

    if !status.success() {
        anyhow::bail!("Process exited with non-zero status: {status} / 进程退出状态非零: {status}");
    }

    // 计算统计
    let elapsed_ms = elapsed.as_secs_f64() * 1000.0;

    let (peak_memory_kb, avg_memory_kb) = if samples.is_empty() {
        (0, 0)
    } else {
        let peak = samples.iter().map(|s| s.memory_kb).max().unwrap_or(0);
        let avg = samples.iter().map(|s| s.memory_kb).sum::<u64>() / samples.len() as u64;
        (peak, avg)
    };

    let (peak_cpu, avg_cpu) = if samples.is_empty() {
        (0.0, 0.0)
    } else {
        let peak = samples.iter().map(|s| s.cpu_percent).fold(0.0f32, f32::max);
        let avg = samples.iter().map(|s| s.cpu_percent).sum::<f32>() / samples.len() as f32;
        (peak as f64, avg as f64)
    };

    let throughput = if elapsed.as_secs_f64() > 0.0 {
        total_mb / elapsed.as_secs_f64()
    } else {
        0.0
    };

    Ok(RunResult {
        elapsed_ms,
        peak_memory_kb,
        avg_memory_kb,
        peak_cpu_percent: peak_cpu,
        avg_cpu_percent: avg_cpu,
        throughput_mb_per_sec: throughput,
    })
}

/// 运行多次测试
fn run_multiple(
    exe: &Path,
    target: &Path,
    runs: usize,
    extra_args: &Option<String>,
    sample_interval: u64,
) -> Result<Vec<RunResult>> {
    let mut results = Vec::with_capacity(runs);

    for i in 1..=runs {
        eprint!("\r  运行 / Run {i}/{runs}...");
        let result = run_single(exe, target, extra_args, sample_interval)?;
        results.push(result);
    }
    eprintln!();

    Ok(results)
}

// ============================================================================
// 统计计算
// ============================================================================

/// 计算统计值
fn calculate_stats(values: &[f64]) -> Statistics {
    if values.is_empty() {
        return Statistics {
            median: 0.0,
            average: 0.0,
            stddev: 0.0,
            min: 0.0,
            max: 0.0,
        };
    }

    let n = values.len() as f64;
    let average = values.iter().sum::<f64>() / n;

    // 样本标准差 (n-1)
    let variance = if values.len() > 1 {
        values.iter().map(|v| (v - average).powi(2)).sum::<f64>() / (n - 1.0)
    } else {
        0.0
    };
    let stddev = variance.sqrt();

    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let median = if sorted.len() % 2 == 0 {
        let mid = sorted.len() / 2;
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[sorted.len() / 2]
    };

    let min = sorted.first().copied().unwrap_or(0.0);
    let max = sorted.last().copied().unwrap_or(0.0);

    Statistics {
        median,
        average,
        stddev,
        min,
        max,
    }
}

/// 生成报告
fn generate_report(exe: &Path, target: &Path, results: &[RunResult]) -> BenchmarkReport {
    let (file_count, total_bytes) = if target.is_dir() {
        scan_audio_files(target)
    } else {
        let size = fs::metadata(target).map(|m| m.len()).unwrap_or(0);
        (1, size)
    };
    let total_mb = total_bytes as f64 / (1024.0 * 1024.0);

    let time_values: Vec<f64> = results.iter().map(|r| r.elapsed_ms / 1000.0).collect();
    let peak_mem_values: Vec<f64> = results
        .iter()
        .map(|r| r.peak_memory_kb as f64 / 1024.0)
        .collect();
    let avg_mem_values: Vec<f64> = results
        .iter()
        .map(|r| r.avg_memory_kb as f64 / 1024.0)
        .collect();
    let throughput_values: Vec<f64> = results.iter().map(|r| r.throughput_mb_per_sec).collect();
    let cpu_peak_values: Vec<f64> = results.iter().map(|r| r.peak_cpu_percent).collect();
    let cpu_avg_values: Vec<f64> = results.iter().map(|r| r.avg_cpu_percent).collect();

    BenchmarkReport {
        executable: exe
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        target: target.to_string_lossy().to_string(),
        file_count,
        total_size_mb: total_mb,
        runs: results.len(),
        timestamp: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        time: calculate_stats(&time_values),
        peak_memory_mb: calculate_stats(&peak_mem_values),
        avg_memory_mb: calculate_stats(&avg_mem_values),
        throughput: calculate_stats(&throughput_values),
        cpu_peak: calculate_stats(&cpu_peak_values),
        cpu_avg: calculate_stats(&cpu_avg_values),
    }
}

// ============================================================================
// 输出格式化
// ============================================================================

/// 输出 Markdown 格式
fn output_markdown(report: &BenchmarkReport) {
    println!("## Benchmark Report / 性能基准报告\n");
    println!("- **Executable / 可执行文件**: {}", report.executable);
    println!(
        "- **Target / 目标**: {} files, {:.2} MB",
        report.file_count, report.total_size_mb
    );
    println!("- **Runs / 运行次数**: {}", report.runs);
    println!("- **Timestamp / 时间戳**: {}\n", report.timestamp);

    println!("### Results / 结果\n");

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        "Metric / 指标",
        "Median / 中位数",
        "Average / 平均值",
        "StdDev / 标准差",
        "Min / 最小",
        "Max / 最大",
    ]);

    // 添加数据行
    add_stats_row(&mut table, "Time (s) / 时间", &report.time, 3);
    add_stats_row(
        &mut table,
        "Peak Memory (MB) / 峰值内存",
        &report.peak_memory_mb,
        2,
    );
    add_stats_row(
        &mut table,
        "Avg Memory (MB) / 平均内存",
        &report.avg_memory_mb,
        2,
    );
    add_stats_row(
        &mut table,
        "Throughput (MB/s) / 吞吐量",
        &report.throughput,
        2,
    );
    add_stats_row(&mut table, "CPU Peak (%) / CPU峰值", &report.cpu_peak, 2);
    add_stats_row(&mut table, "CPU Avg (%) / CPU平均", &report.cpu_avg, 2);

    println!("{table}");
}

/// 输出 JSON 格式
fn output_json(report: &BenchmarkReport) {
    println!(
        "{}",
        serde_json::to_string_pretty(report).unwrap_or_default()
    );
}

/// 输出终端表格格式
fn output_table(report: &BenchmarkReport) {
    println!("Benchmark Report / 性能基准报告");
    println!("================================");
    println!("Executable / 可执行文件: {}", report.executable);
    println!(
        "Target / 目标: {} files, {:.2} MB",
        report.file_count, report.total_size_mb
    );
    println!("Runs / 运行次数: {}", report.runs);
    println!("Timestamp / 时间戳: {}\n", report.timestamp);

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec!["Metric", "Median", "Average", "StdDev", "Min", "Max"]);

    add_stats_row(&mut table, "Time (s)", &report.time, 3);
    add_stats_row(&mut table, "Peak Mem (MB)", &report.peak_memory_mb, 2);
    add_stats_row(&mut table, "Avg Mem (MB)", &report.avg_memory_mb, 2);
    add_stats_row(&mut table, "Throughput", &report.throughput, 2);
    add_stats_row(&mut table, "CPU Peak (%)", &report.cpu_peak, 2);
    add_stats_row(&mut table, "CPU Avg (%)", &report.cpu_avg, 2);

    println!("{table}");
}

/// 添加统计行到表格
fn add_stats_row(table: &mut Table, name: &str, stats: &Statistics, precision: usize) {
    table.add_row(vec![
        Cell::new(name),
        Cell::new(format!("{:.prec$}", stats.median, prec = precision))
            .set_alignment(CellAlignment::Right),
        Cell::new(format!("{:.prec$}", stats.average, prec = precision))
            .set_alignment(CellAlignment::Right),
        Cell::new(format!("{:.prec$}", stats.stddev, prec = precision))
            .set_alignment(CellAlignment::Right),
        Cell::new(format!("{:.prec$}", stats.min, prec = precision))
            .set_alignment(CellAlignment::Right),
        Cell::new(format!("{:.prec$}", stats.max, prec = precision))
            .set_alignment(CellAlignment::Right),
    ]);
}

/// 输出 A/B 对比报告 (Markdown)
fn output_compare_markdown(compare: &CompareReport) {
    println!("## A/B Comparison Report / A/B 对比报告\n");
    println!("- **Timestamp / 时间戳**: {}", compare.timestamp);
    println!(
        "- **Target / 目标**: {} files, {:.2} MB",
        compare.baseline.file_count, compare.baseline.total_size_mb
    );
    println!("- **Runs / 运行次数**: {}\n", compare.baseline.runs);

    println!("### Executables / 可执行文件\n");
    println!("- **Baseline / 基准**: {}", compare.baseline.executable);
    println!("- **Candidate / 候选**: {}\n", compare.candidate.executable);

    println!("### Comparison / 对比\n");

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        "Metric / 指标",
        "Baseline / 基准",
        "Candidate / 候选",
        "Delta / 差值",
        "Delta % / 百分比",
    ]);

    add_compare_row(
        &mut table,
        "Time (s) / 时间",
        compare.baseline.time.median,
        compare.candidate.time.median,
        3,
        true,
    );
    add_compare_row(
        &mut table,
        "Peak Memory (MB) / 峰值内存",
        compare.baseline.peak_memory_mb.median,
        compare.candidate.peak_memory_mb.median,
        2,
        true,
    );
    add_compare_row(
        &mut table,
        "Throughput (MB/s) / 吞吐量",
        compare.baseline.throughput.median,
        compare.candidate.throughput.median,
        2,
        false, // 越大越好
    );
    add_compare_row(
        &mut table,
        "CPU Avg (%) / CPU平均",
        compare.baseline.cpu_avg.median,
        compare.candidate.cpu_avg.median,
        2,
        true,
    );

    println!("{table}");
}

/// 添加对比行
fn add_compare_row(
    table: &mut Table,
    name: &str,
    baseline: f64,
    candidate: f64,
    precision: usize,
    lower_is_better: bool,
) {
    let delta = candidate - baseline;
    let delta_pct = if baseline != 0.0 {
        (delta / baseline) * 100.0
    } else {
        0.0
    };

    // 判断是否改进
    let improved = if lower_is_better {
        delta < 0.0
    } else {
        delta > 0.0
    };

    let delta_str = format!("{delta:+.precision$}");
    let delta_pct_str = if improved {
        format!("**{delta_pct:+.1}%**")
    } else {
        format!("{delta_pct:+.1}%")
    };

    table.add_row(vec![
        Cell::new(name),
        Cell::new(format!("{baseline:.precision$}")).set_alignment(CellAlignment::Right),
        Cell::new(format!("{candidate:.precision$}")).set_alignment(CellAlignment::Right),
        Cell::new(delta_str).set_alignment(CellAlignment::Right),
        Cell::new(delta_pct_str).set_alignment(CellAlignment::Right),
    ]);
}

/// 输出 A/B 对比 JSON
fn output_compare_json(compare: &CompareReport) {
    println!(
        "{}",
        serde_json::to_string_pretty(compare).unwrap_or_default()
    );
}

// ============================================================================
// 主函数
// ============================================================================

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Compare {
            baseline,
            candidate,
            path,
            runs,
            args,
            format,
        }) => {
            // A/B 对比模式
            let target = path
                .or_else(auto_discover_test_data)
                .context("No test data found / 未找到测试数据，请用 -p 指定")?;

            eprintln!(
                "Comparing / 对比:\n  Baseline: {}\n  Candidate: {}\n  Target: {}\n",
                baseline.display(),
                candidate.display(),
                target.display()
            );

            // 运行基准版本
            eprintln!("Running baseline / 运行基准版本...");
            let baseline_results =
                run_multiple(&baseline, &target, runs, &args, cli.sample_interval)?;
            let baseline_report = generate_report(&baseline, &target, &baseline_results);

            // 运行候选版本
            eprintln!("Running candidate / 运行候选版本...");
            let candidate_results =
                run_multiple(&candidate, &target, runs, &args, cli.sample_interval)?;
            let candidate_report = generate_report(&candidate, &target, &candidate_results);

            let compare_report = CompareReport {
                baseline: baseline_report,
                candidate: candidate_report,
                timestamp: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            };

            match format {
                OutputFormat::Markdown => output_compare_markdown(&compare_report),
                OutputFormat::Json => output_compare_json(&compare_report),
                OutputFormat::Table => output_compare_markdown(&compare_report), // 复用 Markdown
            }
        }
        None => {
            // 默认模式
            let exe = cli
                .exe
                .or_else(auto_discover_executable)
                .context("No executable found / 未找到可执行文件，请用 -e 指定或先运行 cargo build --release")?;

            let target = cli
                .path
                .or_else(auto_discover_test_data)
                .context("No test data found / 未找到测试数据，请用 -p 指定")?;

            eprintln!(
                "Benchmarking / 基准测试:\n  Executable: {}\n  Target: {}\n  Runs: {}\n",
                exe.display(),
                target.display(),
                cli.runs
            );

            let results = run_multiple(&exe, &target, cli.runs, &cli.args, cli.sample_interval)?;
            let report = generate_report(&exe, &target, &results);

            match cli.format {
                OutputFormat::Markdown => output_markdown(&report),
                OutputFormat::Json => output_json(&report),
                OutputFormat::Table => output_table(&report),
            }
        }
    }

    Ok(())
}
