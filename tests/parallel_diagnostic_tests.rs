//! 并行解码器诊断测试
//!
//! 用于诊断并行模式DR值偏差问题

use macinmeter_dr_tool::audio::UniversalDecoder;
use std::path::PathBuf;

/// 测试串行和并行模式的样本一致性
#[test]
fn test_serial_vs_parallel_samples() {
    // 使用测试文件 - 先尝试环境变量指定的文件，否则使用fixture
    let test_file = std::env::var("TEST_AUDIO_FILE")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/fixtures/high_sample_rate.wav"));

    if !test_file.exists() {
        eprintln!("警告: 测试文件不存在: {test_file:?}");
        eprintln!("可以通过环境变量指定: TEST_AUDIO_FILE=/path/to/audio.flac cargo test");
        return;
    }

    println!("使用测试文件: {test_file:?}");

    let decoder = UniversalDecoder;

    // 1. 串行模式解码
    println!("=== 串行模式解码 ===");
    let mut serial_decoder = decoder
        .create_streaming(&test_file)
        .expect("串行解码器创建失败");

    let mut serial_samples = Vec::new();
    let mut serial_chunks = 0;

    while let Some(chunk) = serial_decoder.next_chunk().expect("串行解码失败") {
        serial_chunks += 1;
        serial_samples.extend_from_slice(&chunk);
    }

    println!(
        "串行模式: {} 个chunk, {} 个样本",
        serial_chunks,
        serial_samples.len()
    );

    // 2. 并行模式解码
    println!("\n=== 并行模式解码 ===");
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

    println!(
        "并行模式: {} 个chunk, {} 个样本",
        parallel_chunks,
        parallel_samples.len()
    );

    // 3. 对比样本数量
    println!("\n=== 样本数量对比 ===");
    assert_eq!(
        serial_samples.len(),
        parallel_samples.len(),
        "样本数量不一致"
    );
    println!("✅ 样本数量一致: {}", serial_samples.len());

    // 4. 对比前1000个样本的值
    println!("\n=== 样本值对比（前1000个） ===");
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
                println!("  [{i}] 串行={serial_val:.6}, 并行={parallel_val:.6}, 差值={diff:.6}");
            }
        }
    }

    if diff_count > 0 {
        println!("❌ 发现 {diff_count} 处差异，最大差值: {max_diff:.6}");
        println!("\n=== 前20个样本详细对比 ===");
        for i in 0..20.min(serial_samples.len()) {
            let serial = serial_samples[i];
            let parallel = parallel_samples[i];
            let diff = (serial - parallel).abs();
            println!("  [{i}] 串行={serial:.8}, 并行={parallel:.8}, 差值={diff:.8}");
        }
        panic!("样本值不一致");
    } else {
        println!("✅ 前{compare_count}个样本值完全一致");
    }

    // 5. 对比全部样本（使用更宽松的精度）
    println!("\n=== 全部样本对比 ===");
    let epsilon = 1e-6;
    for (i, (&serial_val, &parallel_val)) in serial_samples
        .iter()
        .zip(parallel_samples.iter())
        .enumerate()
    {
        let diff = (serial_val - parallel_val).abs();
        if diff > epsilon {
            println!(
                "❌ 样本[{i}] 不一致: 串行={serial_val:.8}, 并行={parallel_val:.8}, 差值={diff:.8}"
            );

            // 打印附近的样本
            println!("\n附近样本:");
            for j in i.saturating_sub(5)..=(i + 5).min(serial_samples.len() - 1) {
                let s = serial_samples[j];
                let p = parallel_samples[j];
                println!("  [{j}] 串行={s:.8}, 并行={p:.8}");
            }

            panic!("在样本[{i}]处发现不一致");
        }
    }

    println!("✅ 全部样本值一致");
}

/// 测试并行解码器的chunk顺序
#[test]
fn test_parallel_chunk_order() {
    let test_file = PathBuf::from("tests/fixtures/beethoven_9th_2_Scherzo_snippet.flac");

    if !test_file.exists() {
        eprintln!("警告: 测试文件不存在，跳过测试");
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

    println!("=== 检查chunk顺序 ===");

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
            println!(
                "Chunk {chunk_count}: {len} 样本, 首样本={first_sample:.6}, 尾样本={last_sample:.6}"
            );

            // 检查是否有重复的首样本（可能表示顺序错误）
            match prev_first_sample {
                Some(prev) if (prev - first_sample).abs() < 1e-9 && chunk_count > 1 => {
                    println!("⚠️ 警告: Chunk {chunk_count} 的首样本与前一个chunk相同");
                }
                _ => {}
            }

            prev_first_sample = Some(first_sample);
        }
    }

    println!("\n总计: {chunk_count} 个chunk, {total_samples} 个样本");
}
