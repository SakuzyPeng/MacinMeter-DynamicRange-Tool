//! MacinMeter DR Tool - 主程序入口
//!
//! 纯流程控制器，负责协调各个工具模块完成DR分析任务。

use macinmeter_dr_tool::{
    audio::UniversalDecoder,
    error::{AudioError, ErrorCategory},
    tools::{self, AppConfig},
};
use std::path::PathBuf;
use std::process;

/// 错误退出码定义
mod exit_codes {
    /// 通用错误
    pub const GENERAL_ERROR: i32 = 1;
    /// 格式/输入错误
    pub const FORMAT_ERROR: i32 = 2;
    /// 解码失败
    pub const DECODING_ERROR: i32 = 3;
    /// 计算/内存错误
    pub const CALCULATION_ERROR: i32 = 4;
    /// 资源/并发错误
    pub const RESOURCE_ERROR: i32 = 5;
}

/// 获取错误建议文本
fn get_error_suggestion(error: &AudioError) -> &'static str {
    // 优先通过具体错误类型匹配，提供更精确的建议
    match error {
        AudioError::InvalidInput(_) => {
            "检查命令行参数是否正确，使用 --help 查看完整用法 / Check if command-line arguments are correct, use --help to see full usage"
        }
        AudioError::ResourceError(_) => {
            "资源不可用，请检查系统资源或重试；若持续失败请降低并发度 / Resource unavailable, check system resources or retry; if it continues to fail, reduce parallelism"
        }
        AudioError::OutOfMemory => {
            "内存不足，尝试 --serial 串行模式或降低并发度（--parallel-files 1） / Out of memory, try --serial mode or reduce parallelism (--parallel-files 1)"
        }
        // 对于其他错误，使用分类建议
        _ => match ErrorCategory::from_audio_error(error) {
            ErrorCategory::Io => {
                "检查文件路径是否正确，文件是否存在且可读 / Check if file path is correct, file exists and is readable"
            }
            ErrorCategory::Format => {
                "确保输入文件为支持的格式 / Ensure input file is in a supported format"
            }
            ErrorCategory::Decoding => {
                "文件可能损坏或使用不支持的音频编码 / File may be corrupted or use unsupported audio encoding"
            }
            ErrorCategory::Calculation => {
                "计算过程出现异常，请检查音频文件是否包含有效数据 / Calculation error occurred, check if audio file contains valid data"
            }
            ErrorCategory::Other => {
                "请检查输入文件和参数设置 / Please check input file and parameter settings"
            }
        },
    }
}

/// 错误处理和建议
fn handle_error(error: AudioError) -> ! {
    eprintln!("[ERROR] 错误 / Error: {error}");

    // 获取错误分类和建议
    let category = ErrorCategory::from_audio_error(&error);
    eprintln!("[INFO] 建议 / Suggestion: {}", get_error_suggestion(&error));

    // 对于格式错误，额外显示支持的格式列表（大写，与scanner一致）
    if matches!(category, ErrorCategory::Format) {
        let decoder = UniversalDecoder::new();
        let formats = decoder.supported_formats();
        let uppercase_formats: Vec<String> = formats
            .extensions
            .iter()
            .map(|s| s.to_uppercase())
            .collect();
        eprintln!(
            "   Supported formats / 支持的格式: {}",
            uppercase_formats.join(", ")
        );
    }

    // 根据错误类型使用不同的退出码（更精确的映射）
    let exit_code = match &error {
        // 特定错误类型优先匹配
        AudioError::InvalidInput(_) => exit_codes::FORMAT_ERROR,
        AudioError::ResourceError(_) => exit_codes::RESOURCE_ERROR,
        AudioError::OutOfMemory => exit_codes::CALCULATION_ERROR,
        // 通用分类映射
        _ => match category {
            ErrorCategory::Format => exit_codes::FORMAT_ERROR,
            ErrorCategory::Decoding => exit_codes::DECODING_ERROR,
            ErrorCategory::Calculation => exit_codes::CALCULATION_ERROR,
            ErrorCategory::Io | ErrorCategory::Other => exit_codes::GENERAL_ERROR,
        },
    };

    process::exit(exit_code);
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

    // 根据parallel_files配置选择处理模式
    match config.parallel_files {
        None => {
            // 串行模式（明确禁用）
            process_batch_serial(config, &audio_files)
        }
        Some(degree) => {
            // 并行模式：使用统一的并发度计算工具函数
            let actual_degree =
                tools::utils::effective_parallel_degree(degree, Some(audio_files.len()));

            if actual_degree == 1 {
                // 并发度为1，使用串行模式避免开销（仅verbose时提示）
                if config.verbose {
                    println!("[INFO] 并发度为1，使用串行模式 / Parallelism=1, using serial mode");
                }
                process_batch_serial(config, &audio_files)
            } else {
                // 尝试并行处理，失败则降级串行
                tools::process_batch_parallel(&audio_files, config, actual_degree).or_else(|e| {
                    eprintln!("[WARNING] 并行处理失败 / Parallel processing failed: {e}，回退到串行模式 / fallback to serial");
                    process_batch_serial(config, &audio_files)
                })
            }
        }
    }
}

