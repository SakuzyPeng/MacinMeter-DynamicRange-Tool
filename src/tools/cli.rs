//! 命令行接口模块
//!
//! 负责命令行参数解析、配置管理和程序信息展示。

use clap::{Arg, Command};
use std::path::PathBuf;

/// 应用程序版本信息
const VERSION: &str = env!("CARGO_PKG_VERSION");
const DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");

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
    let matches = Command::new("dr-meter")
        .version(VERSION)
        .about(DESCRIPTION)
        .author("MacinMeter Team")
        .arg(
            Arg::new("INPUT")
                .help("音频文件或目录路径 (支持WAV, FLAC, MP3, AAC, OGG)。如果不指定，将扫描可执行文件所在目录")
                .required(false)
                .index(1),
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
                .value_name("FILE"),
        )
        .arg(
            Arg::new("parallel")
                .long("parallel")
                .short('p')
                .help("启用并行解码（默认：启用）")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("parallel")
                .long("parallel")
                .help("⚠️ 实验性：启用并行解码（可能影响DR精度，默认禁用）")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("parallel-batch")
                .long("parallel-batch")
                .help("并行解码批大小（默认：64）")
                .value_name("SIZE")
                .value_parser(clap::value_parser!(usize)),
        )
        .arg(
            Arg::new("parallel-threads")
                .long("parallel-threads")
                .help("并行解码线程数（默认：4）")
                .value_name("COUNT")
                .value_parser(clap::value_parser!(usize)),
        )
        .arg(
            Arg::new("parallel-files")
                .long("parallel-files")
                .help("并行处理文件数（1-16，默认：4）")
                .value_name("COUNT")
                .value_parser(clap::value_parser!(usize)),
        )
        .arg(
            Arg::new("no-parallel-files")
                .long("no-parallel-files")
                .help("禁用多文件并行处理（使用串行模式）")
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches();

    // 确定输入路径（智能路径处理）
    let input_path = match matches.get_one::<String>("INPUT") {
        Some(input) => PathBuf::from(input),
        None => {
            // 双击启动模式：使用可执行文件所在目录
            let exe_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
            super::utils::get_parent_dir(&exe_path).to_path_buf()
        }
    };

    // 🚀 并行解码配置逻辑
    // ⚠️ HOTFIX: 并行解码存在DR计算精度问题，临时默认禁用
    // TODO: 修复并行解码器的样本顺序问题 (Issue #TBD)
    let parallel_decoding = if matches.get_flag("parallel") {
        true // 明确启用并行解码（实验性）
    } else {
        false // 默认禁用并行解码（精度优先）
    };

    let parallel_batch_size = matches
        .get_one::<usize>("parallel-batch")
        .copied()
        .unwrap_or(64); // 默认64包批量

    let parallel_threads = matches
        .get_one::<usize>("parallel-threads")
        .copied()
        .unwrap_or(4); // 默认4线程

    // 🚀 多文件并行配置逻辑
    let parallel_files = if matches.get_flag("no-parallel-files") {
        None // 明确禁用多文件并行
    } else {
        let degree = matches
            .get_one::<usize>("parallel-files")
            .copied()
            .unwrap_or(4); // 默认4并发度

        // 限制并发度范围：1-16
        Some(degree.clamp(1, 16))
    };

    AppConfig {
        input_path,
        verbose: matches.get_flag("verbose"),
        output_path: matches.get_one::<String>("output").map(PathBuf::from),
        parallel_decoding,
        parallel_batch_size,
        parallel_threads,
        parallel_files,
    }
}

/// 显示程序启动信息
pub fn show_startup_info(config: &AppConfig) {
    println!("🚀 MacinMeter DR Tool (foobar2000兼容版) v{VERSION} 启动");
    println!("📝 {DESCRIPTION}");
    if config.verbose {
        println!("🌿 当前分支: foobar2000-plugin (默认批处理模式)");
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
