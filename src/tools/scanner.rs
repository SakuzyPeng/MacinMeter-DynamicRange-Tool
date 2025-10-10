//! 文件扫描模块
//!
//! 负责扫描目录中的音频文件，支持多种音频格式。

use super::cli::AppConfig;
use super::utils;
use crate::{AudioError, AudioResult};
use std::path::PathBuf;

/// 获取支持的音频格式扩展名
///
/// 🚀 从UniversalDecoder获取统一的格式支持声明，确保一致性
fn get_supported_extensions() -> &'static [&'static str] {
    use crate::audio::UniversalDecoder;
    let decoder = UniversalDecoder::new();
    decoder.supported_formats().extensions
}

/// 扫描目录中的音频文件
pub fn scan_audio_files(dir_path: &std::path::Path) -> AudioResult<Vec<PathBuf>> {
    let mut audio_files = Vec::new();

    if !dir_path.exists() {
        return Err(AudioError::IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("目录不存在: {}", dir_path.display()),
        )));
    }

    if !dir_path.is_dir() {
        return Err(AudioError::InvalidInput(format!(
            "路径不是目录: {}",
            dir_path.display()
        )));
    }

    // 遍历目录（不递归子目录）
    let entries = std::fs::read_dir(dir_path).map_err(AudioError::IoError)?;

    // 仅获取一次受支持扩展名，避免循环内重复创建解码器
    let supported_exts = get_supported_extensions();

    for entry in entries {
        let entry = entry.map_err(AudioError::IoError)?;
        let path = entry.path();

        // 只处理文件，跳过目录
        if !path.is_file() {
            continue;
        }

        // 检查文件扩展名
        if let Some(extension) = path.extension()
            && let Some(ext_str) = extension.to_str()
        {
            let ext_lower = ext_str.to_lowercase();
            if supported_exts.contains(&ext_lower.as_str()) {
                audio_files.push(path);
            }
        }
    }

    // 按文件名排序
    audio_files.sort();

    Ok(audio_files)
}

/// 显示文件扫描结果
pub fn show_scan_results(config: &AppConfig, audio_files: &[PathBuf]) {
    if audio_files.is_empty() {
        println!(
            "⚠️  在目录 {} 中没有找到支持的音频文件",
            config.input_path.display()
        );
        let mut supported_formats: Vec<String> = get_supported_extensions()
            .iter()
            .map(|ext| ext.to_uppercase())
            .collect();
        supported_formats.sort();
        let supported_formats = supported_formats.join(", ");
        println!("   支持的格式: {supported_formats}");
        return;
    }

    println!("📁 扫描目录: {}", config.input_path.display());
    println!("🎵 找到 {} 个音频文件", audio_files.len());

    if config.verbose {
        for (i, file) in audio_files.iter().enumerate() {
            println!("   {}. {}", i + 1, utils::extract_filename_lossy(file));
        }
    }
    println!();
}

/// 生成批量输出的头部信息
pub fn create_batch_output_header(config: &AppConfig, audio_files: &[PathBuf]) -> String {
    use super::constants::app_info;
    let mut batch_output = String::new();

    batch_output.push_str("=====================================\n");
    // 与测试用例保持兼容：该行需保持固定文案
    batch_output.push_str("   MacinMeter DR Analysis Report\n");
    batch_output.push_str(&format!("   批量分析结果 {}\n", app_info::VERSION_SUFFIX));
    batch_output.push_str("=====================================\n\n");

    // 添加标准信息到输出（使用共享常量）
    batch_output.push_str(&format!("Git分支: {}\n", app_info::BRANCH_INFO));
    batch_output.push_str(&format!("{}\n", app_info::BASE_DESCRIPTION));
    batch_output.push_str(&format!("{}\n", app_info::CALCULATION_MODE));
    batch_output.push_str(&format!("扫描目录: {}\n", config.input_path.display()));
    batch_output.push_str(&format!("处理文件数: {}\n\n", audio_files.len()));

    // 🎯 添加结果表头（使用固定宽度确保对齐）
    batch_output.push_str("Official DR      Precise DR       文件名\n");
    batch_output.push_str(
        "================================================================================\n",
    );

    batch_output
}

