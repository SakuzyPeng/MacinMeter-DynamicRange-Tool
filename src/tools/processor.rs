//! 音频处理模块
//!
//! 负责音频文件的解码、DR计算和结果处理。

use super::cli::AppConfig;
use super::{formatter, utils};
use crate::{
    AudioError, AudioFormat, AudioResult, DrResult,
    audio::UniversalDecoder,
    core::{
        PeakSelectionStrategy, SilenceFilterConfig, histogram::WindowRmsAnalyzer,
        peak_selection::PeakSelector,
    },
    processing::{
        ChannelSeparator, EdgeTrimConfig, EdgeTrimReport, EdgeTrimmer, SilenceFilterChannelReport,
        SilenceFilterReport,
    },
};

/// DR 分析输出（结果 + 最终格式 + 辅助诊断）
pub type AnalysisOutput = (
    Vec<DrResult>,
    AudioFormat,
    Option<EdgeTrimReport>,
    Option<SilenceFilterReport>,
);

/// 处理单个音频文件
pub fn process_audio_file(
    path: &std::path::Path,
    config: &AppConfig,
) -> AudioResult<AnalysisOutput> {
    // 直接使用流式处理实现：零内存累积，恒定内存使用
    // 注：旧的全量加载方法已移除，避免8GB内存占用问题
    process_audio_file_streaming(path, config)
}

/// 处理单个音频文件并显示详细信息
pub fn process_single_audio_file(
    file_path: &std::path::Path,
    config: &AppConfig,
) -> AudioResult<AnalysisOutput> {
    if config.verbose {
        println!("加载音频文件 / Loading audio file: {}", file_path.display());
        println!(
            "使用流式窗口分析（3秒标准窗口）进行DR计算 / Using streaming window analysis (3-second standard window) for DR calculation"
        );
    }

    // 处理音频文件
    let (dr_results, format, trim_report, silence_report) = process_audio_file(file_path, config)?;

    if config.verbose {
        use crate::tools::utils;
        // 统一对齐：按“显示宽度”对齐左列标签，避免中英混排产生的偏移
        println!("音频格式信息 / Audio format information:");

        let labels = [
            "采样率 / Sample rate:",
            "声道数 / Channels:",
            "位深度 / Bit depth:",
            "样本数 / Sample count:",
            "时长 / Duration:",
        ];
        let widths = vec![0usize; labels.len()];
        let label_col_width = utils::table::effective_widths(&labels, &widths)
            .into_iter()
            .max()
            .unwrap_or(0);

        let sr = format!("{} Hz", format.sample_rate);
        let ch = format!("{}", format.channels);
        let bits = format!("{} bits", format.bits_per_sample);
        let samples = format!("{}", format.sample_count);
        let dur = format!("{:.2} seconds", format.duration_seconds());

        // 使用与文件输出相同的列对齐策略，但在行首增加3个空格缩进
        let line1 = utils::table::format_cols_line(&[labels[0], &sr], &[label_col_width, 0], "");
        print!("   {line1}");
        let line2 = utils::table::format_cols_line(&[labels[1], &ch], &[label_col_width, 0], "");
        print!("   {line2}");
        let line3 = utils::table::format_cols_line(&[labels[2], &bits], &[label_col_width, 0], "");
        print!("   {line3}");
        let line4 =
            utils::table::format_cols_line(&[labels[3], &samples], &[label_col_width, 0], "");
        print!("   {line4}");
        let line5 = utils::table::format_cols_line(&[labels[4], &dur], &[label_col_width, 0], "");
        print!("   {line5}");
    }

    Ok((dr_results, format, trim_report, silence_report))
}

