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
            if get_supported_extensions().contains(&ext_lower.as_str()) {
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
        let supported_formats = get_supported_extensions()
            .iter()
            .map(|ext| ext.to_uppercase())
            .collect::<Vec<_>>()
            .join(", ");
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
    let mut batch_output = String::new();

    batch_output.push_str("=====================================\n");
    batch_output.push_str("   MacinMeter DR Analysis Report\n");
    batch_output.push_str("   批量分析结果 (foobar2000兼容版)\n");
    batch_output.push_str("=====================================\n\n");

    // 添加标准信息到输出
    batch_output.push_str("🌿 Git分支: foobar2000-plugin (默认批处理模式)\n");
    batch_output.push_str("📏 基于foobar2000 DR Meter逆向分析\n");
    batch_output.push_str("✅ 使用批处理DR计算模式\n");
    batch_output.push_str(&format!("📁 扫描目录: {}\n", config.input_path.display()));
    batch_output.push_str(&format!("🎵 处理文件数: {}\n\n", audio_files.len()));

    // 🎯 添加结果表头（DR值在第一列，方便对齐）
    batch_output.push_str("Official DR\tPrecise DR\t文件名\n");
    batch_output.push_str("--------------------------------------------------------\n");

    batch_output
}

/// 生成批量输出的统计信息
pub fn create_batch_output_footer(
    audio_files: &[PathBuf],
    processed_count: usize,
    failed_count: usize,
) -> String {
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
    output.push('\n');
    output.push_str(&format!(
        "生成工具: MacinMeter DR Tool (foo_dr_meter兼容) v{VERSION}\n"
    ));

    output
}

/// 生成批量输出文件路径
pub fn generate_batch_output_path(config: &AppConfig) -> PathBuf {
    config.output_path.clone().unwrap_or_else(|| {
        // 🎯 生成友好的时间格式 YYYY-MM-DD_HH-MM-SS
        let readable_time = {
            use std::time::{SystemTime, UNIX_EPOCH};
            let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
            let secs = duration.as_secs();
            let datetime = chrono::DateTime::<chrono::Utc>::from_timestamp(secs as i64, 0)
                .unwrap_or_else(chrono::Utc::now);
            datetime.format("%Y-%m-%d_%H-%M-%S").to_string()
        };

        // 🎯 使用目录名作为基础名称，而非第一个文件名
        let dir_name = utils::extract_filename(config.input_path.as_path())
            .replace(".", "_")
            .replace(" ", "_");

        config
            .input_path
            .join(format!("{dir_name}_BatchDR_{readable_time}.txt"))
    })
}

/// 显示批量处理完成信息
pub fn show_batch_completion_info(
    output_path: &std::path::Path,
    processed_count: usize,
    total_count: usize,
    failed_count: usize,
    config: &AppConfig,
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
    if processed_count > 0 {
        println!("   📝 单独结果: {processed_count} 个 *_DR_Analysis.txt 文件");
        if config.verbose {
            println!("   💡 每个音频文件都有对应的单独DR结果文件");
        }
    }
}
