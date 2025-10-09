//! MacinMeter DR Tool - 主程序入口
//!
//! 纯流程控制器，负责协调各个工具模块完成DR分析任务。

use macinmeter_dr_tool::{
    error::{AudioError, ErrorCategory},
    tools::{self, AppConfig},
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process;

/// 错误处理和建议
fn handle_error(error: AudioError) {
    eprintln!("❌ 错误: {error}");

    // 提供错误相关的建议
    match error {
        AudioError::IoError(_) => {
            eprintln!("💡 建议: 检查文件路径是否正确，文件是否存在且可读");
        }
        AudioError::FormatError(_) => {
            eprintln!("💡 建议: 确保输入文件是有效的WAV格式");
        }
        AudioError::DecodingError(_) => {
            eprintln!("💡 建议: 文件可能损坏或使用不支持的音频编码");
        }
        AudioError::InvalidInput(_) => {
            eprintln!("💡 建议: 检查命令行参数是否正确");
        }
        AudioError::OutOfMemory => {
            eprintln!("💡 建议: 文件过大，尝试处理较小的音频文件");
        }
        _ => {
            eprintln!("💡 建议: 请检查输入文件和参数设置");
        }
    }

    process::exit(1);
}

/// 批量处理音频文件
fn process_batch_mode(config: &AppConfig) -> Result<(), AudioError> {
    // 扫描目录中的音频文件
    let audio_files = tools::scan_audio_files(&config.input_path)?;

    // 显示扫描结果
    tools::show_scan_results(config, &audio_files);

    if audio_files.is_empty() {
        return Ok(());
    }

    // 🎯 根据parallel_files配置选择处理模式
    match config.parallel_files {
        None => {
            // 串行模式（明确禁用）
            process_batch_serial(config, &audio_files)
        }
        Some(degree) => {
            // 并行模式
            let actual_degree = degree.min(audio_files.len()).min(16);

            if actual_degree == 1 {
                // 并发度为1，使用串行模式避免开销
                println!("💡 并发度为1，使用串行模式");
                process_batch_serial(config, &audio_files)
            } else {
                // 尝试并行处理，失败则降级串行
                tools::process_batch_parallel(&audio_files, config, actual_degree).or_else(|e| {
                    eprintln!("⚠️  并行处理失败: {e}，回退到串行模式");
                    process_batch_serial(config, &audio_files)
                })
            }
        }
    }
}

/// 串行批量处理音频文件（原有逻辑）
fn process_batch_serial(config: &AppConfig, audio_files: &[PathBuf]) -> Result<(), AudioError> {
    // 🎯 根据文件数量选择输出策略
    let is_single_file = audio_files.len() == 1;
    let mut batch_output = if !is_single_file {
        tools::create_batch_output_header(config, audio_files)
    } else {
        String::new()
    };
    let mut processed_count = 0;
    let mut failed_count = 0;
    // 🎯 错误分类统计：记录每种错误类型及对应的失败文件列表
    let mut error_stats: HashMap<ErrorCategory, Vec<String>> = HashMap::new();

    // 逐个处理音频文件
    for (index, audio_file) in audio_files.iter().enumerate() {
        println!(
            "🔄 [{}/{}] 处理: {}",
            index + 1,
            audio_files.len(),
            tools::utils::extract_filename_lossy(audio_file)
        );

        match tools::process_single_audio_file(audio_file, config) {
            Ok((results, format)) => {
                processed_count += 1;

                if is_single_file {
                    // 🎯 单文件模式：只生成单独的DR结果文件
                    let _ = tools::save_individual_result(&results, &format, audio_file, config);
                } else {
                    // 🎯 多文件模式：只添加到批量输出
                    tools::add_to_batch_output(&mut batch_output, &results, &format, audio_file);
                }

                if config.verbose {
                    println!("   ✅ 处理成功");
                }
            }
            Err(e) => {
                failed_count += 1;

                // 🎯 错误分类统计
                let category = ErrorCategory::from_audio_error(&e);
                let filename = tools::utils::extract_filename_lossy(audio_file);
                error_stats
                    .entry(category)
                    .or_default()
                    .push(filename.clone());

                // 🎯 详细错误输出（verbose模式）
                if config.verbose {
                    println!("   ❌ 处理失败");
                    println!("      文件: {}", audio_file.display());
                    println!("      类别: {}", category.display_name());
                    println!("      错误: {e}");
                    if let Some(source) = std::error::Error::source(&e) {
                        println!("      原因: {source}");
                    }
                } else {
                    println!("   ❌ [{}] {e}", category.display_name());
                }

                if !is_single_file {
                    tools::add_failed_to_batch_output(&mut batch_output, audio_file);
                }
            }
        }
    }

    // 🎯 统一处理批量输出收尾工作
    tools::finalize_and_write_batch_output(
        config,
        audio_files,
        batch_output,
        processed_count,
        failed_count,
        &error_stats,
        is_single_file,
    )
}

/// 单文件处理模式
fn process_single_mode(config: &AppConfig) -> Result<(), AudioError> {
    let (results, format) = tools::process_single_audio_file(&config.input_path, config)?;

    // 输出结果（如果用户未指定输出文件，则自动保存）
    tools::output_results(&results, config, &format, config.output_path.is_none())
}

fn main() {
    // 1. 解析命令行参数
    let config = tools::parse_args();

    // 2. 显示启动信息
    tools::show_startup_info(&config);

    // 3. 根据模式选择处理方式
    let result = if config.is_batch_mode() {
        process_batch_mode(&config)
    } else {
        process_single_mode(&config)
    };

    // 4. 处理结果
    match result {
        Ok(()) => tools::show_completion_info(&config),
        Err(error) => handle_error(error),
    }
}
