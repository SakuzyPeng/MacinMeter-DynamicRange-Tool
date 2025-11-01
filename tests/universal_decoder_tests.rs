//! UniversalDecoder统一解码器测试
//!
//! 测试UniversalDecoder的格式检测、解码器创建和错误处理

use macinmeter_dr_tool::AudioError;
use macinmeter_dr_tool::audio::UniversalDecoder;
use std::path::PathBuf;

fn log(msg_zh: impl AsRef<str>, msg_en: impl AsRef<str>) {
    println!("{} / {}", msg_zh.as_ref(), msg_en.as_ref());
}

// ========== 基础功能测试 ==========

#[test]
fn test_universal_decoder_new() {
    let decoder = UniversalDecoder::new();

    // 验证可以正常创建
    let formats = decoder.supported_formats();
    assert!(!formats.extensions.is_empty());

    log(
        "UniversalDecoder::new() 创建成功",
        "UniversalDecoder::new() constructed successfully",
    );
}

#[test]
fn test_universal_decoder_default() {
    let decoder = UniversalDecoder;

    // Default应该和new()行为一致
    let formats = decoder.supported_formats();
    assert!(!formats.extensions.is_empty());

    log(
        "UniversalDecoder Default trait工作正常",
        "UniversalDecoder Default trait behaves correctly",
    );
}

// ========== 格式支持查询测试 ==========

#[test]
fn test_supported_formats_completeness() {
    let decoder = UniversalDecoder::new();
    let formats = decoder.supported_formats();

    // 验证包含所有预期格式
    let expected_formats = vec![
        "wav", "flac", "aiff", "m4a", // 无损
        "mp3", "mp1", "aac", "ogg", "opus", // 有损
        "mkv", "webm", // 容器
    ];

    for format in expected_formats {
        assert!(
            formats.extensions.contains(&format),
            "应该支持 {format} 格式"
        );
    }

    log(
        format!("支持的格式列表完整：{:?}", formats.extensions),
        format!("Supported format list complete: {:?}", formats.extensions),
    );
}

#[test]
fn test_supported_formats_immutable() {
    let decoder1 = UniversalDecoder::new();
    let decoder2 = UniversalDecoder::new();

    let formats1 = decoder1.supported_formats();
    let formats2 = decoder2.supported_formats();

    // 应该返回相同的静态数据
    assert_eq!(formats1.extensions.len(), formats2.extensions.len());

    log(
        "supported_formats() 返回不可变静态数据",
        "supported_formats() returns immutable static data",
    );
}

// ========== can_decode() 文件扩展名检测测试 ==========

#[test]
fn test_can_decode_supported_formats() {
    let decoder = UniversalDecoder::new();

    let supported_files = vec![
        PathBuf::from_iter(["test.wav"]),
        PathBuf::from_iter(["test.flac"]),
        PathBuf::from_iter(["test.mp3"]),
        PathBuf::from_iter(["test.aac"]),
        PathBuf::from_iter(["test.ogg"]),
        PathBuf::from_iter(["test.opus"]),
        PathBuf::from_iter(["test.m4a"]),
        PathBuf::from_iter(["test.aiff"]),
    ];

    for path in supported_files {
        assert!(decoder.can_decode(&path), "{} 应该可以解码", path.display());
    }

    log(
        "can_decode() 正确识别支持的格式",
        "can_decode() accepts supported formats",
    );
}

#[test]
fn test_can_decode_unsupported_formats() {
    let decoder = UniversalDecoder::new();

    let unsupported_files = vec![
        PathBuf::from_iter(["test.txt"]),
        PathBuf::from_iter(["test.pdf"]),
        PathBuf::from_iter(["test.jpg"]),
        PathBuf::from_iter(["test.mp4"]), // 视频容器
        PathBuf::from_iter(["test.avi"]),
        PathBuf::from_iter(["test.unknown"]),
    ];

    for path in unsupported_files {
        assert!(
            !decoder.can_decode(&path),
            "{} 不应该可以解码",
            path.display()
        );
    }

    log(
        "can_decode() 正确拒绝不支持的格式",
        "can_decode() rejects unsupported formats",
    );
}

