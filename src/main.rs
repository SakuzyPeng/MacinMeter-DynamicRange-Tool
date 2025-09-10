//! MacinMeter DR Tool - 音频动态范围分析工具
//!
//! 基于foobar2000 DR Meter逆向分析实现的高精度DR计算工具。

use clap::{Arg, Command};
use std::path::PathBuf;
use std::process;

use macinmeter_dr_tool::{
    DrResult,
    audio::{AudioFormat, UniversalDecoder},
    core::DrCalculator,
    error::{AudioError, AudioResult},
    utils::{
        MemoryStrategySelector, ProcessingStrategy, get_memory_status_report,
        should_use_emergency_mode,
    },
};

/// 应用程序版本信息
const VERSION: &str = env!("CARGO_PKG_VERSION");
const DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");

/// 应用程序配置
#[derive(Debug)]
struct AppConfig {
    /// 输入文件路径（单文件模式）或扫描目录（批量模式）
    input_path: PathBuf,

    /// 是否为批量扫描模式（双击启动时自动启用）
    batch_mode: bool,

    /// 是否启用Sum Doubling补偿
    sum_doubling: bool,

    /// 是否显示详细信息
    verbose: bool,

    /// 输出文件路径（可选，批量模式时自动生成）
    output_path: Option<PathBuf>,

    /// 是否启用SIMD向量化优化
    enable_simd: bool,

    /// 是否启用多线程处理
    enable_multithreading: bool,
    // 🏷️ FEATURE_REMOVAL: 移除精确权重公式选项
    // 📅 移除时间: 2025-08-31
    // 🎯 统一使用最优精度模式（weighted_rms=false）
    // 💡 原因: 精确权重导致+14% RMS误差，偏离foobar2000标准
    // 🔄 回退: 如需重新启用选项，查看git历史
}

impl AppConfig {
    /// 从命令行参数创建配置
    fn from_args() -> Self {
        let matches = Command::new("dr-meter")
            .version(VERSION)
            .about(DESCRIPTION)
            .author("MacinMeter Team")
            .arg(
                Arg::new("INPUT")
                    .help("音频文件或目录路径 (支持WAV, FLAC, MP3, AAC, OGG)。如果不指定，将扫描可执行文件所在目录")
                    .required(false)  // 改为非必需
                    .index(1),
            )
            .arg(
                Arg::new("verbose")
                    .long("verbose")
                    .short('v')
                    .help("显示详细处理信息")
                    .action(clap::ArgAction::SetTrue),
            )
            .arg(
                Arg::new("output")
                    .long("output")
                    .short('o')
                    .help("输出结果到文件")
                    .value_name("FILE"),
            )
            .arg(
                Arg::new("disable-simd")
                    .long("disable-simd")
                    .help("禁用SIMD向量化优化（降低性能但提高兼容性）")
                    .action(clap::ArgAction::SetTrue),
            )
            .arg(
                Arg::new("single-thread")
                    .long("single-thread")
                    .help("禁用多线程处理（单线程模式）")
                    .action(clap::ArgAction::SetTrue),
            )
            // 🏷️ FEATURE_REMOVAL: 移除--weighted-rms参数
            // 📅 移除时间: 2025-08-31
            // 💡 原因: 精确权重模式偏离foobar2000标准，统一使用最优精度模式
            // 🔄 回退: 如需重新启用，查看git历史中的weighted-rms参数定义
            .get_matches();

        // 确定输入路径和模式
        let (input_path, batch_mode) = match matches.get_one::<String>("INPUT") {
            Some(input) => {
                let path = PathBuf::from(input);
                let is_batch = path.is_dir();
                (path, is_batch)
            }
            None => {
                // 双击启动模式：使用可执行文件所在目录
                let exe_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
                let exe_dir = exe_path
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .to_path_buf();
                (exe_dir, true) // 双击启动时自动启用批量模式
            }
        };

        Self {
            input_path,
            batch_mode,
            sum_doubling: true, // 内部强制启用Sum Doubling（用户不可见）
            verbose: matches.get_flag("verbose"),
            output_path: matches.get_one::<String>("output").map(PathBuf::from),
            enable_simd: !matches.get_flag("disable-simd"), // 默认启用，除非明确禁用
            enable_multithreading: !matches.get_flag("single-thread"), // 默认启用多线程
                                                            // 🏷️ FEATURE_REMOVAL: 移除精确权重参数解析
                                                            // 📅 移除时间: 2025-08-31
                                                            // 🎯 统一使用最优精度模式，weighted_rms固定为false
                                                            // 🔄 回退: 如需重新启用选项，查看git历史
        }
    }
}

