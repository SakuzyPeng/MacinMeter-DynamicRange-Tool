//! 块大小统计测试
//!
//! 测试ChunkSizeStats统计功能和边界条件

use macinmeter_dr_tool::audio::ChunkSizeStats;

fn log(msg_zh: impl AsRef<str>, msg_en: impl AsRef<str>) {
    println!("{} / {}", msg_zh.as_ref(), msg_en.as_ref());
}

// ========== 基础创建测试 ==========

#[test]
fn test_chunk_stats_creation() {
    let stats = ChunkSizeStats::new();

    assert_eq!(stats.total_chunks, 0);
    assert_eq!(stats.min_size, usize::MAX);
    assert_eq!(stats.max_size, 0);
    assert_eq!(stats.mean_size, 0.0);

    log(
        "ChunkSizeStats::new() 创建成功",
        "ChunkSizeStats::new() constructed successfully",
    );
}

#[test]
fn test_chunk_stats_default() {
    let stats = ChunkSizeStats::default();

    assert_eq!(stats.total_chunks, 0);
    assert_eq!(stats.min_size, usize::MAX);
    assert_eq!(stats.max_size, 0);

    log(
        "ChunkSizeStats Default trait工作正常",
        "ChunkSizeStats Default trait works correctly",
    );
}

// ========== 空统计测试 ==========

#[test]
fn test_chunk_stats_empty_finalize() {
    let mut stats = ChunkSizeStats::new();

    // 不添加任何chunk就finalize
    stats.finalize();

    assert_eq!(stats.total_chunks, 0);
    assert_eq!(stats.min_size, 0); // finalize后修复为0
    assert_eq!(stats.max_size, 0);
    assert_eq!(stats.mean_size, 0.0);

    log(
        "空统计finalize后min_size修复为0",
        "Empty stats finalize sets min_size to 0",
    );
}

// ========== 单个chunk测试 ==========

#[test]
fn test_chunk_stats_single_chunk() {
    let mut stats = ChunkSizeStats::new();

    stats.add_chunk(1024);

    assert_eq!(stats.total_chunks, 1);
    assert_eq!(stats.min_size, 1024);
    assert_eq!(stats.max_size, 1024);

    stats.finalize();

    assert_eq!(stats.mean_size, 1024.0);

    log(
        "单个chunk统计：min=max=mean=1024",
        "Single chunk stats: min=max=mean=1024",
    );
}

// ========== 多个chunk测试 ==========

#[test]
fn test_chunk_stats_multiple_chunks_same_size() {
    let mut stats = ChunkSizeStats::new();

    for _ in 0..100 {
        stats.add_chunk(512);
    }

    stats.finalize();

    assert_eq!(stats.total_chunks, 100);
    assert_eq!(stats.min_size, 512);
    assert_eq!(stats.max_size, 512);
    assert_eq!(stats.mean_size, 512.0);

    log(
        "100个相同大小chunk：统计正确",
        "100 identical chunks: stats correct",
    );
}

#[test]
fn test_chunk_stats_multiple_chunks_varying_sizes() {
    let mut stats = ChunkSizeStats::new();

    // 添加不同大小的chunk
    stats.add_chunk(256);
    stats.add_chunk(512);
    stats.add_chunk(1024);
    stats.add_chunk(2048);
    stats.add_chunk(4096);

    stats.finalize();

    assert_eq!(stats.total_chunks, 5);
    assert_eq!(stats.min_size, 256);
    assert_eq!(stats.max_size, 4096);

    // 平均值 = (256 + 512 + 1024 + 2048 + 4096) / 5 = 1587.2
    let expected_mean = (256.0 + 512.0 + 1024.0 + 2048.0 + 4096.0) / 5.0;
    assert!((stats.mean_size - expected_mean).abs() < 1e-6);

    log(
        format!(
            "5个不同大小chunk：min={}, max={}, mean={:.1}",
            stats.min_size, stats.max_size, stats.mean_size
        ),
        format!(
            "Five varied chunks: min={}, max={}, mean={:.1}",
            stats.min_size, stats.max_size, stats.mean_size
        ),
    );
}

// ========== 边界值测试 ==========

#[test]
fn test_chunk_stats_zero_size_chunk() {
    let mut stats = ChunkSizeStats::new();

    stats.add_chunk(0);
    stats.add_chunk(100);

    stats.finalize();

    assert_eq!(stats.total_chunks, 2);
    assert_eq!(stats.min_size, 0);
    assert_eq!(stats.max_size, 100);
    assert_eq!(stats.mean_size, 50.0);

    log("零大小chunk处理正确", "Zero-size chunk handled correctly");
}

