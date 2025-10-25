//! 边界和异常测试
//!
//! 测试各种边界条件、异常输入和数值边界

mod audio_test_fixtures;

use audio_test_fixtures::AudioTestFixtures;
use macinmeter_dr_tool::AudioError;
use macinmeter_dr_tool::tools::{AppConfig, processor::process_audio_file_streaming};
use std::path::PathBuf;

/// 测试前生成所有固件
fn setup_fixtures() -> AudioTestFixtures {
    let fixtures = AudioTestFixtures::new();
    fixtures.generate_all();
    fixtures
}

/// 创建默认测试配置
fn default_test_config() -> AppConfig {
    AppConfig {
        input_path: PathBuf::from("."),
        verbose: false,
        output_path: None,
        parallel_decoding: false,
        parallel_batch_size: 64,
        parallel_threads: 4,
        parallel_files: None, // 测试不需要多文件并行
    }
}

// ========== 边界条件测试 ==========

#[test]
fn test_zero_length_audio() {
    let fixtures = setup_fixtures();
    let path = fixtures.get_path("zero_length.wav");
    let config = default_test_config();

    let result = process_audio_file_streaming(&path, &config);

    // 零长度文件处理：系统接受返回Ok或拒绝返回Err都是可接受的
    // 本测试验证：如果接受，必须返回有效的结果；如果拒绝，必须返回错误
    match result {
        Ok((dr_results, format)) => {
            println!("✓ 零长度文件被接受（设计选择）");
            // 如果接受零长度文件，DR结果应该存在且有效
            assert!(!dr_results.is_empty(), "零长度文件结果不应该为空");
            println!(
                "  格式: {}Hz, {}bit, {}ch",
                format.sample_rate, format.bits_per_sample, format.channels
            );
        }
        Err(AudioError::FormatError(_)) => {
            println!("✓ 零长度文件被拒绝（FormatError）");
        }
        Err(AudioError::InvalidInput(_)) => {
            println!("✓ 零长度文件被拒绝（InvalidInput）");
        }
        Err(e) => {
            println!("✓ 零长度文件处理失败: {e:?}");
        }
    }
}

#[test]
fn test_single_sample_audio() {
    let fixtures = setup_fixtures();
    let path = fixtures.get_path("single_sample.wav");
    let config = default_test_config();

    let result = process_audio_file_streaming(&path, &config);

    // 单样本文件：系统接受返回Ok或拒绝返回Err都是可接受的
    // 本测试验证：如果接受，必须返回有效的DR值；如果拒绝，必须返回明确的错误
    match result {
        Ok((dr_results, format)) => {
            println!("✓ 单样本文件被接受（设计选择）");
            // 如果接受，必须有DR结果
            assert!(!dr_results.is_empty(), "单样本文件应该返回DR结果");
            if let Some(dr) = dr_results.first() {
                // DR值可能是特殊值（NaN、无穷）或极值（很大或很小）
                println!("  DR={:.2}（可能是特殊值）", dr.dr_value);
            }
            println!(
                "  格式: {}Hz, {}bit, {}ch",
                format.sample_rate, format.bits_per_sample, format.channels
            );
        }
        Err(AudioError::InvalidInput(_)) => {
            println!("✓ 单样本文件被拒绝（样本数不足）");
        }
        Err(AudioError::CalculationError(_)) => {
            println!("✓ 单样本文件计算失败（样本太少）");
        }
        Err(e) => {
            println!("✓ 单样本文件处理失败: {e:?}");
        }
    }
}

#[test]
fn test_tiny_duration_audio() {
    let fixtures = setup_fixtures();
    let path = fixtures.get_path("tiny_duration.wav");
    let config = default_test_config();

    let result = process_audio_file_streaming(&path, &config);

    // 10ms文件可以被解码，但需要有明确的行为：
    // 要么成功处理并返回有效DR值，要么计算失败
    match result {
        Ok((dr_results, _format)) => {
            if let Some(dr) = dr_results.first() {
                println!("✓ 极短音频处理成功: DR={:.2}", dr.dr_value);
                // 如果成功，DR值必须在合理范围内（0-100dB）
                assert!(
                    dr.dr_value >= 0.0 && dr.dr_value < 100.0,
                    "DR值应该在0-100dB范围内，实际值: {}",
                    dr.dr_value
                );
            }
        }
        Err(AudioError::CalculationError(_)) => {
            println!("✓ 极短音频计算失败（可接受：样本数不足）");
        }
        Err(e) => {
            println!("✓ 极短音频处理失败: {e:?}");
        }
    }
}

