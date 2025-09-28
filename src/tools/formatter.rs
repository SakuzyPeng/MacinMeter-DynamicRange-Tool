//! 输出格式化模块
//!
//! 负责DR分析结果的格式化输出，支持foobar2000兼容格式。

use super::cli::AppConfig;
use super::utils;
use crate::{AudioError, AudioFormat, AudioResult, DrResult};

// 引入symphonia编解码器类型用于精确判断
use symphonia::core::codecs::{
    CODEC_TYPE_AAC, CODEC_TYPE_MP3, CODEC_TYPE_OPUS, CODEC_TYPE_VORBIS, CodecType,
};

/// 应用程序版本信息
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 🎯 根据真实编解码器类型判断是否为有损压缩
///
/// 使用symphonia的编解码器常量进行精确判断，比文件扩展名更准确
fn is_lossy_codec_type(codec_type: CodecType) -> bool {
    matches!(
        codec_type,
        CODEC_TYPE_AAC |      // AAC - 有损
        CODEC_TYPE_MP3 |      // MP3 - 有损
        CODEC_TYPE_VORBIS |   // OGG Vorbis - 有损
        CODEC_TYPE_OPUS // Opus - 有损
    )
    // 无损格式：CODEC_TYPE_FLAC, CODEC_TYPE_ALAC, CODEC_TYPE_PCM_*
}

/// 🎯 智能比特率计算：根据真实编解码器类型选择合适的计算方法
///
/// 有损压缩格式(OPUS/MP3/AAC/OGG): 使用文件大小÷时长计算真实比特率
/// 无损格式(WAV/FLAC/ALAC): 使用采样率×声道×位深计算PCM比特率
///
/// 优先使用从解码器获取的真实codec信息，回退到文件扩展名
/// 如果无法计算有损格式的真实比特率，返回错误而不是估算值
fn calculate_actual_bitrate(
    file_path: &std::path::Path,
    format: &AudioFormat,
    codec_fallback: &str,
) -> AudioResult<u32> {
    // 🎯 优先使用真实的编解码器信息
    let is_lossy_compressed = if let Some(codec_type) = format.codec_type {
        is_lossy_codec_type(codec_type)
    } else {
        // 回退到扩展名判断
        matches!(codec_fallback, "OPUS" | "MP3" | "AAC" | "OGG")
    };

    if is_lossy_compressed {
        // 有损压缩格式：使用文件大小和时长计算真实比特率
        let metadata = std::fs::metadata(file_path).map_err(AudioError::IoError)?;

        let file_size_bytes = metadata.len();
        let duration_seconds = format.sample_count as f64 / format.sample_rate as f64;

        if duration_seconds <= 0.0 {
            return Err(AudioError::InvalidInput(
                "音频时长为零，无法计算比特率".to_string(),
            ));
        }

        // 计算实际比特率：(文件大小 × 8) ÷ 时长 ÷ 1000 = kbps
        let bitrate_bps = (file_size_bytes as f64 * 8.0) / duration_seconds;
        Ok((bitrate_bps / 1000.0).round() as u32)
    } else {
        // 无损格式(WAV/FLAC/M4A-ALAC)：使用PCM比特率公式
        Ok(format.sample_rate * format.channels as u32 * format.bits_per_sample as u32 / 1000)
    }
}

