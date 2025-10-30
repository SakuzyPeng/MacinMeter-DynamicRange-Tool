//! 命令行接口模块
//!
//! 负责命令行参数解析、配置管理和程序信息展示。

use super::constants;
use super::utils::{effective_parallel_degree, get_parent_dir};
use clap::{Arg, Command};
use std::path::PathBuf;

/// 应用程序版本信息
const VERSION: &str = env!("CARGO_PKG_VERSION");
const DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");
const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");

/// CLI 默认值常量（字符串形式，用于 clap）
/// 注意：这些值必须与 constants::defaults::* 保持同步，通过测试验证
const DEFAULT_PARALLEL_BATCH: &str = "64";
const DEFAULT_PARALLEL_THREADS: &str = "4";
const DEFAULT_PARALLEL_FILES: &str = "4";
const DEFAULT_SILENCE_THRESHOLD_DB_STR: &str = "-70";
const DEFAULT_TRIM_THRESHOLD_DB_STR: &str = "-70";
const DEFAULT_TRIM_MIN_RUN_MS_STR: &str = "300";

/// 自定义范围校验函数
fn parse_parallel_degree(s: &str) -> Result<usize, String> {
    let value: usize = s.parse().map_err(|_| format!("'{s}' 不是有效的数字"))?;
    let min = constants::parallel_limits::MIN_PARALLEL_DEGREE;
    let max = constants::parallel_limits::MAX_PARALLEL_DEGREE;
    if value < min {
        return Err(format!("值必须至少为 {min}"));
    }
    if value > max {
        return Err(format!("值不能超过 {max}"));
    }
    Ok(value)
}

/// 批大小范围校验（1-256）
fn parse_batch_size(s: &str) -> Result<usize, String> {
    let value: usize = s.parse().map_err(|_| format!("'{s}' 不是有效的数字"))?;
    let min = constants::parallel_limits::MIN_PARALLEL_BATCH_SIZE;
    let max = constants::parallel_limits::MAX_PARALLEL_BATCH_SIZE;
    if value < min {
        return Err(format!("批大小必须至少为 {min}"));
    }
    if value > max {
        return Err(format!("批大小不能超过 {max}"));
    }
    Ok(value)
}

/// 静音阈值范围校验（-120dB ~ 0dB）
fn parse_silence_threshold(s: &str) -> Result<f64, String> {
    let value: f64 = s
        .parse()
        .map_err(|_| format!("'{s}' 不是有效的浮点数字（示例：-70）"))?;
    if !(-120.0..=0.0).contains(&value) {
        return Err("静音阈值必须在 -120 到 0 dB 之间".to_string());
    }
    Ok(value)
}

/// 裁切最小持续时间校验（50ms ~ 2000ms）
fn parse_trim_min_run(s: &str) -> Result<f64, String> {
    let value: f64 = s
        .parse()
        .map_err(|_| format!("'{s}' 不是有效的浮点数字（示例：300）"))?;
    if !(50.0..=2000.0).contains(&value) {
        return Err("最小持续时间必须在 50 到 2000 毫秒之间".to_string());
    }
    Ok(value)
}

/// 应用程序配置（简化版 - 遵循零配置优雅性原则）
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// 输入文件路径（单文件模式）或扫描目录（批量模式）
    pub input_path: PathBuf,

    /// 是否显示详细信息
    pub verbose: bool,

    /// 输出文件路径（可选，批量模式时自动生成）
    pub output_path: Option<PathBuf>,

    /// 🚀 并行解码配置 - 攻击解码瓶颈的核心优化
    /// 是否启用并行解码（默认：true）
    pub parallel_decoding: bool,

    /// 并行解码批大小（默认：64包）
    pub parallel_batch_size: usize,

    /// 并行解码线程数（默认：4线程）
    pub parallel_threads: usize,

    /// 🚀 多文件并行配置
    /// - None: 禁用多文件并行（串行处理）
    /// - Some(n): 并发度n（默认：4）
    pub parallel_files: Option<usize>,

    /// 🧪 实验性：静音过滤阈值（存在即启用；单位 dBFS）
    pub silence_filter_threshold_db: Option<f64>,

    /// 🧪 实验性：首尾边缘裁切阈值（存在即启用；单位 dBFS）
    pub edge_trim_threshold_db: Option<f64>,

    /// 🧪 实验性：裁切最小持续时间（毫秒）
    pub edge_trim_min_run_ms: Option<f64>,
}

impl AppConfig {
    /// 智能判断是否为批量模式（基于路径类型）
    #[inline]
    pub fn is_batch_mode(&self) -> bool {
        self.input_path.is_dir()
    }

    /// 固定启用Sum Doubling（foobar2000兼容模式）
    #[inline]
    pub fn sum_doubling_enabled(&self) -> bool {
        true // foobar2000-plugin分支固定启用
    }
}