/// 生成批量输出的统计信息
pub fn create_batch_output_footer(
    audio_files: &[PathBuf],
    processed_count: usize,
    failed_count: usize,
    error_stats: &std::collections::HashMap<crate::error::ErrorCategory, Vec<String>>,
) -> String {
    use super::constants::app_info;
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    let mut output = String::new();

    // 添加统计信息
    output.push('\n');
    output.push_str("=====================================\n");
    output.push_str("批量处理统计:\n");
    output.push_str(&format!("   总文件数: {}\n", audio_files.len()));
    output.push_str(&format!("   成功处理: {processed_count}\n"));
    output.push_str(&format!("   处理失败: {failed_count}\n"));
    output.push_str(&format!(
        "   处理成功率: {:.1}%\n",
        processed_count as f64 / audio_files.len() as f64 * 100.0
    ));

    // 🎯 错误分类统计（仅在有失败时显示）
    if !error_stats.is_empty() {
        output.push('\n');
        output.push_str("错误分类统计:\n");

        // 按错误类别排序以确保输出稳定
        let mut sorted_stats: Vec<_> = error_stats.iter().collect();
        sorted_stats.sort_by_key(|(category, files)| {
            (std::cmp::Reverse(files.len()), format!("{category:?}"))
        });

        for (category, files) in sorted_stats {
            output.push_str(&format!(
                "   {}: {} 个文件\n",
                category.display_name(),
                files.len()
            ));

            // 如果失败文件少于等于5个，列出所有文件名
            if files.len() <= 5 {
                for filename in files {
                    output.push_str(&format!("      - {filename}\n"));
                }
            } else {
                // 如果失败文件超过5个，只显示前3个和后2个
                for filename in files.iter().take(3) {
                    output.push_str(&format!("      - {filename}\n"));
                }
                output.push_str(&format!("      ... (省略{}个文件) ...\n", files.len() - 5));
                for filename in files.iter().skip(files.len() - 2) {
                    output.push_str(&format!("      - {filename}\n"));
                }
            }
        }
    }

    output.push('\n');
    output.push_str(&format!(
        "生成工具: {} {} v{VERSION}\n",
        app_info::APP_NAME,
        app_info::VERSION_SUFFIX
    ));

    output
}

/// 生成批量输出文件路径
pub fn generate_batch_output_path(config: &AppConfig) -> PathBuf {
    config.output_path.clone().unwrap_or_else(|| {
        // 🎯 生成友好的时间格式 YYYY-MM-DD_HH-MM-SS
        let readable_time = {
            use std::time::{SystemTime, UNIX_EPOCH};
            let duration = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("系统时间必须晚于UNIX_EPOCH（1970-01-01），系统时钟配置异常");
            let secs = duration.as_secs();
            let datetime = chrono::DateTime::<chrono::Utc>::from_timestamp(secs as i64, 0)
                .unwrap_or_else(chrono::Utc::now);
            datetime.format("%Y-%m-%d_%H-%M-%S").to_string()
        };

        // 🎯 使用目录名作为基础名称，并清理不合法字符（跨平台兼容）
        let dir_name = utils::sanitize_filename(utils::extract_filename(config.input_path.as_path()));

        config
            .input_path
            .join(format!("{dir_name}_BatchDR_{readable_time}.txt"))
    })
}

/// 统一处理批量输出收尾工作
///
/// 将批量输出内容追加统计信息、写入文件，并显示完成提示。
/// 这个函数消除了串行和并行处理器中的重复代码。
///
/// # 参数
///
/// * `config` - 应用配置
/// * `audio_files` - 处理的音频文件列表
/// * `batch_output` - 批量输出内容(取所有权)
/// * `processed_count` - 成功处理的文件数
/// * `failed_count` - 处理失败的文件数
/// * `error_stats` - 错误分类统计
/// * `is_single_file` - 是否为单文件模式
pub fn finalize_and_write_batch_output(
    config: &AppConfig,
    audio_files: &[PathBuf],
    mut batch_output: String,
    processed_count: usize,
    failed_count: usize,
    error_stats: &std::collections::HashMap<crate::error::ErrorCategory, Vec<String>>,
    is_single_file: bool,
) -> AudioResult<()> {
    if !is_single_file {
        // 多文件模式：生成批量输出文件
        batch_output.push_str(&create_batch_output_footer(
            audio_files,
            processed_count,
            failed_count,
            error_stats,
        ));

        let output_path = generate_batch_output_path(config);
        std::fs::write(&output_path, &batch_output).map_err(AudioError::IoError)?;

        show_batch_completion_info(
            &output_path,
            processed_count,
            audio_files.len(),
            failed_count,
            config,
            is_single_file,
        );
    } else {
        // 单文件模式：显示简单的完成信息
        if processed_count > 0 {
            println!("✅ 单文件处理完成");
        } else {
            println!("❌ 单文件处理失败");
        }
    }

    Ok(())
}

/// 显示批量处理完成信息
pub fn show_batch_completion_info(
    output_path: &std::path::Path,
    processed_count: usize,
    total_count: usize,
    failed_count: usize,
    config: &AppConfig,
    is_single_file: bool,
) {
    println!();
    println!("📊 批量处理完成!");
    println!("   成功处理: {processed_count} / {total_count} 个文件");
    if failed_count > 0 {
        println!("   失败文件: {failed_count} 个");
    }

    println!();
    println!("📄 生成的文件:");
    println!("   🗂️  批量汇总: {}", output_path.display());

    // 🎯 修正提示逻辑：只在单文件目录且处理成功时显示单独结果文件
    if is_single_file && processed_count > 0 {
        println!("   📝 单独结果: 1 个 *_DR_Analysis.txt 文件");
        if config.verbose {
            println!("   💡 单文件目录自动生成单独DR结果文件");
        }
    }
}
