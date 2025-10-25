//! 音频处理模块
//!
//! 负责音频文件的解码、DR计算和结果处理。

use super::cli::AppConfig;
use super::{formatter, utils};
use crate::{
    AudioError, AudioFormat, AudioResult, DrResult,
    audio::UniversalDecoder,
    core::{PeakSelectionStrategy, histogram::WindowRmsAnalyzer, peak_selection::PeakSelector},
    processing::ChannelSeparator,
};

/// 处理单个音频文件
pub fn process_audio_file(
    path: &std::path::Path,
    config: &AppConfig,
) -> AudioResult<(Vec<DrResult>, AudioFormat)> {
    // 🚀 直接使用流式处理实现：零内存累积，恒定内存使用
    // 注：旧的全量加载方法已移除，避免8GB内存占用问题
    process_audio_file_streaming(path, config)
}

/// 处理单个音频文件并显示详细信息
pub fn process_single_audio_file(
    file_path: &std::path::Path,
    config: &AppConfig,
) -> AudioResult<(Vec<DrResult>, AudioFormat)> {
    if config.verbose {
        println!("🎵 正在加载音频文件: {}", file_path.display());
        println!("🎯 使用流式窗口分析（3秒标准窗口）进行DR计算");
    }

    // 处理音频文件
    let (dr_results, format) = process_audio_file(file_path, config)?;

    if config.verbose {
        println!("📊 音频格式信息:");
        println!("   采样率: {} Hz", format.sample_rate);
        println!("   声道数: {}", format.channels);
        println!("   位深度: {} 位", format.bits_per_sample);
        println!("   样本数: {}", format.sample_count);
        println!("   时长: {:.2} 秒", format.duration_seconds());
    }

    Ok((dr_results, format))
}

/// 🚀 新的流式处理实现：真正的零内存累积处理
///
/// 利用WindowRmsAnalyzer的流式能力，避免将整个文件加载到内存
pub fn process_audio_file_streaming(
    path: &std::path::Path,
    config: &AppConfig,
) -> AudioResult<(Vec<DrResult>, AudioFormat)> {
    if config.verbose {
        println!("🌊 使用流式处理模式进行DR分析...");
    }

    let decoder = UniversalDecoder;

    // 🚀 创建高性能流式解码器（支持并行解码）
    // 注：直接创建解码器并从中获取格式信息，避免双重 I/O 操作
    let mut streaming_decoder = if config.parallel_decoding {
        if config.verbose {
            println!(
                "⚡ 启用并行解码模式 ({}线程, {}包批量) - 攻击解码瓶颈",
                config.parallel_threads, config.parallel_batch_size
            );
        }
        decoder.create_streaming_parallel(
            path,
            true,
            Some(config.parallel_batch_size),
            Some(config.parallel_threads),
        )?
    } else {
        if config.verbose {
            println!("🔄 使用串行解码模式（BatchPacketReader优化）");
        }
        decoder.create_streaming(path)?
    };

    // 从已创建的解码器获取格式信息（零额外 I/O 开销）
    if config.verbose {
        let format = streaming_decoder.format();
        println!(
            "📊 音频格式: {}声道, {}Hz, {}位",
            format.channels, format.sample_rate, format.bits_per_sample
        );
        println!("🌊 开始流式解码和分析...");
    }

    // 🎯 委托给核心分析引擎（消除150行重复代码）
    analyze_streaming_decoder(&mut *streaming_decoder, config)
}