/// 识别LFE(低频效果)声道的索引位置
///
/// 根据声道总数和标准多声道布局识别LFE声道位置
/// 支持从2.1到11.1.10等主流格式
fn identify_lfe_channels(channel_count: u16) -> Vec<usize> {
    match channel_count {
        // 标准环绕声格式
        3 => vec![2], // 2.1: 声道3是LFE
        4 => vec![3], // 3.1: 声道4是LFE
        6 => vec![5], // 5.1: 声道6是LFE (最常见)
        7 => vec![6], // 6.1: 声道7是LFE
        8 => vec![7], // 7.1: 声道8是LFE (常见)

        // 三维音频格式 (Dolby Atmos / DTS:X)
        10 => vec![7], // 7.1.2: 声道8是LFE，9-10是天花板
        12 => vec![7], // 7.1.4: 声道8是LFE，9-12是天花板 (Dolby Atmos)
        14 => vec![7], // 7.1.6: 声道8是LFE，其余是天花板
        16 => vec![9], // 9.1.6: 声道10是LFE (DTS:X Pro)

        // 超高端格式
        18 => vec![9],  // 9.1.8: 声道10是LFE
        20 => vec![9],  // 9.1.10: 声道10是LFE
        22 => vec![11], // 11.1.10: 声道12是LFE (极高端格式)
        24 => vec![11], // 11.1.12: 声道12是LFE

        // 其他可能格式
        32 => vec![11], // 某些专业格式

        _ => vec![], // 未知格式或无LFE声道
    }
}

/// 创建输出文件头部信息
pub fn create_output_header(config: &AppConfig, format: &AudioFormat) -> String {
    let mut output = String::new();

    // MacinMeter标识头部（兼容foobar2000格式）
    output.push_str(&format!(
        "MacinMeter DR Tool v{VERSION} / Dynamic Range Meter (foobar2000 compatible)\n"
    ));
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    output.push_str(&format!("log date: {now}\n\n"));

    // 分隔线
    output.push_str(
        "--------------------------------------------------------------------------------\n",
    );

    // 文件统计信息
    let file_name = utils::extract_filename(&config.input_path);
    output.push_str(&format!("Statistics for: {file_name}\n"));

    // 从AudioFormat获取真实的音频信息
    output.push_str(&format!("Number of samples: {}\n", format.sample_count));
    let minutes = format.duration_seconds() as u32 / 60;
    let seconds = format.duration_seconds() as u32 % 60;
    output.push_str(&format!("Duration: {minutes}:{seconds:02} \n"));

    output.push_str(
        "--------------------------------------------------------------------------------\n\n",
    );

    output
}

/// 格式化单声道DR结果
pub fn format_mono_results(result: &DrResult) -> String {
    let mut output = String::new();
    // 保留用于将来可能的显示需求
    // let peak_db = utils::linear_to_db(result.peak);
    // let rms_db = utils::linear_to_db(result.rms);

    output.push_str("                 Mono\n\n");
    // 暂时隐藏Peak和RMS显示
    // output.push_str(&format!("Peak Value:     {peak_db:.2} dB   \n"));
    // output.push_str(&format!("Avg RMS:       {rms_db:.2} dB   \n"));
    output.push_str(&format!("DR channel:      {:.2} dB   \n", result.dr_value));

    output
}

/// 格式化立体声DR结果
pub fn format_stereo_results(results: &[DrResult]) -> String {
    let mut output = String::new();
    // 保留用于将来可能的显示需求
    // let left_peak_db = utils::linear_to_db(results[0].peak);
    // let right_peak_db = utils::linear_to_db(results[1].peak);
    // let left_rms_db = utils::linear_to_db(results[0].rms);
    // let right_rms_db = utils::linear_to_db(results[1].rms);

    output.push_str("                 Left              Right\n\n");
    // 暂时隐藏Peak和RMS显示
    // output.push_str(&format!(
    //     "Peak Value:     {left_peak_db:.2} dB   ---     {right_peak_db:.2} dB   \n"
    // ));
    // output.push_str(&format!(
    //     "Avg RMS:       {left_rms_db:.2} dB   ---    {right_rms_db:.2} dB   \n"
    // ));
    output.push_str(&format!(
        "DR channel:      {:.2} dB   ---     {:.2} dB   \n",
        results[0].dr_value, results[1].dr_value
    ));

    output
}

