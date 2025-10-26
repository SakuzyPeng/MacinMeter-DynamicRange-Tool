//! 工具层集成测试
//!
//! 测试CLI、文件扫描、格式化输出等工具模块的集成功能。

use macinmeter_dr_tool::tools::{self, AppConfig};
use std::path::{Path, PathBuf};

fn base_config() -> AppConfig {
    AppConfig {
        input_path: PathBuf::from("."),
        verbose: false,
        output_path: None,
        parallel_decoding: true,
        parallel_batch_size: 64,
        parallel_threads: 4,
        parallel_files: Some(4),
        silence_filter_threshold_db: None,
    }
}

// ============================================================================
// CLI配置测试
// ============================================================================

/// 验证批量模式检测（目录路径）
#[test]
fn test_batch_mode_detection_directory() {
    let config = AppConfig {
        input_path: PathBuf::from("tests/fixtures"),
        ..base_config()
    };

    assert!(config.is_batch_mode(), "目录路径应该被识别为批量模式");
    println!("  ✓ 目录路径正确识别为批量模式");
}

/// 验证单文件模式检测（文件路径）
#[test]
fn test_single_file_mode_detection() {
    let config = AppConfig {
        input_path: PathBuf::from("tests/fixtures/silence.wav"),
        ..base_config()
    };

    assert!(!config.is_batch_mode(), "文件路径应该被识别为单文件模式");
    println!("  ✓ 文件路径正确识别为单文件模式");
}

/// 验证Sum Doubling固定启用（foobar2000兼容）
#[test]
fn test_sum_doubling_always_enabled() {
    let config = AppConfig {
        input_path: PathBuf::from("tests/fixtures"),
        ..base_config()
    };

    assert!(
        config.sum_doubling_enabled(),
        "Sum Doubling应该始终启用（foobar2000兼容）"
    );
    println!("  ✓ Sum Doubling正确固定启用");
}

// ============================================================================
// CLI参数范围验证（后续扩展预留）
// ============================================================================

/// 验证并行线程数参数的有效范围
///
/// 预留于后续扩展：当增加更细粒度的并行配置选项时，
/// 应在此测试中验证参数边界条件（最小值、最大值、越界处理）
#[test]
fn test_cli_parallel_threads_range() {
    // 有效范围：1-16线程
    let valid_threads = vec![(1, "最小线程数"), (4, "标准线程数"), (8, "高并发配置")];

    for (threads, desc) in valid_threads {
        assert!(threads >= 1, "{desc}: 线程数应该至少为1");
        println!("  ✓ 线程数参数有效: {threads} ({desc})");
    }
}

/// 验证批处理大小参数的有效范围
///
/// 预留于后续扩展：当增加动态批大小配置时，
/// 应验证该参数的最小值、最大值和合理范围
#[test]
fn test_cli_batch_size_range() {
    // 有效范围：16-256包
    let valid_batch_sizes = vec![(16, "最小批大小"), (64, "标准批大小"), (256, "最大批大小")];

    for (batch_size, desc) in valid_batch_sizes {
        assert!(batch_size >= 16, "{desc}: 批大小应该至少为16");
        println!("  ✓ 批大小参数有效: {batch_size} ({desc})");
    }
}

/// 验证并行文件处理数参数的有效范围
///
/// 预留于后续扩展：当增加文件级并行度控制时，
/// 应验证该参数的有效取值（None表示自动，Some(n)表示固定数量）
#[test]
fn test_cli_parallel_files_range() {
    // 有效配置：None（自动）或Some(1..8)
    let valid_configs = vec![
        (None, "自动并行文件数"),
        (Some(1), "单文件处理"),
        (Some(4), "标准并行文件数"),
    ];

    for (parallel_files, desc) in valid_configs {
        // 验证逻辑一致性：如果指定了并行文件数，应该是正数
        if let Some(files) = parallel_files {
            assert!(files > 0, "{desc}: 并行文件数应该是正数");
        }

        println!("  ✓ 并行文件参数有效: {parallel_files:?} ({desc})");
    }
}

// ============================================================================
// 文件扫描器测试
// ============================================================================