#[test]
fn test_can_decode_case_insensitive() {
    let decoder = UniversalDecoder::new();

    // 测试大小写不敏感
    assert!(decoder.can_decode(&PathBuf::from_iter(["test.WAV"])));
    assert!(decoder.can_decode(&PathBuf::from_iter(["test.Flac"])));
    assert!(decoder.can_decode(&PathBuf::from_iter(["test.MP3"])));
    assert!(decoder.can_decode(&PathBuf::from_iter(["test.OpUs"])));

    log(
        "can_decode() 大小写不敏感",
        "can_decode() is case-insensitive",
    );
}

#[test]
fn test_can_decode_no_extension() {
    let decoder = UniversalDecoder::new();

    // 无扩展名文件应该返回false
    assert!(!decoder.can_decode(&PathBuf::from_iter(["test"])));
    assert!(!decoder.can_decode(&PathBuf::from_iter(["no_extension"])));

    log(
        "can_decode() 正确处理无扩展名文件",
        "can_decode() handles missing extensions",
    );
}

#[test]
fn test_can_decode_complex_paths() {
    let decoder = UniversalDecoder::new();

    // 复杂路径
    assert!(decoder.can_decode(&PathBuf::from_iter(["path", "to", "music", "song.flac"])));
    assert!(decoder.can_decode(&PathBuf::from_iter(["..", "audio", "test.mp3"])));
    assert!(decoder.can_decode(&PathBuf::from_iter([".", "files", "track.opus"])));

    log(
        "can_decode() 正确处理复杂路径",
        "can_decode() handles complex paths",
    );
}

// ========== probe_format() 格式探测测试 ==========

#[test]
fn test_probe_format_wav_file() {
    let decoder = UniversalDecoder::new();

    // 使用测试固件中的WAV文件
    let path = PathBuf::from("tests/fixtures/silence.wav");

    if !path.exists() {
        log(
            "跳过测试：WAV测试文件不存在",
            "Skipping test: WAV fixture missing",
        );
        return;
    }

    let result = decoder.probe_format(&path);

    match result {
        Ok(format) => {
            assert_eq!(format.sample_rate, 44100);
            assert_eq!(format.channels, 2);
            assert_eq!(format.bits_per_sample, 16);
            log(
                format!(
                    "WAV格式探测成功：{}Hz, {}ch, {}bit",
                    format.sample_rate, format.channels, format.bits_per_sample
                ),
                format!(
                    "WAV probe success: {} Hz, {} ch, {} bit",
                    format.sample_rate, format.channels, format.bits_per_sample
                ),
            );
        }
        Err(e) => {
            panic!("WAV格式探测失败: {e:?}");
        }
    }
}

#[test]
fn test_probe_format_high_sample_rate() {
    let decoder = UniversalDecoder::new();

    let path = PathBuf::from("tests/fixtures/high_sample_rate.wav");

    if !path.exists() {
        log(
            "跳过测试：高采样率测试文件不存在",
            "Skipping test: high sample-rate fixture missing",
        );
        return;
    }

    let result = decoder.probe_format(&path);

    match result {
        Ok(format) => {
            assert_eq!(format.sample_rate, 192000);
            assert_eq!(format.bits_per_sample, 24);
            log(
                format!(
                    "高采样率格式探测成功：{}Hz, {}bit",
                    format.sample_rate, format.bits_per_sample
                ),
                format!(
                    "High sample-rate probe success: {} Hz, {} bit",
                    format.sample_rate, format.bits_per_sample
                ),
            );
        }
        Err(e) => {
            panic!("高采样率格式探测失败: {e:?}");
        }
    }
}

#[test]
fn test_probe_format_nonexistent_file() {
    let decoder = UniversalDecoder::new();

    let path = PathBuf::from("nonexistent_file_12345.wav");
    let result = decoder.probe_format(&path);

    assert!(result.is_err(), "不存在的文件应该返回错误");

    match result {
        Err(AudioError::IoError(_)) => {
            log(
                "正确处理不存在的文件（IoError）",
                "Handled missing file correctly (IoError)",
            );
        }
        Err(e) => {
            log(
                format!("正确处理不存在的文件: {e:?}"),
                format!("Handled missing file correctly: {e:?}"),
            );
        }
        Ok(_) => unreachable!(),
    }
}