/// 🚀 SIMD优化窗口声道分离处理（辅助函数，内存优化版本）
///
/// 使用ChannelSeparator的SIMD优化方法分离声道并送入WindowRmsAnalyzer
///
/// # 内存优化
///
/// 通过复用预分配的left_buffer和right_buffer，避免每个窗口都分配新Vec，
/// 显著降低内存峰值和分配开销（每个并发文件约降低1-1.2MB峰值）。
fn process_window_with_simd_separation(
    window_samples: &[f32],
    channel_count: u32,
    channel_separator: &ChannelSeparator,
    analyzers: &mut [WindowRmsAnalyzer],
    left_buffer: &mut Vec<f32>,
    right_buffer: &mut Vec<f32>,
) {
    // 🛡️ 安全检查：确保analyzers数量与声道数一致（防止多声道扩展时误用）
    debug_assert!(
        !analyzers.is_empty() && analyzers.len() <= 2,
        "当前仅支持1-2声道，实际analyzers数量: {}",
        analyzers.len()
    );
    debug_assert_eq!(
        analyzers.len(),
        channel_count as usize,
        "analyzers数量({})必须与channel_count({})一致",
        analyzers.len(),
        channel_count
    );

    if channel_count == 1 {
        // 单声道：直接处理完整窗口
        analyzers[0].process_samples(window_samples);
    } else if channel_count == 2 {
        // 立体声：使用SIMD优化分离左右声道（复用缓冲区）

        // 🚀 SIMD优化提取左声道（写入预分配缓冲区）
        channel_separator.extract_channel_into(
            window_samples,
            0, // 左声道索引
            2, // 总声道数
            left_buffer,
        );

        // 🚀 SIMD优化提取右声道（写入预分配缓冲区）
        channel_separator.extract_channel_into(
            window_samples,
            1, // 右声道索引
            2, // 总声道数
            right_buffer,
        );

        // 分别送入各声道的WindowRmsAnalyzer（保持窗口完整性）
        analyzers[0].process_samples(left_buffer);
        analyzers[1].process_samples(right_buffer);
    }
}

/// 🔧 内联辅助函数：执行缓冲区compact操作（统一逻辑，减少重复）
#[inline(always)]
fn compact_buffer(
    sample_buffer: &mut Vec<f32>,
    buffer_offset: &mut usize,
    verbose: bool,
    reason: &str,
) {
    if verbose {
        println!(
            "🔧 {}: 移除前{}个样本 ({:.1}KB → {:.1}KB)",
            reason,
            *buffer_offset,
            sample_buffer.len() * 4 / 1024,
            (sample_buffer.len() - *buffer_offset) * 4 / 1024
        );
    }
    sample_buffer.drain(0..*buffer_offset);
    *buffer_offset = 0;
}

