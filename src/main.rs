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

/// 🎯 **dr14兼容性**: 计算符合dr14_t.meter标准的整曲db_rms
///
/// 精确复刻dr14_t.meter的计算口径：
/// 1. 各声道线性RMS: rms_c = sqrt(2 * sum(y_c^2) / N_frames)
/// 2. 线性均值: y_rms_mean = (Σ_c rms_c) / C  
/// 3. 展示: db_rms = 20 * log10(y_rms_mean)
///
/// ⚠️ **关键修正**: 整曲db_rms使用**全部样本**，不排除最后1个样本
/// (尾窗"减1样本"逻辑仅适用于窗口RMS，不适用于整曲RMS)
///
/// # 参数
///
/// * `samples` - 交错音频样本数据
/// * `channels` - 声道数量
///
/// # 返回值
///
/// 返回符合dr14_t.meter标准的db_rms值
fn compute_dr14_display_rms_db(samples: &[f32], channels: usize) -> f64 {
    let frames = samples.len() / channels;
    if frames == 0 {
        return f64::NEG_INFINITY;
    }

    // 🎯 **关键修正**: 整曲db_rms使用全部帧，不排除最后1帧
    // (尾窗"丢1样本"逻辑仅用于窗口处理，不用于整曲RMS)
    let used_frames = frames;

    // 各声道线性RMS（使用全部样本）
    let mut sum_sq = vec![0.0f64; channels];
    for n in 0..used_frames {
        for ch in 0..channels {
            let s = samples[n * channels + ch] as f64;
            sum_sq[ch] += s * s;
        }
    }

    let mut rms = vec![0.0f64; channels];
    for ch in 0..channels {
        rms[ch] = (2.0 * sum_sq[ch] / used_frames as f64).sqrt();
    }

    // 🔍 调试输出：按用户要求一次性打印所有关键信息
    println!("🔍 整曲RMS计算调试 (dr14兼容模式):");
    for (ch, &rms_val) in rms.iter().enumerate() {
        let sum_sq_val = sum_sq[ch];
        let db_val = 20.0 * rms_val.log10();
        println!(
            "  声道{ch}: sum_sq = {sum_sq_val:.6e}, frames = {used_frames}, r_ch = {rms_val:.8} (线性), dB_ch = {db_val:.2} dB"
        );
    }

    // 线性均值 → dB（关键：在线性域平均，然后统一转dB）
    let mean_linear = rms.iter().sum::<f64>() / channels as f64;
    let db_rms = 20.0 * mean_linear.log10();

    println!(
        "  表格RMS: r_mean = {mean_linear:.8} (线性均值), db_rms = {db_rms:.2} dB (20*log10(r_mean))"
    );
    println!(
        "  总样本数: {}, 帧数: {}, 声道数: {}",
        samples.len(),
        used_frames,
        channels
    );

    db_rms
}

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

    /// 是否启用dr14_t.meter兼容模式（实验特性）
    dr14_compat_mode: bool,

    /// 是否输出详细计算调试信息
    debug_calculation: bool,
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
            .arg(
                Arg::new("debug-calculation")
                    .long("debug-calculation")
                    .help("🔍 输出详细的DR计算过程（调试用）")
                    .action(clap::ArgAction::SetTrue),
            )
            .get_matches();

        // 确定输入路径
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
            sum_doubling: matches.get_flag("sum-doubling"),
            verbose: matches.get_flag("verbose"),
            output_path: matches.get_one::<String>("output").map(PathBuf::from),
            enable_simd: !matches.get_flag("disable-simd"), // 默认启用，除非明确禁用
            enable_multithreading: !matches.get_flag("single-thread"), // 默认启用多线程
            dr14_compat_mode: matches.get_flag("dr14-compat-mode"), // 实验特性
            debug_calculation: matches.get_flag("debug-calculation"), // 调试信息
        }
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
        if let Some(extension) = path.extension() {
            if let Some(ext_str) = extension.to_str() {
                let ext_lower = ext_str.to_lowercase();
                if supported_extensions.contains(&ext_lower.as_str()) {
                    audio_files.push(path);
                }
            }
        }
    }

    // 按文件名排序
    audio_files.sort();

    Ok(audio_files)
}