/// 新的流式处理实现：真正的零内存累积处理
///
/// 利用WindowRmsAnalyzer的流式能力，避免将整个文件加载到内存
pub fn process_audio_file_streaming(
    path: &std::path::Path,
    config: &AppConfig,
) -> AudioResult<AnalysisOutput> {
    if config.verbose {
        println!("使用流式处理模式进行DR分析 / Using streaming processing mode for DR analysis...");
    }

    let decoder = UniversalDecoder;

    // 创建高性能流式解码器（支持并行解码）
    // 注：直接创建解码器并从中获取格式信息，避免双重 I/O 操作
    let mut streaming_decoder = if config.parallel_decoding {
        if config.verbose {
            println!(
                "启用并行解码模式 / Parallel decoding enabled ({}threads, {}batch size) - 攻击解码瓶颈 / attacking decode bottleneck",
                config.parallel_threads, config.parallel_batch_size
            );
        }
        decoder.create_streaming_parallel_with_options(
            path,
            true,
            Some(config.parallel_batch_size),
            Some(config.parallel_threads),
            config.dsd_pcm_rate,
            Some(config.dsd_gain_db),
            Some(config.dsd_filter.clone()),
        )?
    } else {
        if config.verbose {
            println!(
                "使用串行解码模式 / Using serial decoding mode (BatchPacketReader optimization)"
            );
        }
        decoder.create_streaming_with_options(
            path,
            config.dsd_pcm_rate,
            Some(config.dsd_gain_db),
            Some(config.dsd_filter.clone()),
        )?
    };

    // 从已创建的解码器获取格式信息（零额外 I/O 开销）
    if config.verbose {
        let format = streaming_decoder.format();
        println!(
            "音频格式 / Audio format: {}声道 / channels, {}Hz, {}位 / bits",
            format.channels, format.sample_rate, format.bits_per_sample
        );
        println!("开始流式解码和分析 / Starting streaming decoding and analysis...");
    }

    // 委托给核心分析引擎（消除150行重复代码）
    analyze_streaming_decoder(&mut *streaming_decoder, config)
}

/// SIMD优化窗口声道分离处理（辅助函数，内存优化版本）
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
    // 安全检查：确保analyzers数量与声道数一致
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
        channel_separator.extract_channel_into(
            window_samples,
            0, // 左声道索引
            2, // 总声道数
            left_buffer,
        );

        channel_separator.extract_channel_into(
            window_samples,
            1, // 右声道索引
            2, // 总声道数
            right_buffer,
        );

        analyzers[0].process_samples(left_buffer);
        analyzers[1].process_samples(right_buffer);
    } else {
        // 多声道（3+）：零拷贝单次遍历跨步处理
        // 使用 process_samples_strided 直接从交错样本提取并处理每个声道
        // 性能收益：单次遍历 vs N次遍历，零Vec分配 vs N个Vec
        // 对Atmos场景（7.1.4=12ch, 9.1.6=16ch）尤为关键
        for (channel_idx, analyzer) in analyzers.iter_mut().enumerate() {
            analyzer.process_samples_strided(window_samples, channel_idx, channel_count as usize);
        }
    }
}

/// 内联辅助函数：执行缓冲区compact操作（统一逻辑，减少重复）
#[inline(always)]
fn compact_buffer(
    sample_buffer: &mut Vec<f32>,
    buffer_offset: &mut usize,
    verbose: bool,
    reason: &str,
) {
    if verbose {
        println!(
            "{}: 移除前{}个样本 ({:.1}KB → {:.1}KB)",
            reason,
            *buffer_offset,
            sample_buffer.len() * 4 / 1024,
            (sample_buffer.len() - *buffer_offset) * 4 / 1024
        );
    }
    sample_buffer.drain(0..*buffer_offset);
    *buffer_offset = 0;
}

