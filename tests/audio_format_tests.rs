//! 音频格式测试
//!
//! 测试AudioFormat结构的各种功能和边界条件

use macinmeter_dr_tool::AudioError;
use macinmeter_dr_tool::audio::AudioFormat;
use symphonia::core::codecs::CODEC_TYPE_FLAC;

fn log(msg_zh: impl AsRef<str>, msg_en: impl AsRef<str>) {
    println!("{} / {}", msg_zh.as_ref(), msg_en.as_ref());
}

// ========== 基础创建测试 ==========

#[test]
fn test_audio_format_new() {
    let format = AudioFormat::new(44100, 2, 16, 1000000);

    assert_eq!(format.sample_rate, 44100);
    assert_eq!(format.channels, 2);
    assert_eq!(format.bits_per_sample, 16);
    assert_eq!(format.sample_count, 1000000);
    assert_eq!(format.codec_type, None);
    assert!(!format.is_partial());
    assert_eq!(format.skipped_packets(), 0);

    log(
        "AudioFormat::new() 创建成功",
        "AudioFormat::new() constructed successfully",
    );
}

#[test]
fn test_audio_format_with_codec() {
    let format = AudioFormat::with_codec(48000, 1, 24, 2000000, CODEC_TYPE_FLAC);

    assert_eq!(format.sample_rate, 48000);
    assert_eq!(format.channels, 1);
    assert_eq!(format.bits_per_sample, 24);
    assert_eq!(format.sample_count, 2000000);
    assert_eq!(format.codec_type, Some(CODEC_TYPE_FLAC));
    assert!(!format.is_partial());
    assert_eq!(format.skipped_packets(), 0);

    log(
        "AudioFormat::with_codec() 创建成功，codec_type已设置",
        "AudioFormat::with_codec() constructed; codec_type set",
    );
}

#[test]
fn test_audio_format_various_sample_rates() {
    let rates = [8000, 16000, 22050, 44100, 48000, 88200, 96000, 192000];

    for &rate in &rates {
        let format = AudioFormat::new(rate, 2, 16, 100000);
        assert_eq!(format.sample_rate, rate);
        assert!(format.validate().is_ok(), "采样率{rate}应该有效");
    }

    log(
        "各种常见采样率验证通过",
        "Common sample rates validated successfully",
    );
}

#[test]
fn test_audio_format_various_bit_depths() {
    let bit_depths = [16, 24, 32];

    for &bits in &bit_depths {
        let format = AudioFormat::new(44100, 2, bits, 100000);
        assert_eq!(format.bits_per_sample, bits);
        assert!(format.validate().is_ok(), "位深{bits}应该有效");
    }

    log(
        "支持的位深度（16/24/32）验证通过",
        "Supported bit depths (16/24/32) validated",
    );
}

// ========== 验证测试 ==========

#[test]
fn test_validate_zero_sample_rate() {
    let format = AudioFormat::new(0, 2, 16, 1000000);
    let result = format.validate();

    assert!(result.is_err(), "采样率为0应该返回错误");

    match result {
        Err(AudioError::FormatError(msg)) => {
            assert!(msg.contains("采样率") || msg.contains("0"));
            log(
                format!("正确拒绝采样率为0: {msg}"),
                format!("Correctly rejected zero sample rate: {msg}"),
            );
        }
        _ => panic!("期望FormatError"),
    }
}

#[test]
fn test_validate_zero_channels() {
    let format = AudioFormat::new(44100, 0, 16, 1000000);
    let result = format.validate();

    assert!(result.is_err(), "声道数为0应该返回错误");

    match result {
        Err(AudioError::FormatError(msg)) => {
            assert!(msg.contains("声道") || msg.contains("0"));
            log(
                format!("正确拒绝声道数为0: {msg}"),
                format!("Correctly rejected zero channels: {msg}"),
            );
        }
        _ => panic!("期望FormatError"),
    }
}

#[test]
fn test_validate_invalid_bit_depth() {
    // 更新：现在支持8/16/24/32/64，所以测试不支持的位深
    let invalid_depths = [12, 20, 48, 128];

    for &bits in &invalid_depths {
        let format = AudioFormat::new(44100, 2, bits, 1000000);
        let result = format.validate();

        assert!(result.is_err(), "位深{bits}应该被拒绝");

        match result {
            Err(AudioError::FormatError(msg)) => {
                assert!(msg.contains("位深") || msg.contains(&bits.to_string()));
                log(
                    format!("正确拒绝位深{bits}: {msg}"),
                    format!("Correctly rejected bit depth {bits}: {msg}"),
                );
            }
            _ => panic!("期望FormatError"),
        }
    }
}

#[test]
fn test_validate_valid_formats() {
    let valid_formats = vec![
        AudioFormat::new(44100, 1, 16, 1000000),
        AudioFormat::new(48000, 2, 24, 2000000),
        AudioFormat::new(96000, 2, 32, 4000000),
        AudioFormat::new(22050, 1, 16, 500000),
    ];

    for format in valid_formats {
        assert!(
            format.validate().is_ok(),
            "有效格式应该通过验证: {format:?}"
        );
    }

    log(
        "所有有效格式验证通过",
        "All valid formats passed validation",
    );
}

