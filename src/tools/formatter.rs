//! 输出格式化模块
//!
//! 负责DR分析结果的格式化输出，支持foobar2000兼容格式。

use super::cli::AppConfig;
use super::constants;
use super::utils;
use crate::{
    AudioError, AudioFormat, AudioResult, DrResult,
    processing::{EdgeTrimReport, SilenceFilterReport},
};

// 引入symphonia编解码器类型用于精确判断
use symphonia::core::codecs::{
    CODEC_TYPE_AAC, CODEC_TYPE_ALAC, CODEC_TYPE_FLAC, CODEC_TYPE_MP3, CODEC_TYPE_OPUS,
    CODEC_TYPE_PCM_ALAW, CODEC_TYPE_PCM_F32BE, CODEC_TYPE_PCM_F32LE, CODEC_TYPE_PCM_F64BE,
    CODEC_TYPE_PCM_F64LE, CODEC_TYPE_PCM_MULAW, CODEC_TYPE_PCM_S8, CODEC_TYPE_PCM_S16BE,
    CODEC_TYPE_PCM_S16LE, CODEC_TYPE_PCM_S24BE, CODEC_TYPE_PCM_S24LE, CODEC_TYPE_PCM_S32BE,
    CODEC_TYPE_PCM_S32LE, CODEC_TYPE_PCM_U8, CODEC_TYPE_PCM_U16BE, CODEC_TYPE_PCM_U16LE,
    CODEC_TYPE_PCM_U24BE, CODEC_TYPE_PCM_U24LE, CODEC_TYPE_PCM_U32BE, CODEC_TYPE_PCM_U32LE,
    CODEC_TYPE_VORBIS, CodecType,
};

/// 应用程序版本信息
const VERSION: &str = env!("CARGO_PKG_VERSION");

// 两列定宽工具已抽到 utils::table

/// 将 CodecType 映射为人类可读的编解码器名称
///
/// 优先使用真实的解码器类型信息，比文件扩展名更准确
fn codec_type_to_string(codec_type: CodecType) -> &'static str {
    match codec_type {
        // 有损压缩格式
        CODEC_TYPE_AAC => "AAC",
        CODEC_TYPE_MP3 => "MP3",
        CODEC_TYPE_VORBIS => "OGG Vorbis",
        CODEC_TYPE_OPUS => "Opus",

        // 无损压缩格式
        CODEC_TYPE_FLAC => "FLAC",
        CODEC_TYPE_ALAC => "ALAC",

        // PCM格式（统一显示为WAV/PCM）
        CODEC_TYPE_PCM_S8 | CODEC_TYPE_PCM_U8 | CODEC_TYPE_PCM_S16LE | CODEC_TYPE_PCM_S16BE
        | CODEC_TYPE_PCM_U16LE | CODEC_TYPE_PCM_U16BE | CODEC_TYPE_PCM_S24LE
        | CODEC_TYPE_PCM_S24BE | CODEC_TYPE_PCM_U24LE | CODEC_TYPE_PCM_U24BE
        | CODEC_TYPE_PCM_S32LE | CODEC_TYPE_PCM_S32BE | CODEC_TYPE_PCM_U32LE
        | CODEC_TYPE_PCM_U32BE | CODEC_TYPE_PCM_F32LE | CODEC_TYPE_PCM_F32BE
        | CODEC_TYPE_PCM_F64LE | CODEC_TYPE_PCM_F64BE | CODEC_TYPE_PCM_ALAW
        | CODEC_TYPE_PCM_MULAW => "WAV/PCM",

        // 未知格式：返回原始描述字符串
        _ => "Unknown",
    }
}

/// 根据真实编解码器类型判断是否为有损压缩
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

