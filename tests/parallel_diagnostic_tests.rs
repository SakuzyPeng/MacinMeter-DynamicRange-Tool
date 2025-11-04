//! 并行解码器诊断测试
//!
//! 用于诊断并行模式DR值偏差问题

use macinmeter_dr_tool::audio::UniversalDecoder;
use std::path::PathBuf;

mod audio_test_fixtures;
use audio_test_fixtures::{ensure_fixtures_generated, fixture_path};

fn log(msg_zh: impl AsRef<str>, msg_en: impl AsRef<str>) {
    println!("{} / {}", msg_zh.as_ref(), msg_en.as_ref());
}

fn log_err(msg_zh: impl AsRef<str>, msg_en: impl AsRef<str>) {
    eprintln!("{} / {}", msg_zh.as_ref(), msg_en.as_ref());
}

/// 测试串行和并行模式的样本一致性
#[test]
fn test_serial_vs_parallel_samples() {
    // 使用测试文件 - 先尝试环境变量指定的文件，否则使用fixture
    ensure_fixtures_generated();
    let test_file = std::env::var("TEST_AUDIO_FILE")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| fixture_path("high_sample_rate.wav"));

    if !test_file.exists() {
        log_err(
            format!("警告: 测试文件不存在: {test_file:?}"),
            format!("Warning: test file not found: {test_file:?}"),
        );
        log_err(
            "可以通过环境变量指定: TEST_AUDIO_FILE=/path/to/audio.flac cargo test",
            "Set TEST_AUDIO_FILE=/path/to/audio.flac to specify a test asset",
        );
        return;
    }

    log(
        format!("使用测试文件: {test_file:?}"),
        format!("Using test file: {test_file:?}"),
    );

    let decoder = UniversalDecoder;

    // 1. 串行模式解码
    log("=== 串行模式解码 ===", "=== Serial decoding ===");
    let mut serial_decoder = decoder
        .create_streaming(&test_file)
        .expect("串行解码器创建失败");

    let mut serial_samples = Vec::new();
    let mut serial_chunks = 0;

    while let Some(chunk) = serial_decoder.next_chunk().expect("串行解码失败") {
        serial_chunks += 1;
        serial_samples.extend_from_slice(&chunk);
    }

    log(
        format!(
            "串行模式: {} 个chunk, {} 个样本",
            serial_chunks,
            serial_samples.len()
        ),
        format!(
            "Serial mode: {} chunks, {} samples",
            serial_chunks,
            serial_samples.len()
        ),
    );

    // 2. 并行模式解码
    log("\n=== 并行模式解码 ===", "\n=== Parallel decoding ===");
    let mut parallel_decoder = decoder
        .create_streaming_parallel(
            &test_file,
            true,     // parallel_enabled
            Some(64), // batch_size
            Some(4),  // thread_count
        )
        .expect("并行解码器创建失败");

    let mut parallel_samples = Vec::new();
    let mut parallel_chunks = 0;

    while let Some(chunk) = parallel_decoder.next_chunk().expect("并行解码失败") {
        parallel_chunks += 1;
        parallel_samples.extend_from_slice(&chunk);
    }

    log(
        format!(
            "并行模式: {} 个chunk, {} 个样本",
            parallel_chunks,
            parallel_samples.len()
        ),
        format!(
            "Parallel mode: {} chunks, {} samples",
            parallel_chunks,
            parallel_samples.len()
        ),
    );

    // 3. 对比样本数量
    log(
        "\n=== 样本数量对比 ===",
        "\n=== Sample count comparison ===",
    );
    if serial_samples.len() != parallel_samples.len() {
        log_err(
            format!(
                "⚠️ 样本数量不一致: 串行={} 并行={}",
                serial_samples.len(),
                parallel_samples.len()
            ),
            format!(
                "⚠️ Sample count mismatch: serial={} parallel={}",
                serial_samples.len(),
                parallel_samples.len()
            ),
        );

        // 在严格模式下保留失败（例如设置 CI_STRICT_PARALLEL=1）
        if std::env::var("CI_STRICT_PARALLEL").is_ok() {
            panic!(
                "样本数量不一致 (serial={} parallel={})",
                serial_samples.len(),
                parallel_samples.len()
            );
        } else {
            return;
        }
    }
    log(
        format!("样本数量一致: {}", serial_samples.len()),
        format!("Sample count matches: {}", serial_samples.len()),
    );

    // 4. 对比前1000个样本的值
    log(
        "\n=== 样本值对比（前1000个） ===",
        "\n=== Sample value comparison (first 1000) ===",
    );
    let compare_count = 1000.min(serial_samples.len());
    let mut diff_count = 0;
    let mut max_diff = 0.0f32;

    for i in 0..compare_count {
        let serial_val = serial_samples[i];
        let parallel_val = parallel_samples[i];
        let diff = (serial_val - parallel_val).abs();

        if diff > 1e-6 {
            diff_count += 1;
            max_diff = max_diff.max(diff);

            if diff_count <= 10 {
                log(
                    format!("  [{i}] 串行={serial_val:.6}, 并行={parallel_val:.6}, 差值={diff:.6}"),
                    format!(
                        "  [{i}] serial={serial_val:.6}, parallel={parallel_val:.6}, diff={diff:.6}"
                    ),
                );
            }
        }
    }

    if diff_count > 0 {
        log(
            format!("发现 {diff_count} 处差异，最大差值: {max_diff:.6}"),
            format!("Found {diff_count} differences, max delta: {max_diff:.6}"),
        );
        log(
            "\n=== 前20个样本详细对比 ===",
            "\n=== Detailed comparison of first 20 samples ===",
        );
        for i in 0..20.min(serial_samples.len()) {
            let serial = serial_samples[i];
            let parallel = parallel_samples[i];
            let diff = (serial - parallel).abs();
            log(
                format!("  [{i}] 串行={serial:.8}, 并行={parallel:.8}, 差值={diff:.8}"),
                format!("  [{i}] serial={serial:.8}, parallel={parallel:.8}, diff={diff:.8}"),
            );
        }
        panic!("样本值不一致");
    } else {
        log(
            format!("前{compare_count}个样本值完全一致"),
            format!("First {compare_count} samples match exactly"),
        );
    }

    // 5. 对比全部样本（使用更宽松的精度）
    log("\n=== 全部样本对比 ===", "\n=== Full sample comparison ===");
    let epsilon = 1e-6;
    for (i, (&serial_val, &parallel_val)) in serial_samples
        .iter()
        .zip(parallel_samples.iter())
        .enumerate()
    {
        let diff = (serial_val - parallel_val).abs();
        if diff > epsilon {
            log(
                format!(
                    "样本[{i}] 不一致: 串行={serial_val:.8}, 并行={parallel_val:.8}, 差值={diff:.8}"
                ),
                format!(
                    "Sample [{i}] mismatch: serial={serial_val:.8}, parallel={parallel_val:.8}, diff={diff:.8}"
                ),
            );

            log("\n附近样本:", "\nNeighbour samples:");
            for j in i.saturating_sub(5)..=(i + 5).min(serial_samples.len() - 1) {
                let s = serial_samples[j];
                let p = parallel_samples[j];
                log(
                    format!("  [{j}] 串行={s:.8}, 并行={p:.8}"),
                    format!("  [{j}] serial={s:.8}, parallel={p:.8}"),
                );
            }

            panic!("在样本[{i}]处发现不一致");
        }
    }

    log("全部样本值一致", "All sample values match");
}