/// 🎯 核心DR分析引擎（私有函数）：处理任何StreamingDecoder实现
///
/// 包含完整的流式DR分析流程：声道检查→窗口分析→DR计算
/// 消除process_audio_file_streaming和process_streaming_decoder的~150行重复代码
fn analyze_streaming_decoder(
    streaming_decoder: &mut dyn crate::audio::StreamingDecoder,
    config: &AppConfig,
) -> AudioResult<(Vec<DrResult>, AudioFormat)> {
    #[cfg(feature = "flame-prof")]
    let _guard_processing = {
        let enabled = std::env::var("DR_FLAME").map(|v| v == "1").unwrap_or(false);
        let scope = std::env::var("DR_FLAME_SCOPE").unwrap_or_else(|_| "app".to_string());
        if enabled && scope == "processing" {
            match pprof::ProfilerGuard::new(250) {
                Ok(g) => Some(g),
                Err(e) => {
                    eprintln!("⚠️  启用 processing 范围火焰图采样失败: {e}");
                    None
                }
            }
        } else {
            None
        }
    };
    let format = streaming_decoder.format();

    // 🎯 声道数检查：支持单声道和立体声，拒绝多声道
    if format.channels > 2 {
        return Err(AudioError::InvalidInput(format!(
            "目前仅支持单声道和立体声文件 (1-2声道)，当前为{}声道。\n\
            💡 多声道支持正在开发中，敬请期待未来版本。\n\
            📝 原因：暂未找到多声道SIMD优化的业界标准实现。",
            format.channels
        )));
    }

    // 样本数最小值在流式解码结束后基于“实际解码帧数”再校验，
    // 以兼容未知总长度（如部分 Opus 流）场景，避免误判。

    // 🔧 为每个声道创建独立的WindowRmsAnalyzer（流式处理核心）
    let mut analyzers: Vec<WindowRmsAnalyzer> = (0..format.channels)
        .map(|_| WindowRmsAnalyzer::new(format.sample_rate, config.sum_doubling_enabled()))
        .collect();

    // 🚀 创建SIMD优化的声道分离器
    let channel_separator = ChannelSeparator::new();

    // 🎯 使用集中管理的窗口时长常量（foobar2000标准）
    use super::constants::buffers::{
        BUFFER_CAPACITY_MULTIPLIER, MAX_BUFFER_RATIO, window_alignment_enabled,
    };
    use super::constants::dr_analysis::WINDOW_DURATION_SECONDS;
    // 使用整数计算避免浮点舍入误差（窗口固定为3秒）
    let window_size_samples = (format.sample_rate as usize)
        * (WINDOW_DURATION_SECONDS as usize)
        * (format.channels as usize);

    // 🚀 阶段D内存优化：预分配sample_buffer容量（减少扩容抖动）
    // 通过内部策略开关控制（默认启用，debug模式可通过环境变量禁用）
    let window_align_enabled = window_alignment_enabled();
    let mut sample_buffer = if window_align_enabled {
        Vec::with_capacity(window_size_samples * BUFFER_CAPACITY_MULTIPLIER)
    } else {
        Vec::new()
    };

    // 🚀 阶段B内存优化：引入offset+compact机制（消除每窗口drain的内存搬移）
    let mut buffer_offset = 0usize;
    // Compact阈值：当已处理样本占比超过50%时触发compact
    const COMPACT_THRESHOLD_RATIO: f64 = 0.5;

    // 🚀 阶段A内存优化：预分配声道分离缓冲区（复用，避免每窗口分配）
    // 每个缓冲区容量 = 窗口样本数 / 声道数（即单声道的样本数）
    let channel_buffer_capacity = window_size_samples / format.channels as usize;
    let mut left_buffer = Vec::with_capacity(channel_buffer_capacity);
    // 单声道时不分配 right_buffer 容量，降低峰值内存
    let mut right_buffer = if format.channels > 1 {
        Vec::with_capacity(channel_buffer_capacity)
    } else {
        Vec::new()
    };

    let mut total_chunks = 0;
    let mut total_samples_processed = 0u64;
    let mut windows_processed = 0;

    if config.verbose {
        println!(
            "🎯 窗口配置: {:.1}秒 = {} 个样本 ({}Hz × {} 声道)",
            WINDOW_DURATION_SECONDS, window_size_samples, format.sample_rate, format.channels
        );
        println!("🚀 内存优化: 预分配声道缓冲区 ({channel_buffer_capacity} 样本容量 × 2 声道)");
        println!(
            "🚀 阶段B优化: offset+compact机制 (阈值: {:.0}%)",
            COMPACT_THRESHOLD_RATIO * 100.0
        );
        if window_align_enabled {
            println!(
                "🚀 阶段D优化: sample_buffer预分配 (容量: {} 样本, 硬上限: {:.1}×窗口) [启用]",
                window_size_samples * BUFFER_CAPACITY_MULTIPLIER,
                MAX_BUFFER_RATIO
            );
        } else {
            println!(
                "🚀 阶段D优化: sample_buffer预分配 [禁用 - 环境变量DR_DISABLE_WINDOW_ALIGN=1]"
            );
        }
    }

    // 🌊 智能缓冲流式处理：积累chunk到标准窗口大小，保持算法精度
    while let Some(chunk_samples) = streaming_decoder.next_chunk()? {
        total_chunks += 1;
        total_samples_processed += chunk_samples.len() as u64;

        // 积累chunk到缓冲区
        sample_buffer.extend_from_slice(&chunk_samples);

        if config.verbose && total_chunks % 500 == 0 {
            let progress = streaming_decoder.progress() * 100.0;
            println!(
                "⌛ 智能缓冲进度: {progress:.1}% (已处理{total_chunks}个chunk, 缓冲: {:.1}KB, 偏移: {buffer_offset})",
                sample_buffer.len() * 4 / 1024
            );
        }

        // 🎯 当积累到完整窗口时，处理并移动offset（消除drain的内存搬移）
        while sample_buffer.len() - buffer_offset >= window_size_samples {
            windows_processed += 1;

            if config.verbose && windows_processed % 20 == 0 {
                println!("🔧 处理第{windows_processed}个{WINDOW_DURATION_SECONDS:.1}秒标准窗口...");
            }

            // 提取一个完整的标准窗口（从offset开始）
            let window_samples = &sample_buffer[buffer_offset..buffer_offset + window_size_samples];

            // 🚀 使用SIMD优化的声道分离处理（保持窗口完整性，复用缓冲区）
            process_window_with_simd_separation(
                window_samples,
                format.channels as u32,
                &channel_separator,
                &mut analyzers,
                &mut left_buffer,
                &mut right_buffer,
            );

            // 🚀 阶段B优化：仅移动offset，延迟实际内存搬移
            buffer_offset += window_size_samples;

            // 🚀 阶段D优化：硬上限检查（防止缓冲区无限增长）
            // 仅在窗口对齐优化启用时执行硬上限检查
            if window_align_enabled {
                let max_buffer_size = (window_size_samples as f64 * MAX_BUFFER_RATIO) as usize;
                if sample_buffer.len() > max_buffer_size && buffer_offset > window_size_samples {
                    compact_buffer(
                        &mut sample_buffer,
                        &mut buffer_offset,
                        config.verbose,
                        &format!("触发硬上限Compact: 缓冲区超过{MAX_BUFFER_RATIO:.1}×窗口"),
                    );
                }
                // 🎯 Compact触发：当已处理样本占比超过阈值时，执行一次性内存整理
                else if buffer_offset > 0
                    && buffer_offset as f64 / sample_buffer.len() as f64 > COMPACT_THRESHOLD_RATIO
                {
                    compact_buffer(
                        &mut sample_buffer,
                        &mut buffer_offset,
                        config.verbose,
                        "执行Compact",
                    );
                }
            }
            // 阶段D优化禁用时，仅使用阶段B的compact机制
            else if buffer_offset > 0
                && buffer_offset as f64 / sample_buffer.len() as f64 > COMPACT_THRESHOLD_RATIO
            {
                compact_buffer(
                    &mut sample_buffer,
                    &mut buffer_offset,
                    config.verbose,
                    "执行Compact",
                );
            }
        }
    }

    // 🏁 处理最后剩余的不足标准窗口大小的样本（从offset开始）
    //
    // 📝 尾块处理策略说明：
    // 末尾不足3秒的尾块直接参与计算（符合多数实现标准）：
    // - 尾块样本计入 20% RMS 统计（通过 WindowRmsAnalyzer.process_samples）
    // - 尾块峰值参与峰值检测（主Peak、次Peak更新）
    // - 此行为与 foobar2000 DR Meter 一致，确保完整音频内容被分析
    let remaining_samples = sample_buffer.len() - buffer_offset;
    if remaining_samples > 0 {
        if config.verbose {
            println!(
                "🔧 处理最后剩余样本: {} 个 ({:.2}秒)...",
                remaining_samples,
                remaining_samples as f64 / (format.sample_rate as f64 * format.channels as f64)
            );
        }

        process_window_with_simd_separation(
            &sample_buffer[buffer_offset..],
            format.channels as u32,
            &channel_separator,
            &mut analyzers,
            &mut left_buffer,
            &mut right_buffer,
        );
    }

    if config.verbose {
        println!(
            "✅ 流式处理完成：共处理 {} 个chunk，总样本数: {}M",
            total_chunks,
            total_samples_processed / 1_000_000
        );
        println!("🔧 计算最终DR值...");
    }

    // 🎯 最小样本数校验（基于实际解码帧数）
    // - 兼容未知总长度的流式格式（如部分Opus），避免基于header的误判
    // - 对于零长度/单样本输入，在此处统一返回错误
    const MINIMUM_SAMPLES_FOR_ANALYSIS: u64 = 2;
    let actual_frames = if format.channels > 0 {
        total_samples_processed / format.channels as u64
    } else {
        0
    };
    if actual_frames < MINIMUM_SAMPLES_FOR_ANALYSIS {
        return Err(AudioError::InvalidInput(format!(
            "音频文件样本数过少，无法进行可靠的DR分析。\n\
            要求最少：{MINIMUM_SAMPLES_FOR_ANALYSIS} 个样本，实际：{actual_frames} 个样本。\n\
            💡 音频文件需要足够的样本用于RMS计算和峰值检测。"
        )));
    }

    // 🎯 从每个WindowRmsAnalyzer获取最终DR结果
    let mut dr_results = Vec::new();

    for (channel_idx, analyzer) in analyzers.iter().enumerate() {
        // 使用WindowRmsAnalyzer的20%采样算法
        let rms_20_percent = analyzer.calculate_20_percent_rms();

        // 获取峰值信息
        let window_primary_peak = analyzer.get_largest_peak();
        let window_secondary_peak = analyzer.get_second_largest_peak();

        // 🎯 使用官方峰值选择策略系统（与foobar2000一致）
        let peak_strategy = PeakSelectionStrategy::default(); // PreferSecondary
        let peak_for_dr = peak_strategy.select_peak(window_primary_peak, window_secondary_peak);

        // 计算DR值：DR = -20 * log10(RMS / Peak)
        let dr_value = if peak_for_dr > 0.0 && rms_20_percent > 0.0 {
            -20.0 * (rms_20_percent / peak_for_dr).log10()
        } else {
            0.0
        };

        // 📝 样本计数说明：
        // - sample_count 表示"参与分析的总帧数"（每帧包含所有声道样本）
        // - total_samples_processed 是交错样本总数，除以声道数得到帧数
        // - 此计数与最终 format.sample_count 一致性由解码器保证
        dr_results.push(DrResult::new_with_peaks(
            channel_idx,
            dr_value,
            rms_20_percent,
            peak_for_dr,
            window_primary_peak,
            window_secondary_peak,
            total_samples_processed as usize / format.channels as usize,
        ));
    }

    if config.verbose {
        println!("✅ DR计算完成，共 {} 个声道", dr_results.len());
    }

    // 🎯 获取包含实际样本数的最终格式信息（关键修复：AAC等格式）
    let mut final_format = streaming_decoder.format();

    // 🎯 检测截断：比较预期样本数与实际解码样本数
    // 如果实际处理的样本少于预期，标记为部分分析（is_partial）
    let expected_samples = final_format.sample_count;
    let actual_samples = total_samples_processed / final_format.channels as u64;

    // 调试输出：了解样本数差异
    if config.verbose {
        eprintln!(
            "[DEBUG] 样本数统计: 预期={expected_samples}, 实际={actual_samples}, 总交错样本={total_samples_processed}"
        );
    }

    if actual_samples < expected_samples {
        let skipped_approx = (expected_samples - actual_samples) as usize;
        if config.verbose {
            println!(
                "⚠️  检测到文件截断: 预期 {expected_samples} 个样本，实际解码 {actual_samples} 个样本（缺少约 {skipped_approx} 个）"
            );
        }
        final_format.mark_as_partial(skipped_approx);
    } else if actual_samples > expected_samples && config.verbose {
        eprintln!("[WARNING] 实际解码样本({actual_samples}) 多于预期({expected_samples})");
    }

    // 在函数返回前停止 processing 范围的采样并生成火焰图，避免包含尾段 drop/dealloc
    #[cfg(feature = "flame-prof")]
    if let Some(guard) = _guard_processing
        && let Ok(report) = guard.report().build()
    {
        use std::fs::File;
        let mut options = pprof::flamegraph::Options::default();
        let out_path = std::env::var("DR_FLAME_FILE")
            .unwrap_or_else(|_| "flamegraph-processing.svg".to_string());
        if let Ok(file) = File::create(&out_path)
            && report.flamegraph_with_options(file, &mut options).is_ok()
        {
            eprintln!("✅ FlameGraph(processing) 生成成功: {out_path}");
        }
    }

    Ok((dr_results, final_format))
}

