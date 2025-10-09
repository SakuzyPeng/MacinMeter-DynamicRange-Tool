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
        println!("🎯 使用批处理计算模式进行DR分析");
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

    // 先探测格式获取音频参数（用于友好的日志输出）
    let format = decoder.probe_format(path)?;

    if config.verbose {
        println!(
            "📊 音频格式: {}声道, {}Hz, {}位",
            format.channels, format.sample_rate, format.bits_per_sample
        );
        println!("🌊 开始流式解码和分析...");
    }

    // 🚀 创建高性能流式解码器（支持并行解码）
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

    // 🎯 委托给核心分析引擎（消除150行重复代码）
    analyze_streaming_decoder(&mut *streaming_decoder, config)
}

/// 🚀 SIMD优化窗口声道分离处理（辅助函数）
///
/// 使用ChannelSeparator的SIMD优化方法分离声道并送入WindowRmsAnalyzer
fn process_window_with_simd_separation(
    window_samples: &[f32],
    channel_count: u32,
    channel_separator: &ChannelSeparator,
    analyzers: &mut [WindowRmsAnalyzer],
) {
    if channel_count == 1 {
        // 单声道：直接处理完整窗口
        analyzers[0].process_samples(window_samples);
    } else if channel_count == 2 {
        // 立体声：使用SIMD优化分离左右声道

        // 🚀 SIMD优化提取左声道
        let left_samples = channel_separator.extract_channel_samples_optimized(
            window_samples,
            0, // 左声道索引
            2, // 总声道数
        );

        // 🚀 SIMD优化提取右声道
        let right_samples = channel_separator.extract_channel_samples_optimized(
            window_samples,
            1, // 右声道索引
            2, // 总声道数
        );

        // 分别送入各声道的WindowRmsAnalyzer（保持窗口完整性）
        analyzers[0].process_samples(&left_samples);
        analyzers[1].process_samples(&right_samples);
    }
}

/// 🎯 核心DR分析引擎（私有函数）：处理任何StreamingDecoder实现
///
/// 包含完整的流式DR分析流程：声道检查→窗口分析→DR计算
/// 消除process_audio_file_streaming和process_streaming_decoder的~150行重复代码
fn analyze_streaming_decoder(
    streaming_decoder: &mut dyn crate::audio::StreamingDecoder,
    config: &AppConfig,
) -> AudioResult<(Vec<DrResult>, AudioFormat)> {
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

    // 🔧 为每个声道创建独立的WindowRmsAnalyzer（流式处理核心）
    let mut analyzers: Vec<WindowRmsAnalyzer> = (0..format.channels)
        .map(|_| WindowRmsAnalyzer::new(format.sample_rate, config.sum_doubling_enabled()))
        .collect();

    // 🚀 创建SIMD优化的声道分离器
    let channel_separator = ChannelSeparator::new();

    // 🎯 使用集中管理的窗口时长常量（foobar2000标准）
    use super::constants::dr_analysis::WINDOW_DURATION_SECONDS;
    let window_size_samples =
        (format.sample_rate as f64 * WINDOW_DURATION_SECONDS * format.channels as f64) as usize;
    let mut sample_buffer = Vec::new();

    let mut total_chunks = 0;
    let mut total_samples_processed = 0u64;
    let mut windows_processed = 0;

    if config.verbose {
        println!(
            "🎯 窗口配置: {:.1}秒 = {} 个样本 ({}Hz × {} 声道)",
            WINDOW_DURATION_SECONDS, window_size_samples, format.sample_rate, format.channels
        );
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
                "⌛ 智能缓冲进度: {progress:.1}% (已处理{total_chunks}个chunk, 缓冲: {:.1}KB)",
                sample_buffer.len() * 4 / 1024
            );
        }

        // 🎯 当积累到完整窗口时，处理并清空缓冲区（保持算法精度）
        while sample_buffer.len() >= window_size_samples {
            windows_processed += 1;

            if config.verbose && windows_processed % 20 == 0 {
                println!("🔧 处理第{windows_processed}个{WINDOW_DURATION_SECONDS:.1}秒标准窗口...");
            }

            // 提取一个完整的标准窗口
            let window_samples = &sample_buffer[0..window_size_samples];

            // 🚀 使用SIMD优化的声道分离处理（保持窗口完整性）
            process_window_with_simd_separation(
                window_samples,
                format.channels as u32,
                &channel_separator,
                &mut analyzers,
            );

            // 移除已处理的样本，保留剩余部分继续积累
            sample_buffer.drain(0..window_size_samples);
        }
    }

    // 🏁 处理最后剩余的不足标准窗口大小的样本
    if !sample_buffer.is_empty() {
        if config.verbose {
            println!(
                "🔧 处理最后剩余样本: {} 个 ({:.2}秒)...",
                sample_buffer.len(),
                sample_buffer.len() as f64 / (format.sample_rate as f64 * format.channels as f64)
            );
        }

        process_window_with_simd_separation(
            &sample_buffer,
            format.channels as u32,
            &channel_separator,
            &mut analyzers,
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
    let final_format = streaming_decoder.format();

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
            // 🎯 DR值在第一列，方便对齐，保持"dB"后缀与单文件一致
            batch_output.push_str(&format!(
                "DR{official_dr}\t{precise_dr:.2} dB\t{file_name}\n"
            ));
        }
        None => {
            batch_output.push_str(&format!("-\t无有效声道\t{file_name}\n"));
        }
    }
}

/// 批量处理失败文件的结果添加到批量输出
pub fn add_failed_to_batch_output(batch_output: &mut String, file_path: &std::path::Path) {
    let file_name = utils::extract_filename_lossy(file_path);
    // 🎯 匹配新格式：Official DR\tPrecise DR\t文件名
    batch_output.push_str(&format!("-\t处理失败\t{file_name}\n"));
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
        println!("   ⚠️  保存单独结果文件失败: {e}");
    } else if config.verbose {
        let parent_dir = utils::get_parent_dir(audio_file);
        let file_stem = utils::extract_file_stem(audio_file);
        let individual_path = parent_dir.join(format!("{file_stem}_DR_Analysis.txt"));
        println!("   📄 单独结果已保存: {}", individual_path.display());
    }

    Ok(())
}