#[test]
fn test_chunk_stats_large_chunk_sizes() {
    let mut stats = ChunkSizeStats::new();

    let large_size = 1_000_000;
    stats.add_chunk(large_size);
    stats.add_chunk(large_size * 2);

    stats.finalize();

    assert_eq!(stats.total_chunks, 2);
    assert_eq!(stats.min_size, large_size);
    assert_eq!(stats.max_size, large_size * 2);
    assert_eq!(stats.mean_size, large_size as f64 * 1.5);

    log(
        format!("大chunk size处理正确（{large_size} samples）"),
        format!("Large chunk sizes handled correctly ({large_size} samples)"),
    );
}

// ========== min/max更新测试 ==========

#[test]
fn test_chunk_stats_min_max_updates() {
    let mut stats = ChunkSizeStats::new();

    // 先添加中等大小
    stats.add_chunk(1000);
    assert_eq!(stats.min_size, 1000);
    assert_eq!(stats.max_size, 1000);

    // 添加更小的
    stats.add_chunk(500);
    assert_eq!(stats.min_size, 500);
    assert_eq!(stats.max_size, 1000);

    // 添加更大的
    stats.add_chunk(2000);
    assert_eq!(stats.min_size, 500);
    assert_eq!(stats.max_size, 2000);

    // 添加中间值不改变min/max
    stats.add_chunk(1500);
    assert_eq!(stats.min_size, 500);
    assert_eq!(stats.max_size, 2000);

    log("min/max动态更新正确", "min/max updates behave correctly");
}

// ========== 平均值计算测试 ==========

#[test]
fn test_chunk_stats_mean_calculation() {
    let mut stats = ChunkSizeStats::new();

    stats.add_chunk(100);
    stats.add_chunk(200);
    stats.add_chunk(300);

    // finalize前mean_size应该是0
    assert_eq!(stats.mean_size, 0.0);

    stats.finalize();

    // finalize后计算平均值
    assert_eq!(stats.mean_size, 200.0);

    log(
        format!("finalize触发平均值计算：{}", stats.mean_size),
        format!("Mean computed after finalize: {}", stats.mean_size),
    );
}

#[test]
fn test_chunk_stats_mean_precision() {
    let mut stats = ChunkSizeStats::new();

    // 测试浮点精度
    stats.add_chunk(333);
    stats.add_chunk(333);
    stats.add_chunk(334);

    stats.finalize();

    let expected = (333.0 + 333.0 + 334.0) / 3.0;
    assert!((stats.mean_size - expected).abs() < 1e-9);

    log(
        format!("平均值计算精度正确：{:.9}", stats.mean_size),
        format!("Mean precision verified: {:.9}", stats.mean_size),
    );
}

// ========== 实际场景模拟测试 ==========

#[test]
fn test_chunk_stats_fixed_size_format() {
    // 模拟MP3/AAC固定包大小格式
    let mut stats = ChunkSizeStats::new();

    for _ in 0..1000 {
        stats.add_chunk(1152); // MP3常见frame size
    }

    stats.finalize();

    assert_eq!(stats.total_chunks, 1000);
    assert_eq!(stats.min_size, 1152);
    assert_eq!(stats.max_size, 1152);
    assert_eq!(stats.mean_size, 1152.0);

    // 变化系数应该是1.0（固定大小）
    let variation_ratio = stats.max_size as f64 / stats.min_size as f64;
    assert_eq!(variation_ratio, 1.0);

    log(
        format!("固定包大小格式模拟（MP3）：变化系数={variation_ratio:.2}x"),
        format!("Fixed-size frames (MP3) variation ratio = {variation_ratio:.2}x"),
    );
}

#[test]
fn test_chunk_stats_variable_size_format() {
    // 模拟FLAC/OGG可变包大小格式
    let mut stats = ChunkSizeStats::new();

    // FLAC包大小通常在256-8192之间变化
    let sizes = vec![256, 512, 1024, 2048, 4096, 8192, 4096, 2048, 1024, 512];

    for &size in &sizes {
        stats.add_chunk(size);
    }

    stats.finalize();

    assert_eq!(stats.total_chunks, 10);
    assert_eq!(stats.min_size, 256);
    assert_eq!(stats.max_size, 8192);

    // 变化系数应该>2.0（可变大小）
    let variation_ratio = stats.max_size as f64 / stats.min_size as f64;
    assert!(variation_ratio > 2.0);

    log(
        format!("可变包大小格式模拟（FLAC）：变化系数={variation_ratio:.2}x"),
        format!("Variable-size frames (FLAC) variation ratio = {variation_ratio:.2}x"),
    );
}

