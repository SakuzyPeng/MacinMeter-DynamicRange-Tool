//! MacinMeter DR Tool - 主程序入口
//!
//! 纯流程控制器，负责协调各个工具模块完成DR分析任务。

use macinmeter_dr_tool::{
    error::AudioError,
    tools::{self, AppConfig},
};
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

    // 🎯 根据文件数量选择输出策略
    let is_single_file = audio_files.len() == 1;
    let mut batch_output = if !is_single_file {
        tools::create_batch_output_header(config, &audio_files)
    } else {
        String::new()
    };
    let mut processed_count = 0;
    let mut failed_count = 0;

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
                println!("   ❌ 处理失败: {e}");
                if !is_single_file {
                    tools::add_failed_to_batch_output(&mut batch_output, audio_file);
                }
            }
        }
    }

    // 🎯 只有多文件模式才生成批量输出文件
    if !is_single_file {
        batch_output.push_str(&tools::create_batch_output_footer(
            &audio_files,
            processed_count,
            failed_count,
        ));
        let output_path = tools::generate_batch_output_path(config);
        std::fs::write(&output_path, &batch_output).map_err(AudioError::IoError)?;

        // 显示批量完成信息
        tools::show_batch_completion_info(
            &output_path,
            processed_count,
            audio_files.len(),
            failed_count,
            config,
        );
    } else {
        // 🎯 单文件模式：显示简单的完成信息
        if processed_count > 0 {
            println!("✅ 单文件处理完成");
        } else {
            println!("❌ 单文件处理失败");
        }
    }

    Ok(())
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