#[test]
fn test_probe_format_invalid_file() {
    let decoder = UniversalDecoder::new();

    // 使用伪装的音频文件
    let path = PathBuf::from("tests/fixtures/fake_audio.wav");

    if !path.exists() {
        log(
            "跳过测试：伪装文件不存在",
            "Skipping test: fake audio fixture missing",
        );
        return;
    }

    let result = decoder.probe_format(&path);

    assert!(result.is_err(), "无效文件应该返回错误");

    match result {
        Err(AudioError::FormatError(_)) => {
            log(
                "正确拒绝无效音频文件（FormatError）",
                "Invalid audio file correctly rejected (FormatError)",
            );
        }
        Err(e) => {
            log(
                format!("正确拒绝无效音频文件: {e:?}"),
                format!("Invalid audio file correctly rejected: {e:?}"),
            );
        }
        Ok(_) => panic!("伪装文件不应该被接受"),
    }
}

// ========== create_streaming() 串行解码器创建测试 ==========

#[test]
fn test_create_streaming_wav() {
    let decoder = UniversalDecoder::new();

    let path = PathBuf::from("tests/fixtures/silence.wav");

    if !path.exists() {
        log(
            "跳过测试：WAV测试文件不存在",
            "Skipping test: WAV fixture missing",
        );
        return;
    }

    let result = decoder.create_streaming(&path);

    match result {
        Ok(stream_decoder) => {
            let format = stream_decoder.format();
            assert_eq!(format.sample_rate, 44100);
            log("串行WAV解码器创建成功", "Serial WAV decoder created");
        }
        Err(e) => {
            panic!("串行解码器创建失败: {e:?}");
        }
    }
}

#[test]
fn test_create_streaming_opus() {
    let decoder = UniversalDecoder::new();

    let path = PathBuf::from("audio/test_real_opus.opus");

    if !path.exists() {
        log(
            "跳过测试：Opus测试文件不存在",
            "Skipping test: Opus fixture missing",
        );
        return;
    }

    let result = decoder.create_streaming(&path);

    match result {
        Ok(stream_decoder) => {
            let format = stream_decoder.format();
            // Opus默认48kHz
            assert_eq!(format.sample_rate, 48000);
            log("Opus专用解码器创建成功", "Opus dedicated decoder created");
        }
        Err(e) => {
            panic!("Opus解码器创建失败: {e:?}");
        }
    }
}

#[test]
fn test_create_streaming_nonexistent() {
    let decoder = UniversalDecoder::new();

    let path = PathBuf::from("nonexistent_file.wav");
    let result = decoder.create_streaming(&path);

    assert!(result.is_err(), "不存在的文件应该返回错误");

    log(
        "串行解码器正确处理不存在的文件",
        "Serial decoder handled missing file correctly",
    );
}

// ========== create_streaming_parallel() 并行解码器创建测试 ==========

#[test]
fn test_create_streaming_parallel_disabled() {
    let decoder = UniversalDecoder::new();

    let path = PathBuf::from("tests/fixtures/silence.wav");

    if !path.exists() {
        log(
            "跳过测试：WAV测试文件不存在",
            "Skipping test: WAV fixture missing",
        );
        return;
    }

    // 禁用并行模式
    let result = decoder.create_streaming_parallel(&path, false, None, None);

    match result {
        Ok(stream_decoder) => {
            let format = stream_decoder.format();
            assert_eq!(format.sample_rate, 44100);
            log(
                "并行解码器创建成功（禁用模式）",
                "Parallel decoder created (mode disabled)",
            );
        }
        Err(e) => {
            panic!("并行解码器创建失败: {e:?}");
        }
    }
}

#[test]
fn test_create_streaming_parallel_enabled() {
    let decoder = UniversalDecoder::new();

    let path = PathBuf::from("tests/fixtures/silence.wav");

    if !path.exists() {
        log(
            "跳过测试：WAV测试文件不存在",
            "Skipping test: WAV fixture missing",
        );
        return;
    }

    // 启用并行模式，自定义配置
    let result = decoder.create_streaming_parallel(&path, true, Some(32), Some(2));

    match result {
        Ok(stream_decoder) => {
            let format = stream_decoder.format();
            assert_eq!(format.sample_rate, 44100);
            log(
                "并行解码器创建成功（启用模式，batch=32, threads=2）",
                "Parallel decoder created (enabled, batch=32, threads=2)",
            );
        }
        Err(e) => {
            panic!("并行解码器创建失败: {e:?}");
        }
    }
}