// ========== 部分分析标记测试 ==========

#[test]
fn test_mark_as_partial_no_skipped() {
    let mut format = AudioFormat::new(44100, 2, 16, 1000000);

    assert!(!format.is_partial());
    assert_eq!(format.skipped_packets(), 0);

    format.mark_as_partial(0);

    assert!(format.is_partial());
    assert_eq!(format.skipped_packets(), 0);

    log(
        "标记为部分分析（0个跳过包）",
        "Marked as partial analysis (0 skipped packets)",
    );
}

#[test]
fn test_mark_as_partial_with_skipped() {
    let mut format = AudioFormat::new(44100, 2, 16, 1000000);

    format.mark_as_partial(42);

    assert!(format.is_partial());
    assert_eq!(format.skipped_packets(), 42);

    log(
        "标记为部分分析（42个跳过包）",
        "Marked as partial analysis (42 skipped packets)",
    );
}

#[test]
fn test_mark_as_partial_multiple_times() {
    let mut format = AudioFormat::new(44100, 2, 16, 1000000);

    format.mark_as_partial(10);
    assert_eq!(format.skipped_packets(), 10);

    // 再次标记会覆盖
    format.mark_as_partial(20);
    assert_eq!(format.skipped_packets(), 20);
    assert!(format.is_partial());

    log(
        "多次标记部分分析（最后一次覆盖）",
        "Repeated partial mark updates (last one wins)",
    );
}

// ========== 文件大小估算测试 ==========

#[test]
fn test_estimated_file_size_16bit_stereo() {
    let format = AudioFormat::new(44100, 2, 16, 44100); // 1秒，立体声，16bit

    let expected = 44100 * 2 * 2; // samples * channels * bytes_per_sample
    let actual = format.estimated_pcm_size_bytes();

    assert_eq!(actual, expected);
    log(
        format!("16bit立体声文件大小估算: {actual} bytes"),
        format!("16-bit stereo size estimate: {actual} bytes"),
    );
}

#[test]
fn test_estimated_file_size_24bit_mono() {
    let format = AudioFormat::new(48000, 1, 24, 48000); // 1秒，单声道，24bit

    let expected = 48000 * 3; // samples * (24/8 bytes_per_sample)
    let actual = format.estimated_pcm_size_bytes();

    assert_eq!(actual, expected);
    log(
        format!("24bit单声道文件大小估算: {actual} bytes"),
        format!("24-bit mono size estimate: {actual} bytes"),
    );
}

#[test]
fn test_estimated_file_size_32bit_stereo() {
    let format = AudioFormat::new(96000, 2, 32, 96000); // 1秒，立体声，32bit

    let expected = 96000 * 2 * 4; // samples * channels * (32/8)
    let actual = format.estimated_pcm_size_bytes();

    assert_eq!(actual, expected);
    log(
        format!("32bit立体声文件大小估算: {actual} bytes"),
        format!("32-bit stereo size estimate: {actual} bytes"),
    );
}

#[test]
fn test_estimated_file_size_edge_cases() {
    // 零样本
    let format_zero = AudioFormat::new(44100, 2, 16, 0);
    assert_eq!(format_zero.estimated_pcm_size_bytes(), 0);

    // 单个样本
    let format_one = AudioFormat::new(44100, 2, 16, 1);
    assert_eq!(format_one.estimated_pcm_size_bytes(), 4); // 1 * 2 * 2

    // 极大样本数
    let format_large = AudioFormat::new(44100, 2, 16, u64::MAX / 1000);
    let size = format_large.estimated_pcm_size_bytes();
    assert!(size > 0);

    log(
        "文件大小估算边界情况通过",
        "File size edge cases handled correctly",
    );
}

// ========== 时长计算测试 ==========

#[test]
fn test_duration_seconds_one_second() {
    let format = AudioFormat::new(44100, 2, 16, 44100);
    let duration = format.duration_seconds();

    assert!((duration - 1.0).abs() < 1e-6, "1秒音频时长应该是1.0");
    log(
        format!("1秒音频时长计算: {duration:.6}s"),
        format!("1-second duration computed as {duration:.6}s"),
    );
}

#[test]
fn test_duration_seconds_various_lengths() {
    let test_cases = vec![
        (44100, 44100, 1.0),         // 1秒
        (44100, 88200, 2.0),         // 2秒
        (44100, 22050, 0.5),         // 0.5秒
        (48000, 144000, 3.0),        // 3秒
        (96000, 96000, 1.0),         // 1秒（高采样率）
        (44100, 44100 * 60, 60.0),   // 1分钟
        (44100, 44100 * 180, 180.0), // 3分钟
    ];

    for (sample_rate, sample_count, expected_duration) in test_cases {
        let format = AudioFormat::new(sample_rate, 2, 16, sample_count);
        let actual = format.duration_seconds();

        assert!(
            (actual - expected_duration).abs() < 1e-6,
            "采样率{sample_rate}，样本数{sample_count}，期望{expected_duration}s，实际{actual}s"
        );
    }

    log("各种时长计算验证通过", "Duration calculations validated");
}