/// 串行批量处理音频文件（原有逻辑）
fn process_batch_serial(config: &AppConfig, audio_files: &[PathBuf]) -> Result<(), AudioError> {
    // 根据文件数量选择输出策略
    let is_single_file = audio_files.len() == 1;
    let mut batch_output = if !is_single_file {
        tools::create_batch_output_header(config, audio_files)
    } else {
        String::new()
    };

    // 使用统一的批处理统计管理（串行版本）
    let mut stats = tools::SerialBatchStats::new();

    // 收集边界风险预警
    let mut batch_warnings = Vec::new();

    // 逐个处理音频文件
    for (index, audio_file) in audio_files.iter().enumerate() {
        // 进度提示：verbose模式显示详细信息，静默模式仅显示基本进度
        if config.verbose {
            println!(
                "[PROCESSING] [{}/{}] 处理 / Processing: {}",
                index + 1,
                audio_files.len(),
                tools::utils::extract_filename_lossy(audio_file)
            );
        }

        match tools::process_single_audio_file(audio_file, config) {
            Ok((results, format, trim_report, silence_report)) => {
                stats.inc_processed();

                if is_single_file {
                    // 单文件模式：只生成单独的DR结果文件
                    let _ = tools::save_individual_result(
                        &results,
                        &format,
                        audio_file,
                        config,
                        trim_report,
                        silence_report,
                    );
                } else {
                    // 多文件模式：添加到批量输出并收集预警信息
                    if let Some(warning) = tools::add_to_batch_output(
                        &mut batch_output,
                        &results,
                        &format,
                        audio_file,
                        config.exclude_lfe,
                    ) {
                        batch_warnings.push(warning);
                    }
                }

                if config.verbose {
                    println!("   [OK] 处理成功 / Processing succeeded");
                }
            }
            Err(e) => {
                // 错误分类统计（使用统一的 BatchStats）
                let category = ErrorCategory::from_audio_error(&e);
                let filename = tools::utils::extract_filename_lossy(audio_file);

                // 详细错误输出（verbose模式）
                if config.verbose {
                    println!("   [FAIL] 处理失败 / Processing failed");
                    println!("      文件 / File: {}", audio_file.display());
                    println!("      类别 / Category: {}", category.display_name());
                    println!("      错误 / Error: {e}");
                    if let Some(source) = std::error::Error::source(&e) {
                        println!("      原因 / Cause: {source}");
                    }
                } else {
                    // 静默模式：至少显示失败的文件
                    println!(
                        "[FAIL] [{}/{}] {} - [{}] {e} / 处理失败",
                        index + 1,
                        audio_files.len(),
                        filename,
                        category.display_name()
                    );
                }

                if !is_single_file {
                    tools::add_failed_to_batch_output(&mut batch_output, audio_file);
                }

                // 最后记录统计，避免 clone（直接 move filename）
                stats.inc_failed(category, filename);
            }
        }
    }

    // 统一处理批量输出收尾工作（使用统计快照）
    let snapshot = stats.snapshot();
    tools::finalize_and_write_batch_output(
        config,
        audio_files,
        batch_output,
        snapshot.processed,
        snapshot.failed,
        &snapshot.error_stats,
        is_single_file,
        batch_warnings,
    )
}

/// 单文件处理模式
fn process_single_mode(config: &AppConfig) -> Result<(), AudioError> {
    let (results, format, trim_report, silence_report) =
        tools::process_single_audio_file(&config.input_path, config)?;

    // 输出结果（如果用户未指定输出文件，则自动保存）
    tools::output_results(
        &results,
        config,
        &format,
        trim_report,
        silence_report,
        config.output_path.is_none(),
    )
}

/// 应用程序主逻辑（便于测试和复用）
fn run() -> Result<(), AudioError> {
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

    // 4. 处理结果并返回
    match result {
        Ok(()) => {
            tools::show_completion_info(&config);
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn main() {
    // 性能优化：提升线程优先级以提高Intel混合架构P-core命中率
    // 静默失败：优化失败不影响程序功能，仅可能影响性能
    let _ = macinmeter_dr_tool::tools::utils::optimize_for_performance();

    // 可选：CPU火焰图分析（需开启 feature: flame-prof 且设置 DR_FLAME=1）
    #[cfg(feature = "flame-prof")]
    let _guard = {
        let enabled = std::env::var("DR_FLAME").map(|v| v == "1").unwrap_or(false);
        // 作用域：app（默认）/ processing / decode
        let scope = std::env::var("DR_FLAME_SCOPE").unwrap_or_else(|_| "app".to_string());
        if enabled && scope == "app" {
            // 采样频率：每秒 250 次（更细分辨率）
            match pprof::ProfilerGuard::new(250) {
                Ok(g) => Some(g),
                Err(e) => {
                    eprintln!(
                        "[WARNING] 启用火焰图采样失败 / Failed to enable flame graph sampling: {e}"
                    );
                    None
                }
            }
        } else {
            None
        }
    };

    // 执行主逻辑，统一处理错误
    let result = run();

    // 在退出前生成火焰图（仅在启用时）
    #[cfg(feature = "flame-prof")]
    if let Some(guard) = _guard
        && let Ok(report) = guard.report().build()
    {
        use std::fs::File;
        let mut options = pprof::flamegraph::Options::default();
        // 输出路径可通过环境变量自定义
        let out_path =
            std::env::var("DR_FLAME_FILE").unwrap_or_else(|_| "flamegraph.svg".to_string());
        if let Ok(file) = File::create(&out_path)
            && report.flamegraph_with_options(file, &mut options).is_ok()
        {
            eprintln!("FlameGraph generated successfully / 生成成功: {out_path}");
        }
        // 如需生成 pprof 二进制，可在启用 protobuf 特性后再输出
    }

    if let Err(error) = result {
        handle_error(error);
    }
}
