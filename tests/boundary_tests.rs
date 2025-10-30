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
        silence_filter_threshold_db: None,
        edge_trim_threshold_db: None,
        edge_trim_min_run_ms: None,
    }
}

// ========== 边界条件测试 ==========

#[test]
fn test_zero_length_audio() {
    let fixtures = setup_fixtures();
    let path = fixtures.get_path("zero_length.wav");
    let config = default_test_config();

    let result = process_audio_file_streaming(&path, &config);

    // 零长度文件必须被拒绝（样本数为0）
    assert!(
        matches!(
            result,
            Err(AudioError::FormatError(_)) | Err(AudioError::InvalidInput(_))
        ),
        "零长度文件应该返回 FormatError 或 InvalidInput，但得到: {result:?}"
    );

    match result {
        Err(AudioError::FormatError(_)) => {
            println!("✓ 零长度文件被正确拒绝（FormatError）");
        }
        Err(AudioError::InvalidInput(_)) => {
            println!("✓ 零长度文件被正确拒绝（InvalidInput）");
        }
        Err(e) => {
            println!("✓ 零长度文件被拒绝: {e:?}");
        }
        Ok(_) => unreachable!(),
    }
}

#[test]
fn test_single_sample_audio() {
    let fixtures = setup_fixtures();
    let path = fixtures.get_path("single_sample.wav");
    let config = default_test_config();

    let result = process_audio_file_streaming(&path, &config);

    // 单样本文件样本太少，应该返回错误
    assert!(
        matches!(
            result,
            Err(AudioError::InvalidInput(_)) | Err(AudioError::CalculationError(_))
        ),
        "单样本文件应该返回 InvalidInput 或 CalculationError，但得到: {result:?}"
    );

    match result {
        Err(AudioError::InvalidInput(_)) => {
            println!("✓ 单样本文件被拒绝（样本数不足）");
        }
        Err(AudioError::CalculationError(_)) => {
            println!("✓ 单样本文件计算失败（样本太少）");
        }
        Err(e) => {
            println!("✓ 单样本文件处理失败: {e:?}");
        }
        Ok(_) => unreachable!(),
    }
}

#[test]
fn test_tiny_duration_audio() {
    let fixtures = setup_fixtures();
    let path = fixtures.get_path("tiny_duration.wav");
    let config = default_test_config();

    let result = process_audio_file_streaming(&path, &config);

    // 10ms文件可以被解码，但应该返回有限的DR值（0-40dB）
    match result {
        Ok((dr_results, _format, _, _)) => {
            assert!(!dr_results.is_empty(), "处理成功应该返回DR结果");
            if let Some(dr) = dr_results.first() {
                // DR值必须有限且在合理范围内
                assert!(
                    dr.dr_value.is_finite(),
                    "DR值必须是有限数，不能是 NaN 或无穷"
                );
                assert!(
                    dr.dr_value >= 0.0 && dr.dr_value <= 40.0,
                    "10ms音频的DR应该在0-40dB范围内，实际值: {}",
                    dr.dr_value
                );
                println!("✓ 极短音频处理成功: DR={:.2}dB", dr.dr_value);
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
        Ok((dr_results, _format, _, _)) => {
            assert!(!dr_results.is_empty(), "处理成功应该返回DR结果");
            if let Some(dr) = dr_results.first() {
                // 静音文件的DR应该是有限且非常接近0（使用DR_ZERO_EPS逻辑）
                // 注：不同平台的SIMD实现可能产生微小浮点数差异，使用容差1e-6
                const SILENCE_DR_TOLERANCE: f64 = 1e-6;
                assert!(
                    dr.dr_value.is_finite(),
                    "静音文件DR必须是有限数，不能是 NaN 或无穷"
                );
                assert!(
                    dr.dr_value.abs() < SILENCE_DR_TOLERANCE,
                    "静音文件DR应该接近0（±{}），实际值: {}",
                    SILENCE_DR_TOLERANCE,
                    dr.dr_value
                );
                println!("✓ 静音文件处理成功: DR={:.9}dB（接近0）", dr.dr_value);
            }
        }
        Err(AudioError::CalculationError(_)) => {
            println!("✓ 静音文件计算失败（可接受：RMS为0）");
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
        Ok((dr_results, _format, _, _)) => {
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
        Ok((dr_results, _format, _, _)) => {
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
        Ok((dr_results, format, _, _)) => {
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
#[ignore] // 诊断式测试：需要真实的截断/损坏音频文件触发skipped_packets检测
fn test_truncated_wav() {
    let fixtures = setup_fixtures();
    let path = fixtures.get_path("truncated.wav");
    let mut config = default_test_config();
    config.verbose = true;

    let result = process_audio_file_streaming(&path, &config);

    // 诊断目标：验证截断检测机制
    // - 当解码器检测到损坏包时，应标记 is_partial() == true
    // - 当预期样本 > 实际样本时，应标记 is_partial() == true
    // 当前的测试文件可能不足以触发这些条件，因此标记为 #[ignore]
    match result {
        Ok((dr_results, format, _, _)) => {
            println!("截断文件处理结果: is_partial={}", format.is_partial());
            println!(
                "  DR结果数: {}, 跳过包数: {}",
                dr_results.len(),
                format.skipped_packets()
            );

            if !format.is_partial() {
                println!("ℹ️  注：未检测到截断（test fixture可能没有真实的损坏包）");
            }
        }
        Err(e) => {
            println!("截断文件处理失败: {e:?}");
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
            Ok((dr_results, _, _, _)) => {
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
