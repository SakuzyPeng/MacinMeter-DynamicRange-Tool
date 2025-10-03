//! 🛡️ 错误路径和异常场景测试
//!
//! **优先级1：错误处理和边界条件**
//!
//! 验证系统在异常情况下的健壮性和错误处理能力
//!
//! ## 🎯 测试策略
//!
//! 1. **Universal Decoder错误** - 格式探测失败、连续解码失败、无效文件
//! 2. **Sample Conversion错误** - 未实现格式、SIMD回退场景
//! 3. **DR Calculator错误** - 声道不匹配、空数据、多声道拒绝
//!
//! ## ⚠️ 测试约束
//!
//! - 使用真实错误场景而非mock
//! - 验证错误消息的准确性
//! - 确保优雅降级而非panic

use macinmeter_dr_tool::audio::universal_decoder::UniversalDecoder;
use macinmeter_dr_tool::core::dr_calculator::DrCalculator;
use macinmeter_dr_tool::error::AudioError;
use macinmeter_dr_tool::processing::sample_conversion::{SampleConversion, SampleConverter};
use std::path::Path;

// ========== Universal Decoder 错误路径测试 ==========

/// 验证不存在的文件返回正确错误
#[test]
fn test_nonexistent_file_error() {
    println!("📊 测试不存在文件的错误处理");

    let decoder = UniversalDecoder;
    let result = decoder.create_streaming(Path::new("/nonexistent/file.flac"));

    assert!(result.is_err(), "应该返回错误");

    if let Err(e) = result {
        println!("  ✓ 正确返回错误: {e}");
        // 验证错误类型
        match e {
            AudioError::IoError(_) => println!("  ✓ 错误类型正确: IO错误"),
            _ => panic!("错误类型不正确，期望IO错误，实际: {e:?}"),
        }
    }

    println!("✅ 不存在文件错误处理通过");
}

/// 验证无效音频文件返回格式错误
#[test]
fn test_invalid_audio_format_error() {
    println!("📊 测试无效音频格式的错误处理");

    // 创建临时文本文件伪装成音频文件
    let temp_dir = std::env::temp_dir();
    let fake_audio = temp_dir.join("fake_audio.flac");

    std::fs::write(&fake_audio, b"This is not an audio file").unwrap();

    let decoder = UniversalDecoder;
    let result = decoder.create_streaming(&fake_audio);

    assert!(result.is_err(), "应该返回格式错误");

    if let Err(e) = result {
        println!("  ✓ 正确返回错误: {e}");
        match e {
            AudioError::FormatError(_) | AudioError::IoError(_) => {
                println!("  ✓ 错误类型正确: 格式错误或IO错误");
            }
            _ => panic!("错误类型不正确，期望FormatError，实际: {e:?}"),
        }
    }

    // 清理临时文件
    let _ = std::fs::remove_file(&fake_audio);

    println!("✅ 无效音频格式错误处理通过");
}

/// 验证空文件返回正确错误
#[test]
fn test_empty_file_error() {
    println!("📊 测试空文件的错误处理");

    let temp_dir = std::env::temp_dir();
    let empty_file = temp_dir.join("empty.wav");

    // 创建空文件
    std::fs::write(&empty_file, b"").unwrap();

    let decoder = UniversalDecoder;
    let result = decoder.create_streaming(&empty_file);

    assert!(result.is_err(), "应该返回错误");

    if let Err(e) = result {
        println!("  ✓ 正确返回错误: {e}");
    }

    // 清理临时文件
    let _ = std::fs::remove_file(&empty_file);

    println!("✅ 空文件错误处理通过");
}

/// 验证不支持的文件扩展名处理
#[test]
fn test_unsupported_extension() {
    println!("📊 测试不支持的文件扩展名");

    let temp_dir = std::env::temp_dir();
    let unsupported = temp_dir.join("test.xyz");

    // 创建带有不支持扩展名的文件
    std::fs::write(&unsupported, b"some random data").unwrap();

    let decoder = UniversalDecoder;
    let result = decoder.create_streaming(&unsupported);

    // 应该返回错误或者尝试探测格式后失败
    assert!(result.is_err(), "应该返回错误");

    if let Err(e) = result {
        println!("  ✓ 正确返回错误: {e}");
    }

    // 清理临时文件
    let _ = std::fs::remove_file(&unsupported);

    println!("✅ 不支持的扩展名错误处理通过");
}