/// 核心DR分析引擎（私有函数）：处理任何StreamingDecoder实现
///
/// 包含完整的流式DR分析流程：声道检查→窗口分析→DR计算
/// 消除process_audio_file_streaming和process_streaming_decoder的~150行重复代码
fn analyze_streaming_decoder(
    streaming_decoder: &mut dyn crate::audio::StreamingDecoder,
    config: &AppConfig,
) -> AudioResult<AnalysisOutput> {
    #[cfg(feature = "flame-prof")]
    let _guard_processing = {
        let enabled = std::env::var("DR_FLAME").map(|v| v == "1").unwrap_or(false);
        let scope = std::env::var("DR_FLAME_SCOPE").unwrap_or_else(|_| "app".to_string());
        if enabled && scope == "processing" {
            match pprof::ProfilerGuard::new(250) {
                Ok(g) => Some(g),
                Err(e) => {
                    eprintln!(
                        "[WARNING] 启用 processing 范围火焰图采样失败 / Failed to enable processing scope flame graph sampling: {e}"
                    );
                    None
                }
            }
        } else {
            None
        }
    };
    let format = streaming_decoder.format();

    // 多声道支持：基于foobar2000 DR Meter实测行为
    // 每个声道独立计算DR，最终Official DR为算术平均值（四舍五入到整数）

    // 样本数最小值在流式解码结束后基于"实际解码帧数"再校验，
    // 以兼容未知总长度（如部分 Opus 流）场景，避免误判。

    // 为每个声道创建独立的WindowRmsAnalyzer（流式处理核心）
    let silence_filter_config = config
        .silence_filter_threshold_db
        .map(SilenceFilterConfig::enabled)
        .unwrap_or_else(SilenceFilterConfig::disabled);

    let mut analyzers: Vec<WindowRmsAnalyzer> = (0..format.channels)
        .map(|_| {
            WindowRmsAnalyzer::with_silence_filter(
                format.sample_rate,
                config.sum_doubling_enabled(),
                silence_filter_config,
            )
        })
        .collect();

    // 创建SIMD优化的声道分离器
    let channel_separator = ChannelSeparator::new();

    // 创建边缘裁切器（如果启用）
    let mut trim_config_applied: Option<EdgeTrimConfig> = None;
    let mut edge_trimmer = if let Some(threshold_db) = config.edge_trim_threshold_db {
        let min_run_ms = config
            .edge_trim_min_run_ms
            .unwrap_or_else(|| EdgeTrimConfig::default().min_run_ms);
        let trim_config = EdgeTrimConfig::enabled(threshold_db, min_run_ms);
        trim_config_applied = Some(trim_config);

        if config.verbose {
            println!(
                "[EXPERIMENTAL] Enable edge trimming / 启用首尾边缘裁切: threshold / 阈值 {threshold_db:.1} dBFS, min duration / 最小持续 {min_run_ms:.0} ms, hysteresis / 迟滞 {:.0} ms",
                trim_config.hysteresis_ms
            );
        }

        Some(EdgeTrimmer::new(
            trim_config,
            format.channels as usize,
            format.sample_rate,
        ))
    } else {
        None
    };

    let mut trim_report: Option<EdgeTrimReport> = None;
    let mut silence_filter_report: Option<SilenceFilterReport> = None;

    // 使用集中管理的窗口时长常量（foobar2000标准）
    use super::constants::buffers::{
        BUFFER_CAPACITY_MULTIPLIER, MAX_BUFFER_RATIO, window_alignment_enabled,
    };
    use super::constants::dr_analysis::{WINDOW_DURATION_COEFFICIENT, WINDOW_DURATION_SECONDS};

    // 窗口长度计算 - foobar2000精确公式
    // 逆向分析（sub_180007FB0）：window_samples = floor(sample_rate * 3.004081632653061)
    // 然后乘以声道数得到总样本数
    let window_samples_per_channel =
        (format.sample_rate as f64 * WINDOW_DURATION_COEFFICIENT).floor() as usize;
    let window_size_samples = window_samples_per_channel * (format.channels as usize);

    // 内存优化策略：预分配sample_buffer容量（减少扩容抖动）
    // 通过内部策略开关控制（默认启用，debug模式可通过环境变量禁用）
    let window_align_enabled = window_alignment_enabled();
    let mut sample_buffer = if window_align_enabled {
        Vec::with_capacity(window_size_samples * BUFFER_CAPACITY_MULTIPLIER)
    } else {
        Vec::new()
    };

    // 内存优化策略：引入offset+compact机制（消除每窗口drain的内存搬移）
    let mut buffer_offset = 0usize;
    // Compact阈值：当已处理样本占比超过50%时触发compact
    const COMPACT_THRESHOLD_RATIO: f64 = 0.5;

    // 内存优化策略：预分配声道分离缓冲区（复用，避免每窗口分配）
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
            "窗口配置 / Window config: {:.1}秒 / seconds = {} 样本 / samples ({}Hz × {} 声道 / channels)",
            WINDOW_DURATION_SECONDS, window_size_samples, format.sample_rate, format.channels
        );
        println!(
            "内存优化 / Memory optimization: 预分配声道缓冲区 / pre-allocate channel buffer ({channel_buffer_capacity} samples)"
        );
        println!(
            "缓冲管理 / Buffer management: offset+compact (阈值 / threshold: {:.0}%)",
            COMPACT_THRESHOLD_RATIO * 100.0
        );
        if window_align_enabled {
            println!(
                "样本缓冲 / Sample buffer: 预分配 {:.1}×窗口 / pre-allocated to {:.1}x window size, 硬上限 / hard limit: {:.1}x",
                BUFFER_CAPACITY_MULTIPLIER as f64,
                BUFFER_CAPACITY_MULTIPLIER as f64,
                MAX_BUFFER_RATIO
            );
        }
    }

    // 智能缓冲流式处理：积累chunk到标准窗口大小，保持算法精度
    while let Some(chunk_samples) = streaming_decoder.next_chunk()? {
        total_chunks += 1;

        // 首尾边缘裁切（如果启用）
        let processed_samples = if let Some(ref mut trimmer) = edge_trimmer {
            trimmer.process_chunk(&chunk_samples)
        } else {
            chunk_samples
        };

        // 修复：累加实际处理后的样本数（启用裁切时会减少）
        total_samples_processed += processed_samples.len() as u64;

        // 积累chunk到缓冲区
        sample_buffer.extend_from_slice(&processed_samples);

        if config.verbose && total_chunks % 500 == 0 {
            let progress = streaming_decoder.progress() * 100.0;
            println!(
                "[PROGRESS] Smart buffer progress / 智能缓冲进度: {progress:.1}% (processed / 已处理 {total_chunks} chunks, buffer / 缓冲: {:.1}KB, offset / 偏移: {buffer_offset})",
                sample_buffer.len() * 4 / 1024
            );
        }

        // 当积累到完整窗口时，处理并移动offset（消除drain的内存搬移）
        while sample_buffer.len() - buffer_offset >= window_size_samples {
            windows_processed += 1;

            if config.verbose && windows_processed % 20 == 0 {
                println!(
                    "处理第 / Processing window #{windows_processed} {WINDOW_DURATION_SECONDS:.1}秒 / second standard window..."
                );
            }

            // 提取一个完整的标准窗口（从offset开始）
            let window_samples = &sample_buffer[buffer_offset..buffer_offset + window_size_samples];

            // 使用SIMD优化的声道分离处理（保持窗口完整性，复用缓冲区）
            process_window_with_simd_separation(
                window_samples,
                format.channels as u32,
                &channel_separator,
                &mut analyzers,
                &mut left_buffer,
                &mut right_buffer,
            );

            // Offset+compact优化：仅移动offset，延迟实际内存搬移
            buffer_offset += window_size_samples;

            // 硬上限优化：防止缓冲区无限增长
            // 仅在窗口对齐优化启用时执行硬上限检查
            if window_align_enabled {
                let max_buffer_size = (window_size_samples as f64 * MAX_BUFFER_RATIO) as usize;
                if sample_buffer.len() > max_buffer_size && buffer_offset > window_size_samples {
                    compact_buffer(
                        &mut sample_buffer,
                        &mut buffer_offset,
                        config.verbose,
                        &format!(
                            "Trigger hard limit compact / 触发硬上限Compact: buffer exceeded / 缓冲区超过 {MAX_BUFFER_RATIO:.1}×window / 窗口"
                        ),
                    );
                }
                // Compact触发：当已处理样本占比超过阈值时，执行一次性内存整理
                else if buffer_offset > 0
                    && buffer_offset as f64 / sample_buffer.len() as f64 > COMPACT_THRESHOLD_RATIO
                {
                    compact_buffer(
                        &mut sample_buffer,
                        &mut buffer_offset,
                        config.verbose,
                        "Executing compact / 执行Compact",
                    );
                }
            }
            // 窗口对齐优化禁用时，仅使用compact阈值机制
            else if buffer_offset > 0
                && buffer_offset as f64 / sample_buffer.len() as f64 > COMPACT_THRESHOLD_RATIO
            {
                compact_buffer(
                    &mut sample_buffer,
                    &mut buffer_offset,
                    config.verbose,
                    "Executing compact / 执行Compact",
                );
            }
        }
    }

    // 处理边缘裁切的尾部缓冲区并输出诊断
    if let Some(trimmer) = edge_trimmer {
        let (final_chunk, trim_stats) = trimmer.finalize();
        // 将尾部缓冲区内容加入sample_buffer
        if !final_chunk.is_empty() {
            total_samples_processed += final_chunk.len() as u64;
            sample_buffer.extend_from_slice(&final_chunk);
        }

        if let Some(cfg) = trim_config_applied {
            trim_report = Some(EdgeTrimReport {
                config: cfg,
                stats: trim_stats,
            });
        }

        // 输出裁切诊断信息（包含详细的参数和样本统计）
        if config.verbose
            || trim_stats.leading_samples_trimmed > 0
            || trim_stats.trailing_samples_trimmed > 0
        {
            let leading_sec =
                trim_stats.leading_duration_sec(format.sample_rate, format.channels as usize);
            let trailing_sec =
                trim_stats.trailing_duration_sec(format.sample_rate, format.channels as usize);
            let total_sec =
                trim_stats.total_duration_sec(format.sample_rate, format.channels as usize);
            let total_trimmed =
                trim_stats.leading_samples_trimmed + trim_stats.trailing_samples_trimmed;

            println!("Edge trimming diagnostics (experimental) / 边缘裁切诊断（实验功能）:");
            if let Some(cfg) = trim_config_applied {
                println!(
                    "   阈值: {:.1} dBFS, 最小持续: {:.0}ms, 迟滞: {:.0}ms",
                    cfg.threshold_db, cfg.min_run_ms, cfg.hysteresis_ms
                );
            }

            if trim_stats.leading_samples_trimmed > 0 {
                println!(
                    "   首部 / Leading: 裁切 / trimmed {} 样本 / samples ({:.3}秒 / seconds)",
                    trim_stats.leading_samples_trimmed, leading_sec
                );
            } else {
                println!(
                    "   首部 / Leading: 保留全部（无符合min_run的静音段）/ Kept all (no silence segments matching min_run)"
                );
            }

            if trim_stats.trailing_samples_trimmed > 0 {
                println!(
                    "   尾部 / Trailing: 裁切 / trimmed {} 样本 / samples ({:.3}秒 / seconds)",
                    trim_stats.trailing_samples_trimmed, trailing_sec
                );
            } else {
                println!(
                    "   尾部 / Trailing: 保留全部（无符合min_run的静音段）/ Kept all (no silence segments matching min_run)"
                );
            }

            if total_trimmed > 0 {
                println!(
                    "   总计 / Total: 裁切 / trimmed {total_trimmed} 样本 / samples，损失 / lost {total_sec:.3}秒 / seconds音频内容 / audio content"
                );
            } else {
                println!(
                    "   总计 / Total: 无裁切（边缘静音均短于min_run阈值）/ No trimming (all edge silences shorter than min_run threshold)"
                );
            }
        }
    }

    // 处理最后剩余的不足标准窗口大小的样本（从offset开始）
    //
    // 尾块处理策略说明：
    // 末尾不足3秒的尾块直接参与计算（符合多数实现标准）：
    // - 尾块样本计入 20% RMS 统计（通过 WindowRmsAnalyzer.process_samples）
    // - 尾块峰值参与峰值检测（主Peak、次Peak更新）
    // - 此行为与 foobar2000 DR Meter 一致，确保完整音频内容被分析
    let remaining_samples = sample_buffer.len() - buffer_offset;
    if remaining_samples > 0 {
        if config.verbose {
            println!(
                "处理最后剩余样本 / Processing remaining samples: {} ({:.2} seconds)...",
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
            "流式处理完成 / Streaming processing completed: {} chunks processed, {} M samples total",
            total_chunks,
            total_samples_processed / 1_000_000
        );
        println!("计算最终DR值 / Calculating final DR value...");
    }

    // 最小样本数校验（基于实际解码帧数）
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
            音频文件需要足够的样本用于RMS计算和峰值检测。"
        )));
    }

    // 从每个WindowRmsAnalyzer获取最终DR结果
    let mut dr_results = Vec::new();

    for (channel_idx, analyzer) in analyzers.iter().enumerate() {
        // 使用WindowRmsAnalyzer的20%采样算法
        let rms_20_percent = analyzer.calculate_20_percent_rms();

        // 获取峰值信息
        let window_primary_peak = analyzer.get_largest_peak();
        let window_secondary_peak = analyzer.get_second_largest_peak();

        // 使用官方峰值选择策略系统（与foobar2000一致）
        let peak_strategy = PeakSelectionStrategy::default(); // PreferSecondary
        let peak_for_dr = peak_strategy.select_peak(window_primary_peak, window_secondary_peak);

        // 计算DR值：DR = -20 * log10(RMS / Peak)
        let dr_value = if peak_for_dr > 0.0 && rms_20_percent > 0.0 {
            -20.0 * (rms_20_percent / peak_for_dr).log10()
        } else {
            0.0
        };

        // 样本计数说明：
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

    if let Some(threshold_db) = config.silence_filter_threshold_db {
        let mut channel_reports = Vec::with_capacity(analyzers.len());
        for (idx, analyzer) in analyzers.iter().enumerate() {
            let (valid_windows, filtered_windows, total_windows) = analyzer.window_statistics();
            channel_reports.push(SilenceFilterChannelReport {
                channel_index: idx,
                valid_windows,
                filtered_windows,
                total_windows,
            });
        }

        if config.verbose {
            println!(
                "静音过滤诊断 / Silence filtering diagnostics: 阈值 / threshold {threshold_db:.1} dBFS"
            );
            for channel in &channel_reports {
                if channel.total_windows == 0 {
                    println!(
                        "   • 声道 / Channel {}: 无窗口参与（文件过短）/ No analysis windows (file too short)",
                        channel.channel_index + 1
                    );
                } else if channel.filtered_windows > 0 {
                    println!(
                        "   • 声道 / Channel {}: 过滤 / filtered {}/{} 窗口 / windows ({:.2}%) - 有效窗口 / valid windows {}",
                        channel.channel_index + 1,
                        channel.filtered_windows,
                        channel.total_windows,
                        channel.filtered_percent(),
                        channel.valid_windows,
                    );
                } else {
                    println!(
                        "   • 声道 / Channel {}: No silence windows detected / 未检测到静音窗口 (retained all / 保留全部 {} windows)",
                        channel.channel_index + 1,
                        channel.total_windows
                    );
                }
            }
        }

        silence_filter_report = Some(SilenceFilterReport {
            threshold_db,
            channels: channel_reports,
        });
    }

    if config.verbose {
        println!(
            "DR计算完成 / DR calculation completed, {} channels total",
            dr_results.len()
        );
    }

    // 获取包含实际样本数的最终格式信息（关键修复：AAC等格式）
    let mut final_format = streaming_decoder.format();

    // 检测截断：比较预期样本数与实际解码样本数
    // 如果实际处理的样本少于预期，标记为部分分析（is_partial）
    let expected_samples = final_format.sample_count;
    let actual_samples = total_samples_processed / final_format.channels as u64;

    // 调试输出：了解样本数差异
    if config.verbose {
        eprintln!(
            "[DEBUG] 样本数统计: 预期={expected_samples}, 实际={actual_samples}, 总交错样本={total_samples_processed}"
        );
    }

    // 若启用了首尾裁切，将格式信息同步为裁切后的样本数，避免误判“部分分析”
    if config.edge_trim_threshold_db.is_some() {
        final_format.update_sample_count(actual_samples);
    }

    if actual_samples < final_format.sample_count {
        let skipped_approx = (expected_samples - actual_samples) as usize;
        if config.verbose {
            println!(
                "[WARNING] 检测到文件截断 / File truncation detected: 预期 / expected {expected_samples} samples, 实际解码 / actual {actual_samples} samples (缺少约 / missing ~{skipped_approx})"
            );
        }
        // 若确实是编码损坏导致的缺失，则标记部分分析；裁切场景已通过 update_sample_count 避免进入此分支
        final_format.mark_as_partial(skipped_approx);
    } else if actual_samples > expected_samples && config.verbose {
        eprintln!(
            "[WARNING] Actual decoded samples exceed expected / 实际解码样本多于预期: actual / 实际 {actual_samples} > expected / 预期 {expected_samples}"
        );
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
            eprintln!("FlameGraph(processing) generated successfully / 生成成功: {out_path}");
        }
    }

    Ok((dr_results, final_format, trim_report, silence_filter_report))
}