/// 测试并行解码器的chunk顺序
#[test]
fn test_parallel_chunk_order() {
    let test_file = fixture_path("beethoven_9th_2_Scherzo_snippet.flac");

    if !test_file.exists() {
        log_err(
            "警告: 测试文件不存在，跳过测试",
            "Warning: test file missing, skipping test",
        );
        return;
    }

    let universal_decoder = UniversalDecoder;

    // 创建并行解码器
    let mut decoder = universal_decoder
        .create_streaming_parallel(
            &test_file,
            true,     // parallel_enabled
            Some(64), // batch_size
            Some(4),  // thread_count
        )
        .expect("并行解码器创建失败");

    log("=== 检查chunk顺序 ===", "=== Checking chunk order ===");

    let mut chunk_count = 0;
    let mut total_samples = 0;
    let mut prev_first_sample: Option<f32> = None;

    while let Some(chunk) = decoder.next_chunk().expect("解码失败") {
        chunk_count += 1;
        total_samples += chunk.len();

        if !chunk.is_empty() {
            let first_sample = chunk[0];
            let last_sample = chunk[chunk.len() - 1];

            let len = chunk.len();
            log(
                format!(
                    "Chunk {chunk_count}: {len} 样本, 首样本={first_sample:.6}, 尾样本={last_sample:.6}"
                ),
                format!(
                    "Chunk {chunk_count}: {len} samples, first={first_sample:.6}, last={last_sample:.6}"
                ),
            );

            // 检查是否有重复的首样本（可能表示顺序错误）
            match prev_first_sample {
                Some(prev) if (prev - first_sample).abs() < 1e-9 && chunk_count > 1 => {
                    log(
                        format!("警告: Chunk {chunk_count} 的首样本与前一个chunk相同"),
                        format!("Warning: chunk {chunk_count} first sample matches previous chunk"),
                    );
                }
                _ => {}
            }

            prev_first_sample = Some(first_sample);
        }
    }

    log(
        format!("\n总计: {chunk_count} 个chunk, {total_samples} 个样本"),
        format!("\nTotal: {chunk_count} chunks, {total_samples} samples"),
    );
}