// ========== Sample Conversion 错误路径测试 ==========

/// 验证未实现的f64转换返回错误
#[test]
fn test_f64_conversion_not_implemented() {
    println!("📊 测试f64转换未实现错误");

    let converter = SampleConverter::new();
    let input: Vec<f64> = vec![0.0, 0.5, -0.5, 1.0];
    let mut output = Vec::new();

    let result = converter.convert_f64_to_f32(&input, &mut output);

    assert!(result.is_err(), "f64转换应该返回错误");

    if let Err(e) = result {
        println!("  ✓ 正确返回错误: {e}");
        match e {
            AudioError::FormatError(msg) => {
                assert!(
                    msg.contains("f64") || msg.contains("未实现"),
                    "错误消息应该提到f64或未实现"
                );
                println!("  ✓ 错误消息正确: {msg}");
            }
            _ => panic!("错误类型不正确，期望FormatError，实际: {e:?}"),
        }
    }

    println!("✅ f64转换未实现错误处理通过");
}

/// 验证未实现的u8转换返回错误
#[test]
fn test_u8_conversion_not_implemented() {
    println!("📊 测试u8转换未实现错误");

    let converter = SampleConverter::new();
    let input: Vec<u8> = vec![0, 128, 255];
    let mut output = Vec::new();

    let result = converter.convert_u8_to_f32(&input, &mut output);

    assert!(result.is_err(), "u8转换应该返回错误");

    if let Err(e) = result {
        println!("  ✓ 正确返回错误: {e}");
        match e {
            AudioError::FormatError(msg) => {
                assert!(
                    msg.contains("u8") || msg.contains("未实现"),
                    "错误消息应该提到u8或未实现"
                );
                println!("  ✓ 错误消息正确: {msg}");
            }
            _ => panic!("错误类型不正确，期望FormatError，实际: {e:?}"),
        }
    }

    println!("✅ u8转换未实现错误处理通过");
}

// ========== DR Calculator 错误路径测试 ==========

/// 验证空样本数据返回错误
#[test]
fn test_empty_samples_error() {
    println!("📊 测试空样本数据错误处理");

    let calculator = DrCalculator::new(2).unwrap();
    let empty_samples: Vec<f32> = Vec::new();

    let result = calculator.calculate_dr_from_samples(&empty_samples, 2);

    assert!(result.is_err(), "空样本应该返回错误");

    if let Err(e) = result {
        println!("  ✓ 正确返回错误: {e}");
        match e {
            AudioError::InvalidInput(msg) => {
                assert!(
                    msg.contains("空") || msg.contains("为空"),
                    "错误消息应该提到空数据"
                );
                println!("  ✓ 错误消息正确: {msg}");
            }
            _ => panic!("错误类型不正确，期望InvalidInput，实际: {e:?}"),
        }
    }

    println!("✅ 空样本数据错误处理通过");
}

/// 验证声道数不匹配返回错误
#[test]
fn test_channel_count_mismatch_error() {
    println!("📊 测试声道数不匹配错误处理");

    // 创建期望2声道的计算器
    let calculator = DrCalculator::new(2).unwrap();

    // 提供1声道的数据（44100个样本 = 1秒单声道）
    let mono_samples: Vec<f32> = vec![0.5; 44100];

    let result = calculator.calculate_dr_from_samples(&mono_samples, 1);

    assert!(result.is_err(), "声道数不匹配应该返回错误");

    if let Err(e) = result {
        println!("  ✓ 正确返回错误: {e}");
        match e {
            AudioError::InvalidInput(msg) => {
                assert!(
                    msg.contains("声道") && msg.contains("不匹配"),
                    "错误消息应该提到声道不匹配"
                );
                println!("  ✓ 错误消息正确: {msg}");
            }
            _ => panic!("错误类型不正确，期望InvalidInput，实际: {e:?}"),
        }
    }

    println!("✅ 声道数不匹配错误处理通过");
}