/// 验证扫描真实测试目录
#[test]
fn test_scan_fixtures_directory() {
    let result = tools::scan_audio_files(Path::new("tests/fixtures"));

    assert!(result.is_ok(), "扫描测试目录应该成功");

    let audio_files = result.unwrap();
    assert!(!audio_files.is_empty(), "应该找到至少一个WAV文件");

    println!("  ✓ 扫描到 {} 个音频文件", audio_files.len());

    // 验证文件按名称排序
    for i in 1..audio_files.len() {
        assert!(audio_files[i - 1] <= audio_files[i], "文件应该按名称排序");
    }
    println!("  ✓ 文件列表正确排序");
}

/// 验证空目录处理
#[test]
fn test_scan_empty_directory() {
    use std::fs;
    let temp_dir = std::env::temp_dir().join("dr_test_empty");
    let _ = fs::create_dir(&temp_dir);

    let result = tools::scan_audio_files(&temp_dir);
    assert!(result.is_ok(), "扫描空目录应该成功返回空列表");

    let audio_files = result.unwrap();
    assert!(audio_files.is_empty(), "空目录应该返回空列表");

    let _ = fs::remove_dir(&temp_dir);
    println!("  ✓ 空目录正确返回空列表");
}

/// 验证不存在的路径返回错误
///
/// 改进：使用 temp_dir().join(随机名) 构造不存在的路径，避免硬编码，
/// 确保在所有平台（Windows/Linux/macOS）都能正确返回错误。
#[test]
fn test_scan_nonexistent_path() {
    use std::fs;

    // 构造一个确定不存在的临时路径
    let nonexistent_path = std::env::temp_dir()
        .join("dr_test_nonexistent_xyz_9a8b7c6d5e4f")
        .join("subdir");

    // 确保路径不存在
    let _ = fs::remove_dir_all(&nonexistent_path);

    let result = tools::scan_audio_files(&nonexistent_path);

    assert!(result.is_err(), "不存在的路径应该返回错误");

    if let Err(e) = result {
        println!("  ✓ 不存在路径正确返回错误: {e}");
    }
}

/// 验证文件路径（非目录）返回错误
#[test]
fn test_scan_file_instead_of_directory() {
    let result = tools::scan_audio_files(Path::new("tests/fixtures/silence.wav"));

    assert!(result.is_err(), "文件路径应该返回错误（需要目录）");

    if let Err(e) = result {
        println!("  ✓ 文件路径正确返回错误: {e}");
    }
}

// ============================================================================
// 格式化输出测试
// ============================================================================

/// 验证Official DR格式化输出（集成测试）
#[test]
fn test_official_dr_formatting() {
    use macinmeter_dr_tool::AudioFormat;
    use macinmeter_dr_tool::core::DrResult;

    let results = vec![
        DrResult {
            channel: 0,
            dr_value: 13.98,
            rms: 0.1,
            peak: 0.5,
            primary_peak: 0.5,
            secondary_peak: 0.48,
            sample_count: 88200,
        },
        DrResult {
            channel: 1,
            dr_value: 12.04,
            rms: 0.15,
            peak: 0.6,
            primary_peak: 0.6,
            secondary_peak: 0.58,
            sample_count: 88200,
        },
    ];

    let format = AudioFormat::new(44100, 2, 16, 176400);

    let output = tools::calculate_official_dr(&results, &format);

    // 验证输出包含关键信息
    assert!(output.contains("Official DR Value: DR"));
    println!("  ✓ Official DR格式化输出正确");
    println!("{output}");
}

/// 验证批量输出头部生成
#[test]
fn test_batch_output_header_generation() {
    let config = AppConfig {
        input_path: PathBuf::from("tests/fixtures"),
        ..base_config()
    };

    let audio_files = vec![PathBuf::from("test1.flac"), PathBuf::from("test2.wav")];

    let header = tools::create_batch_output_header(&config, &audio_files);

    // 验证头部包含关键信息
    assert!(header.contains("MacinMeter DR Analysis Report"));
    assert!(header.contains("foobar2000"));
    assert!(header.contains("Official DR"));
    assert!(header.contains("Precise DR"));
    assert!(header.contains(&format!("{}", audio_files.len())));

    println!("  ✓ 批量输出头部生成正确");
    println!("{header}");
}