/// 智能比特率计算：根据真实编解码器类型选择合适的计算方法
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
    // 部分分析时无法准确计算比特率（样本数不完整）
    if format.is_partial() {
        return Err(AudioError::InvalidInput(
            "部分分析模式下无法准确计算比特率".to_string(),
        ));
    }

    // 优先使用真实的编解码器信息
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
        // 使用 u64 防止极端采样率/声道/位深组合下的溢出
        // 例如：384kHz × 32ch × 32bit = 393,216,000 bps (接近 u32 上限)
        let bitrate_bps =
            format.sample_rate as u64 * format.channels as u64 * format.bits_per_sample as u64;
        let bitrate_kbps = bitrate_bps / 1000;

        // 确保结果在 u32 范围内（实际音频不会超过）
        Ok(bitrate_kbps.min(u32::MAX as u64) as u32)
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
pub fn create_output_header(
    config: &AppConfig,
    format: &AudioFormat,
    edge_trim_report: Option<EdgeTrimReport>,
    silence_filter_report: Option<SilenceFilterReport>,
) -> String {
    let mut output = String::new();

    // 使用统一的头部标识常量（避免跨模块文案漂移）
    let header_line = constants::app_info::format_output_header(VERSION);
    output.push_str(&format!("{header_line}\n"));
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    output.push_str(&format!("日志时间 / Log date: {now}\n\n"));

    // 分隔线（长度与标题一致）
    let sep_dash = utils::table::separator_for_lines_with_char(&[&header_line], '-');
    output.push_str(&sep_dash);

    // 文件统计信息
    let file_name = utils::extract_filename(&config.input_path);
    output.push_str(&format!("统计对象 / Statistics for: {file_name}\n"));

    // 从AudioFormat获取真实的音频信息
    output.push_str(&format!(
        "样本总数 / Number of samples: {}\n",
        format.sample_count
    ));

    // 智能时长显示：<1小时用 MM:SS，≥1小时用 HH:MM:SS
    let total_seconds = format.duration_seconds() as u32;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    let duration_display = if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    };
    output.push_str(&format!("时长 / Duration: {duration_display}\n"));

    if let Some(report) = edge_trim_report {
        let cfg = report.config;
        let leading_sec = report.leading_duration_sec(format.sample_rate, format.channels as usize);
        let trailing_sec =
            report.trailing_duration_sec(format.sample_rate, format.channels as usize);
        let total_sec = report.total_duration_sec(format.sample_rate, format.channels as usize);
        let total_samples = report.total_samples_trimmed();

        output.push_str(&format!(
            "边缘裁切启用 / Edge trimming enabled: threshold {threshold_db:.1} dBFS, min run {min_run_ms:.0} ms (hysteresis {hysteresis_ms:.0} ms)\n",
            threshold_db = cfg.threshold_db,
            min_run_ms = cfg.min_run_ms,
            hysteresis_ms = cfg.hysteresis_ms
        ));
        output.push_str(&format!(
            "裁切剔除 / Trimmed: {total_sec:.3} s ({total_samples} samples) | 前端 / Leading {leading_sec:.3} s, 末端 / Trailing {trailing_sec:.3} s\n"
        ));
    }

    if let Some(report) = silence_filter_report {
        output.push_str(&format!(
            "静音过滤启用 / Silence filter enabled: threshold {threshold_db:.1} dBFS\n",
            threshold_db = report.threshold_db
        ));

        for channel in &report.channels {
            if channel.total_windows == 0 {
                output.push_str(&format!(
                    "通道 Channel {}: 无可分析窗口（文件过短）/ No analysis windows (file too short)\n",
                    channel.channel_index + 1,
                ));
            } else if channel.filtered_windows == 0 {
                output.push_str(&format!(
                    "通道 Channel {}: 未移除静音窗口 / No silent windows removed （总窗口 / Total: {}）\n",
                    channel.channel_index + 1,
                    channel.total_windows
                ));
            } else {
                output.push_str(&format!(
                    "通道 Channel {}: 移除 {filtered}/{total} 窗口 ({percent:.2}%)，有效 / Valid {valid}\n",
                    channel.channel_index + 1,
                    filtered = channel.filtered_windows,
                    total = channel.total_windows,
                    percent = channel.filtered_percent(),
                    valid = channel.valid_windows,
                ));
            }
        }
    }

    let sep_dash2 = utils::table::separator_for_lines_with_char(&[&header_line], '-');
    output.push_str(&sep_dash2);
    output.push('\n');

    output
}

/// 格式化单声道DR结果
pub fn format_mono_results(result: &DrResult, show_rms_peak: bool) -> String {
    let mut output = String::new();
    output.push_str("                 单声道 / Mono\n\n");
    let dr_value = format!("{:.2} dB", result.dr_value);
    output.push_str(&utils::table::format_cols_line(
        &["DR通道 / DR Channel:", &dr_value],
        &[28, 14],
        "",
    ));

    if show_rms_peak {
        // 诊断：RMS与Peak（以dBFS显示）
        output.push('\n');
        output.push_str("RMS/Peak 诊断 / RMS/Peak Diagnostics\n");
        let label_widths = [22, 14];
        let rms_db = format!("{} dB", utils::linear_to_db_string(result.rms));
        let peak_sel_db = format!("{} dB", utils::linear_to_db_string(result.peak));
        let peak1_db = format!("{} dB", utils::linear_to_db_string(result.primary_peak));
        let peak2_db = format!("{} dB", utils::linear_to_db_string(result.secondary_peak));

        output.push_str(&utils::table::format_cols_line(
            &["RMS(20%):", &rms_db],
            &label_widths,
            "",
        ));
        output.push_str(&utils::table::format_cols_line(
            &["Peak(选用/selected):", &peak_sel_db],
            &label_widths,
            "",
        ));
        output.push_str(&utils::table::format_cols_line(
            &["Primary Peak:", &peak1_db],
            &label_widths,
            "",
        ));
        output.push_str(&utils::table::format_cols_line(
            &["Secondary Peak:", &peak2_db],
            &label_widths,
            "",
        ));
        output.push('\n');
    } else {
        output.push('\n');
    }

    output
}