/// 🚀 处理StreamingDecoder进行DR分析（插件专用API）
///
/// 为插件提供的零算法重复接口，接受任何实现StreamingDecoder的对象
pub fn process_streaming_decoder(
    streaming_decoder: &mut dyn crate::audio::StreamingDecoder,
    config: &AppConfig,
) -> AudioResult<(Vec<DrResult>, AudioFormat)> {
    if config.verbose {
        println!("🌊 使用StreamingDecoder进行DR分析...");
    }

    // 🎯 直接委托给核心分析引擎（消除150行重复代码）
    analyze_streaming_decoder(streaming_decoder, config)
}

/// 输出DR计算结果（foobar2000兼容格式）
pub fn output_results(
    results: &[DrResult],
    config: &AppConfig,
    format: &AudioFormat,
    auto_save: bool,
) -> AudioResult<()> {
    // 使用模块化的方法组装输出内容
    let mut output = String::new();

    // 1. 创建头部信息
    output.push_str(&formatter::create_output_header(config, format));

    // 2. 根据声道数格式化DR结果
    output.push_str(&formatter::format_dr_results_by_channel_count(
        results, format,
    ));

    // 3. 添加foobar2000标准分隔线
    output.push_str(
        "--------------------------------------------------------------------------------\n\n",
    );

    // 4. 计算并添加Official DR Value
    output.push_str(&formatter::calculate_official_dr(results, format));

    // 5. 添加音频技术信息
    output.push_str(&formatter::format_audio_info(config, format));

    // 6. 写入输出（文件或控制台）
    formatter::write_output(&output, config, auto_save)
}