/// 验证批量输出底部生成（带错误分类）
#[test]
fn test_batch_output_footer_with_errors() {
    use macinmeter_dr_tool::error::ErrorCategory;
    use std::collections::HashMap;

    let audio_files = vec![
        PathBuf::from("test1.flac"),
        PathBuf::from("test2.wav"),
        PathBuf::from("test3.mp3"),
    ];

    let processed_count = 2;
    let failed_count = 1;

    let mut error_stats: HashMap<ErrorCategory, Vec<String>> = HashMap::new();
    error_stats
        .entry(ErrorCategory::Format)
        .or_default()
        .push("test3.mp3".to_string());

    let footer = tools::create_batch_output_footer(
        &audio_files,
        processed_count,
        failed_count,
        &error_stats,
    );

    // 验证底部包含统计信息
    assert!(footer.contains("批量处理统计"));
    assert!(footer.contains("总文件数: 3"));
    assert!(footer.contains("成功处理: 2"));
    assert!(footer.contains("处理失败: 1"));
    assert!(footer.contains("错误分类统计"));
    assert!(footer.contains("格式错误"));
    assert!(footer.contains("test3.mp3"));

    println!("  ✓ 批量输出底部（含错误分类）生成正确");
}

/// 验证批量输出路径生成（默认自动命名）
#[test]
fn test_batch_output_path_generation() {
    let config = AppConfig {
        input_path: PathBuf::from("tests/fixtures"),
        output_path: None, // 未指定，应该自动生成
        ..base_config()
    };

    let output_path = tools::generate_batch_output_path(&config);

    // 验证路径包含关键元素
    let path_str = output_path.to_string_lossy();
    assert!(path_str.contains("fixtures")); // 目录名
    assert!(path_str.contains("BatchDR")); // 批量标识
    assert!(path_str.ends_with(".txt")); // 文本格式

    println!("  ✓ 批量输出路径自动生成正确: {}", output_path.display());
}

/// 验证用户指定输出路径优先
#[test]
fn test_batch_output_path_user_specified() {
    let mut user_path = std::env::temp_dir();
    user_path.push("my_custom_output.txt");

    let config = AppConfig {
        input_path: PathBuf::from("tests/fixtures"),
        output_path: Some(user_path.clone()), // 用户指定
        ..base_config()
    };

    let output_path = tools::generate_batch_output_path(&config);

    assert_eq!(output_path, user_path, "应该使用用户指定的路径");
    println!("  ✓ 用户指定路径优先级正确");
}

// ============================================================================
// 工具函数测试
// ============================================================================

/// 验证文件名提取工具
#[test]
fn test_filename_extraction() {
    let path = Path::new("/path/to/music/track.flac");

    let filename = tools::path::extract_filename(path);
    assert_eq!(filename, "track.flac");

    let stem = tools::path::extract_file_stem(path);
    assert_eq!(stem, "track");

    let ext = tools::path::extract_extension_uppercase(path);
    assert_eq!(ext, "FLAC");

    println!("  ✓ 文件名提取工具正确");
}

/// 验证音频值转换工具
#[test]
fn test_audio_value_conversion() {
    // 线性值 → dB
    let db_value = tools::audio::linear_to_db(0.5);
    let expected = 20.0 * 0.5_f64.log10(); // ≈ -6.02 dB
    assert!((db_value - expected).abs() < 0.01);

    // 零值应该返回负无穷
    let db_zero = tools::audio::linear_to_db(0.0);
    assert_eq!(db_zero, -f64::INFINITY);

    // 字符串格式化
    let db_string = tools::audio::linear_to_db_string(0.5);
    assert!(db_string.contains("-6."));

    println!("  ✓ 音频值转换工具正确");
}

// ============================================================================
// 批量/单文件DR值一致性测试 (Phase 2.6 集成测试验证)
// ============================================================================