/// 验证多声道（> 2）被正确拒绝
#[test]
fn test_multi_channel_rejection() {
    println!("📊 测试多声道拒绝");

    let calculator = DrCalculator::new(6).unwrap();

    // 提供5.1声道的数据（6声道 * 44100样本 = 1秒）
    let multi_channel_samples: Vec<f32> = vec![0.5; 6 * 44100];

    let result = calculator.calculate_dr_from_samples(&multi_channel_samples, 6);

    assert!(result.is_err(), "多声道应该被拒绝");

    if let Err(e) = result {
        println!("  ✓ 正确返回错误: {e}");
        match e {
            AudioError::InvalidInput(msg) => {
                assert!(
                    msg.contains("单声道") || msg.contains("立体声") || msg.contains("1-2"),
                    "错误消息应该说明仅支持单声道和立体声"
                );
                println!("  ✓ 错误消息正确: {msg}");
            }
            _ => panic!("错误类型不正确，期望InvalidInput，实际: {e:?}"),
        }
    }

    println!("✅ 多声道拒绝通过");
}

/// 验证样本数不是声道数倍数返回错误
#[test]
fn test_samples_not_multiple_of_channels() {
    println!("📊 测试样本数非声道数倍数错误");

    let calculator = DrCalculator::new(2).unwrap();

    // 提供奇数个样本（不是2的倍数）
    let invalid_samples: Vec<f32> = vec![0.5; 44101];

    let result = calculator.calculate_dr_from_samples(&invalid_samples, 2);

    assert!(result.is_err(), "样本数非声道数倍数应该返回错误");

    if let Err(e) = result {
        println!("  ✓ 正确返回错误: {e}");
        match e {
            AudioError::InvalidInput(msg) => {
                assert!(
                    msg.contains("整数倍") || msg.contains("倍数"),
                    "错误消息应该提到整数倍"
                );
                println!("  ✓ 错误消息正确: {msg}");
            }
            _ => panic!("错误类型不正确，期望InvalidInput，实际: {e:?}"),
        }
    }

    println!("✅ 样本数非声道数倍数错误处理通过");
}

// ========== 边界条件测试 ==========

/// 验证极短音频（< 3秒）的处理
#[test]
fn test_very_short_audio() {
    println!("📊 测试极短音频处理（< 3秒）");

    let calculator = DrCalculator::new(2).unwrap();

    // 1秒立体声音频（可能不足以计算准确的DR）
    let short_samples: Vec<f32> = vec![0.5; 2 * 44100];

    let result = calculator.calculate_dr_from_samples(&short_samples, 2);

    // 应该能够计算，但结果可能不准确
    match result {
        Ok(dr_results) => {
            println!("  ✓ 短音频计算成功");
            println!("  DR结果: {dr_results:?}");
        }
        Err(e) => {
            println!("  ⚠️ 短音频返回错误: {e}");
            // 这也是可接受的行为，取决于实现
        }
    }

    println!("✅ 极短音频处理验证通过");
}

/// 验证零值样本的处理
#[test]
fn test_zero_samples() {
    println!("📊 测试全零样本处理");

    let calculator = DrCalculator::new(2).unwrap();

    // 全零样本（静音）
    let zero_samples: Vec<f32> = vec![0.0; 2 * 44100 * 5]; // 5秒静音

    let result = calculator.calculate_dr_from_samples(&zero_samples, 2);

    match result {
        Ok(dr_results) => {
            println!("  ✓ 全零样本计算成功");
            println!("  DR结果: {dr_results:?}");
            // 全零样本应该产生无穷大或特殊值
        }
        Err(e) => {
            println!("  ⚠️ 全零样本返回错误: {e}");
            // 返回错误也是合理的
        }
    }

    println!("✅ 全零样本处理验证通过");
}
