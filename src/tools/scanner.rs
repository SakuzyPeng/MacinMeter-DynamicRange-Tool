//! 文件扫描模块
//!
//! 负责扫描目录中的音频文件，支持多种音频格式。

use super::cli::AppConfig;
use super::utils;
use crate::{AudioError, AudioResult};
use comfy_table::{CellAlignment, ContentArrangement, Table, presets::ASCII_MARKDOWN};
use std::path::PathBuf;

/// 获取支持的音频格式扩展名
///
/// 从UniversalDecoder获取统一的格式支持声明，确保一致性
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
            " 在目录 {} 中没有找到支持的音频文件 / No supported audio files found in directory {}",
            config.input_path.display(),
            config.input_path.display()
        );
        let mut supported_formats: Vec<String> = get_supported_extensions()
            .iter()
            .map(|ext| ext.to_uppercase())
            .collect();
        supported_formats.sort();
        let supported_formats = supported_formats.join(", ");
        println!("   Supported formats / 支持的格式: {supported_formats}");
        return;
    }

    println!(
        "扫描目录 / Scanning directory: {}",
        config.input_path.display()
    );
    println!(
        "找到 {} 个音频文件 / Found {} audio files",
        audio_files.len(),
        audio_files.len()
    );

    if config.verbose {
        for (i, file) in audio_files.iter().enumerate() {
            println!("   {}. {}", i + 1, utils::extract_filename_lossy(file));
        }
    }
    println!();
}

/// 生成批量输出的头部信息（精简版，使用 comfy-table）
pub fn create_batch_output_header(config: &AppConfig, audio_files: &[PathBuf]) -> String {
    let mut output = String::new();

    // 标题
    output.push_str("## MacinMeter DR Batch Report\n\n");

    // 元数据（精简为一行）
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    output.push_str(&format!(
        "**Generated**: {} | **Files**: {} | **Directory**: {}\n\n",
        now,
        audio_files.len(),
        config.input_path.display()
    ));

    // DSD 注释（精简为一行）
    let has_dsd = audio_files.iter().any(|p| {
        p.extension()
            .and_then(|s| s.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("dsf") || e.eq_ignore_ascii_case("dff"))
    });
    if has_dsd {
        let rate = config.dsd_pcm_rate.unwrap_or(352_800);
        output.push_str(&format!(
            "> DSD files downsampled to {rate} Hz for analysis\n\n"
        ));
    }

    // Markdown 表头
    output.push_str("| DR | Precise | File |\n");
    output.push_str("|----|---------|------|\n");

    output
}

/// 生成批量输出的统计信息（精简版，使用 comfy-table）
pub fn create_batch_output_footer(
    audio_files: &[PathBuf],
    processed_count: usize,
    failed_count: usize,
    error_stats: &std::collections::HashMap<crate::error::ErrorCategory, Vec<String>>,
) -> String {
    use super::constants::app_info;
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    let mut output = String::new();

    // 统计表格
    output.push_str("### Summary\n\n");

    let success_rate = if audio_files.is_empty() {
        0.0
    } else {
        processed_count as f64 / audio_files.len() as f64 * 100.0
    };

    let mut table = Table::new();
    table
        .load_preset(ASCII_MARKDOWN)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["Metric", "Value"]);

    table.add_row(vec!["Total", &audio_files.len().to_string()]);
    table.add_row(vec![
        "Success",
        &format!("{processed_count} ({success_rate:.0}%)"),
    ]);
    if failed_count > 0 {
        table.add_row(vec!["Failed", &failed_count.to_string()]);
    }

    output.push_str(&table.to_string());
    output.push('\n');

    // 错误分类（仅在有失败时显示）
    if !error_stats.is_empty() {
        output.push_str("\n**Errors**:\n");

        let mut sorted_stats: Vec<_> = error_stats.iter().collect();
        sorted_stats.sort_by_key(|(category, files)| {
            (std::cmp::Reverse(files.len()), format!("{category:?}"))
        });

        for (category, files) in sorted_stats {
            output.push_str(&format!("- {}: {}\n", category.display_name(), files.len()));
            // 最多显示 3 个文件名
            for filename in files.iter().take(3) {
                output.push_str(&format!("  - {filename}\n"));
            }
            if files.len() > 3 {
                output.push_str(&format!("  - ... (+{})\n", files.len() - 3));
            }
        }
    }

    output.push_str(&format!("\n---\n*{} v{}*\n", app_info::APP_NAME, VERSION));

    output
}

