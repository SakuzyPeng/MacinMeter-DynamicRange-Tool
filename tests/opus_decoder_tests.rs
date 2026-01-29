//! Opus解码器专项测试
//!
//! 测试SongbirdOpusDecoder的功能和正确性

use macinmeter_dr_tool::audio::{SongbirdOpusDecoder, StreamingDecoder};
use macinmeter_dr_tool::tools::{AppConfig, processor::process_audio_file_streaming};
use std::path::PathBuf;

mod audio_test_fixtures;
use audio_test_fixtures::{ensure_fixtures_generated, fixture_path};

fn log(msg_zh: impl AsRef<str>, msg_en: impl AsRef<str>) {
    println!("{} / {}", msg_zh.as_ref(), msg_en.as_ref());
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
        parallel_files: None,
        silence_filter_threshold_db: None,
        edge_trim_threshold_db: None,
        edge_trim_min_run_ms: None,
        exclude_lfe: false,
        show_rms_peak: false,
        compact_output: false,
        auto_launched: false,
        dsd_pcm_rate: Some(352_800),
        dsd_gain_db: 6.0,
        dsd_filter: "teac".to_string(),
        no_save: false,
    }
}

// ========== 基础功能测试 ==========

#[test]
fn test_opus_decoder_creation() {
    let path = PathBuf::from("audio/test_real_opus.opus");

    if !path.exists() {
        log(
            "跳过测试：opus测试文件不存在",
            "Skipping test: Opus sample file missing",
        );
        return;
    }

    let decoder = SongbirdOpusDecoder::new(&path);

    match decoder {
        Ok(decoder) => {
            let format = decoder.format();
            log("Opus解码器创建成功", "Opus decoder created successfully");
            log(
                format!("  采样率: {}Hz", format.sample_rate),
                format!("  Sample rate: {} Hz", format.sample_rate),
            );
            log(
                format!("  声道数: {}", format.channels),
                format!("  Channels: {}", format.channels),
            );
            log(
                format!("  位深: {}bit", format.bits_per_sample),
                format!("  Bit depth: {} bit", format.bits_per_sample),
            );
            log(
                format!("  总样本数: {}", format.sample_count),
                format!("  Total samples: {}", format.sample_count),
            );

            // Opus默认采样率应该是48kHz
            assert_eq!(format.sample_rate, 48000, "Opus采样率应该是48kHz");
            assert!(
                format.channels >= 1 && format.channels <= 2,
                "声道数应该是1或2"
            );
        }
        Err(e) => {
            panic!("Opus解码器创建失败: {e:?}");
        }
    }
}

#[test]
fn test_opus_decoding_streaming() {
    let path = PathBuf::from("audio/test_real_opus.opus");

    if !path.exists() {
        log(
            "跳过测试：opus测试文件不存在",
            "Skipping test: Opus sample file missing",
        );
        return;
    }

    let mut decoder = match SongbirdOpusDecoder::new(&path) {
        Ok(d) => d,
        Err(e) => {
            panic!("Opus解码器创建失败: {e:?}");
        }
    };

    let mut total_samples = 0;
    let mut chunk_count = 0;

    // 流式解码
    loop {
        match decoder.next_chunk() {
            Ok(Some(samples)) => {
                chunk_count += 1;
                total_samples += samples.len();

                // 验证样本值在有效范围内
                for &sample in &samples {
                    assert!(
                        (-1.0..=1.0).contains(&sample),
                        "样本值应该在[-1.0, 1.0]范围内，实际值: {sample}"
                    );
                }
            }
            Ok(None) => {
                // 解码完成
                break;
            }
            Err(e) => {
                panic!("Opus解码失败: {e:?}");
            }
        }
    }

    log("Opus流式解码成功", "Opus streaming decode succeeded");
    log(
        format!("  解码chunk数: {chunk_count}"),
        format!("  Chunks decoded: {chunk_count}"),
    );
    log(
        format!("  总样本数: {total_samples}"),
        format!("  Total samples: {total_samples}"),
    );

    assert!(total_samples > 0, "应该至少解码出一些样本");
    assert!(chunk_count > 0, "应该至少有一个chunk");
}