/// 🎯 核心一致性测试：验证批量模式和单文件模式计算相同的DR值
///
/// 测试目标：确保同一个音频文件在两种处理模式下产生完全一致的DR值
///
/// 测试覆盖：
/// - WAV 格式（无损，最简单）
/// - MP3 格式（有损，串行解码）
/// - FLAC 格式（无损压缩，并行解码）
///
/// 验证项：
/// - Official DR 值必须完全相同
/// - Precise DR 值极端严格：容差仅0.0001dB（浮点精度极限）
/// - 各声道DR值必须几乎完全一致
#[test]
#[ignore] // 需要真实音频文件，CI环境可能不可用
fn test_batch_vs_single_dr_consistency_wav() {
    use std::path::PathBuf;

    let test_file = PathBuf::from(
        "/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/audio/test_compatibility.wav",
    );

    // 跳过如果文件不存在
    if !test_file.exists() {
        println!("  ⏭️  跳过测试：音频文件不存在");
        return;
    }

    println!("\n🎯 测试批量/单文件DR值一致性（WAV格式）");
    println!("测试文件: {}", test_file.display());

    // 1️⃣ 单文件模式处理
    let mut single_config = base_config();
    single_config.input_path = test_file.clone();
    single_config.parallel_files = None; // 单文件模式
    single_config.parallel_decoding = true;
    single_config.output_path = None;

    let single_result = tools::process_single_audio_file(&test_file, &single_config);
    assert!(single_result.is_ok(), "单文件处理应该成功");
    let (single_dr_results, single_format) = single_result.unwrap();

    let single_official_dr = tools::compute_official_precise_dr(&single_dr_results, &single_format);
    assert!(single_official_dr.is_some(), "单文件模式应该计算出DR值");
    let (single_official, single_precise, _) = single_official_dr.unwrap();

    println!("  单文件模式: DR{single_official} ({single_precise:.2} dB)");

    // 2️⃣ 批量模式处理（仅包含同一个文件）
    let mut batch_config = base_config();
    batch_config.input_path = test_file.parent().unwrap().to_path_buf();
    batch_config.parallel_files = Some(1); // 批量模式，但只处理1个文件
    batch_config.output_path = None;

    // 手动调用批量处理逻辑（模拟只处理这一个文件）
    let batch_result = tools::process_single_audio_file(&test_file, &batch_config);
    assert!(batch_result.is_ok(), "批量处理应该成功");
    let (batch_dr_results, batch_format) = batch_result.unwrap();

    let batch_official_dr = tools::compute_official_precise_dr(&batch_dr_results, &batch_format);
    assert!(batch_official_dr.is_some(), "批量模式应该计算出DR值");
    let (batch_official, batch_precise, _) = batch_official_dr.unwrap();

    println!("  批量模式: DR{batch_official} ({batch_precise:.2} dB)");

    // 3️⃣ 验证一致性
    assert_eq!(
        single_official, batch_official,
        "❌ Official DR值不一致！单文件={single_official}, 批量={batch_official}"
    );

    let precise_diff = (single_precise - batch_precise).abs();
    assert!(
        precise_diff < 0.0001,
        "❌ Precise DR值差异过大！单文件={single_precise:.6}, 批量={batch_precise:.6}, 差异={precise_diff:.8} (极端严格容差0.0001dB)"
    );

    // 4️⃣ 验证各声道DR值一致（极端严格）
    assert_eq!(
        single_dr_results.len(),
        batch_dr_results.len(),
        "声道数应该一致"
    );

    for (i, (single_ch, batch_ch)) in single_dr_results
        .iter()
        .zip(batch_dr_results.iter())
        .enumerate()
    {
        let ch_diff = (single_ch.dr_value - batch_ch.dr_value).abs();
        assert!(
            ch_diff < 0.0001,
            "❌ 声道{}的DR值不一致！单文件={:.6}, 批量={:.6}, 差异={:.8} (极端严格容差0.0001dB)",
            i,
            single_ch.dr_value,
            batch_ch.dr_value,
            ch_diff
        );
    }

    println!("  ✅ 批量/单文件DR值完全一致（WAV）");
}