/// 格式化中等多声道DR结果（3-8声道）
pub fn format_medium_multichannel_results(results: &[DrResult]) -> String {
    let mut output = String::new();

    // 生成声道标题行
    let mut header = String::new();
    for i in 0..results.len() {
        header.push_str(&format!("          Channel {}", i + 1));
    }
    output.push_str(&header);
    output.push_str("\n\n");

    // 暂时隐藏Peak Value行
    // output.push_str("Peak Value:");
    // for (i, result) in results.iter().enumerate() {
    //     let peak_db_str = format!("{} dB", utils::linear_to_db_string(result.peak));
    //     if i < results.len() - 1 {
    //         output.push_str(&format!("     {peak_db_str:>8}   ---"));
    //     } else {
    //         output.push_str(&format!("     {peak_db_str:>8}   "));
    //     }
    // }
    // output.push('\n');

    // 暂时隐藏Avg RMS行
    // output.push_str("Avg RMS:");
    // for (i, result) in results.iter().enumerate() {
    //     let rms_db_str = format!("{} dB", utils::linear_to_db_string(result.rms));
    //     if i < results.len() - 1 {
    //         output.push_str(&format!("       {rms_db_str:>8}   ---"));
    //     } else {
    //         output.push_str(&format!("       {rms_db_str:>8}   "));
    //     }
    // }
    // output.push('\n');

    // DR channel行
    output.push_str("DR channel:");
    for (i, result) in results.iter().enumerate() {
        let dr_value_str = if result.peak > 0.0 && result.rms > 0.0 {
            format!("{:.2} dB", result.dr_value)
        } else {
            "0.00 dB".to_string()
        };
        if i < results.len() - 1 {
            output.push_str(&format!("     {dr_value_str:>8}   ---"));
        } else {
            output.push_str(&format!("     {dr_value_str:>8}   "));
        }
    }
    output.push('\n');

    output
}