/// 处理StreamingDecoder进行DR分析（插件专用API）
///
/// 为插件提供的零算法重复接口，接受任何实现StreamingDecoder的对象
pub fn process_streaming_decoder(
    streaming_decoder: &mut dyn crate::audio::StreamingDecoder,
    config: &AppConfig,
) -> AudioResult<AnalysisOutput> {
    if config.verbose {
        println!("使用StreamingDecoder进行DR分析 / Using StreamingDecoder for DR analysis...");
    }

    // 直接委托给核心分析引擎（消除150行重复代码）
    analyze_streaming_decoder(streaming_decoder, config)
}

/// 输出DR计算结果（foobar2000兼容格式）
pub fn output_results(
    results: &[DrResult],
    config: &AppConfig,
    format: &AudioFormat,
    edge_trim_report: Option<EdgeTrimReport>,
    silence_filter_report: Option<SilenceFilterReport>,
    auto_save: bool,
) -> AudioResult<()> {
    // 使用模块化的方法组装输出内容
    let mut output = String::new();

    // 1. 创建头部信息
    output.push_str(&formatter::create_output_header(
        config,
        format,
        edge_trim_report,
        silence_filter_report,
    ));

    // 2. 根据声道数格式化DR结果
    output.push_str(&formatter::format_dr_results_by_channel_count(
        results,
        format,
        config.show_rms_peak,
    ));

    // 3. 添加标准分隔线（长度与单文件标题一致）
    let header_line =
        crate::tools::constants::app_info::format_output_header(env!("CARGO_PKG_VERSION"));
    let sep_dash = crate::tools::utils::table::separator_for_lines_with_char(&[&header_line], '-');
    output.push_str(&sep_dash);
    output.push('\n');

    // 4. 计算并添加Official DR Value
    output.push_str(&formatter::calculate_official_dr(
        results,
        format,
        config.exclude_lfe,
    ));

    // 5. 添加音频技术信息
    output.push_str(&formatter::format_audio_info(config, format));

    // 6. 写入输出（文件或控制台）
    formatter::write_output(&output, config, auto_save)
}