/// 批量处理的单个文件结果添加到批量输出
pub fn add_to_batch_output(
    batch_output: &mut String,
    results: &[DrResult],
    format: &AudioFormat,
    file_path: &std::path::Path,
) {
    let file_name = utils::extract_filename_lossy(file_path);

    // 🎯 使用统一的DR聚合函数（修复：与单文件口径一致，排除LFE+静音）
    match formatter::compute_official_precise_dr(results, format) {
        Some((official_dr, precise_dr, _excluded_count)) => {
            // 🎯 使用固定宽度对齐（左对齐17字符），确保列对齐美观
            batch_output.push_str(&format!(
                "{:<17}{:<17}{}\n",
                format!("DR{}", official_dr),
                format!("{:.2} dB", precise_dr),
                file_name
            ));
        }
        None => {
            batch_output.push_str(&format!("{:<17}{:<17}{}\n", "-", "无有效声道", file_name));
        }
    }
}

/// 批量处理失败文件的结果添加到批量输出
pub fn add_failed_to_batch_output(batch_output: &mut String, file_path: &std::path::Path) {
    let file_name = utils::extract_filename_lossy(file_path);
    // 🎯 使用固定宽度对齐（与成功结果格式一致）
    batch_output.push_str(&format!("{:<17}{:<17}{}\n", "-", "处理失败", file_name));
}

/// 为单个文件生成独立的DR结果文件
pub fn save_individual_result(
    results: &[DrResult],
    format: &AudioFormat,
    audio_file: &std::path::Path,
    config: &AppConfig,
) -> AudioResult<()> {
    let temp_config = AppConfig {
        input_path: audio_file.to_path_buf(),
        verbose: false,
        output_path: None,
        parallel_decoding: false,
        parallel_batch_size: super::constants::defaults::PARALLEL_BATCH_SIZE,
        parallel_threads: super::constants::defaults::PARALLEL_THREADS,
        parallel_files: None, // 单文件处理不需要并行
    };

    if let Err(e) = output_results(results, &temp_config, format, true) {
        eprintln!("   ⚠️  保存单独结果文件失败: {e}");
    } else if config.verbose {
        let parent_dir = utils::get_parent_dir(audio_file);
        let file_stem = utils::extract_file_stem(audio_file);
        let individual_path = parent_dir.join(format!("{file_stem}_DR_Analysis.txt"));
        println!("   📄 单独结果已保存: {}", individual_path.display());
    }

    Ok(())
}