/// 生成批量处理结果文件路径
fn generate_batch_output_path(scan_dir: &std::path::Path) -> PathBuf {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    scan_dir.join(format!("DR_Analysis_Results_{timestamp}.txt"))
}

/// 显示标准分支信息
fn display_standard_info(dr14_compat_mode: bool) {
    println!("📋 项目分支和标准信息:");
    println!("   🌿 Git分支: master (主线分支)");
    println!("   📐 标准来源: Measuring_DR_ENv3.md");
    println!("   🎯 参考实现: dr14_t.meter 项目对比验证");

    if dr14_compat_mode {
        println!("   🧪 当前模式: dr14_t.meter 兼容模式");
        println!("   🔧 预处理: 44.1kHz + 16bit 量化 (需要 ffmpeg)");
        println!("   📊 精度目标: 99.75% 匹配 dr14_t.meter 结果");
    } else {
        println!("   ✅ 当前模式: 标准模式 (推荐)");
        println!("   🔧 预处理: 保持原始音频质量");
        println!("   📊 精度目标: 符合 Measuring_DR_ENv3.md 规范");
    }

    println!("   🏠 项目主页: https://github.com/SakuzyPeng/MacinMeter-DynamicRange-Tool");
    println!();
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
fn process_single_audio_file(
    file_path: &std::path::Path,
    config: &AppConfig,
) -> AudioResult<Vec<DrResult>> {
    if config.verbose {
        println!("🎵 正在加载音频文件: {}", file_path.display());
    }

    // 智能加载音频文件（自动选择解码器）
    let (format, samples) = load_audio_file(file_path, config.verbose, config.dr14_compat_mode)?;

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
                calculator.calculate_dr_with_debug(config.debug_calculation)
            },
        )?
    };

    Ok(results)
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

    // 显示标准信息
    display_standard_info(config.dr14_compat_mode);

    // 准备批量输出
    let mut batch_output = String::new();
    batch_output.push_str("=====================================\n");
    batch_output.push_str("   MacinMeter DR Analysis Report\n");
    batch_output.push_str("   批量分析结果\n");
    batch_output.push_str("=====================================\n\n");

    // 添加标准信息到输出
    batch_output.push_str("🌿 Git分支: master (主线分支)\n");
    batch_output.push_str("📐 标准来源: Measuring_DR_ENv3.md\n");
    if config.dr14_compat_mode {
        batch_output.push_str("🧪 当前模式: dr14_t.meter 兼容模式\n");
        batch_output.push_str("📊 精度目标: 99.75% 匹配 dr14_t.meter 结果\n");
    } else {
        batch_output.push_str("✅ 当前模式: 标准模式 (推荐)\n");
        batch_output.push_str("📊 精度目标: 符合 Measuring_DR_ENv3.md 规范\n");
    }
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
            Ok(results) => {
                processed_count += 1;

                // 加载格式信息（用于显示）
                if let Ok((format, samples)) =
                    load_audio_file(audio_file, false, config.dr14_compat_mode)
                {
                    let file_name = audio_file.file_name().unwrap_or_default().to_string_lossy();

                    if config.dr14_compat_mode {
                        // dr14兼容模式：显示统一结果
                        let avg_dr: f64 =
                            results.iter().map(|r| r.dr_value).sum::<f64>() / results.len() as f64;
                        let dr14_dr = avg_dr.round() as i32;
                        let global_max_peak =
                            results.iter().map(|r| r.global_peak).fold(0.0f64, f64::max);
                        let dr14_peak_db = 20.0 * global_max_peak.log10();
                        let dr14_rms_db =
                            compute_dr14_display_rms_db(&samples, format.channels as usize);

                        batch_output.push_str(&format!(
                            "{}\tDR{}\t{:.2}\t{:.2}\t{}Hz\t{}\t{:.1}s\n",
                            file_name,
                            dr14_dr,
                            dr14_peak_db,
                            dr14_rms_db,
                            format.sample_rate,
                            format.channels,
                            format.duration_seconds
                        ));
                    } else {
                        // 标准模式：显示分声道结果
                        for result in results {
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
    batch_output.push_str(&format!("生成工具: MacinMeter DR Tool v{VERSION}\n"));

    // 确定输出文件路径
    let output_path = config
        .output_path
        .clone()
        .unwrap_or_else(|| generate_batch_output_path(&config.input_path));

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
    println!("📄 结果已保存到: {}", output_path.display());

    Ok(())
}

/// 输出DR计算结果
fn output_results(
    results: &[DrResult],
    config: &AppConfig,
    samples: &[f32],
    format: &AudioFormat,
) -> AudioResult<()> {
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
    if config.dr14_compat_mode {
        // 🧪 dr14_t.meter兼容模式：显示单一结果行
        output.push_str("DR\tPeak\tRMS\n");
        output.push_str("-------------------------------------\n");

        // 计算dr14_t.meter格式的显示值
        let avg_dr: f64 = results.iter().map(|r| r.dr_value).sum::<f64>() / results.len() as f64;
        let dr14_dr = avg_dr.round() as i32;

        // 全局最大Peak（所有声道的最大值）
        let global_max_peak = results.iter().map(|r| r.global_peak).fold(0.0f64, f64::max);
        let dr14_peak_db = 20.0 * global_max_peak.log10();

        // 🎯 **dr14兼容性**: 使用正确的整曲db_rms计算口径
        // 先按声道算线性RMS，再通道线性均值，最后转dB（与dr14_t.meter完全一致）
        let dr14_rms_db = compute_dr14_display_rms_db(samples, format.channels as usize);

        output.push_str(&format!(
            "DR{dr14_dr}\t{dr14_peak_db:.2} dB\t{dr14_rms_db:.2} dB\n"
        ));
    } else {
        // 标准模式：显示分声道详细结果
        output.push_str("动态范围 (DR) 结果:\n");
        output.push_str("-------------------------------------\n");

        for result in results {
            // 使用DR计算实际使用的数值进行显示（与dr14_t.meter一致）
            let peak_db = 20.0 * result.peak.log10();
            let rms_db = 20.0 * result.rms.log10();

            // 计算全局统计值（用于对比）
            let global_peak_db = 20.0 * result.global_peak.log10();
            let global_rms_db = 20.0 * result.global_rms.log10();

            output.push_str(&format!(
                "声道 {}: DR{} (RMS:{:.2}dB, Peak:{:.2}dB) [全局统计: RMS:{:.2}dB, Peak:{:.2}dB]\n",
                result.channel + 1,
                result.dr_value_rounded(),
                rms_db,
                peak_db,
                global_rms_db,
                global_peak_db
            ));
        }

        output.push('\n');

        // 平均DR值
        if results.len() > 1 {
            let avg_dr: f64 =
                results.iter().map(|r| r.dr_value).sum::<f64>() / results.len() as f64;
            output.push_str(&format!("平均DR值: DR{}\n", avg_dr.round() as i32));
        }
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

    println!("🚀 MacinMeter DR Tool v{VERSION} 启动");
    println!("📝 {DESCRIPTION}");
    println!();

    // 显示标准信息
    display_standard_info(config.dr14_compat_mode);

    // 根据模式选择处理方式
    let result = if config.batch_mode {
        // 批量模式：扫描目录处理多个文件
        process_batch_files(&config)
    } else {
        // 单文件模式：处理单个音频文件
        match process_single_audio_file(&config.input_path, &config) {
            Ok(results) => {
                // 为单文件模式输出结果
                if let Ok((format, samples)) =
                    load_audio_file(&config.input_path, false, config.dr14_compat_mode)
                {
                    output_results(&results, &config, &samples, &format)
                } else {
                    println!("⚠️  无法重新加载文件格式信息");
                    Ok(())
                }
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