/// 批量处理的单个文件结果添加到批量输出
/// 批量预警信息
#[derive(Debug, Clone)]
pub struct BatchWarningInfo {
    pub file_name: String,
    pub official_dr: i32,
    pub precise_dr: f64,
    pub risk_level: formatter::BoundaryRiskLevel,
    pub direction: formatter::BoundaryDirection,
    pub distance: f64,
}

pub fn add_to_batch_output(
    batch_output: &mut String,
    results: &[DrResult],
    format: &AudioFormat,
    file_path: &std::path::Path,
    exclude_lfe: bool,
) -> Option<BatchWarningInfo> {
    let file_name = utils::extract_filename_lossy(file_path);

    // 使用统一的DR聚合函数（修复：与单文件口径一致，排除LFE+静音）
    match formatter::compute_official_precise_dr(results, format, exclude_lfe) {
        Some((official_dr, precise_dr, _excluded_count, excluded_lfe)) => {
            // 使用统一两列对齐风格，尾字段包含文件名与注记
            let col1 = format!("DR{official_dr}");
            let col2 = format!("{precise_dr:.2} dB");
            let tail = if excluded_lfe > 0 {
                format!("{file_name} [LFE excluded / 已剔除LFE]")
            } else if exclude_lfe && !format.has_channel_layout_metadata && format.channels > 2 {
                format!("{file_name} [LFE requested, layout missing / 请求LFE剔除，未检测布局]")
            } else {
                file_name.clone()
            };
            batch_output.push_str(&utils::table::format_two_cols_line(&col1, &col2, &tail));

            // 检测边界风险
            formatter::detect_boundary_risk_level(official_dr, precise_dr).map(
                |(risk_level, direction, distance)| BatchWarningInfo {
                    file_name,
                    official_dr,
                    precise_dr,
                    risk_level,
                    direction,
                    distance,
                },
            )
        }
        None => {
            batch_output.push_str(&format!("{:<17}{:<17}{}\n", "-", "无有效声道", file_name));
            None
        }
    }
}