#[test]
fn test_chunk_stats_real_world_distribution() {
    // 模拟真实解码场景的包大小分布
    let mut stats = ChunkSizeStats::new();

    // 大部分是1024，偶尔有512或2048
    for _ in 0..90 {
        stats.add_chunk(1024);
    }
    for _ in 0..5 {
        stats.add_chunk(512);
    }
    for _ in 0..5 {
        stats.add_chunk(2048);
    }

    stats.finalize();

    assert_eq!(stats.total_chunks, 100);
    assert_eq!(stats.min_size, 512);
    assert_eq!(stats.max_size, 2048);

    // 平均值应该接近1024（因为90%都是1024）
    let expected = (90.0 * 1024.0 + 5.0 * 512.0 + 5.0 * 2048.0) / 100.0;
    assert!((stats.mean_size - expected).abs() < 1e-6);

    log(
        format!(
            "真实分布模拟：mean={:.1} (期望{:.1})",
            stats.mean_size, expected
        ),
        format!(
            "Real-world distribution: mean={:.1} (expected {:.1})",
            stats.mean_size, expected
        ),
    );
}

// ========== Clone trait测试 ==========

#[test]
fn test_chunk_stats_clone() {
    let mut stats1 = ChunkSizeStats::new();

    stats1.add_chunk(100);
    stats1.add_chunk(200);
    stats1.finalize();

    let stats2 = stats1.clone();

    assert_eq!(stats1.total_chunks, stats2.total_chunks);
    assert_eq!(stats1.min_size, stats2.min_size);
    assert_eq!(stats1.max_size, stats2.max_size);
    assert_eq!(stats1.mean_size, stats2.mean_size);

    log(
        "ChunkSizeStats Clone trait工作正常",
        "ChunkSizeStats Clone trait behaves correctly",
    );
}

#[test]
fn test_chunk_stats_clone_independence() {
    let mut stats1 = ChunkSizeStats::new();
    stats1.add_chunk(100);

    let mut stats2 = stats1.clone();
    stats2.add_chunk(200);

    // stats1不应该受stats2影响
    assert_eq!(stats1.total_chunks, 1);
    assert_eq!(stats2.total_chunks, 2);

    log(
        "Clone后的统计对象互相独立",
        "Cloned stats remain independent",
    );
}

// ========== Debug trait测试 ==========

#[test]
fn test_chunk_stats_debug() {
    let mut stats = ChunkSizeStats::new();
    stats.add_chunk(1024);
    stats.finalize();

    let debug_str = format!("{stats:?}");

    assert!(debug_str.contains("1024"));
    assert!(debug_str.contains("total_chunks"));

    log(
        "ChunkSizeStats Debug trait工作正常",
        "ChunkSizeStats Debug trait works",
    );
}

// ========== 多次finalize测试 ==========

#[test]
fn test_chunk_stats_multiple_finalize() {
    let mut stats = ChunkSizeStats::new();

    stats.add_chunk(100);
    stats.add_chunk(200);

    stats.finalize();
    let mean1 = stats.mean_size;

    // 再次finalize不应该改变结果
    stats.finalize();
    let mean2 = stats.mean_size;

    assert_eq!(mean1, mean2);

    log(
        format!("多次finalize不改变结果：mean={mean1}"),
        format!("Repeated finalize calls leave mean unchanged: {mean1}"),
    );
}

// ========== 压力测试 ==========

#[test]
fn test_chunk_stats_large_number_of_chunks() {
    let mut stats = ChunkSizeStats::new();

    // 模拟处理大量chunk（如长音频文件）
    for i in 0..10000 {
        let size = 512 + (i % 1536); // 512到2048之间变化
        stats.add_chunk(size);
    }

    stats.finalize();

    assert_eq!(stats.total_chunks, 10000);
    assert!(stats.min_size >= 512);
    assert!(stats.max_size <= 2048);
    assert!(stats.mean_size > 0.0);

    log(
        format!(
            "大量chunk处理：total={}, min={}, max={}, mean={:.1}",
            stats.total_chunks, stats.min_size, stats.max_size, stats.mean_size
        ),
        format!(
            "Large-volume chunks: total={}, min={}, max={}, mean={:.1}",
            stats.total_chunks, stats.min_size, stats.max_size, stats.mean_size
        ),
    );
}

// ========== 综合场景测试 ==========

#[test]
fn test_chunk_stats_typical_workflow() {
    // 模拟典型的解码统计工作流
    let mut stats = ChunkSizeStats::new();

    // 1. 初始状态验证
    assert_eq!(stats.total_chunks, 0);

    // 2. 模拟解码过程
    for _ in 0..100 {
        stats.add_chunk(1024);
    }

    // 3. 验证未finalize前的状态
    assert_eq!(stats.total_chunks, 100);
    assert_eq!(stats.mean_size, 0.0); // 未finalize

    // 4. 完成统计
    stats.finalize();

    // 5. 验证最终结果
    assert_eq!(stats.mean_size, 1024.0);
    assert!(stats.min_size <= stats.max_size);

    log("典型工作流测试通过", "Typical workflow test passed");
}