/// 智能加载并处理音频文件，根据文件大小自动选择处理策略
///
/// 处理策略：
/// - 小文件(< 200MB): 全内存加载+处理，最佳性能
/// - 大文件(>= 200MB): 流式处理，恒定内存使用
/// - 超大文件或内存不足: 强制流式处理，确保安全
fn process_audio_file_smart(
    path: &std::path::Path,
    config: &AppConfig,
) -> AudioResult<(Vec<DrResult>, AudioFormat)> {
    // 🛡️ 安全检查：文件大小预检和内存策略分析
    let memory_selector = MemoryStrategySelector::new();
    let memory_estimate = memory_selector.analyze_file(path)?;

    // 验证处理策略的安全性
    memory_selector.validate_strategy(&memory_estimate)?;

    if config.verbose {
        println!(
            "📊 内存分析: 预估峰值 {:.1}MB, 策略: {:?}",
            memory_estimate.peak_memory as f64 / (1024.0 * 1024.0),
            memory_estimate.recommended_strategy
        );
    }

    // 获取文件扩展名
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    match memory_estimate.recommended_strategy {
        ProcessingStrategy::FullMemory => {
            // 小文件使用全内存加载+处理（最佳性能）
            if config.verbose {
                println!(
                    "💾 使用全内存模式，预估内存: {:.1}MB",
                    memory_estimate.peak_memory as f64 / (1024.0 * 1024.0)
                );
            }
            let (format, samples) = load_audio_file_full_memory(path, &extension, config.verbose)?;
            let dr_results = process_samples_with_dr_calculator(&samples, &format, config)?;
            Ok((dr_results, format))
        }
        ProcessingStrategy::StreamingBlocks | ProcessingStrategy::Adaptive => {
            // 大文件使用流式处理（内存安全）
            if config.verbose {
                println!("🌊 使用动态流式模式（智能内存管理）");

                // 显示动态内存管理状态
                if let Ok(memory_report) = get_memory_status_report() {
                    println!("{memory_report}");
                }

                // 检查是否需要紧急模式
                if let Ok(emergency) = should_use_emergency_mode()
                    && emergency
                {
                    println!("⚠️ 检测到内存压力，启用紧急模式（降级处理）");
                }
            }
            process_audio_file_streaming(path, &extension, config)
        }
    }
}

/// 使用DR计算器处理样本数据的辅助函数
fn process_samples_with_dr_calculator(
    samples: &[f32],
    format: &AudioFormat,
    config: &AppConfig,
) -> AudioResult<Vec<DrResult>> {
    // 创建DR计算器
    let calculator = DrCalculator::new(
        format.channels as usize,
        config.sum_doubling,
        format.sample_rate,
        3.0, // 官方规范的3秒块
    )?;

    // 计算DR（返回所有声道的结果）
    calculator.calculate_dr_from_samples(samples, format.channels as usize)
}

/// 全内存加载模式（小文件优化）
fn load_audio_file_full_memory(
    path: &std::path::Path,
    extension: &str,
    verbose: bool,
) -> AudioResult<(AudioFormat, Vec<f32>)> {
    if verbose {
        println!("🎵 使用统一解码器（全内存模式，.{extension}格式）...");
    }

    let decoder = UniversalDecoder::new();
    decoder.decode_full(path)
}

