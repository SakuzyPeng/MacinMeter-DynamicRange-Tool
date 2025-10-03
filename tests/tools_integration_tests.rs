//! 工具层集成测试
//!
//! 测试CLI、文件扫描、格式化输出等工具模块的集成功能。

use macinmeter_dr_tool::tools::{self, AppConfig};
use std::path::{Path, PathBuf};

// ============================================================================
// CLI配置测试
// ============================================================================

/// 验证批量模式检测（目录路径）
#[test]
fn test_batch_mode_detection_directory() {
    let config = AppConfig {
        input_path: PathBuf::from("tests/fixtures"),
        verbose: false,
        output_path: None,
        parallel_decoding: true,
        parallel_batch_size: 64,
        parallel_threads: 4,
        parallel_files: Some(4),
    };

    assert!(config.is_batch_mode(), "目录路径应该被识别为批量模式");
    println!("  ✓ 目录路径正确识别为批量模式");
}

/// 验证单文件模式检测（文件路径）
#[test]
fn test_single_file_mode_detection() {
    let config = AppConfig {
        input_path: PathBuf::from("tests/fixtures/silence.wav"),
        verbose: false,
        output_path: None,
        parallel_decoding: true,
        parallel_batch_size: 64,
        parallel_threads: 4,
        parallel_files: Some(4),
    };

    assert!(!config.is_batch_mode(), "文件路径应该被识别为单文件模式");
    println!("  ✓ 文件路径正确识别为单文件模式");
}

/// 验证Sum Doubling固定启用（foobar2000兼容）
#[test]
fn test_sum_doubling_always_enabled() {
    let config = AppConfig {
        input_path: PathBuf::from("tests/fixtures"),
        verbose: false,
        output_path: None,
        parallel_decoding: true,
        parallel_batch_size: 64,
        parallel_threads: 4,
        parallel_files: Some(4),
    };

    assert!(
        config.sum_doubling_enabled(),
        "Sum Doubling应该始终启用（foobar2000兼容）"
    );
    println!("  ✓ Sum Doubling正确固定启用");
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
#[test]
fn test_scan_nonexistent_path() {
    let result = tools::scan_audio_files(Path::new("/nonexistent/path/xyz123"));

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
        verbose: false,
        output_path: None,
        parallel_decoding: true,
        parallel_batch_size: 64,
        parallel_threads: 4,
        parallel_files: Some(4),
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
        verbose: false,
        output_path: None, // 未指定，应该自动生成
        parallel_decoding: true,
        parallel_batch_size: 64,
        parallel_threads: 4,
        parallel_files: Some(4),
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
    let user_path = PathBuf::from("/tmp/my_custom_output.txt");

    let config = AppConfig {
        input_path: PathBuf::from("tests/fixtures"),
        verbose: false,
        output_path: Some(user_path.clone()), // 用户指定
        parallel_decoding: true,
        parallel_batch_size: 64,
        parallel_threads: 4,
        parallel_files: Some(4),
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