/// 格式化立体声DR结果
pub fn format_stereo_results(results: &[DrResult], show_rms_peak: bool) -> String {
    let mut output = String::new();
    output.push_str("                         左声道 / Left      右声道 / Right\n\n");
    let left_value = format!("{:.2} dB", results[0].dr_value);
    let right_value = format!("{:.2} dB", results[1].dr_value);
    output.push_str(&utils::table::format_cols_line(
        &["DR通道 / DR Channel:", &left_value, &right_value],
        &[28, 14, 14],
        "",
    ));

    if show_rms_peak {
        output.push('\n');
        output.push_str("RMS/Peak 诊断 / RMS/Peak Diagnostics\n");
        let widths = [18, 14, 14];
        let l_rms = format!("{} dB", utils::linear_to_db_string(results[0].rms));
        let r_rms = format!("{} dB", utils::linear_to_db_string(results[1].rms));
        let l_peak = format!("{} dB", utils::linear_to_db_string(results[0].peak));
        let r_peak = format!("{} dB", utils::linear_to_db_string(results[1].peak));
        let l_p1 = format!("{} dB", utils::linear_to_db_string(results[0].primary_peak));
        let r_p1 = format!("{} dB", utils::linear_to_db_string(results[1].primary_peak));
        let l_p2 = format!(
            "{} dB",
            utils::linear_to_db_string(results[0].secondary_peak)
        );
        let r_p2 = format!(
            "{} dB",
            utils::linear_to_db_string(results[1].secondary_peak)
        );

        output.push_str(&utils::table::format_cols_line(
            &["RMS(20%):", &l_rms, &r_rms],
            &widths,
            "",
        ));
        output.push_str(&utils::table::format_cols_line(
            &["Peak(选用/sel):", &l_peak, &r_peak],
            &widths,
            "",
        ));
        output.push_str(&utils::table::format_cols_line(
            &["Primary Peak:", &l_p1, &r_p1],
            &widths,
            "",
        ));
        output.push_str(&utils::table::format_cols_line(
            &["Secondary Peak:", &l_p2, &r_p2],
            &widths,
            "",
        ));
        output.push('\n');
    } else {
        output.push('\n');
    }

    output
}

/// 格式化中等多声道DR结果（3+声道）
/// 单文件模式使用列表格式便于查看，多文件模式使用简化格式
pub fn format_medium_multichannel_results(results: &[DrResult], format: &AudioFormat) -> String {
    let mut output = String::new();

    // 表头采用与批量列表风格一致的两列固定宽度
    output.push_str(&format!(
        "声道明细 / Channel breakdown ({} channels):\n",
        results.len()
    ));
    // 头部也使用统一对齐逻辑，确保列首字符对齐
    let header_line =
        utils::table::format_two_cols_line("Official DR", "Precise DR", "通道 / Channel");
    output.push_str(&header_line);
    // 下方补一条等宽'='分割线，匹配表头宽度
    let header_sep = utils::table::separator_from(&header_line);
    output.push_str(&header_sep);

    // 识别 LFE 位置（优先容器元数据，其次标准布局）
    let lfe_channels: Vec<usize> =
        if format.has_channel_layout_metadata && !format.lfe_indices.is_empty() {
            format
                .lfe_indices
                .iter()
                .copied()
                .filter(|&idx| idx < results.len())
                .collect()
        } else {
            identify_lfe_channels(format.channels)
                .into_iter()
                .filter(|&idx| idx < results.len())
                .collect()
        };

    for (i, result) in results.iter().enumerate() {
        let dr_rounded = result.dr_value_rounded();
        let col_official = format!("DR{dr_rounded}");
        let col_precise = format!("{:.2} dB", result.dr_value);

        let is_silent = result.peak <= constants::dr_analysis::DR_ZERO_EPS
            || result.rms <= constants::dr_analysis::DR_ZERO_EPS;
        let is_lfe = lfe_channels.contains(&i);
        // 通道编号宽度2，右对齐，避免 1 位与 2 位编号导致轻微视觉跳动
        let mut label = format!("通道 / Channel {:>2}", i + 1);
        if is_lfe {
            label.push_str("  [LFE]");
        } else if is_silent {
            label.push_str("  [Silent / 静音]");
        }
        output.push_str(&utils::table::format_two_cols_line(
            &col_official,
            &col_precise,
            &label,
        ));
    }

    // 附加 RMS/Peak 诊断表（每通道）
    output.push('\n');
    output.push_str("RMS/Peak 诊断 / RMS/Peak Diagnostics\n");
    let header = utils::table::format_two_cols_line("RMS(20%)", "Peak(选用/sel)", "通道 / Channel");
    output.push_str(&header);
    let sep = utils::table::separator_from(&header);
    output.push_str(&sep);
    for (i, r) in results.iter().enumerate() {
        let col_rms = format!("{} dB", utils::linear_to_db_string(r.rms));
        let col_peak = format!("{} dB", utils::linear_to_db_string(r.peak));
        let mut label = format!("通道 / Channel {:>2}", i + 1);
        // 附注：静音/LFE保持一致
        let is_silent = r.peak <= constants::dr_analysis::DR_ZERO_EPS
            || r.rms <= constants::dr_analysis::DR_ZERO_EPS;
        let lfe_channels: Vec<usize> =
            if format.has_channel_layout_metadata && !format.lfe_indices.is_empty() {
                format.lfe_indices.clone()
            } else {
                identify_lfe_channels(format.channels)
            };
        if lfe_channels.contains(&i) {
            label.push_str("  [LFE]");
        } else if is_silent {
            label.push_str("  [Silent / 静音]");
        }
        output.push_str(&utils::table::format_two_cols_line(
            &col_rms, &col_peak, &label,
        ));
    }

    output
}