/// 格式化大量多声道DR结果（9+声道）
pub fn format_large_multichannel_results(results: &[DrResult], format: &AudioFormat) -> String {
    let mut output = String::new();

    // 暂时隐藏Peak和RMS列的表头
    // output.push_str(
    //     "              声道             Peak dB        RMS dB         DR值        备注\n\n",
    // );
    output.push_str(
        "              声道                                            DR值        备注\n\n",
    );

    for (i, result) in results.iter().enumerate() {
        // 保留用于将来可能的显示需求
        // let peak_db_str = utils::linear_to_db_string(result.peak);
        // let rms_db_str = utils::linear_to_db_string(result.rms);

        let dr_value_str = if result.peak > 0.0 && result.rms > 0.0 {
            format!("{:.2}", result.dr_value)
        } else {
            "0.00".to_string()
        };

        // 检查是否为LFE声道或静音声道
        let lfe_channels = identify_lfe_channels(format.channels);
        let note = if lfe_channels.contains(&i) {
            "LFE (已排除)"
        } else if result.peak == 0.0 && result.rms == 0.0 {
            "静音声道"
        } else {
            ""
        };

        // 暂时隐藏Peak和RMS的显示
        // output.push_str(&format!(
        //     "            Channel {:2}:     {:>8} dB     {:>8} dB      {:>6} dB    {}\n",
        //     i + 1,
        //     peak_db_str,
        //     rms_db_str,
        //     dr_value_str,
        //     note
        // ));
        output.push_str(&format!(
            "            Channel {:2}:                                     {:>6} dB    {}\n",
            i + 1,
            dr_value_str,
            note
        ));
    }

    // 添加LFE声道说明
    let lfe_channels = identify_lfe_channels(format.channels);
    if !lfe_channels.is_empty() {
        output.push('\n');
        let format_name = match format.channels {
            3 => "2.1",
            4 => "3.1",
            6 => "5.1",
            7 => "6.1",
            8 => "7.1",
            10 => "7.1.2",
            12 => "7.1.4 (Dolby Atmos)",
            14 => "7.1.6",
            16 => "9.1.6 (DTS:X Pro)",
            18 => "9.1.8",
            20 => "9.1.10",
            22 => "11.1.10",
            24 => "11.1.12",
            _ => "多声道",
        };
        output.push_str(&format!(
            "注: 检测为{format_name}格式，LFE(低频效果)声道已从DR计算中排除，符合音频分析标准。\n"
        ));
        output.push_str(&format!(
            "    LFE声道位置: Channel {}\n",
            lfe_channels
                .iter()
                .map(|&i| (i + 1).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    output
}

/// 计算并格式化Official DR Value
pub fn calculate_official_dr(results: &[DrResult], format: &AudioFormat) -> String {
    let mut output = String::new();

    if !results.is_empty() {
        // 筛选有效声道：排除LFE声道和静音声道
        let valid_results: Vec<&DrResult> = results
            .iter()
            .enumerate()
            .filter(|(i, result)| {
                let lfe_channels = identify_lfe_channels(format.channels);
                !lfe_channels.contains(i) && result.peak > 0.0 && result.rms > 0.0
            })
            .map(|(_, result)| result)
            .collect();

        if !valid_results.is_empty() {
            let avg_dr: f64 =
                valid_results.iter().map(|r| r.dr_value).sum::<f64>() / valid_results.len() as f64;
            output.push_str(&format!("Official DR Value: DR{}\n", avg_dr.round() as i32));
            output.push_str(&format!("Precise DR Value: {avg_dr:.2} dB\n\n"));

            // 显示计算说明
            let excluded_count = results.len() - valid_results.len();
            if excluded_count > 0 {
                output.push_str(&format!(
                    "DR计算基于 {} 个有效声道 (已排除 {} 个LFE/静音声道)\n\n",
                    valid_results.len(),
                    excluded_count
                ));
            }
        } else {
            output.push_str("Official DR Value: 无有效声道\n\n");
        }
    }

    output
}

/// 格式化音频技术信息
pub fn format_audio_info(config: &AppConfig, format: &AudioFormat) -> String {
    let mut output = String::new();

    output.push_str(&format!("Samplerate:        {} Hz\n", format.sample_rate));
    output.push_str(&format!("Channels:          {}\n", format.channels));
    output.push_str(&format!("Bits per sample:   {}\n", format.bits_per_sample));

    // 🎯 智能比特率计算：压缩格式使用真实比特率，未压缩格式使用PCM比特率
    let codec = utils::extract_extension_uppercase(&config.input_path);
    let bitrate_display = match calculate_actual_bitrate(&config.input_path, format, &codec) {
        Ok(bitrate) => format!("{bitrate} kbps"),
        Err(_) => "N/A".to_string(), // 计算失败时显示N/A，不影响整体分析
    };
    output.push_str(&format!("Bitrate:           {bitrate_display}\n"));

    output.push_str(&format!("Codec:             {codec}\n"));

    output.push_str(
        "================================================================================\n",
    );

    output
}

/// 根据声道数选择合适的格式化方法
pub fn format_dr_results_by_channel_count(results: &[DrResult], format: &AudioFormat) -> String {
    match results.len() {
        0 => "ERROR: 无音频数据\n".to_string(),
        1 => format_mono_results(&results[0]),
        2 => format_stereo_results(results),
        3..=8 => format_medium_multichannel_results(results),
        _ => format_large_multichannel_results(results, format),
    }
}

/// 处理输出写入（文件或控制台）
pub fn write_output(output: &str, config: &AppConfig, auto_save: bool) -> AudioResult<()> {
    match &config.output_path {
        Some(output_path) => {
            // 用户指定了输出文件路径
            std::fs::write(output_path, output).map_err(AudioError::IoError)?;
            println!("📄 结果已保存到: {}", output_path.display());
        }
        None => {
            if auto_save {
                // 自动保存模式：生成基于音频文件名的输出文件路径
                let parent_dir = utils::get_parent_dir(&config.input_path);
                let file_stem = utils::extract_file_stem(&config.input_path);
                let auto_output_path = parent_dir.join(format!("{file_stem}_DR_Analysis.txt"));
                std::fs::write(&auto_output_path, output).map_err(AudioError::IoError)?;
                println!("📄 结果已保存到: {}", auto_output_path.display());
            } else {
                // 控制台输出模式
                print!("{output}");
            }
        }
    }
    Ok(())
}