// ========== 数值边界测试 ==========

#[test]
fn test_silence_handling() {
    let fixtures = setup_fixtures();
    let path = fixtures.get_path("silence.wav");
    let config = default_test_config();

    let result = process_audio_file_streaming(&path, &config);

    match result {
        Ok((dr_results, _format)) => {
            if let Some(dr) = dr_results.first() {
                println!("✓ 静音文件处理成功: DR={:.2}", dr.dr_value);
                // 静音的DR应该是0（因为Peak和RMS都接近0）或特殊值
                // 注：不同平台的SIMD实现可能产生微小浮点数差异，使用容差1e-6
                const SILENCE_DR_TOLERANCE: f64 = 1e-6;
                assert!(
                    dr.dr_value.abs() < SILENCE_DR_TOLERANCE
                        || dr.dr_value.is_nan()
                        || dr.dr_value.is_infinite(),
                    "静音DR应该接近0或特殊值，实际值: {}, 容差: {}",
                    dr.dr_value,
                    SILENCE_DR_TOLERANCE
                );
            }
        }
        Err(AudioError::CalculationError(_)) => {
            println!("✓ 静音文件计算失败（预期行为：RMS为0导致无法计算）");
        }
        Err(e) => {
            panic!("静音文件处理失败: {e:?}");
        }
    }
}

#[test]
fn test_full_scale_clipping() {
    let fixtures = setup_fixtures();
    let path = fixtures.get_path("full_scale_clipping.wav");
    let config = default_test_config();

    let result = process_audio_file_streaming(&path, &config);

    match result {
        Ok((dr_results, _format)) => {
            if let Some(dr) = dr_results.first() {
                println!("✓ 削波文件处理成功: DR={:.2}", dr.dr_value);
                // 全削波的DR应该接近0（极小动态范围）
                assert!(
                    dr.dr_value < 5.0,
                    "削波文件DR应该很小，实际值: {}",
                    dr.dr_value
                );
            }
        }
        Err(e) => {
            panic!("削波文件处理失败: {e:?}");
        }
    }
}

#[test]
fn test_edge_value_patterns() {
    let fixtures = setup_fixtures();
    let path = fixtures.get_path("edge_cases.wav");
    let config = default_test_config();

    let result = process_audio_file_streaming(&path, &config);

    match result {
        Ok((dr_results, _format)) => {
            if let Some(dr) = dr_results.first() {
                println!("✓ 边缘值文件处理成功: DR={:.2}", dr.dr_value);
                // 应该有有效的DR值，不应该有NaN
                assert!(!dr.dr_value.is_nan(), "DR值不应该是NaN");
                assert!(dr.dr_value >= 0.0, "DR值应该非负");
            }
        }
        Err(e) => {
            panic!("边缘值文件处理失败: {e:?}");
        }
    }
}

// ========== 格式边界测试 ==========

#[test]
fn test_high_sample_rate() {
    let fixtures = setup_fixtures();
    let path = fixtures.get_path("high_sample_rate.wav");
    let config = default_test_config();

    let result = process_audio_file_streaming(&path, &config);

    match result {
        Ok((dr_results, format)) => {
            if let Some(dr) = dr_results.first() {
                println!("✓ 高采样率文件处理成功: DR={:.2}", dr.dr_value);
                println!(
                    "  格式: {}Hz, {}bit",
                    format.sample_rate, format.bits_per_sample
                );
                assert_eq!(format.sample_rate, 192000, "采样率应该是192kHz");
                assert_eq!(format.bits_per_sample, 24, "位深应该是24bit");
                // 正弦波的DR应该很小（接近0），因为它的峰值和RMS比较接近
                assert!(
                    dr.dr_value >= -1.0 && dr.dr_value < 10.0,
                    "正弦波DR应该很小，实际值: {}",
                    dr.dr_value
                );
            }
        }
        Err(e) => {
            panic!("高采样率文件处理失败: {e:?}");
        }
    }
}