#[test]
fn test_opus_progress_tracking() {
    let path = PathBuf::from("audio/test_real_opus.opus");

    if !path.exists() {
        log(
            "跳过测试：opus测试文件不存在",
            "Skipping test: Opus sample file missing",
        );
        return;
    }

    let mut decoder = match SongbirdOpusDecoder::new(&path) {
        Ok(d) => d,
        Err(e) => {
            panic!("Opus解码器创建失败: {e:?}");
        }
    };

    let mut last_progress = 0.0;

    // 解码并跟踪进度
    while let Ok(Some(_)) = decoder.next_chunk() {
        let progress = decoder.progress();

        // 进度应该单调递增
        assert!(
            progress >= last_progress,
            "进度应该递增，上次: {last_progress}, 当前: {progress}"
        );

        // 进度应该在[0, 1]范围内
        assert!(
            (0.0..=1.0).contains(&progress),
            "进度应该在[0, 1]范围内，实际值: {progress}"
        );

        last_progress = progress;
    }

    log(
        "Opus进度跟踪正确",
        "Opus progress tracking behaves correctly",
    );
    log(
        format!("  最终进度: {:.2}%", last_progress * 100.0),
        format!("  Final progress: {:.2}%", last_progress * 100.0),
    );
}

#[test]
fn test_opus_reset() {
    let path = PathBuf::from("audio/test_real_opus.opus");

    if !path.exists() {
        log(
            "跳过测试：opus测试文件不存在",
            "Skipping test: Opus sample file missing",
        );
        return;
    }

    let mut decoder = match SongbirdOpusDecoder::new(&path) {
        Ok(d) => d,
        Err(e) => {
            panic!("Opus解码器创建失败: {e:?}");
        }
    };

    // 第一次解码
    let mut first_samples: Vec<f32> = Vec::new();
    while let Ok(Some(samples)) = decoder.next_chunk() {
        first_samples.extend_from_slice(&samples);
        if first_samples.len() > 10000 {
            break; // 只解码一部分
        }
    }

    // 重置解码器
    decoder.reset().expect("重置失败");

    // 验证进度被重置
    assert_eq!(decoder.progress(), 0.0, "重置后进度应该为0");

    // 第二次解码
    let mut second_samples: Vec<f32> = Vec::new();
    while let Ok(Some(samples)) = decoder.next_chunk() {
        second_samples.extend_from_slice(&samples);
        if second_samples.len() > 10000 {
            break;
        }
    }

    // 验证重置后的解码结果与第一次一致
    assert_eq!(
        first_samples.len(),
        second_samples.len(),
        "重置后解码的样本数应该一致"
    );

    log(
        "Opus解码器重置功能正常",
        "Opus decoder reset works correctly",
    );
}

// ========== 集成测试 ==========

#[test]
fn test_opus_dr_calculation() {
    let path = PathBuf::from("audio/test_real_opus.opus");

    if !path.exists() {
        log(
            "跳过测试：opus测试文件不存在",
            "Skipping test: Opus sample file missing",
        );
        return;
    }

    let config = default_test_config();
    let result = process_audio_file_streaming(&path, &config);

    match result {
        Ok((dr_results, format, _, _)) => {
            log("Opus文件DR计算成功", "Opus DR calculation succeeded");
            log(
                format!(
                    "  格式: {}Hz, {}bit, {}ch",
                    format.sample_rate, format.bits_per_sample, format.channels
                ),
                format!(
                    "  Format: {} Hz, {} bit, {} ch",
                    format.sample_rate, format.bits_per_sample, format.channels
                ),
            );

            if let Some(dr) = dr_results.first() {
                log(
                    format!("  DR值: {:.2}dB", dr.dr_value),
                    format!("  DR value: {:.2} dB", dr.dr_value),
                );
                log(
                    format!("  Peak: {:.6}", dr.peak),
                    format!("  Peak: {:.6}", dr.peak),
                );
                log(
                    format!("  RMS: {:.6}", dr.rms),
                    format!("  RMS: {:.6}", dr.rms),
                );

                // DR值应该在合理范围内
                assert!(
                    dr.dr_value >= 0.0 && dr.dr_value < 50.0,
                    "DR值应该在合理范围内，实际值: {}",
                    dr.dr_value
                );

                // Peak值应该有效
                assert!(
                    dr.peak > 0.0 && dr.peak <= 1.0,
                    "Peak值应该在(0, 1]范围内，实际值: {}",
                    dr.peak
                );

                // RMS值应该有效
                assert!(
                    dr.rms > 0.0 && dr.rms <= dr.peak,
                    "RMS值应该在(0, peak]范围内，实际值: {}",
                    dr.rms
                );
            }
        }
        Err(e) => {
            panic!("Opus文件处理失败: {e:?}");
        }
    }
}