/// 格式化大量多声道DR结果（9+声道）
pub fn format_large_multichannel_results(results: &[DrResult], format: &AudioFormat) -> String {
    let mut output = String::new();

    // 与批量风格一致：两列固定宽度 + 尾字段
    output.push_str(&format!(
        "声道明细 / Channel breakdown ({} channels):\n",
        results.len()
    ));
    let header_line =
        utils::table::format_two_cols_line("Official DR", "Precise DR", "通道 / Channel");
    output.push_str(&header_line);
    let header_sep = utils::table::separator_from(&header_line);
    output.push_str(&header_sep);

    // 识别 LFE（优先容器元数据）
    let lfe_channels: Vec<usize> =
        if format.has_channel_layout_metadata && !format.lfe_indices.is_empty() {
            format
                .lfe_indices
                .iter()
                .copied()
                .filter(|&idx| idx < results.len())
                .collect()
        } else {
            identify_lfe_channels(format.channels)
                .into_iter()
                .filter(|&idx| idx < results.len())
                .collect()
        };

    for (i, result) in results.iter().enumerate() {
        let dr_rounded = result.dr_value_rounded();
        let col_official = format!("DR{dr_rounded}");
        let col_precise = if result.peak > constants::dr_analysis::DR_ZERO_EPS
            && result.rms > constants::dr_analysis::DR_ZERO_EPS
        {
            format!("{:.2} dB", result.dr_value)
        } else {
            "0.00 dB".to_string()
        };

        let is_silent = result.peak <= constants::dr_analysis::DR_ZERO_EPS
            || result.rms <= constants::dr_analysis::DR_ZERO_EPS;
        let is_lfe = lfe_channels.contains(&i);
        let mut label = format!("通道 / Channel {:>2}", i + 1);
        if is_lfe {
            label.push_str("  [LFE]");
        } else if is_silent {
            label.push_str("  [Silent / 静音]");
        }

        output.push_str(&utils::table::format_two_cols_line(
            &col_official,
            &col_precise,
            &label,
        ));
    }

    // 添加LFE声道说明
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
            "注 / Note: 检测为 {format_name} 格式，包含 LFE (低频效果) 声道；所有非静音声道参与 DR 聚合（与 foobar2000 口径一致）。\n"
        ));
        output.push_str(&format!(
            "Note: detected as {format_name} layout, includes LFE channel(s); all non‑silent channels participate in DR aggregation (foobar2000‑compatible).\n"
        ));
        output.push_str(&format!(
            "    LFE声道位置 / LFE channels: Channel {}\n",
            lfe_channels
                .iter()
                .map(|&i| (i + 1).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    // 附加 RMS/Peak 诊断表（每通道）
    output.push('\n');
    output.push_str("RMS/Peak 诊断 / RMS/Peak Diagnostics\n");
    let header = utils::table::format_two_cols_line("RMS(20%)", "Peak(选用/sel)", "通道 / Channel");
    output.push_str(&header);
    let sep = utils::table::separator_from(&header);
    output.push_str(&sep);
    for (i, r) in results.iter().enumerate() {
        let col_rms = format!("{} dB", utils::linear_to_db_string(r.rms));
        let col_peak = format!("{} dB", utils::linear_to_db_string(r.peak));
        let mut label = format!("通道 / Channel {:>2}", i + 1);
        if lfe_channels.contains(&i) {
            label.push_str("  [LFE]");
        } else if r.peak <= constants::dr_analysis::DR_ZERO_EPS
            || r.rms <= constants::dr_analysis::DR_ZERO_EPS
        {
            label.push_str("  [Silent / 静音]");
        }
        output.push_str(&utils::table::format_two_cols_line(
            &col_rms, &col_peak, &label,
        ));
    }

    output
}

/// 统一的DR聚合计算（核心函数）
///
/// 基于foobar2000 DR Meter实测行为：所有非静音声道参与计算（不排除LFE）
///
/// # 计算口径说明
///
/// **Official DR 算法**（foobar2000 兼容实现）：
/// 1. 筛选有效声道：仅排除静音声道（peak=0且rms=0）
/// 2. 计算平均 DR：对所有有效声道的 DR 值求算术平均
/// 3. 四舍五入：将平均 DR 值四舍五入为整数
///
/// **重要**: 根据foobar2000实测，LFE声道不会被排除，所有非静音声道都参与DR计算
///
/// **与其他定义的区别**：
/// - 本实现采用 **通道级平均** 方式，与 foobar2000 DR Meter 完全一致
/// - 不同于某些实现直接对全局 Peak/RMS 计算 DR
/// - 符合 Pleasurize Music Foundation 的 DR 标准（2009）
///
/// # 返回
/// - `Some((official_dr, precise_dr, excluded_count))`: 成功计算
///   - `official_dr`: 官方DR值（四舍五入整数）
///   - `precise_dr`: 精确DR值（保留完整小数）
///   - `excluded_count`: 被排除的声道数（仅静音声道）
/// - `None`: 无有效声道
///
/// # 示例
/// ```ignore
/// let (official, precise, excluded) = compute_official_precise_dr(results, format)?;
/// println!("DR{} ({:.2} dB, 排除{}声道)", official, precise, excluded);
/// ```
pub fn compute_official_precise_dr(
    results: &[DrResult],
    format: &AudioFormat,
    exclude_lfe: bool,
) -> Option<(i32, f64, usize, usize)> {
    if results.is_empty() {
        return None;
    }

    // 识别应排除的 LFE 声道（仅当显式存在声道布局元数据时生效）
    let lfe_indices: Vec<usize> = if exclude_lfe && format.has_channel_layout_metadata {
        if !format.lfe_indices.is_empty() {
            format
                .lfe_indices
                .iter()
                .copied()
                .filter(|&idx| idx < results.len())
                .collect()
        } else {
            identify_lfe_channels(format.channels)
                .into_iter()
                .filter(|&idx| idx < results.len())
                .collect()
        }
    } else {
        Vec::new()
    };

    // 筛选有效声道：排除静音声道 + （可选）LFE 声道
    // 使用DR_ZERO_EPS阈值兼容解码器产生的近零噪声
    let mut excluded_silent = 0usize;
    let mut excluded_lfe = 0usize;
    let valid_results: Vec<&DrResult> = results
        .iter()
        .enumerate()
        .filter_map(|(i, result)| {
            let is_silent = result.peak <= constants::dr_analysis::DR_ZERO_EPS
                || result.rms <= constants::dr_analysis::DR_ZERO_EPS;
            let is_lfe = lfe_indices.contains(&i);
            if is_silent {
                excluded_silent += 1;
                None
            } else if is_lfe {
                excluded_lfe += 1;
                None
            } else {
                Some(result)
            }
        })
        .collect();

    if valid_results.is_empty() {
        return None;
    }

    // 计算平均DR值
    let avg_dr: f64 =
        valid_results.iter().map(|r| r.dr_value).sum::<f64>() / valid_results.len() as f64;
    let official_dr = avg_dr.round() as i32;
    let excluded_count = results.len() - valid_results.len();

    Some((official_dr, avg_dr, excluded_count, excluded_lfe))
}

/// DR边界风险阈值常量（避免浮点精度问题）
const DR_BOUNDARY_STRICT: f64 = 0.031; // 高风险阈值（容忍浮点误差）
const DR_BOUNDARY_LOOSE: f64 = 0.051; // 中风险阈值（容忍浮点误差）

/// 预警风险级别
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoundaryRiskLevel {
    /// 高风险：距上边界 ≤0.03 dB
    High,
    /// 中风险：距上边界 0.03~0.05 dB
    Medium,
    /// 无风险
    None,
}

/// 预警方向（接近上边界或下边界）
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoundaryDirection {
    Upper,
    Lower,
}

/// 检测DR边界风险级别（用于批量模式）
///
/// 返回风险级别、接近的边界方向以及距离该边界的距离
pub fn detect_boundary_risk_level(
    official_dr: i32,
    precise_dr: f64,
) -> Option<(BoundaryRiskLevel, BoundaryDirection, f64)> {
    fn classify(distance: f64) -> Option<BoundaryRiskLevel> {
        if distance < 0.0 {
            None
        } else if distance <= DR_BOUNDARY_STRICT {
            Some(BoundaryRiskLevel::High)
        } else if distance <= DR_BOUNDARY_LOOSE {
            Some(BoundaryRiskLevel::Medium)
        } else {
            None
        }
    }

    fn priority(level: BoundaryRiskLevel) -> u8 {
        match level {
            BoundaryRiskLevel::High => 2,
            BoundaryRiskLevel::Medium => 1,
            BoundaryRiskLevel::None => 0,
        }
    }

    let boundary_upper = official_dr as f64 + 0.5;
    let boundary_lower = official_dr as f64 - 0.5;
    let distance_to_upper = boundary_upper - precise_dr;
    let distance_to_lower = precise_dr - boundary_lower;

    let upper_candidate = classify(distance_to_upper)
        .map(|level| (level, BoundaryDirection::Upper, distance_to_upper));
    let lower_candidate = classify(distance_to_lower)
        .map(|level| (level, BoundaryDirection::Lower, distance_to_lower));

    match (upper_candidate, lower_candidate) {
        (Some(upper), Some(lower)) => {
            if priority(upper.0) > priority(lower.0) {
                Some(upper)
            } else if priority(upper.0) < priority(lower.0) {
                Some(lower)
            } else if upper.2 <= lower.2 {
                Some(upper)
            } else {
                Some(lower)
            }
        }
        (Some(upper), None) => Some(upper),
        (None, Some(lower)) => Some(lower),
        (None, None) => None,
    }
}

/// DR边界风险检测（双向四舍五入预警）
///
/// 检测precise DR是否接近任何rounding boundary（上下两边），可能导致与foobar2000的Official DR不同
///
/// # 风险场景
/// - **上边界风险**：precise_dr ≈ (official_dr + 0.5)，可能被向上舍入到 DR(official_dr+1)
///   - 例：precise=11.49, official=DR11, 但可能被舍为DR12（距离仅0.01）
///
/// - **下边界风险**：precise_dr ≈ (official_dr - 0.5)，可能被向下舍入到 DR(official_dr-1)
///   - 例：precise=15.51, official=DR16, 但可能被舍为DR15（距离仅0.01）
///
/// # 预警级别
/// - 高风险（距任何边界 ≤0.03 dB）：精确度在0.01 dB内，舍入方向可能改变
/// - 中风险（距任何边界 0.03~0.05 dB）：需留意foobar2000的对比结果
///
/// 返回预警消息（如果需要预警），否则返回None
pub fn detect_dr_boundary_warning(official_dr: i32, precise_dr: f64) -> Option<String> {
    detect_boundary_risk_level(official_dr, precise_dr).map(
        |(risk_level, direction, distance)| {
            let (header_zh, header_en, recommendation) = match risk_level {
                BoundaryRiskLevel::High => (
                    "边界风险（高）",
                    "Boundary Risk (High)",
                    "建议 / Recommendation: 使用 foobar2000 DR Meter 交叉验证 / Cross-validate with foobar2000",
                ),
                BoundaryRiskLevel::Medium => (
                    "边界风险（中）",
                    "Boundary Risk (Medium)",
                    "建议 / Recommendation: 留意与 foobar2000 的对比结果 / Compare with foobar2000 results",
                ),
                BoundaryRiskLevel::None => ("", "", ""),
            };

            let (boundary_desc_zh, boundary_desc_en, target_dr) = match direction {
                BoundaryDirection::Upper => (
                    format!("DR{official_dr}/DR{} 上边界", official_dr + 1),
                    format!("upper boundary between DR{official_dr} and DR{}", official_dr + 1),
                    official_dr + 1,
                ),
                BoundaryDirection::Lower => (
                    format!("DR{}/DR{official_dr} 下边界", (official_dr - 1).max(0)),
                    format!(
                        "lower boundary between DR{} and DR{official_dr}",
                        (official_dr - 1).max(0)
                    ),
                    (official_dr - 1).max(0),
                ),
            };

            format!(
                "{header_zh} / {header_en}\n\
                 Precise DR {precise_dr:.2} dB 距离 {boundary_desc_zh} {distance:.2} dB\n\
                 Distance to {boundary_desc_en}: {distance:.2} dB\n\
                 可能被舍入至 DR{target_dr} 而非 DR{official_dr}\n\
                 May round to DR{target_dr} instead of DR{official_dr}\n\
                 {recommendation}\n"
            )
        },
    )
}
/// 计算并格式化Official DR Value
pub fn calculate_official_dr(
    results: &[DrResult],
    format: &AudioFormat,
    exclude_lfe: bool,
) -> String {
    let mut output = String::new();

    // 使用统一的DR聚合函数
    match compute_official_precise_dr(results, format, exclude_lfe) {
        Some((official_dr, precise_dr, excluded_count, excluded_lfe)) => {
            output.push_str(&format!("Official DR Value: DR{official_dr}\n"));
            output.push_str(&format!("Precise DR Value: {precise_dr:.2} dB\n"));

            // 边界风险预警（四舍五入跨级检测）
            if let Some(warning) = detect_dr_boundary_warning(official_dr, precise_dr) {
                output.push('\n');
                output.push_str(&warning);
            }

            // 保持现有边界预警逻辑，不进行额外复算提示

            output.push('\n');

            // 显示计算说明（仅当有排除声道时）
            if excluded_count > 0 {
                let valid_count = results.len() - excluded_count;
                if excluded_lfe > 0 {
                    let silent = excluded_count - excluded_lfe;
                    let lfe = excluded_lfe;
                    output.push_str(&format!(
                        "DR计算基于 {valid_count} 个有效声道 (已排除 {silent} 个静音声道, {lfe} 个LFE声道)\n",
                    ));
                    output.push_str(&format!(
                        "Aggregation based on {valid_count} valid channels (excluded {silent} silent, {lfe} LFE).\n\n"
                    ));
                } else {
                    output.push_str(&format!(
                        "DR计算基于 {valid_count} 个有效声道 (已排除 {excluded_count} 个静音声道)\n"
                    ));
                    output.push_str(&format!(
                        "Aggregation based on {valid_count} valid channels (excluded {excluded_count} silent).\n\n"
                    ));
                }
            }

            // 若启用了 LFE 排除但无法识别布局，友好提示
            if exclude_lfe && !format.has_channel_layout_metadata && format.channels > 2 {
                output.push_str(
                    "注 / Note: 请求排除 LFE，但未检测到声道布局元数据；未执行 LFE 剔除。\n",
                );
                output.push_str(
                    "Note: LFE exclusion requested but no channel layout metadata detected; LFE exclusion not performed.\n\n",
                );
            }
        }
        None => {
            output.push_str("Official DR Value: 无有效声道\n\n");
        }
    }

    output
}

/// 格式化音频技术信息
pub fn format_audio_info(config: &AppConfig, format: &AudioFormat) -> String {
    let mut output = String::new();

    // 统一对齐：按“显示宽度”对齐左列标签，避免中英混排产生的偏移
    let labels = [
        "采样率 / Sample rate:",
        "声道数 / Channels:",
        "位深 / Bits per sample:",
        "比特率 / Bitrate:",
        "编码 / Codec:",
    ];

    // 计算统一的标签列宽（按Unicode显示宽度）
    let widths = vec![0usize; labels.len()];
    let eff = utils::table::effective_widths(&labels, &widths);
    let label_col_width = eff.into_iter().max().unwrap_or(0);

    // 值列
    // 采样率显示：若发生重采样（如DSD→PCM降采样），显示“源 → 处理（DSD降采样）”
    let sample_rate_s = if let Some(proc_sr) = format.processed_sample_rate {
        if proc_sr != format.sample_rate {
            format!(
                "{} Hz → {} Hz (DSD downsampled / DSD降采样)",
                format.sample_rate, proc_sr
            )
        } else {
            format!("{} Hz", format.sample_rate)
        }
    } else {
        format!("{} Hz", format.sample_rate)
    };
    let channels_s = format!("{}", format.channels);
    let bits_s = format!("{}", format.bits_per_sample);

    // 智能比特率计算：压缩格式使用真实比特率，未压缩格式使用PCM比特率
    let extension_fallback = utils::extract_extension_uppercase(&config.input_path);
    let bitrate_display =
        match calculate_actual_bitrate(&config.input_path, format, &extension_fallback) {
            Ok(bitrate) => format!("{bitrate} kbps"),
            Err(_) => "N/A".to_string(), // 计算失败时显示N/A（如部分分析模式）
        };

    // 优先使用真实的编解码器类型，回退到文件扩展名
    let codec_display = if let Some(codec_type) = format.codec_type {
        codec_type_to_string(codec_type).to_string()
    } else {
        extension_fallback
    };

    // 逐行输出（两列对齐：标签列固定宽度，值列不定宽）
    output.push_str(&utils::table::format_cols_line(
        &[labels[0], &sample_rate_s],
        &[label_col_width, 0],
        "",
    ));
    // 如使用 352.8 kHz（整数比）进行 DSD 降采样，提示 foobar2000 常见显示为 384 kHz
    if let Some(proc_sr) = format.processed_sample_rate {
        if proc_sr == 352_800 {
            let note = "Note: foobar2000 often shows 384 kHz; we use 352.8 kHz integer ratio to avoid fractional resampling. / 注：foobar2000 常见显示为 384 kHz；本工具采用 352.8 kHz 的 44.1k 整数比，避免分数重采样。";
            output.push_str(&utils::table::format_cols_line(
                &["", note],
                &[label_col_width, 0],
                "",
            ));
        }
    }
    output.push_str(&utils::table::format_cols_line(
        &[labels[1], &channels_s],
        &[label_col_width, 0],
        "",
    ));
    output.push_str(&utils::table::format_cols_line(
        &[labels[2], &bits_s],
        &[label_col_width, 0],
        "",
    ));
    output.push_str(&utils::table::format_cols_line(
        &[labels[3], &bitrate_display],
        &[label_col_width, 0],
        "",
    ));
    output.push_str(&utils::table::format_cols_line(
        &[labels[4], &codec_display],
        &[label_col_width, 0],
        "",
    ));

    // 结尾分隔线（长度与标题一致）
    let sep_eq =
        utils::table::separator_for_lines(&[&constants::app_info::format_output_header(VERSION)]);
    output.push_str(&sep_eq);

    output
}

/// 根据声道数选择合适的格式化方法
pub fn format_dr_results_by_channel_count(
    results: &[DrResult],
    format: &AudioFormat,
    show_rms_peak: bool,
) -> String {
    let mut output = String::new();

    // 部分分析警告（如果跳过了损坏的音频包）
    if format.is_partial() {
        output.push_str(&format!(
            " 部分分析警告：跳过了 {} 个损坏的音频包\n",
            format.skipped_packets()
        ));
        output.push_str("    分析结果可能不完整，建议检查源文件质量。\n\n");
    }

    // 根据声道数选择格式化方法
    output.push_str(&match results.len() {
        0 => "ERROR: 无音频数据\n".to_string(),
        1 => format_mono_results(&results[0], show_rms_peak),
        2 => format_stereo_results(results, show_rms_peak),
        3..=8 => format_medium_multichannel_results(results, format),
        _ => format_large_multichannel_results(results, format),
    });

    output
}

/// 处理输出写入（文件或控制台）
pub fn write_output(output: &str, config: &AppConfig, auto_save: bool) -> AudioResult<()> {
    match &config.output_path {
        Some(output_path) => {
            // 用户指定了输出文件路径
            std::fs::write(output_path, output).map_err(AudioError::IoError)?;
            println!("Results saved / 结果已保存到: {}", output_path.display());
        }
        None => {
            if auto_save {
                // 自动保存模式：生成基于音频文件名的输出文件路径
                let parent_dir = utils::get_parent_dir(&config.input_path);
                let file_stem = utils::extract_file_stem(&config.input_path);
                let auto_output_path = parent_dir.join(format!("{file_stem}_DR_Analysis.txt"));
                std::fs::write(&auto_output_path, output).map_err(AudioError::IoError)?;
                println!(
                    "Results saved / 结果已保存到: {}",
                    auto_output_path.display()
                );
            } else {
                // 控制台输出模式
                print!("{output}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boundary_warning_high_risk() {
        // 高风险：10.48 距上边界 0.02，foobar可能测得10.53→DR11
        let warning = detect_dr_boundary_warning(10, 10.48);
        assert!(warning.is_some());
        let msg = warning.unwrap();
        assert!(msg.contains("边界风险（高）"));
        assert!(msg.contains("Boundary Risk (High)"));
        assert!(msg.contains("DR11")); // 提示可能变为DR11

        // 高风险：10.50 正好在边界上
        let warning = detect_dr_boundary_warning(10, 10.50);
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("边界风险（高）"));

        // 高风险：10.47 距上边界 0.03
        let warning = detect_dr_boundary_warning(10, 10.47);
        assert!(warning.is_some());
    }

    #[test]
    fn test_boundary_warning_medium_risk() {
        // 中风险：10.45 距上边界 0.05（刚好触发）
        let warning = detect_dr_boundary_warning(10, 10.45);
        assert!(warning.is_some());
        let msg = warning.unwrap();
        assert!(msg.contains("边界风险"));
        assert!(msg.contains("Boundary Risk"));

        // 中风险：10.46 距上边界 0.04
        let warning = detect_dr_boundary_warning(10, 10.46);
        assert!(warning.is_some());
    }

    #[test]
    fn test_boundary_warning_safe_zone() {
        // 安全：10.30 距上边界 0.20（远离边界）
        let warning = detect_dr_boundary_warning(10, 10.30);
        assert!(warning.is_none());

        // 安全：10.44 距上边界 0.06（刚好安全）
        let warning = detect_dr_boundary_warning(10, 10.44);
        assert!(warning.is_none());

        // 安全：10.10 距上边界 0.40
        let warning = detect_dr_boundary_warning(10, 10.10);
        assert!(warning.is_none());
    }

    #[test]
    fn test_boundary_warning_no_risk_when_above() {
        // 10.52 (DR11)：接近10.5下边界，距离仅0.02 dB → 有风险（双向预警）
        let warning = detect_dr_boundary_warning(11, 10.52);
        assert!(warning.is_some(), "10.52 应该接近下边界10.5，距离仅0.02");
        assert!(warning.unwrap().contains("DR10"), "应该警告可能被舍为DR10");

        // 10.60 (DR11)：距离上下边界都较远 → 无风险
        let warning = detect_dr_boundary_warning(11, 10.60);
        assert!(warning.is_none(), "10.60距离两个边界都远，应该无风险");
    }

    #[test]
    fn test_boundary_warning_direction() {
        // 验证双向预警系统（上下两个边界都检测）

        // 10.48 (DR10)：接近上边界10.5，距离仅0.02 dB → 高风险
        let warning = detect_dr_boundary_warning(10, 10.48);
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("DR11"), "应该警告可能被舍为DR11");

        // 9.52 (DR10)：接近下边界9.5，距离仅0.02 dB → 高风险（双向预警捕捉）
        let warning = detect_dr_boundary_warning(10, 9.52);
        assert!(warning.is_some(), "9.52应该接近下边界9.5，距离仅0.02");
        assert!(warning.unwrap().contains("DR9"), "应该警告可能被舍为DR9");

        // 10.29 dB → DR10，距离上边界 10.5 还有 0.21，不预警
        assert!(detect_dr_boundary_warning(10, 10.29).is_none());

        // 10.47 dB → DR10，距离上边界 10.5 只有 0.03，预警
        assert!(detect_dr_boundary_warning(10, 10.47).is_some());

        // 10.53 dB → DR11，接近下边界10.5，距离仅0.03 dB → 预警（可能被舍为DR10）
        let warning = detect_dr_boundary_warning(11, 10.53);
        assert!(warning.is_some(), "10.53应该接近下边界10.5，距离仅0.03");
        assert!(warning.unwrap().contains("DR10"));
    }
}
