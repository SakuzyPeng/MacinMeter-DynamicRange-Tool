//! MacinMeter DR Tool - 音频动态范围分析工具
//!
//! 基于foobar2000 DR Meter逆向分析实现的高精度DR计算工具。

use clap::{Arg, Command};
use std::path::PathBuf;
use std::process;

use macinmeter_dr_tool::{
    DrResult, SafeRunner,
    audio::{AudioFormat, MultiDecoder, WavDecoder},
    core::DrCalculator,
    error::{AudioError, AudioResult},
    processing::BatchProcessor,
};

/// 应用程序版本信息
const VERSION: &str = env!("CARGO_PKG_VERSION");
const DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");

/// 格式化数字显示（添加千位分隔符）
fn format_number(num: usize) -> String {
    if num >= 1_000_000 {
        format!("{:.1}M", num as f64 / 1_000_000.0)
    } else if num >= 1_000 {
        format!("{:.1}K", num as f64 / 1_000.0)
    } else {
        num.to_string()
    }
}

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

/// 智能加载音频文件（自动选择解码器）
///
/// 根据文件扩展名自动选择合适的解码器：
/// - .wav -> WavDecoder (基于hound，性能优化)
/// - .flac, .mp3, .m4a, .aac, .ogg -> MultiDecoder (基于symphonia)
fn load_audio_file(path: &std::path::Path, verbose: bool) -> AudioResult<(AudioFormat, Vec<f32>)> {
    // 获取文件扩展名
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    match extension.as_str() {
        "wav" => {
            // 使用专门的WAV解码器（性能优化）
            if verbose {
                println!("🎼 使用WAV专用解码器...");
            }
            let mut decoder = WavDecoder::new();
            let format = decoder.load_file(path)?;
            let samples = decoder.samples().to_vec();
            Ok((format, samples))
        }
        "flac" | "mp3" | "m4a" | "aac" | "ogg" => {
            // 使用多格式解码器
            if verbose {
                println!("🎵 使用多格式解码器 (.{extension}格式)...");
            }
            let mut decoder = MultiDecoder::new();
            let format = decoder.load_file(path)?;
            let samples = decoder.samples().to_vec();
            Ok((format, samples))
        }
        "" => Err(AudioError::FormatError("文件缺少扩展名".to_string())),
        _ => Err(AudioError::FormatError(format!(
            "不支持的音频格式: .{extension}"
        ))),
    }
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

    // 智能加载音频文件（自动选择解码器）
    let (format, samples) = load_audio_file(file_path, config.verbose)?;

    if config.verbose {
        println!("📊 音频格式信息:");
        println!("   采样率: {} Hz", format.sample_rate);
        println!("   声道数: {}", format.channels);
        println!("   位深度: {} 位", format.bits_per_sample);
        println!("   样本数: {}", format.sample_count);
        println!("   时长: {:.2} 秒", format.duration_seconds);
    }

    // 创建安全运行器
    let runner = SafeRunner::new("DR计算");

    // 决定使用哪种处理方式
    let results = if config.enable_simd || config.enable_multithreading {
        // 使用SIMD批量处理器（高性能模式）
        if config.verbose {
            println!("🚀 使用高性能批量处理器...");
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

        let batch_processor = BatchProcessor::new(config.enable_multithreading, None);

        // 显示SIMD能力信息
        if config.verbose {
            let caps = batch_processor.simd_capabilities();
            println!("💻 SIMD能力检测:");
            println!("   SSE2: {}", caps.sse2);
            println!("   SSE4.1: {}", caps.sse4_1);
            println!("   AVX: {}", caps.avx);
            println!("   推荐并行度: {}x", caps.recommended_parallelism());
        }

        let batch_result = batch_processor.process_interleaved_batch(
            &samples,
            format.channels as usize,
            format.sample_rate,
            config.sum_doubling,
            true, // foobar2000兼容模式
            // 🏷️ FEATURE_REMOVAL: 固定使用最优精度模式
            // 📅 修改时间: 2025-08-31
            // 🎯 统一使用weighted_rms=false以保持与foobar2000最优精度匹配
            // 🔄 回退: 如需重新启用选项，查看git历史
            false, // weighted_rms固定为false
        )?;

        // 显示性能统计
        if config.verbose {
            let stats = &batch_result.performance_stats;
            println!("📊 性能统计:");

            // 优化时间显示格式
            let duration_display = if stats.total_duration_us >= 1_000_000 {
                format!("{:.2}s", stats.total_duration_us as f64 / 1_000_000.0)
            } else if stats.total_duration_us >= 1_000 {
                format!("{:.1}ms", stats.total_duration_us as f64 / 1_000.0)
            } else {
                format!("{}μs", stats.total_duration_us)
            };

            // 优化处理速度显示格式
            let speed_display = if stats.samples_per_second >= 1_000_000.0 {
                format!("{:.1}M samples/s", stats.samples_per_second / 1_000_000.0)
            } else if stats.samples_per_second >= 1_000.0 {
                format!("{:.1}K samples/s", stats.samples_per_second / 1_000.0)
            } else {
                format!("{:.0} samples/s", stats.samples_per_second)
            };

            println!("   处理时间: {duration_display}");
            println!("   处理速度: {speed_display}");
            println!(
                "   处理样本: {} ({} 声道)",
                format_number(stats.total_samples),
                stats.channels_processed
            );

            // SIMD信息（仅在有意义时显示）
            if batch_result.simd_usage.used_simd || stats.simd_speedup > 1.0 {
                println!(
                    "   SIMD加速: {:.1}x (覆盖率: {:.1}%)",
                    stats.simd_speedup,
                    batch_result.simd_usage.simd_coverage * 100.0
                );
            }
        }

        batch_result.dr_results
    } else {
        // 使用传统DR计算器（兼容模式）
        runner.run_with_protection(
            &samples,
            format.channels as usize,
            format.sample_rate,
            || {
                if config.verbose {
                    println!("⚡ 使用传统计算器（兼容模式）...");
                }

                let mut calculator = DrCalculator::new_with_mode(
                    format.channels as usize,
                    config.sum_doubling,
                    true, // 启用foobar2000模式
                    format.sample_rate,
                )?;

                // 🏷️ FEATURE_REMOVAL: 固定使用最优精度模式
                // 📅 修改时间: 2025-08-31
                // 🎯 统一使用weighted_rms=false以保持与foobar2000最优精度匹配
                // 🔄 回退: 如需重新启用选项，查看git历史
                calculator.set_weighted_rms(false); // 固定为false，最优精度

                calculator.process_interleaved_samples(&samples)?;
                calculator.calculate_dr()
            },
        )?
    };

    Ok((results, format))
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
    let minutes = format.duration_seconds as u32 / 60;
    let seconds = format.duration_seconds as u32 % 60;
    output.push_str(&format!("Duration: {minutes}:{seconds:02} \n"));

    output.push_str(
        "--------------------------------------------------------------------------------\n\n",
    );

    // foobar2000标准DR结果表格格式

    // foobar2000风格的表格输出
    if results.len() >= 2 {
        // 转换为dB格式
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

        // 计算RMS的dB值
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

        // foobar2000标准表格格式
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
    } else {
        // 单声道情况的fallback
        for (i, result) in results.iter().enumerate() {
            let peak_db = if result.peak > 0.0 {
                20.0 * result.peak.log10()
            } else {
                -f64::INFINITY
            };

            let channel_name = if i == 0 {
                "Mono"
            } else {
                &format!("Channel {}", i + 1)
            };

            output.push_str(&format!(
                "{channel_name}: Peak: {peak_db:.2} dB, DR: {:.2} dB\n",
                result.dr_value
            ));
        }
    }

    // foobar2000标准分隔线和底部信息
    output.push_str(
        "--------------------------------------------------------------------------------\n\n",
    );

    // Official DR Value
    if results.len() > 1 {
        let avg_dr: f64 = results.iter().map(|r| r.dr_value).sum::<f64>() / results.len() as f64;
        output.push_str(&format!(
            "Official DR Value: DR{}\n\n",
            avg_dr.round() as i32
        ));
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
                            format.duration_seconds
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