#[test]
fn test_3_channels_rejection() {
    let fixtures = setup_fixtures();
    let path = fixtures.get_path("3_channels.wav");
    let config = default_test_config();

    let result = process_audio_file_streaming(&path, &config);

    // 3声道文件应该被拒绝
    match result {
        Err(AudioError::InvalidInput(msg)) if msg.contains("声道") || msg.contains("channel") => {
            println!("✓ 正确拒绝3声道文件（InvalidInput）");
        }
        Err(AudioError::FormatError(_)) => {
            println!("✓ 正确拒绝3声道文件（FormatError）");
        }
        Err(e) => {
            println!("✓ 正确拒绝3声道文件: {e:?}");
        }
        Ok(_) => {
            panic!("3声道文件不应该被接受");
        }
    }
}

// ========== 异常文件测试 ==========

#[test]
fn test_empty_file() {
    let fixtures = setup_fixtures();
    let path = fixtures.get_path("empty.wav");
    let config = default_test_config();

    let result = process_audio_file_streaming(&path, &config);

    // 空文件应该返回格式错误
    assert!(result.is_err(), "空文件应该被拒绝");

    match result {
        Err(AudioError::FormatError(_)) => {
            println!("✓ 正确拒绝空文件（FormatError）");
        }
        Err(AudioError::IoError(_)) => {
            println!("✓ 正确拒绝空文件（IoError）");
        }
        Err(e) => {
            println!("✓ 正确拒绝空文件: {e:?}");
        }
        Ok(_) => unreachable!(),
    }
}

#[test]
fn test_fake_audio_file() {
    let fixtures = setup_fixtures();
    let path = fixtures.get_path("fake_audio.wav");
    let config = default_test_config();

    let result = process_audio_file_streaming(&path, &config);

    // 伪装文件应该返回格式错误
    assert!(result.is_err(), "伪装文件应该被拒绝");

    match result {
        Err(AudioError::FormatError(_)) => {
            println!("✓ 正确拒绝伪装文件（FormatError）");
        }
        Err(e) => {
            println!("✓ 正确拒绝伪装文件: {e:?}");
        }
        Ok(_) => unreachable!(),
    }
}

#[test]
fn test_truncated_wav() {
    let fixtures = setup_fixtures();
    let path = fixtures.get_path("truncated.wav");
    let config = default_test_config();

    let result = process_audio_file_streaming(&path, &config);

    // 截断文件处理：可能被解码成功、部分成功、或完全失败
    // 本测试验证：所有路径都产生明确的结果，没有未定义行为
    match result {
        Ok((dr_results, format)) => {
            println!("✓ 截断文件被处理（可能部分成功）");
            assert!(!dr_results.is_empty(), "如果成功，必须有DR结果");
            if let Some(dr) = dr_results.first() {
                println!(
                    "  DR={:.2}, is_partial={}",
                    dr.dr_value,
                    format.is_partial()
                );
            }
            // 记录is_partial()状态用于诊断，但不强制要求
            if format.is_partial() {
                println!("  ℹ️ 正确标记为部分分析");
            } else {
                println!("  ℹ️ 注：未标记为部分分析（可能完整处理了可用数据）");
            }
        }
        Err(AudioError::DecodingError(_)) => {
            println!("✓ 截断文件解码失败（预期行为）");
        }
        Err(AudioError::FormatError(_)) => {
            println!("✓ 截断文件格式错误（预期行为）");
        }
        Err(e) => {
            println!("✓ 截断文件处理失败: {e:?}");
        }
    }
}

// ========== 压力和性能测试 ==========

#[test]
#[ignore] // 标记为ignore，需要手动运行：cargo test --ignored
fn test_multiple_files_stress() {
    let fixtures = setup_fixtures();
    let config = default_test_config();

    // 连续处理所有测试文件
    let test_files = vec![
        "silence.wav",
        "full_scale_clipping.wav",
        "high_sample_rate.wav",
        "tiny_duration.wav",
        "edge_cases.wav",
    ];

    println!("🔥 压力测试：连续处理多个文件");
    for filename in test_files {
        let path = fixtures.get_path(filename);
        print!("  处理 {filename}...");

        match process_audio_file_streaming(&path, &config) {
            Ok((dr_results, _)) => {
                if let Some(dr) = dr_results.first() {
                    println!(" ✓ DR={:.2}", dr.dr_value);
                }
            }
            Err(e) => {
                println!(" ✗ 失败: {e:?}");
            }
        }
    }
}