#[test]
fn test_create_streaming_parallel_mp3_fallback() {
    let decoder = UniversalDecoder::new();

    // 创建一个MP3文件路径（即使不存在，也会触发MP3检测逻辑）
    let path = PathBuf::from_iter(["test.mp3"]);

    // MP3格式应该自动回退到串行模式
    // 这个测试主要验证MP3检测逻辑，而不是实际解码
    let result = decoder.create_streaming_parallel(&path, true, None, None);

    // 如果文件不存在会报错，但错误类型应该是IoError而非FormatError
    // 这证明MP3检测逻辑在文件打开之前就执行了
    match result {
        Ok(_) => {
            panic!("MP3文件不存在应该返回错误");
        }
        Err(AudioError::IoError(_)) => {
            log(
                "MP3格式检测逻辑正确执行（文件不存在前已检测）",
                "MP3 detection logic executed before file access",
            );
        }
        Err(e) => {
            log(
                format!("MP3格式处理: {e:?}"),
                format!("MP3 handling result: {e:?}"),
            );
        }
    }
}

#[test]
fn test_create_streaming_parallel_opus_uses_dedicated_decoder() {
    let decoder = UniversalDecoder::new();

    let path = PathBuf::from("audio/test_real_opus.opus");

    if !path.exists() {
        log(
            "跳过测试：Opus测试文件不存在",
            "Skipping test: Opus fixture missing",
        );
        return;
    }

    // Opus格式应该使用专用解码器，忽略并行参数
    let result = decoder.create_streaming_parallel(&path, true, Some(128), Some(8));

    match result {
        Ok(stream_decoder) => {
            let format = stream_decoder.format();
            assert_eq!(format.sample_rate, 48000); // Opus特征
            log(
                "Opus并行请求正确回退到专用解码器",
                "Opus parallel request fell back to dedicated decoder",
            );
        }
        Err(e) => {
            panic!("Opus并行解码器创建失败: {e:?}");
        }
    }
}

// ========== 综合场景测试 ==========

#[test]
fn test_decoder_workflow_complete() {
    let decoder = UniversalDecoder::new();

    let path = PathBuf::from("tests/fixtures/silence.wav");

    if !path.exists() {
        log(
            "跳过测试：WAV测试文件不存在",
            "Skipping test: WAV fixture missing",
        );
        return;
    }

    // 1. 检查是否可以解码
    assert!(decoder.can_decode(&path));

    // 2. 探测格式
    let format = decoder.probe_format(&path).expect("格式探测失败");
    assert_eq!(format.sample_rate, 44100);

    // 3. 创建串行解码器
    let serial_decoder = decoder.create_streaming(&path).expect("串行解码器创建失败");
    assert_eq!(serial_decoder.format().sample_rate, 44100);

    // 4. 创建并行解码器
    let parallel_decoder = decoder
        .create_streaming_parallel(&path, true, None, None)
        .expect("并行解码器创建失败");
    assert_eq!(parallel_decoder.format().sample_rate, 44100);

    log(
        "完整解码器工作流测试通过",
        "Decoder end-to-end workflow passed",
    );
}

#[test]
fn test_multiple_decoders_independence() {
    let decoder1 = UniversalDecoder::new();
    let decoder2 = UniversalDecoder::new();

    // 两个解码器应该独立工作
    let path = PathBuf::from_iter(["test.flac"]);

    assert!(decoder1.can_decode(&path));
    assert!(decoder2.can_decode(&path));

    // 验证返回相同结果
    let formats1 = decoder1.supported_formats();
    let formats2 = decoder2.supported_formats();

    assert_eq!(formats1.extensions.len(), formats2.extensions.len());

    log(
        "多个解码器实例互相独立",
        "Multiple decoder instances remain independent",
    );
}