/// 解析命令行参数并创建配置
pub fn parse_args() -> AppConfig {
    let matches = Command::new(env!("CARGO_PKG_NAME"))
        .version(VERSION)
        .about(DESCRIPTION)
        .author(AUTHORS)
        .arg(
            Arg::new("INPUT")
                .help("音频文件或目录路径 (支持WAV, FLAC, MP3, AAC, OGG)。如果不指定，将扫描可执行文件所在目录")
                .required(false)
                .index(1)
                .value_parser(clap::value_parser!(PathBuf))
                .value_hint(clap::ValueHint::AnyPath),
        )
        .arg(
            Arg::new("verbose")
                .long("verbose")
                .short('v')
                .help("显示详细处理信息")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("output")
                .long("output")
                .short('o')
                .help("输出结果到文件")
                .value_name("FILE")
                .value_parser(clap::value_parser!(PathBuf))
                .value_hint(clap::ValueHint::FilePath),
        )
        .arg(
            Arg::new("serial")
                .long("serial")
                .short('s')
                .help("禁用并行解码，使用串行模式（仅影响单文件解码，与多文件并行无关）")
                .action(clap::ArgAction::SetTrue)
                .conflicts_with_all(["parallel-batch", "parallel-threads"]),
        )
        .arg(
            Arg::new("parallel-batch")
                .long("parallel-batch")
                .help("并行解码批大小 (范围: 1-256)")
                .value_name("SIZE")
                .value_parser(parse_batch_size)
                .default_value(DEFAULT_PARALLEL_BATCH),
        )
        .arg(
            Arg::new("parallel-threads")
                .long("parallel-threads")
                .help("并行解码线程数 (范围: 1-16)")
                .value_name("COUNT")
                .value_parser(parse_parallel_degree)
                .default_value(DEFAULT_PARALLEL_THREADS),
        )
        .arg(
            Arg::new("parallel-files")
                .long("parallel-files")
                .help("并行处理文件数 (范围: 1-16)")
                .value_name("COUNT")
                .value_parser(parse_parallel_degree)
                .default_value(DEFAULT_PARALLEL_FILES),
        )
        .arg(
            Arg::new("no-parallel-files")
                .long("no-parallel-files")
                .help("禁用多文件并行处理（使用串行模式）")
                .action(clap::ArgAction::SetTrue)
                .conflicts_with("parallel-files"),
        )
        .arg(
            Arg::new("filter-silence")
                .long("filter-silence")
                .help("🧪 启用窗口静音过滤；可选指定阈值（dBFS，范围 -120~0，默认 -70）")
                .value_name("DB")
                .num_args(0..=1)
                .require_equals(true)
                .default_missing_value(DEFAULT_SILENCE_THRESHOLD_DB_STR)
                .value_parser(parse_silence_threshold),
        )
        .arg(
            Arg::new("trim-edges")
                .long("trim-edges")
                .help("🧪 P0: 启用首尾样本级静音裁切；可选指定阈值（dBFS，范围 -120~0，默认 -70）")
                .value_name("DB")
                .num_args(0..=1)
                .require_equals(true)
                .default_missing_value(DEFAULT_TRIM_THRESHOLD_DB_STR)
                .value_parser(parse_silence_threshold),
        )
        .arg(
            Arg::new("trim-min-run")
                .long("trim-min-run")
                .help("🧪 P0: 裁切最小持续时间（毫秒，范围 50-2000，默认 300）")
                .value_name("MS")
                .requires("trim-edges")
                .value_parser(parse_trim_min_run)
                .default_value(DEFAULT_TRIM_MIN_RUN_MS_STR),
        )
        .get_matches();

    // 确定输入路径（智能路径处理）
    let input_path = match matches.get_one::<PathBuf>("INPUT") {
        Some(input) => input.clone(),
        None => {
            // 双击启动模式：使用可执行文件所在目录
            let exe_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
            get_parent_dir(&exe_path).to_path_buf()
        }
    };

    // 🚀 并行解码配置逻辑（性能优先策略）
    // ✅ 已验证：SequencedChannel保证样本顺序，DR精度无损
    // 📊 性能提升：3.71倍 (57.47 → 213.19 MB/s, 10次平均测试)
    // 🔥 默认启用并行解码（性能优先，精度保证）
    let parallel_decoding = !matches.get_flag("serial");

    // clap 保证默认值存在，直接 unwrap
    let parallel_batch_size = matches
        .get_one::<usize>("parallel-batch")
        .copied()
        .expect("parallel-batch has default value");

    let parallel_threads = matches
        .get_one::<usize>("parallel-threads")
        .copied()
        .expect("parallel-threads has default value");

    // 🚀 多文件并行配置逻辑
    let parallel_files = if matches.get_flag("no-parallel-files") {
        None // 明确禁用多文件并行
    } else {
        // clap 保证默认值存在，直接 unwrap
        let degree = matches
            .get_one::<usize>("parallel-files")
            .copied()
            .expect("parallel-files has default value");

        // 使用统一的并发度计算工具函数（限制范围：1-16）
        // 注意：虽然 parse_parallel_degree 已验证范围，但 effective_parallel_degree
        // 还会进一步规范化（处理 CPU 核心数等），这是双重保险
        Some(effective_parallel_degree(degree, None))
    };

    // 🧪 P0阶段：首尾边缘裁切配置
    let edge_trim_threshold_db = matches.get_one::<f64>("trim-edges").copied();
    let edge_trim_min_run_ms = if edge_trim_threshold_db.is_some() {
        // trim-edges启用时，解析trim-min-run（有默认值）
        matches.get_one::<f64>("trim-min-run").copied()
    } else {
        None // trim-edges未启用，忽略trim-min-run
    };

    AppConfig {
        input_path,
        verbose: matches.get_flag("verbose"),
        output_path: matches.get_one::<PathBuf>("output").cloned(),
        parallel_decoding,
        parallel_batch_size,
        parallel_threads,
        parallel_files,
        silence_filter_threshold_db: matches.get_one::<f64>("filter-silence").copied(),
        edge_trim_threshold_db,
        edge_trim_min_run_ms,
    }
}