/// 生成批量输出文件路径
pub fn generate_batch_output_path(config: &AppConfig) -> PathBuf {
    config.output_path.clone().unwrap_or_else(|| {
        // 生成友好的时间格式 YYYY-MM-DD_HH-MM-SS
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

        // 使用目录名作为基础名称，并清理不合法字符（跨平台兼容）
        let dir_name =
            utils::sanitize_filename(utils::extract_filename(config.input_path.as_path()));

        config
            .input_path
            .join(format!("{dir_name}_BatchDR_{readable_time}.txt"))
    })
}

/// 统一处理批量输出收尾工作（精简版，使用 comfy-table）
#[allow(clippy::too_many_arguments)]
pub fn finalize_and_write_batch_output(
    config: &AppConfig,
    audio_files: &[PathBuf],
    mut batch_output: String,
    processed_count: usize,
    failed_count: usize,
    error_stats: &std::collections::HashMap<crate::error::ErrorCategory, Vec<String>>,
    is_single_file: bool,
    mut batch_warnings: Vec<super::processor::BatchWarningInfo>,
    exclusion_stats: &super::processor::BatchExclusionStats,
) -> AudioResult<()> {
    if !is_single_file {
        // 多文件模式：生成批量输出文件

        // 添加排除标记脚注（在表格结束后、警告之前）
        if exclusion_stats.has_lfe_excluded || exclusion_stats.has_silent_excluded {
            batch_output.push('\n');
            if exclusion_stats.has_lfe_excluded && exclusion_stats.has_silent_excluded {
                batch_output.push_str("*LFE excluded / †Silent channels excluded\n");
            } else if exclusion_stats.has_lfe_excluded {
                batch_output.push_str("*LFE excluded\n");
            } else {
                batch_output.push_str("†Silent channels excluded\n");
            }
        }

        // 边界风险预警（精简为 5 列）
        if !batch_warnings.is_empty() {
            // 按风险等级（高 → 中 → 低）和距离（升序）排序
            batch_warnings.sort_by(|a, b| {
                use super::formatter::BoundaryRiskLevel::{High, Medium, None};
                let priority = |level: super::formatter::BoundaryRiskLevel| match level {
                    High => 2,
                    Medium => 1,
                    None => 0,
                };
                priority(b.risk_level)
                    .cmp(&priority(a.risk_level))
                    .then_with(|| {
                        a.distance
                            .partial_cmp(&b.distance)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
            });

            batch_output.push_str(&format!(
                "### Boundary Warnings ({})\n\n",
                batch_warnings.len()
            ));

            let mut table = Table::new();
            table
                .load_preset(ASCII_MARKDOWN)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(vec!["DR", "Precise", "Risk", "Potential", "File"]);

            // 设置数值列右对齐
            table
                .column_mut(0)
                .expect("DR column exists")
                .set_cell_alignment(CellAlignment::Right);
            table
                .column_mut(1)
                .expect("Precise column exists")
                .set_cell_alignment(CellAlignment::Right);

            for warning in &batch_warnings {
                let risk = match warning.risk_level {
                    super::formatter::BoundaryRiskLevel::High => "High",
                    super::formatter::BoundaryRiskLevel::Medium => "Medium",
                    super::formatter::BoundaryRiskLevel::None => "Low",
                };

                let potential = match warning.direction {
                    super::formatter::BoundaryDirection::Upper => warning.official_dr + 1,
                    super::formatter::BoundaryDirection::Lower => (warning.official_dr - 1).max(0),
                };

                table.add_row(vec![
                    format!("{}", warning.official_dr),
                    format!("{:.2}", warning.precise_dr),
                    risk.to_string(),
                    format!("DR{potential}"),
                    warning.file_name.clone(),
                ]);
            }

            batch_output.push_str(&table.to_string());
            batch_output.push_str("\n\n");
        }

        batch_output.push_str(&create_batch_output_footer(
            audio_files,
            processed_count,
            failed_count,
            error_stats,
        ));

        // 如果启用了 --no-save，只输出到控制台
        if config.no_save {
            print!("{batch_output}");
        } else {
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
        }
    } else {
        // 单文件模式
        if processed_count > 0 {
            println!("Processing completed");
        } else {
            println!("Processing failed");
        }
    }

    Ok(())
}

/// 显示批量处理完成信息（精简版）
pub fn show_batch_completion_info(
    output_path: &std::path::Path,
    processed_count: usize,
    total_count: usize,
    failed_count: usize,
    _config: &AppConfig,
    _is_single_file: bool,
) {
    println!();
    if failed_count > 0 {
        println!("Completed: {processed_count}/{total_count} files ({failed_count} failed)");
    } else {
        println!("Completed: {processed_count}/{total_count} files");
    }
    println!("Report: {}", output_path.display());
}
