//! 音频处理模块
//!
//! 负责音频文件的解码、DR计算和结果处理。

use super::cli::AppConfig;
use super::{formatter, utils};
use crate::{
    AudioFormat, AudioResult, DrResult, PeakSelectionStrategy, audio::UniversalDecoder,
    core::DrCalculator,
};

/// 处理单个音频文件
pub fn process_audio_file(
    path: &std::path::Path,
    config: &AppConfig,
) -> AudioResult<(Vec<DrResult>, AudioFormat)> {
    if config.verbose {
        println!("🎯 使用批处理模式进行DR计算...");
    }

    let decoder = UniversalDecoder::new();

    // 先探测格式获取音频参数
    let format = decoder.probe_format(path)?;

    // 创建高性能流式解码器收集所有样本（使用优化的逐包模式）
    let mut streaming_decoder = decoder.create_streaming_optimized(path)?;

    if config.verbose {
        println!("📦 收集所有音频样本中...");
    }

    // 收集所有音频样本
    let mut all_samples = Vec::new();
    let mut total_chunks = 0;

    while let Some(chunk_samples) = streaming_decoder.next_chunk()? {
        total_chunks += 1;

        if config.verbose && total_chunks % 500 == 0 {
            let progress = streaming_decoder.progress() * 100.0;
            println!(
                "⌛ 样本收集进度: {progress:.1}% (已收集{total_chunks}个chunk, 总样本: {})",
                all_samples.len()
            );
        }

        // 收集所有样本到内存中
        all_samples.extend_from_slice(&chunk_samples);
    }

    if config.verbose {
        println!(
            "✅ 样本收集完成：共收集 {} 个decoder chunk，总样本数: {}",
            total_chunks,
            all_samples.len()
        );
        println!("🔧 现在进行DR计算处理...");
    }

    // 创建DR计算器
    let dr_calculator = DrCalculator::new_advanced(
        format.channels as usize,
        config.sum_doubling_enabled(),
        format.sample_rate,
        3.0,
        PeakSelectionStrategy::PreferSecondary,
    )?;

    // 🔍 [TRACE] 计算DR值
    #[cfg(debug_assertions)]
    eprintln!("🔍 [MAIN] 开始调用DrCalculator::calculate_dr_from_samples");
    #[cfg(debug_assertions)]
    eprintln!(
        "🔍 [MAIN] 输入: samples={}, channels={}",
        all_samples.len(),
        format.channels
    );

    let dr_results =
        dr_calculator.calculate_dr_from_samples(&all_samples, format.channels as usize)?;

    #[cfg(debug_assertions)]
    eprintln!(
        "🔍 [MAIN] DrCalculator返回结果: {} 个声道",
        dr_results.len()
    );

    if config.verbose {
        println!("✅ DR计算完成");
    }

    Ok((dr_results, format))
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

    // foobar2000兼容模式：显示分声道结果
    for result in results {
        let peak_db = utils::linear_to_db(result.peak);
        let rms_db = utils::linear_to_db(result.rms);
        batch_output.push_str(&format!(
            "{}_Ch{}\tDR{}\t{:.2}\t{:.2}\t{}Hz\t{}\t{:.1}s\n",
            file_name,
            result.channel + 1,
            result.dr_value_rounded(),
            peak_db,
            rms_db,
            format.sample_rate,
            format.channels,
            format.duration_seconds()
        ));
    }
}

/// 批量处理失败文件的结果添加到批量输出
pub fn add_failed_to_batch_output(batch_output: &mut String, file_path: &std::path::Path) {
    let file_name = utils::extract_filename_lossy(file_path);
    batch_output.push_str(&format!("{file_name}\t处理失败\t-\t-\t-\t-\t-\n"));
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
