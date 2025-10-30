//! 多文件并行处理模块
//!
//! 使用rayon实现文件级并行处理，保证输出顺序一致性

use super::cli::AppConfig;
use super::{
    ParallelBatchStats, add_failed_to_batch_output, add_to_batch_output,
    create_batch_output_header, finalize_and_write_batch_output, process_single_audio_file,
    processor::AnalysisOutput, save_individual_result, utils,
};
use crate::AudioError;
use crate::error::ErrorCategory;
use rayon::prelude::*;
use std::panic;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// 有序结果容器（保证输出顺序）
struct OrderedResult {
    /// 原始文件索引（用于排序）
    index: usize,

    /// 文件路径
    file_path: PathBuf,

    /// 处理结果
    result: Result<AnalysisOutput, AudioError>,
}

/// 🚀 多文件并行处理（优雅实现）
///
/// 核心特性：
/// - 使用rayon线程池精确控制并发度
/// - 线程安全的统计信息收集
/// - 索引排序保证输出顺序
/// - 自动降级错误处理
pub fn process_batch_parallel(
    audio_files: &[PathBuf],
    config: &AppConfig,
    parallel_degree: usize,
) -> Result<(), AudioError> {
    println!("⚡ 启用多文件并行处理：{parallel_degree} 并发度");

    // 1️⃣ 创建统一的批处理统计管理（并行版本）
    let stats = ParallelBatchStats::new();

    // 进度输出节流计数器（每 50 个文件打印一次）
    let progress_counter = AtomicUsize::new(0);

    // 2️⃣ 创建自定义rayon线程池（精确控制并发度）
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(parallel_degree)
        .stack_size(4 * 1024 * 1024) // 🔧 4MB栈空间：支持96kHz高采样率解码（默认1MB不足）
        .thread_name(|i| format!("dr-worker-{i}"))
        .panic_handler(|_| {
            eprintln!("⚠️  工作线程 panic，但批处理将继续");
        })
        .build()
        .map_err(|e| AudioError::ResourceError(format!("线程池创建失败: {e}")))?;

    // 3️⃣ 并行处理并收集结果（保留索引用于排序）
    let results: Vec<OrderedResult> = pool.install(|| {
        audio_files
            .par_iter()
            .enumerate()
            .map(|(index, audio_file)| {
                // 静默处理单个文件（避免输出混乱）
                let silent_config = AppConfig {
                    verbose: false,
                    ..config.clone()
                };

                // 简短进度提示（节流：每 50 个文件打印一次）
                if !config.verbose {
                    let count = progress_counter.fetch_add(1, Ordering::Relaxed) + 1;
                    if count.is_multiple_of(50) {
                        print!(".");
                        use std::io::Write;
                        std::io::stdout().flush().ok();
                    }
                }

                // 任务级 panic 隔离：将 panic 转换为错误，防止单文件崩全局
                let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                    process_single_audio_file(audio_file, &silent_config)
                }))
                .unwrap_or_else(|_| {
                    Err(AudioError::ResourceError(
                        "文件处理过程中发生内部错误（panic）".to_string(),
                    ))
                });

                // 更新统计（使用统一的 ParallelBatchStats）
                match &result {
                    Ok(_) => {
                        let count = stats.inc_processed();
                        if config.verbose {
                            println!(
                                "✅ [{}/{}] {}",
                                count,
                                audio_files.len(),
                                utils::extract_filename_lossy(audio_file)
                            );
                        }
                    }
                    Err(e) => {
                        let category = ErrorCategory::from_audio_error(e);
                        let filename = utils::extract_filename_lossy(audio_file);

                        if config.verbose {
                            // verbose 模式需要准确的 count，显式传递 &str（会产生一次 clone）
                            let count = stats.inc_failed(category, filename.as_str());
                            println!("❌ [{}/{}] {} - {}", count, audio_files.len(), filename, e);
                        } else {
                            // 非 verbose 模式直接 move filename，零开销
                            stats.inc_failed(category, filename);
                        }
                    }
                }

                OrderedResult {
                    index,
                    file_path: audio_file.clone(),
                    result,
                }
            })
            .collect()
    });

    if !config.verbose {
        println!(); // 进度点换行
    }

    // 4️⃣ 按原始顺序排序结果（关键：保证输出顺序）
    let mut sorted_results = results;
    sorted_results.sort_by_key(|r| r.index);

    // 5️⃣ 按序输出到批量文件（与串行模式输出格式完全一致）
    let is_single_file = audio_files.len() == 1;
    let mut batch_output = if !is_single_file {
        // 预估容量：header(~500字节) + 每个文件(~250字节)
        let estimated_capacity = 500 + audio_files.len() * 250;
        let mut output = String::with_capacity(estimated_capacity);
        output.push_str(&create_batch_output_header(config, audio_files));
        output
    } else {
        String::new()
    };

    // 收集边界风险预警
    let mut batch_warnings = Vec::new();

    for ordered_result in sorted_results {
        match ordered_result.result {
            Ok((results, format, trim_report, silence_report)) => {
                if is_single_file {
                    save_individual_result(
                        &results,
                        &format,
                        &ordered_result.file_path,
                        config,
                        trim_report,
                        silence_report,
                    )?;
                } else {
                    // 收集预警信息
                    if let Some(warning) = add_to_batch_output(
                        &mut batch_output,
                        &results,
                        &format,
                        &ordered_result.file_path,
                    ) {
                        batch_warnings.push(warning);
                    }
                }
            }
            Err(_) => {
                if !is_single_file {
                    add_failed_to_batch_output(&mut batch_output, &ordered_result.file_path);
                }
            }
        }
    }

    // 6️⃣ 统一处理批量输出收尾工作（使用统计快照）
    let snapshot = stats.snapshot();
    finalize_and_write_batch_output(
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
