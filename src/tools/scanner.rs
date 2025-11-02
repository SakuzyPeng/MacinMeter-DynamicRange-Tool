//! 文件扫描模块
//!
//! 负责扫描目录中的音频文件，支持多种音频格式。

use super::cli::AppConfig;
use super::utils;
use crate::{AudioError, AudioResult};
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

/// 生成批量输出的头部信息
pub fn create_batch_output_header(config: &AppConfig, audio_files: &[PathBuf]) -> String {
    use super::constants::app_info;
    let mut batch_output = String::new();

    // 动态标题与分割线（按显示宽度自适配），并应用可配置左右留白
    let title_main = {
        use crate::tools::constants::formatting::{HEADER_TITLE_LEFT_PAD, HEADER_TITLE_RIGHT_PAD};
        let base = "MacinMeter DR Analysis Report / MacinMeter DR分析报告";
        crate::tools::utils::table::pad_title_spaces(
            base,
            HEADER_TITLE_LEFT_PAD,
            HEADER_TITLE_RIGHT_PAD,
        )
    };
    let title_sub = {
        use crate::tools::constants::formatting::{SUBTITLE_LEFT_PAD, SUBTITLE_RIGHT_PAD};
        let base = format!(
            "批量分析结果 {} / Batch Analysis Results (foobar2000 Compatible)",
            app_info::VERSION_SUFFIX
        );
        crate::tools::utils::table::pad_title_spaces(&base, SUBTITLE_LEFT_PAD, SUBTITLE_RIGHT_PAD)
    };
    let top_sep = crate::tools::utils::table::separator_for_lines(&[&title_main, &title_sub]);
    batch_output.push_str(&top_sep);
    batch_output.push_str(&title_main);
    batch_output.push('\n');
    batch_output.push_str(&title_sub);
    batch_output.push('\n');
    let bottom_sep = crate::tools::utils::table::separator_for_lines(&[&title_main, &title_sub]);
    batch_output.push_str(&bottom_sep);

    // 在头部下方添加日志时间（与单文件报告保持一致位置）
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    batch_output.push_str(&format!("日志时间 / Log date: {now}\n\n"));

    // 添加标准信息到输出（使用共享常量）
    batch_output.push_str(&format!(
        "Git分支 / Git Branch: {}\n",
        app_info::BRANCH_INFO
    ));
    batch_output.push_str(&format!("{}\n", app_info::BASE_DESCRIPTION));
    batch_output.push_str(&format!("{}\n", app_info::CALCULATION_MODE));
    batch_output.push_str(&format!(
        "扫描目录 / Scanned Directory: {}\n",
        config.input_path.display()
    ));
    batch_output.push_str(&format!(
        "处理文件数 / Files to Process: {}\n\n",
        audio_files.len()
    ));

    // 添加结果表头（使用固定宽度确保对齐）
    let header_line = crate::tools::utils::table::format_two_cols_line(
        "Official DR",
        "Precise DR",
        "文件名 / File Name",
    );
    batch_output.push_str(&header_line);
    let sep = crate::tools::utils::table::separator_from(&header_line);
    batch_output.push_str(&sep);

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
    let stats_title = "批量处理统计 / Batch Processing Statistics:";
    let stats_sep = crate::tools::utils::table::separator_for_lines(&[stats_title]);
    output.push_str(&stats_sep);
    output.push_str(stats_title);
    output.push('\n');
    output.push_str(&format!(
        "   总文件数 / Total Files: {}\n",
        audio_files.len()
    ));
    output.push_str(&format!(
        "   成功处理 / Processed Successfully: {processed_count}\n"
    ));
    output.push_str(&format!("   处理失败 / Failed: {failed_count}\n"));
    output.push_str(&format!(
        "   处理成功率 / Success Rate: {:.1}%\n",
        processed_count as f64 / audio_files.len() as f64 * 100.0
    ));

    // 错误分类统计（仅在有失败时显示）
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
        "生成工具 / Generated by: {} {} v{VERSION}\n",
        app_info::APP_NAME,
        app_info::VERSION_SUFFIX
    ));

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
) -> AudioResult<()> {
    if !is_single_file {
        // 多文件模式：生成批量输出文件

        // 添加边界风险预警汇总（在footer之前）
        if !batch_warnings.is_empty() {
            // 按风险等级（高 → 中 → 低）和距离（升序）排序，保证输出稳定
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

            batch_output.push('\n');
            let warnings_title = {
                use crate::tools::constants::formatting::{
                    WARNINGS_TITLE_LEFT_PAD, WARNINGS_TITLE_RIGHT_PAD,
                };
                let base = "边界风险警告 / Boundary Risk Warnings";
                crate::tools::utils::table::pad_title_spaces(
                    base,
                    WARNINGS_TITLE_LEFT_PAD,
                    WARNINGS_TITLE_RIGHT_PAD,
                )
            };
            let warnings_sep = crate::tools::utils::table::separator_for_lines(&[&warnings_title]);
            batch_output.push_str(&warnings_sep);
            batch_output.push_str(&warnings_title);
            batch_output.push('\n');
            batch_output.push_str(&warnings_sep);
            batch_output.push('\n');
            batch_output
                .push_str("以下文件的DR值接近四舍五入边界，可能与foobar2000结果相差±1级：\n");
            batch_output.push_str("The following files have DR values near rounding boundaries and may differ from foobar2000 by ±1 level:\n\n");

            // 使用统一列对齐：6列定宽 + 文件名尾字段
            let header_cols = [
                "Official DR",
                "Precise DR",
                "风险等级 / Risk",
                "边界方向 / Boundary",
                "Δ距离 / ΔDistance",
                "foobar2000 可能值 / May Report",
            ];
            let base_widths = [13usize, 13, 23, 23, 21, 25];
            let eff_widths =
                crate::tools::utils::table::effective_widths(&header_cols, &base_widths);

            // 为特定列增加视觉右移1字符（仅影响表头，不改变列宽），解决个别终端下表头与数据行相差1字符的问题。
            let mut shifted_header_cols = header_cols;
            // Δ距离 / ΔDistance 列（索引4）与 foobar2000 可能值 / May Report 列（索引5）
            shifted_header_cols[4] = " Δ距离 / ΔDistance";
            shifted_header_cols[5] = " foobar2000 可能值 / May Report";
            let header = crate::tools::utils::table::format_cols_line(
                &shifted_header_cols,
                &eff_widths,
                " 文件名 / File Name",
            );
            batch_output.push_str(&header);
            let warn_sep = crate::tools::utils::table::separator_from(&header);
            batch_output.push_str(&warn_sep);

            for warning in &batch_warnings {
                let risk_label = match warning.risk_level {
                    super::formatter::BoundaryRiskLevel::High => "高风险 / High",
                    super::formatter::BoundaryRiskLevel::Medium => "中风险 / Medium",
                    super::formatter::BoundaryRiskLevel::None => "低风险 / Low",
                };

                let (direction_label, potential_dr) = match warning.direction {
                    super::formatter::BoundaryDirection::Upper => {
                        ("上边界 / Upper", warning.official_dr + 1)
                    }
                    super::formatter::BoundaryDirection::Lower => {
                        ("下边界 / Lower", (warning.official_dr - 1).max(0))
                    }
                };

                let line = crate::tools::utils::table::format_cols_line(
                    &[
                        &format!("DR{}", warning.official_dr),
                        &format!("{:.2} dB", warning.precise_dr),
                        risk_label,
                        direction_label,
                        &format!("Δ{:.2} dB", warning.distance),
                        &format!("DR{potential_dr}"),
                    ],
                    &eff_widths,
                    &warning.file_name,
                );
                batch_output.push_str(&line);
            }

            batch_output.push('\n');
        }

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
            println!("单文件处理完成 / Single file processing completed");
        } else {
            println!("单文件处理失败 / Single file processing failed");
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
    println!("批量处理完成 / Batch processing completed!");
    println!(
        "   成功处理 / Successfully processed: {processed_count} / {total_count} 文件 / files"
    );
    if failed_count > 0 {
        println!("   失败文件 / Failed files: {failed_count}");
    }

    println!();
    println!("生成的文件 / Generated files:");
    println!("   批量汇总 / Batch summary: {}", output_path.display());

    // 修正提示逻辑：只在单文件目录且处理成功时显示单独结果文件
    if is_single_file && processed_count > 0 {
        println!("   单独结果 / Individual result: 1 *_DR_Analysis.txt file");
        if config.verbose {
            println!(
                "   单文件目录自动生成单独DR结果文件 / Single-file directory auto-generates individual DR result file"
            );
        }
    }
}
