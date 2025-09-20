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

    AppConfig {
        input_path,
        verbose: matches.get_flag("verbose"),
        output_path: matches.get_one::<String>("output").map(PathBuf::from),
    }
}

/// 显示程序启动信息
pub fn show_startup_info(config: &AppConfig) {
    println!("🚀 MacinMeter DR Tool (foobar2000兼容版) v{VERSION} 启动");
    println!("📝 {DESCRIPTION}");
    if config.verbose {
        println!("🌿 当前分支: foobar2000-plugin (默认批处理模式)");
    }
    println!();
}

/// 显示程序完成信息
pub fn show_completion_info(config: &AppConfig) {
    if config.verbose {
        println!("✅ 所有任务处理完成！");
    }
}
