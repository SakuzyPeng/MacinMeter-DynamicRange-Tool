//! 多文件并行处理模块
//!
//! 使用rayon实现文件级并行处理，保证输出顺序一致性

use super::cli::AppConfig;
use super::{
    add_failed_to_batch_output, add_to_batch_output, create_batch_output_footer,
    create_batch_output_header, generate_batch_output_path, process_single_audio_file,
    save_individual_result, show_batch_completion_info, utils,
};
use crate::AudioError;
use crate::error::ErrorCategory;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// 有序结果容器（保证输出顺序）
struct OrderedResult {
    /// 原始文件索引（用于排序）
    index: usize,

    /// 文件路径
    file_path: PathBuf,

    /// 处理结果
    result: Result<(Vec<crate::DrResult>, crate::AudioFormat), AudioError>,
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

    // 1️⃣ 创建线程安全的共享状态
    let error_stats = Arc::new(Mutex::new(HashMap::<ErrorCategory, Vec<String>>::new()));
    let processed_count = Arc::new(AtomicUsize::new(0));
    let failed_count = Arc::new(AtomicUsize::new(0));

    // 2️⃣ 创建自定义rayon线程池（精确控制并发度）
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(parallel_degree)
        .thread_name(|i| format!("dr-worker-{i}"))
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

                // 简短进度提示（避免verbose混乱）
                if !config.verbose {
                    print!(".");
                    use std::io::Write;
                    std::io::stdout().flush().ok();
                }

                let result = process_single_audio_file(audio_file, &silent_config);

                // 更新统计（线程安全）
                match &result {
                    Ok(_) => {
                        let count = processed_count.fetch_add(1, Ordering::Relaxed) + 1;
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
                        let count = failed_count.fetch_add(1, Ordering::Relaxed) + 1;

                        // 错误分类统计（需要锁）
                        let category = ErrorCategory::from_audio_error(e);
                        let filename = utils::extract_filename_lossy(audio_file);

                        if let Ok(mut stats) = error_stats.lock() {
                            stats.entry(category).or_default().push(filename.clone());
                        }

                        if config.verbose {
                            println!("❌ [{}/{}] {} - {}", count, audio_files.len(), filename, e);
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
        create_batch_output_header(config, audio_files)
    } else {
        String::new()
    };

    for ordered_result in sorted_results {
        match ordered_result.result {
            Ok((results, format)) => {
                if is_single_file {
                    save_individual_result(&results, &format, &ordered_result.file_path, config)?;
                } else {
                    add_to_batch_output(
                        &mut batch_output,
                        &results,
                        &format,
                        &ordered_result.file_path,
                    );
                }
            }
            Err(_) => {
                if !is_single_file {
                    add_failed_to_batch_output(&mut batch_output, &ordered_result.file_path);
                }
            }
        }
    }

    // 6️⃣ 生成批量输出文件
    if !is_single_file {
        let error_stats_final = error_stats.lock().unwrap().clone();
        let processed = processed_count.load(Ordering::Relaxed);
        let failed = failed_count.load(Ordering::Relaxed);

        batch_output.push_str(&create_batch_output_footer(
            audio_files,
            processed,
            failed,
            &error_stats_final,
        ));

        let output_path = generate_batch_output_path(config);
        std::fs::write(&output_path, &batch_output).map_err(AudioError::IoError)?;

        show_batch_completion_info(&output_path, processed, audio_files.len(), failed, config);
    } else {
        // 单文件模式：显示简单的完成信息
        let processed = processed_count.load(Ordering::Relaxed);
        if processed > 0 {
            println!("✅ 单文件处理完成");
        } else {
            println!("❌ 单文件处理失败");
        }
    }

    Ok(())
}