/// 流式处理模式（大文件安全，真正的零累积）
fn process_audio_file_streaming(
    path: &std::path::Path,
    extension: &str,
    config: &AppConfig,
) -> AudioResult<(Vec<DrResult>, AudioFormat)> {
    if config.verbose {
        println!("🌊 使用统一解码器（流式恒定内存模式，.{extension}格式）...");
    }

    let decoder = UniversalDecoder::new();

    // 先探测格式获取音频参数
    let format = decoder.probe_format(path)?;

    // 创建流式解码器
    let mut streaming_decoder = decoder.create_streaming(path)?;

    // 创建DR计算器（流式处理模式）
    let mut dr_calculator = DrCalculator::new(
        format.channels as usize,
        config.sum_doubling,
        format.sample_rate,
        3.0, // 官方规范的3秒块
    )?;

    if config.verbose {
        println!("📦 开始真正流式DR计算，块大小: 3秒，恒定内存: ~50MB...");
    }

    let mut total_chunks = 0;

    // 流式处理每个音频块
    while let Some(chunk_samples) = streaming_decoder.next_chunk()? {
        total_chunks += 1;

        if config.verbose {
            let progress = streaming_decoder.progress() * 100.0;
            if total_chunks % 10 == 0 || progress >= 100.0 {
                println!("⏳ 流式计算进度: {progress:.1}% (已处理{total_chunks}个块)");
            }
        }

        // 处理当前块（恒定内存）
        dr_calculator.process_chunk(&chunk_samples, format.channels as usize)?;

        // 强制清理内存（确保恒定内存使用）
        drop(chunk_samples);
    }

    if config.verbose {
        println!("✅ 流式DR计算完成，总处理块数: {total_chunks}");
    }

    // 完成DR计算并返回结果（多声道）
    let dr_results = dr_calculator.finalize()?;
    Ok((dr_results, format))
}

/// 扫描目录中的音频文件
fn scan_audio_files(dir_path: &std::path::Path) -> AudioResult<Vec<PathBuf>> {
    let mut audio_files = Vec::new();

    // 支持的音频格式扩展名
    let supported_extensions = ["wav", "flac", "mp3", "m4a", "aac", "ogg"];

    if !dir_path.exists() {
        return Err(AudioError::IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("目录不存在: {}", dir_path.display()),
        )));
    }

    if !dir_path.is_dir() {
        return Err(AudioError::InvalidInput(format!(
            "路径不是目录: {}",
            dir_path.display()
        )));
    }

    // 遍历目录（不递归子目录）
    let entries = std::fs::read_dir(dir_path).map_err(AudioError::IoError)?;

    for entry in entries {
        let entry = entry.map_err(AudioError::IoError)?;
        let path = entry.path();

        // 只处理文件，跳过目录
        if !path.is_file() {
            continue;
        }

        // 检查文件扩展名
        if let Some(extension) = path.extension()
            && let Some(ext_str) = extension.to_str()
        {
            let ext_lower = ext_str.to_lowercase();
            if supported_extensions.contains(&ext_lower.as_str()) {
                audio_files.push(path);
            }
        }
    }

    // 按文件名排序
    audio_files.sort();

    Ok(audio_files)
}

/// 生成批量处理结果文件路径
fn generate_batch_output_path(
    scan_dir: &std::path::Path,
    first_audio_file: Option<&std::path::Path>,
) -> PathBuf {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // 如果有音频文件，使用第一个文件名；否则使用目录名
    let base_name = if let Some(first_file) = first_audio_file {
        // 获取文件名（不包含扩展名）
        first_file
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("audio")
            .to_string()
    } else {
        // 使用目录名
        scan_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("batch")
            .to_string()
    };

    scan_dir.join(format!("{base_name}_BatchDR_Results_{timestamp}.txt"))
}

/// 生成单文件处理结果文件路径
fn generate_single_output_path(audio_file: &std::path::Path) -> PathBuf {
    let parent_dir = audio_file
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let file_stem = audio_file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("audio");

    parent_dir.join(format!("{file_stem}_DR_Analysis.txt"))
}