/// 显示程序启动信息
pub fn show_startup_info(config: &AppConfig) {
    println!(
        "🚀 {} {} v{VERSION} 启动",
        constants::app_info::APP_NAME,
        constants::app_info::VERSION_SUFFIX
    );
    println!("📝 {DESCRIPTION}");
    if config.verbose {
        println!("🌿 当前分支: {}", constants::app_info::BRANCH_INFO);
        if config.parallel_decoding {
            println!(
                "⚡ 并行解码: 启用 ({}线程, {}包批量) - 预期3-5倍性能提升",
                config.parallel_threads, config.parallel_batch_size
            );
        } else {
            println!("⚡ 并行解码: 禁用 (串行模式)");
        }

        // 多文件并行配置
        if let Some(degree) = config.parallel_files {
            println!("🔥 多文件并行: 启用 ({degree}并发度) - 预期2-16倍加速");
        } else {
            println!("🔥 多文件并行: 禁用 (串行处理)");
        }
    }
    println!();
}

/// 显示程序完成信息
pub fn show_completion_info(config: &AppConfig) {
    if config.verbose {
        println!("✅ 所有任务处理完成！");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证 CLI 字符串常量与 constants::defaults 保持同步
    ///
    /// 这个测试确保 DEFAULT_* 字符串常量（用于 clap 帮助文本）
    /// 与 constants::defaults::* 数值常量（实际运行时使用）保持一致。
    /// 如果手动修改了任何一方，编译时测试会失败，防止漂移。
    #[test]
    fn test_cli_defaults_match_constants() {
        assert_eq!(
            DEFAULT_PARALLEL_BATCH.parse::<usize>().unwrap(),
            constants::defaults::PARALLEL_BATCH_SIZE,
            "DEFAULT_PARALLEL_BATCH 必须与 constants::defaults::PARALLEL_BATCH_SIZE 同步"
        );

        assert_eq!(
            DEFAULT_PARALLEL_THREADS.parse::<usize>().unwrap(),
            constants::defaults::PARALLEL_THREADS,
            "DEFAULT_PARALLEL_THREADS 必须与 constants::defaults::PARALLEL_THREADS 同步"
        );

        assert_eq!(
            DEFAULT_PARALLEL_FILES.parse::<usize>().unwrap(),
            constants::defaults::PARALLEL_FILES_DEGREE,
            "DEFAULT_PARALLEL_FILES 必须与 constants::defaults::PARALLEL_FILES_DEGREE 同步"
        );

        let default_threshold = DEFAULT_SILENCE_THRESHOLD_DB_STR
            .parse::<f64>()
            .expect("DEFAULT_SILENCE_THRESHOLD_DB_STR 应该是有效浮点数");
        assert!(
            (-120.0..=0.0).contains(&default_threshold),
            "DEFAULT_SILENCE_THRESHOLD_DB 必须在 -120 到 0 dB 范围内"
        );
    }

    /// 验证自定义范围校验函数的正确性
    #[test]
    fn test_parse_parallel_degree_valid() {
        assert_eq!(parse_parallel_degree("1").unwrap(), 1);
        assert_eq!(parse_parallel_degree("4").unwrap(), 4);
        assert_eq!(parse_parallel_degree("16").unwrap(), 16);
    }

    #[test]
    fn test_parse_parallel_degree_invalid() {
        assert!(parse_parallel_degree("0").is_err());
        assert!(parse_parallel_degree("17").is_err());
        assert!(parse_parallel_degree("abc").is_err());
    }

    #[test]
    fn test_parse_batch_size_valid() {
        assert_eq!(parse_batch_size("1").unwrap(), 1);
        assert_eq!(parse_batch_size("64").unwrap(), 64);
        assert_eq!(parse_batch_size("256").unwrap(), 256);
    }

    #[test]
    fn test_parse_batch_size_invalid() {
        assert!(parse_batch_size("0").is_err());
        assert!(parse_batch_size("257").is_err());
        assert!(parse_batch_size("xyz").is_err());
    }
}