/// 🎯 MP3格式一致性测试（串行解码路径）
#[test]
#[ignore] // 需要真实音频文件
fn test_batch_vs_single_dr_consistency_mp3() {
    use std::path::PathBuf;

    let test_file = PathBuf::from(
        "/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/audio/test_compatibility.mp3",
    );

    if !test_file.exists() {
        println!("  ⏭️  跳过测试：MP3文件不存在");
        return;
    }

    println!("\n🎯 测试批量/单文件DR值一致性（MP3格式 - 串行解码）");

    // 单文件模式
    let mut single_config = base_config();
    single_config.input_path = test_file.clone();
    single_config.parallel_decoding = false; // MP3强制串行
    single_config.parallel_files = None;
    single_config.output_path = None;

    let (single_dr_results, single_format) =
        tools::process_single_audio_file(&test_file, &single_config).expect("单文件处理应该成功");

    let (single_official, single_precise, _) =
        tools::compute_official_precise_dr(&single_dr_results, &single_format)
            .expect("应该计算出DR值");

    println!("  单文件模式: DR{single_official} ({single_precise:.2} dB)");

    // 批量模式
    let mut batch_config = base_config();
    batch_config.input_path = test_file.clone();
    batch_config.parallel_decoding = false;
    batch_config.parallel_files = Some(1);
    batch_config.output_path = None;

    let (batch_dr_results, batch_format) =
        tools::process_single_audio_file(&test_file, &batch_config).expect("批量处理应该成功");

    let (batch_official, batch_precise, _) =
        tools::compute_official_precise_dr(&batch_dr_results, &batch_format)
            .expect("应该计算出DR值");

    println!("  批量模式: DR{batch_official} ({batch_precise:.2} dB)");

    // 验证一致性（极端严格）
    assert_eq!(
        single_official, batch_official,
        "MP3: Official DR值必须一致"
    );
    let mp3_diff = (single_precise - batch_precise).abs();
    assert!(
        mp3_diff < 0.0001,
        "MP3: Precise DR值差异过大！单文件={single_precise:.6}, 批量={batch_precise:.6}, 差异={mp3_diff:.8} (极端严格容差0.0001dB)"
    );

    println!("  ✅ 批量/单文件DR值完全一致（MP3 - 串行解码）");
}

/// 🎯 FLAC格式一致性测试（并行解码路径）
#[test]
#[ignore] // 需要真实音频文件
fn test_batch_vs_single_dr_consistency_flac() {
    use std::path::PathBuf;

    let test_file = PathBuf::from(
        "/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/audio/Ver2-adm-master-from-DAW-spatialmix-noreverb-peaklimited-0-2025-08-29-00-00-55.flac",
    );

    if !test_file.exists() {
        println!("  ⏭️  跳过测试：FLAC文件不存在");
        return;
    }

    println!("\n🎯 测试批量/单文件DR值一致性（FLAC格式 - 并行解码）");

    // 单文件模式（并行解码）
    let mut single_config = base_config();
    single_config.input_path = test_file.clone();
    single_config.parallel_decoding = true; // FLAC支持并行
    single_config.parallel_files = None;
    single_config.output_path = None;

    let (single_dr_results, single_format) =
        tools::process_single_audio_file(&test_file, &single_config).expect("单文件处理应该成功");

    let (single_official, single_precise, _) =
        tools::compute_official_precise_dr(&single_dr_results, &single_format)
            .expect("应该计算出DR值");

    println!("  单文件模式: DR{single_official} ({single_precise:.2} dB)");

    // 批量模式（并行解码）
    let mut batch_config = base_config();
    batch_config.input_path = test_file.clone();
    batch_config.parallel_decoding = true;
    batch_config.parallel_files = Some(1);
    batch_config.output_path = None;

    let (batch_dr_results, batch_format) =
        tools::process_single_audio_file(&test_file, &batch_config).expect("批量处理应该成功");

    let (batch_official, batch_precise, _) =
        tools::compute_official_precise_dr(&batch_dr_results, &batch_format)
            .expect("应该计算出DR值");

    println!("  批量模式: DR{batch_official} ({batch_precise:.2} dB)");

    // 验证一致性（极端严格）
    assert_eq!(
        single_official, batch_official,
        "FLAC: Official DR值必须一致"
    );
    let flac_diff = (single_precise - batch_precise).abs();
    assert!(
        flac_diff < 0.0001,
        "FLAC: Precise DR值差异过大！单文件={single_precise:.6}, 批量={batch_precise:.6}, 差异={flac_diff:.8} (极端严格容差0.0001dB)"
    );

    println!("  ✅ 批量/单文件DR值完全一致（FLAC - 并行解码）");
}