#[test]
fn test_duration_seconds_precision() {
    // 测试高精度时长计算
    let format = AudioFormat::new(44100, 2, 16, 44101); // 稍微超过1秒
    let duration = format.duration_seconds();

    let expected = 44101.0 / 44100.0;
    assert!((duration - expected).abs() < 1e-9);

    log(
        format!("时长计算高精度验证: {duration:.9}s"),
        format!("High-precision duration validated: {duration:.9}s"),
    );
}

#[test]
fn test_duration_seconds_edge_cases() {
    // 零样本
    let format_zero = AudioFormat::new(44100, 2, 16, 0);
    assert_eq!(format_zero.duration_seconds(), 0.0);

    // 单个样本
    let format_one = AudioFormat::new(44100, 2, 16, 1);
    let duration = format_one.duration_seconds();
    assert!((duration - 1.0 / 44100.0).abs() < 1e-9);

    log(
        "时长计算边界情况通过",
        "Duration edge cases handled correctly",
    );
}

// ========== 更新样本数测试 ==========

#[test]
fn test_update_sample_count() {
    let mut format = AudioFormat::new(44100, 2, 16, 1000000);

    assert_eq!(format.sample_count, 1000000);

    format.update_sample_count(2000000);
    assert_eq!(format.sample_count, 2000000);

    log(
        "更新样本数: 1000000 -> 2000000",
        "Sample count updated: 1000000 -> 2000000",
    );
}

#[test]
fn test_update_sample_count_affects_duration() {
    let mut format = AudioFormat::new(44100, 2, 16, 44100); // 1秒

    assert!((format.duration_seconds() - 1.0).abs() < 1e-6);

    format.update_sample_count(88200); // 更新为2秒
    assert!((format.duration_seconds() - 2.0).abs() < 1e-6);

    log(
        "更新样本数影响时长计算",
        "Sample count update affects duration",
    );
}

#[test]
fn test_update_sample_count_affects_file_size() {
    let mut format = AudioFormat::new(44100, 2, 16, 44100);

    let size_before = format.estimated_pcm_size_bytes();

    format.update_sample_count(88200); // 样本数翻倍
    let size_after = format.estimated_pcm_size_bytes();

    assert_eq!(size_after, size_before * 2);

    log(
        "更新样本数影响文件大小估算",
        "Sample count update affects file size estimate",
    );
}

// ========== Clone和PartialEq测试 ==========

#[test]
fn test_audio_format_clone() {
    let format1 = AudioFormat::new(44100, 2, 16, 1000000);
    let format2 = format1.clone();

    assert_eq!(format1.sample_rate, format2.sample_rate);
    assert_eq!(format1.channels, format2.channels);
    assert_eq!(format1.bits_per_sample, format2.bits_per_sample);
    assert_eq!(format1.sample_count, format2.sample_count);

    log(
        "AudioFormat Clone trait工作正常",
        "AudioFormat Clone trait behaves correctly",
    );
}

#[test]
fn test_audio_format_partial_eq() {
    let format1 = AudioFormat::new(44100, 2, 16, 1000000);
    let format2 = AudioFormat::new(44100, 2, 16, 1000000);
    let format3 = AudioFormat::new(48000, 2, 16, 1000000);

    assert_eq!(format1, format2, "相同参数应该相等");
    assert_ne!(format1, format3, "不同采样率应该不相等");

    log(
        "AudioFormat PartialEq trait工作正常",
        "AudioFormat PartialEq trait behaves correctly",
    );
}

#[test]
fn test_audio_format_debug() {
    let format = AudioFormat::new(44100, 2, 16, 1000000);
    let debug_str = format!("{format:?}");

    assert!(debug_str.contains("44100"));
    assert!(debug_str.contains("2"));
    assert!(debug_str.contains("16"));

    log(
        format!("AudioFormat Debug trait工作正常: {debug_str}"),
        format!("AudioFormat Debug trait works: {debug_str}"),
    );
}

// ========== 综合场景测试 ==========

#[test]
fn test_typical_flac_format() {
    let format = AudioFormat::with_codec(44100, 2, 24, 10662000, CODEC_TYPE_FLAC);

    assert!(format.validate().is_ok());
    assert!((format.duration_seconds() - 241.678).abs() < 0.1); // 约4分钟
    assert_eq!(format.estimated_pcm_size_bytes(), 10662000 * 2 * 3);

    log(
        "典型FLAC格式场景测试通过",
        "Typical FLAC format scenario validated",
    );
}

#[test]
fn test_partial_analysis_workflow() {
    let mut format = AudioFormat::new(44100, 2, 16, 1000000);

    // 初始状态
    assert!(!format.is_partial());

    // 验证通过
    assert!(format.validate().is_ok());

    // 模拟部分分析
    format.mark_as_partial(15);

    // 部分分析后仍然有效
    assert!(format.validate().is_ok());
    assert!(format.is_partial());
    assert_eq!(format.skipped_packets(), 15);

    log(
        "部分分析工作流测试通过",
        "Partial analysis workflow validated",
    );
}