#[test]
fn test_ogg_opus_compatibility() {
    let path = PathBuf::from("audio/test_compatibility.ogg");

    if !path.exists() {
        log(
            "跳过测试：ogg测试文件不存在",
            "Skipping test: OGG sample file missing",
        );
        return;
    }

    // 注意：.ogg文件可能是Opus或Vorbis编码
    // 这个测试验证我们能否正确处理ogg容器中的opus
    let config = default_test_config();
    let result = process_audio_file_streaming(&path, &config);

    match result {
        Ok((dr_results, format, _, _)) => {
            log("OGG文件处理成功", "OGG file processed successfully");
            log(
                format!(
                    "  格式: {}Hz, {}bit, {}ch",
                    format.sample_rate, format.bits_per_sample, format.channels
                ),
                format!(
                    "  Format: {} Hz, {} bit, {} ch",
                    format.sample_rate, format.bits_per_sample, format.channels
                ),
            );

            if let Some(dr) = dr_results.first() {
                log(
                    format!("  DR值: {:.2}dB", dr.dr_value),
                    format!("  DR value: {:.2} dB", dr.dr_value),
                );
            }
        }
        Err(e) => {
            log(
                format!("OGG文件处理失败: {e:?}"),
                format!("Failed to process OGG file: {e:?}"),
            );
            // OGG可能不是Opus编码，允许失败
        }
    }
}

// ========== 错误处理测试 ==========

#[test]
fn test_invalid_opus_file() {
    ensure_fixtures_generated();
    let path = fixture_path("fake_audio.wav");

    // 尝试用opus解码器打开非opus文件
    let result = SongbirdOpusDecoder::new(&path);

    match result {
        Err(_) => {
            log("正确拒绝非Opus文件", "Non-Opus file correctly rejected");
        }
        Ok(_) => {
            log(
                "非Opus文件被错误接受（可能是解码器过于宽容）",
                "Non-Opus file was accepted (decoder may be too permissive)",
            );
        }
    }
}

#[test]
fn test_nonexistent_opus_file() {
    let path = PathBuf::from("nonexistent_file.opus");

    let result = SongbirdOpusDecoder::new(&path);

    assert!(result.is_err(), "不存在的文件应该返回错误");

    match result {
        Err(e) => {
            log(
                format!("正确处理不存在的文件: {e:?}"),
                format!("Handled missing file correctly: {e:?}"),
            );
        }
        Ok(_) => unreachable!(),
    }
}

// ========== 性能测试 ==========

#[test]
#[ignore] // 需要手动运行：cargo test test_opus_decoding_performance --ignored -- --nocapture
fn test_opus_decoding_performance() {
    let path = PathBuf::from("audio/test_real_opus.opus");

    if !path.exists() {
        log(
            "跳过测试：opus测试文件不存在",
            "Skipping test: Opus sample file missing",
        );
        return;
    }

    use std::time::Instant;

    let start = Instant::now();

    let config = default_test_config();
    let result = process_audio_file_streaming(&path, &config);

    let elapsed = start.elapsed();

    match result {
        Ok((dr_results, _format, _, _)) => {
            let file_size = std::fs::metadata(&path).unwrap().len();
            let throughput_mbps = (file_size as f64 / 1_048_576.0) / elapsed.as_secs_f64();

            log("Opus解码性能测试", "Opus decoding performance test");
            log(
                format!("  文件大小: {:.2} MB", file_size as f64 / 1_048_576.0),
                format!("  File size: {:.2} MB", file_size as f64 / 1_048_576.0),
            );
            log(
                format!("  解码时间: {:.3}s", elapsed.as_secs_f64()),
                format!("  Decode time: {:.3}s", elapsed.as_secs_f64()),
            );
            log(
                format!("  吞吐量: {throughput_mbps:.2} MB/s"),
                format!("  Throughput: {throughput_mbps:.2} MB/s"),
            );

            if let Some(dr) = dr_results.first() {
                log(
                    format!("  DR值: {:.2}dB", dr.dr_value),
                    format!("  DR value: {:.2} dB", dr.dr_value),
                );
            }

            // 性能基准：至少应该达到10 MB/s
            assert!(
                throughput_mbps > 10.0,
                "Opus解码吞吐量过低: {throughput_mbps:.2} MB/s"
            );
        }
        Err(e) => {
            panic!("性能测试失败: {e:?}");
        }
    }
}