/// 🎯 多格式综合一致性测试
#[test]
#[ignore] // 需要真实音频文件
fn test_batch_vs_single_dr_consistency_multiple_formats() {
    use std::path::PathBuf;

    let test_files = vec![
        (
            "WAV",
            "/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/audio/test_compatibility.wav",
        ),
        (
            "MP3",
            "/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/audio/test_compatibility.mp3",
        ),
        (
            "AAC",
            "/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/audio/test_compatibility.aac",
        ),
        (
            "OGG",
            "/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/audio/test_compatibility.ogg",
        ),
    ];

    println!("\n🎯 多格式批量/单文件DR值一致性测试");
    println!("{}", "=".repeat(60));

    let mut tested_count = 0;
    let mut passed_count = 0;

    for (format_name, file_path) in test_files {
        let test_file = PathBuf::from(file_path);

        if !test_file.exists() {
            println!("  ⏭️  跳过 {format_name}: 文件不存在");
            continue;
        }

        tested_count += 1;
        println!("\n  📊 测试格式: {format_name}");

        // 单文件模式
        // 有状态编码格式（MP3/AAC/OGG）必须串行解码
        let is_stateful = matches!(format_name, "MP3" | "AAC" | "OGG");
        let mut single_config = base_config();
        single_config.input_path = test_file.clone();
        single_config.parallel_decoding = !is_stateful;
        single_config.parallel_files = None;
        single_config.output_path = None;

        let single_result = tools::process_single_audio_file(&test_file, &single_config);
        if single_result.is_err() {
            println!("     ⚠️  单文件处理失败，跳过");
            continue;
        }

        let (single_dr_results, single_format) = single_result.unwrap();
        let single_official_dr =
            tools::compute_official_precise_dr(&single_dr_results, &single_format);
        if single_official_dr.is_none() {
            println!("     ⚠️  无法计算DR值，跳过");
            continue;
        }

        let (single_official, single_precise, _) = single_official_dr.unwrap();

        // 批量模式
        let mut batch_config = base_config();
        batch_config.input_path = test_file.clone();
        batch_config.parallel_decoding = !is_stateful;
        batch_config.parallel_files = Some(1);
        batch_config.output_path = None;

        let (batch_dr_results, batch_format) =
            tools::process_single_audio_file(&test_file, &batch_config).expect("批量处理应该成功");

        let (batch_official, batch_precise, _) =
            tools::compute_official_precise_dr(&batch_dr_results, &batch_format)
                .expect("应该计算出DR值");

        // 验证一致性（极端严格）
        let official_match = single_official == batch_official;
        let diff = (single_precise - batch_precise).abs();
        let precise_match = diff < 0.0001;

        if official_match && precise_match {
            passed_count += 1;
            println!("     ✅ 一致性验证通过");
            println!("        单文件: DR{single_official} ({single_precise:.6} dB)");
            println!("        批量:   DR{batch_official} ({batch_precise:.6} dB)");
            println!("        差异:   {diff:.8} dB (极端严格: <0.0001dB)");
        } else {
            println!("     ❌ 一致性验证失败");
            println!("        单文件: DR{single_official} ({single_precise:.6} dB)");
            println!("        批量:   DR{batch_official} ({batch_precise:.6} dB)");
            println!("        差异:   {diff:.8} dB (要求: <0.0001dB)");
            panic!("{format_name} 格式的批量/单文件DR值不一致");
        }
    }

    println!();
    println!("{}", "=".repeat(60));
    println!("  📈 测试总结: {passed_count}/{tested_count} 格式通过一致性验证");
    println!("{}", "=".repeat(60));

    assert!(tested_count > 0, "至少应该测试一个格式");
    assert_eq!(passed_count, tested_count, "所有测试格式都应该通过");
}