/// 批量处理失败文件的结果添加到批量输出
pub fn add_failed_to_batch_output(batch_output: &mut String, file_path: &std::path::Path) {
    let file_name = utils::extract_filename_lossy(file_path);
    // 使用固定宽度对齐（与成功结果格式一致）
    batch_output.push_str(&format!("{:<17}{:<17}{}\n", "-", "处理失败", file_name));
}

/// 为单个文件生成独立的DR结果文件
pub fn save_individual_result(
    results: &[DrResult],
    format: &AudioFormat,
    audio_file: &std::path::Path,
    config: &AppConfig,
    edge_trim_report: Option<EdgeTrimReport>,
    silence_filter_report: Option<SilenceFilterReport>,
) -> AudioResult<()> {
    let temp_config = AppConfig {
        input_path: audio_file.to_path_buf(),
        verbose: false,
        output_path: None,
        parallel_decoding: false,
        parallel_batch_size: super::constants::defaults::PARALLEL_BATCH_SIZE,
        parallel_threads: super::constants::defaults::PARALLEL_THREADS,
        parallel_files: None, // 单文件处理不需要并行
        silence_filter_threshold_db: None,
        edge_trim_threshold_db: None,
        edge_trim_min_run_ms: None,
        exclude_lfe: false,
        show_rms_peak: config.show_rms_peak,
        dsd_pcm_rate: config.dsd_pcm_rate,
        dsd_gain_db: config.dsd_gain_db,
        dsd_filter: config.dsd_filter.clone(),
    };

    if let Err(e) = output_results(
        results,
        &temp_config,
        format,
        edge_trim_report,
        silence_filter_report,
        true,
    ) {
        eprintln!("   [WARNING] 保存单独结果文件失败 / Failed to save individual result file: {e}");
    } else if config.verbose {
        let parent_dir = utils::get_parent_dir(audio_file);
        let file_stem = utils::extract_file_stem(audio_file);
        let individual_path = parent_dir.join(format!("{file_stem}_DR_Analysis.txt"));
        println!(
            "   Individual result saved / 单独结果已保存: {}",
            individual_path.display()
        );
    }

    Ok(())
}
