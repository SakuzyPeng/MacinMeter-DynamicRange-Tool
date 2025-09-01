//! MacinMeter DR Tool - 音频动态范围分析工具
//!
//! 基于 Measuring_DR_ENv3.md 标准实现的高精度DR计算工具。
//! 以 dr14_t.meter 项目作为参考实现，提供符合行业标准的DR测量。

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
    /// 输入文件路径
    input_path: PathBuf,

    /// 是否启用Sum Doubling补偿
    sum_doubling: bool,

    /// 是否显示详细信息
    verbose: bool,

    /// 输出文件路径（可选）
    output_path: Option<PathBuf>,

    /// 是否启用SIMD向量化优化
    enable_simd: bool,

    /// 是否启用多线程处理
    enable_multithreading: bool,

    /// 是否启用dr14_t.meter兼容模式（实验特性）
    dr14_compat_mode: bool,
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
                    .help("音频文件路径 (支持WAV, FLAC, MP3, AAC, OGG)")
                    .required(true)
                    .index(1),
            )
            .arg(
                Arg::new("sum-doubling")
                    .long("sum-doubling")
                    .short('s')
                    .help("启用Sum Doubling补偿（交错数据）")
                    .action(clap::ArgAction::SetTrue),
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
            .arg(
                Arg::new("dr14-compat-mode")
                    .long("dr14-compat-mode")
                    .help("🧪 实验特性：模拟dr14_t.meter的预处理（44.1kHz+16bit量化）")
                    .action(clap::ArgAction::SetTrue),
            )
            .get_matches();

        Self {
            input_path: PathBuf::from(matches.get_one::<String>("INPUT").unwrap()),
            sum_doubling: matches.get_flag("sum-doubling"),
            verbose: matches.get_flag("verbose"),
            output_path: matches.get_one::<String>("output").map(PathBuf::from),
            enable_simd: !matches.get_flag("disable-simd"), // 默认启用，除非明确禁用
            enable_multithreading: !matches.get_flag("single-thread"), // 默认启用多线程
            dr14_compat_mode: matches.get_flag("dr14-compat-mode"), // 实验特性
        }
    }
}

/// 使用ffmpeg进行dr14_t.meter兼容预处理
///
/// 直接调用ffmpeg，完全模拟dr14_t.meter的预处理行为：
/// `ffmpeg -i "input" -b:a 16 -ar 44100 -y "output" -loglevel quiet`
fn preprocess_with_ffmpeg(
    input_path: &std::path::Path,
    verbose: bool,
) -> AudioResult<std::path::PathBuf> {
    use std::process::Command;

    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!(
        "dr14_compat_{}.wav",
        input_path.file_stem().unwrap_or_default().to_string_lossy()
    ));

    if verbose {
        println!("🔄 ffmpeg预处理: {} → 44.1kHz/16bit", input_path.display());
    }

    // 构建ffmpeg命令（完全模拟dr14_t.meter）
    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-i",
        &input_path.to_string_lossy(),
        "-b:a",
        "16",
        "-ar",
        "44100",
        "-y",
        &temp_file.to_string_lossy(),
        "-loglevel",
        "quiet",
    ]);

    if verbose {
        println!(
            "   执行命令: ffmpeg -i \"{}\" -b:a 16 -ar 44100 -y \"{}\" -loglevel quiet",
            input_path.display(),
            temp_file.display()
        );
    }

    // 执行ffmpeg命令
    let output = cmd
        .output()
        .map_err(|e| AudioError::DecodingError(format!("ffmpeg执行失败: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AudioError::DecodingError(format!(
            "ffmpeg处理失败: {stderr}"
        )));
    }

    // 验证输出文件存在
    if !temp_file.exists() {
        return Err(AudioError::DecodingError(
            "ffmpeg预处理后文件不存在".to_string(),
        ));
    }

    if verbose {
        println!("✅ 预处理完成: {}", temp_file.display());
    }

    Ok(temp_file)
}