/// 为单个音频文件保存DR结果到对应的txt文件
fn save_individual_result(
    audio_file: &std::path::Path,
    results: &[DrResult],
    format: &AudioFormat,
    config: &AppConfig,
) -> AudioResult<()> {
    // 创建临时配置，用于生成单文件输出
    let temp_config = AppConfig {
        input_path: audio_file.to_path_buf(),
        batch_mode: false,
        sum_doubling: config.sum_doubling,
        verbose: false,    // 避免冗余输出
        output_path: None, // 让系统自动生成文件名
        enable_simd: config.enable_simd,
        enable_multithreading: config.enable_multithreading,
    };

    // 调用output_results生成单独的文件
    output_results(results, &temp_config, format, true)?; // auto_save = true

    Ok(())
}

/// 处理单个音频文件
fn process_single_audio_file(
    file_path: &std::path::Path,
    config: &AppConfig,
) -> AudioResult<(Vec<DrResult>, AudioFormat)> {
    if config.verbose {
        println!("🎵 正在加载音频文件: {}", file_path.display());
    }

    // 🎯 智能处理音频文件（自动选择内存策略）
    let (dr_results, format) = process_audio_file_smart(file_path, config)?;

    if config.verbose {
        println!("📊 音频格式信息:");
        println!("   采样率: {} Hz", format.sample_rate);
        println!("   声道数: {}", format.channels);
        println!("   位深度: {} 位", format.bits_per_sample);
        println!("   样本数: {}", format.sample_count);
        println!("   时长: {:.2} 秒", format.duration_seconds());
    }

    if config.verbose {
        println!("🧱 使用智能内存管理的块处理模式（3秒块）...");
        println!(
            "   SIMD优化: {}",
            if config.enable_simd {
                "启用"
            } else {
                "禁用"
            }
        );
        println!(
            "   多线程: {}",
            if config.enable_multithreading {
                "启用"
            } else {
                "禁用"
            }
        );
    }

    // 直接使用多声道DR结果
    Ok((dr_results, format))
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

/// 检查指定声道是否为LFE声道
fn is_lfe_channel(channel_index: usize, channel_count: u16) -> bool {
    let lfe_channels = identify_lfe_channels(channel_count);
    lfe_channels.contains(&channel_index)
}

/// 输出DR计算结果（foobar2000兼容格式）
fn output_results(
    results: &[DrResult],
    config: &AppConfig,
    format: &AudioFormat,
    auto_save: bool,
) -> AudioResult<()> {
    // 准备输出内容
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

    // 文件统计信息（需要从音频文件获取）
    let file_name = config
        .input_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Unknown");
    output.push_str(&format!("Statistics for: {file_name}\n"));

    // 从AudioFormat获取真实的音频信息（单声道样本数，匹配foobar2000）
    output.push_str(&format!("Number of samples: {}\n", format.sample_count));
    let minutes = format.duration_seconds() as u32 / 60;
    let seconds = format.duration_seconds() as u32 % 60;
    output.push_str(&format!("Duration: {minutes}:{seconds:02} \n"));

    output.push_str(
        "--------------------------------------------------------------------------------\n\n",
    );

    // foobar2000标准DR结果表格格式 - 智能多声道支持
    match results.len() {
        0 => {
            output.push_str("ERROR: 无音频数据\n");
        }
        1 => {
            // 单声道格式
            let result = &results[0];
            let peak_db = if result.peak > 0.0 {
                20.0 * result.peak.log10()
            } else {
                -f64::INFINITY
            };
            let rms_db = if result.rms > 0.0 {
                20.0 * result.rms.log10()
            } else {
                -f64::INFINITY
            };

            output.push_str("                 Mono\n\n");
            output.push_str(&format!("Peak Value:     {peak_db:.2} dB   \n"));
            output.push_str(&format!("Avg RMS:       {rms_db:.2} dB   \n"));
            output.push_str(&format!("DR channel:      {:.2} dB   \n", result.dr_value));
        }
        2 => {
            // 立体声格式 - 传统Left/Right显示
            let left_peak_db = if results[0].peak > 0.0 {
                20.0 * results[0].peak.log10()
            } else {
                -f64::INFINITY
            };
            let right_peak_db = if results[1].peak > 0.0 {
                20.0 * results[1].peak.log10()
            } else {
                -f64::INFINITY
            };
            let left_rms_db = if results[0].rms > 0.0 {
                20.0 * results[0].rms.log10()
            } else {
                -f64::INFINITY
            };
            let right_rms_db = if results[1].rms > 0.0 {
                20.0 * results[1].rms.log10()
            } else {
                -f64::INFINITY
            };

            output.push_str("                 Left              Right\n\n");
            output.push_str(&format!(
                "Peak Value:     {left_peak_db:.2} dB   ---     {right_peak_db:.2} dB   \n"
            ));
            output.push_str(&format!(
                "Avg RMS:       {left_rms_db:.2} dB   ---    {right_rms_db:.2} dB   \n"
            ));
            output.push_str(&format!(
                "DR channel:      {:.2} dB   ---     {:.2} dB   \n",
                results[0].dr_value, results[1].dr_value
            ));
        }
        3..=8 => {
            // 中等多声道格式（3-8声道） - 横向表格显示

            // 生成声道标题行 - 每列固定19字符宽度
            let mut header = String::new();
            for i in 0..results.len() {
                header.push_str(&format!("          Channel {}", i + 1));
            }
            output.push_str(&header);
            output.push_str("\n\n");

            // Peak Value行
            output.push_str("Peak Value:");
            for (i, result) in results.iter().enumerate() {
                let peak_db_str = if result.peak > 0.0 {
                    format!("{:.2} dB", 20.0 * result.peak.log10())
                } else {
                    "-1.#J dB".to_string()
                };

                if i < results.len() - 1 {
                    output.push_str(&format!("     {peak_db_str:>8}   ---"));
                } else {
                    output.push_str(&format!("     {peak_db_str:>8}   "));
                }
            }
            output.push('\n');

            // Avg RMS行
            output.push_str("Avg RMS:");
            for (i, result) in results.iter().enumerate() {
                let rms_db_str = if result.rms > 0.0 {
                    format!("{:.2} dB", 20.0 * result.rms.log10())
                } else {
                    "-1.#J dB".to_string()
                };

                if i < results.len() - 1 {
                    output.push_str(&format!("       {rms_db_str:>8}   ---"));
                } else {
                    output.push_str(&format!("       {rms_db_str:>8}   "));
                }
            }
            output.push('\n');

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
        }
        _ => {
            // 大量多声道格式（9+声道） - 横排（纵向列表）显示，智能LFE声道处理

            output.push_str(
                "              声道             Peak dB        RMS dB         DR值        备注\n\n",
            );

            for (i, result) in results.iter().enumerate() {
                let peak_db_str = if result.peak > 0.0 {
                    format!("{:.2}", 20.0 * result.peak.log10())
                } else {
                    "-1.#J".to_string()
                };

                let rms_db_str = if result.rms > 0.0 {
                    format!("{:.2}", 20.0 * result.rms.log10())
                } else {
                    "-1.#J".to_string()
                };

                let dr_value_str = if result.peak > 0.0 && result.rms > 0.0 {
                    format!("{:.2}", result.dr_value)
                } else {
                    "0.00".to_string()
                };

                // 检查是否为LFE声道或静音声道
                let note = if is_lfe_channel(i, format.channels) {
                    "LFE (已排除)"
                } else if result.peak == 0.0 && result.rms == 0.0 {
                    "静音声道"
                } else {
                    ""
                };

                output.push_str(&format!(
                    "            Channel {:2}:     {:>8} dB     {:>8} dB      {:>6} dB    {}\n",
                    i + 1,
                    peak_db_str,
                    rms_db_str,
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
        }
    }

    // foobar2000标准分隔线和底部信息
    output.push_str(
        "--------------------------------------------------------------------------------\n\n",
    );

    // Official DR Value - 排除LFE声道和静音声道
    if !results.is_empty() {
        // 筛选有效声道：排除LFE声道和静音声道
        let valid_results: Vec<&DrResult> = results
            .iter()
            .enumerate()
            .filter(|(i, result)| {
                // 排除LFE声道
                !is_lfe_channel(*i, format.channels) &&
                // 排除静音声道
                result.peak > 0.0 && result.rms > 0.0
            })
            .map(|(_, result)| result)
            .collect();

        if !valid_results.is_empty() {
            let avg_dr: f64 =
                valid_results.iter().map(|r| r.dr_value).sum::<f64>() / valid_results.len() as f64;
            output.push_str(&format!(
                "Official DR Value: DR{}\n\n",
                avg_dr.round() as i32
            ));

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

    // 音频技术信息（foobar2000标准格式）
    output.push_str(&format!("Samplerate:        {} Hz\n", format.sample_rate));
    output.push_str(&format!("Channels:          {}\n", format.channels));
    output.push_str(&format!("Bits per sample:   {}\n", format.bits_per_sample));

    // 计算码率（采样率 × 声道数 × 位深度 / 1000）
    let bitrate =
        format.sample_rate * format.channels as u32 * format.bits_per_sample as u32 / 1000;
    output.push_str(&format!("Bitrate:           {bitrate} kbps\n"));

    // 根据文件扩展名推断编解码器
    let codec = config
        .input_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.to_uppercase())
        .unwrap_or_else(|| "Unknown".to_string());
    output.push_str(&format!("Codec:             {codec}\n"));

    // foobar2000标准结尾
    output.push_str(
        "================================================================================\n",
    );

    // 输出到文件或控制台
    match &config.output_path {
        Some(output_path) => {
            // 用户指定了输出文件路径
            std::fs::write(output_path, &output).map_err(AudioError::IoError)?;
            println!("📄 结果已保存到: {}", output_path.display());
        }
        None => {
            if auto_save {
                // 自动保存模式：生成基于音频文件名的输出文件路径
                let auto_output_path = generate_single_output_path(&config.input_path);
                std::fs::write(&auto_output_path, &output).map_err(AudioError::IoError)?;
                println!("📄 结果已保存到: {}", auto_output_path.display());
            } else {
                // 控制台输出模式
                print!("{output}");
            }
        }
    }

    Ok(())
}

/// 批量处理音频文件
fn process_batch_files(config: &AppConfig) -> AudioResult<()> {
    // 扫描目录中的音频文件
    let audio_files = scan_audio_files(&config.input_path)?;

    if audio_files.is_empty() {
        println!(
            "⚠️  在目录 {} 中没有找到支持的音频文件",
            config.input_path.display()
        );
        println!("   支持的格式: WAV, FLAC, MP3, AAC, OGG");
        return Ok(());
    }

    println!("📁 扫描目录: {}", config.input_path.display());
    println!("🎵 找到 {} 个音频文件", audio_files.len());
    if config.verbose {
        for (i, file) in audio_files.iter().enumerate() {
            println!(
                "   {}. {}",
                i + 1,
                file.file_name().unwrap_or_default().to_string_lossy()
            );
        }
    }
    println!();

    // 准备批量输出
    let mut batch_output = String::new();
    batch_output.push_str("=====================================\n");
    batch_output.push_str("   MacinMeter DR Analysis Report\n");
    batch_output.push_str("   批量分析结果 (foobar2000兼容版)\n");
    batch_output.push_str("=====================================\n\n");

    // 添加标准信息到输出
    batch_output.push_str("🌿 Git分支: early-version (foobar2000兼容版)\n");
    batch_output.push_str("📐 标准来源: foobar2000 DR Meter 逆向工程\n");
    batch_output.push_str("✅ 当前模式: 高精度DR分析模式\n");
    batch_output.push_str("📊 精度目标: 基于foobar2000逆向分析的高精度实现\n");
    batch_output.push_str(&format!("📁 扫描目录: {}\n", config.input_path.display()));
    batch_output.push_str(&format!("🎵 处理文件数: {}\n\n", audio_files.len()));

    // 添加结果表头
    batch_output.push_str("文件名\tDR\tPeak(dB)\tRMS(dB)\t采样率\t声道\t时长\n");
    batch_output.push_str("--------------------------------------------------------\n");

    let mut processed_count = 0;
    let mut failed_count = 0;

    // 逐个处理音频文件
    for (index, audio_file) in audio_files.iter().enumerate() {
        println!(
            "🔄 [{}/{}] 处理: {}",
            index + 1,
            audio_files.len(),
            audio_file.file_name().unwrap_or_default().to_string_lossy()
        );

        match process_single_audio_file(audio_file, config) {
            Ok((results, format)) => {
                processed_count += 1;

                // 🆕 为每个音频文件生成单独的DR结果文件
                if let Err(e) = save_individual_result(audio_file, &results, &format, config) {
                    println!("   ⚠️  保存单独结果文件失败: {e}");
                } else if config.verbose {
                    let individual_path = generate_single_output_path(audio_file);
                    println!("   📄 单独结果已保存: {}", individual_path.display());
                }

                // 使用已获取的格式信息（无需重复加载）
                {
                    let file_name = audio_file.file_name().unwrap_or_default().to_string_lossy();

                    // foobar2000兼容模式：显示分声道结果
                    for result in &results {
                        let peak_db = 20.0 * result.peak.log10();
                        let rms_db = 20.0 * result.rms.log10();
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

                if config.verbose {
                    println!("   ✅ 处理成功");
                }
            }
            Err(e) => {
                failed_count += 1;
                println!("   ❌ 处理失败: {e}");

                let file_name = audio_file.file_name().unwrap_or_default().to_string_lossy();
                batch_output.push_str(&format!("{file_name}\t处理失败\t-\t-\t-\t-\t-\n"));
            }
        }
    }

    // 添加统计信息
    batch_output.push('\n');
    batch_output.push_str("=====================================\n");
    batch_output.push_str("批量处理统计:\n");
    batch_output.push_str(&format!("   总文件数: {}\n", audio_files.len()));
    batch_output.push_str(&format!("   成功处理: {processed_count}\n"));
    batch_output.push_str(&format!("   处理失败: {failed_count}\n"));
    batch_output.push_str(&format!(
        "   处理成功率: {:.1}%\n",
        processed_count as f64 / audio_files.len() as f64 * 100.0
    ));
    batch_output.push('\n');
    batch_output.push_str(&format!(
        "生成工具: MacinMeter DR Tool (foo_dr_meter兼容) v{VERSION}\n"
    ));

    // 确定输出文件路径
    let output_path = config.output_path.clone().unwrap_or_else(|| {
        generate_batch_output_path(&config.input_path, audio_files.first().map(|p| p.as_path()))
    });

    // 写入结果文件
    std::fs::write(&output_path, &batch_output).map_err(AudioError::IoError)?;

    println!();
    println!("📊 批量处理完成!");
    println!(
        "   成功处理: {} / {} 个文件",
        processed_count,
        audio_files.len()
    );
    if failed_count > 0 {
        println!("   失败文件: {failed_count} 个");
    }

    println!();
    println!("📄 生成的文件:");
    println!("   🗂️  批量汇总: {}", output_path.display());
    if processed_count > 0 {
        println!("   📝 单独结果: {processed_count} 个 *_DR_Analysis.txt 文件");
        if config.verbose {
            println!("   💡 每个音频文件都有对应的单独DR结果文件");
        }
    }

    Ok(())
}

/// 处理应用程序错误
fn handle_error(error: AudioError) -> ! {
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

fn main() {
    // 解析命令行参数
    let config = AppConfig::from_args();

    println!("🚀 MacinMeter DR Tool (foobar2000兼容版) v{VERSION} 启动");
    println!("📝 {DESCRIPTION}");
    println!();

    // 根据模式选择处理方式
    let result = if config.batch_mode {
        // 批量模式：扫描目录处理多个文件
        process_batch_files(&config)
    } else {
        // 单文件模式：处理单个音频文件
        match process_single_audio_file(&config.input_path, &config) {
            Ok((results, format)) => {
                // 为单文件模式输出结果
                // 如果用户未指定输出文件，则自动保存（auto_save = true）
                output_results(&results, &config, &format, config.output_path.is_none())
            }
            Err(e) => Err(e),
        }
    };

    // 处理错误
    if let Err(error) = result {
        handle_error(error);
    } else if config.verbose {
        println!("✅ 所有任务处理完成！");
    }
}