/// 智能加载音频文件（自动选择解码器）
///
/// 根据文件扩展名自动选择合适的解码器：
/// - .wav -> WavDecoder (基于hound，性能优化)
/// - .flac, .mp3, .m4a, .aac, .ogg -> MultiDecoder (基于symphonia)
fn load_audio_file(
    path: &std::path::Path,
    verbose: bool,
    dr14_compat_mode: bool,
) -> AudioResult<(AudioFormat, Vec<f32>)> {
    // dr14_t.meter兼容模式：使用ffmpeg预处理
    if dr14_compat_mode {
        if verbose {
            println!("🧪 实验特性: dr14_t.meter兼容模式");
        }

        // 使用ffmpeg预处理到44.1kHz/16bit WAV
        let preprocessed_path = preprocess_with_ffmpeg(path, verbose)?;

        // 用WAV解码器加载预处理后的文件
        let mut decoder = WavDecoder::new();
        let format = decoder.load_file(&preprocessed_path)?;
        let samples = decoder.samples().to_vec();

        // 清理临时文件
        if let Err(e) = std::fs::remove_file(&preprocessed_path) {
            if verbose {
                println!("⚠️  清理临时文件失败: {e}");
            }
        } else if verbose {
            println!("🗑️  临时文件已清理");
        }

        return Ok((format, samples));
    }

    // 标准模式：保持原始音频质量
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

/// 处理单个音频文件
fn process_audio_file(config: &AppConfig) -> AudioResult<()> {
    if config.verbose {
        println!("🎵 正在加载音频文件: {}", config.input_path.display());
    }

    // 智能加载音频文件（自动选择解码器）
    let (format, samples) =
        load_audio_file(&config.input_path, config.verbose, config.dr14_compat_mode)?;

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
            true, // Measuring_DR_ENv3.md 标准模式
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
                    true, // 启用Measuring_DR_ENv3.md标准模式
                    format.sample_rate,
                )?;

                calculator.process_interleaved_samples(&samples)?;
                calculator.calculate_dr()
            },
        )?
    };

    // 输出结果
    output_results(&results, config)?;

    if config.verbose {
        println!("✅ 处理完成！");
    }

    Ok(())
}

/// 输出DR计算结果
fn output_results(results: &[DrResult], config: &AppConfig) -> AudioResult<()> {
    // 准备输出内容
    let mut output = String::new();

    // 标题
    output.push_str("=====================================\n");
    output.push_str("   MacinMeter DR Analysis Report\n");
    output.push_str("=====================================\n\n");

    // 文件信息
    output.push_str(&format!("文件: {}\n", config.input_path.display()));
    output.push_str(&format!(
        "Sum Doubling: {}\n",
        if config.sum_doubling {
            "启用"
        } else {
            "禁用"
        }
    ));
    output.push_str(&format!(
        "SIMD优化: {}\n",
        if config.enable_simd {
            "启用"
        } else {
            "禁用"
        }
    ));
    output.push_str(&format!(
        "多线程处理: {}\n",
        if config.enable_multithreading {
            "启用"
        } else {
            "禁用"
        }
    ));
    output.push('\n');

    // DR计算结果
    output.push_str("动态范围 (DR) 结果:\n");
    output.push_str("-------------------------------------\n");

    for result in results {
        // 计算dr14_t.meter兼容显示值（dB）
        let global_peak_db = 20.0 * result.global_peak.log10();
        let global_rms_db = 20.0 * result.global_rms.log10();

        output.push_str(&format!(
            "声道 {}: DR{} (RMS:{:.2}dB, Peak:{:.2}dB) [算法内部: RMS:{:.6}, Peak:{:.6}]\n",
            result.channel + 1,
            result.dr_value_rounded(),
            global_rms_db,
            global_peak_db,
            result.rms,
            result.peak
        ));
    }

    output.push('\n');

    // 平均DR值
    if results.len() > 1 {
        let avg_dr: f64 = results.iter().map(|r| r.dr_value).sum::<f64>() / results.len() as f64;
        output.push_str(&format!("平均DR值: DR{}\n", avg_dr.round() as i32));
    }

    output.push('\n');
    output.push_str("生成工具: MacinMeter DR Tool v");
    output.push_str(VERSION);
    output.push('\n');

    // 输出到文件或控制台
    match &config.output_path {
        Some(output_path) => {
            std::fs::write(output_path, &output)?;
            println!("📄 结果已保存到: {}", output_path.display());
        }
        None => {
            print!("{output}");
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

    if config.verbose {
        println!("🚀 MacinMeter DR Tool v{VERSION} 启动");
        println!("📝 {DESCRIPTION}");
        println!();
    }

    // 处理音频文件
    if let Err(error) = process_audio_file(&config) {
        handle_error(error);
    }
}
