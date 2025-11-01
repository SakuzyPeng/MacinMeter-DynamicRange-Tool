//! SIMD性能基准测试
//!
//! 验证SIMD优化的性能表现，确保达到预期的效率。
//!
//! ## 性能目标
//! - SIMD效率 >= 75%（大数据集）
//! - 吞吐量 >= 40M样本/秒（Docker环境），700-800M/s（本地环境）
//! - 对齐vs非对齐overhead < 15%
//! - 小数据集性能可接受
//!
//! ## 运行方式
//! 由于包含大数据集测试，可能导致CI链接器资源耗尽，默认禁用。
//!
//! **本地运行性能测试**:
//! ```bash
//! cargo test --features simd-perf-tests --test simd_performance_tests
//! ```
//!
//! **CI环境**:
//! 默认跳过编译（无需 --features 参数）

// 通过 Cargo feature 控制编译（默认禁用，避免CI链接器崩溃）
#![cfg(feature = "simd-perf-tests")]

use macinmeter_dr_tool::{SampleConversion, SampleConverter};
use std::time::Instant;

fn log(msg_zh: impl AsRef<str>, msg_en: impl AsRef<str>) {
    println!("{} / {}", msg_zh.as_ref(), msg_en.as_ref());
}

/// 创建大规模i16测试数据
fn create_large_i16_data(count: usize) -> Vec<i16> {
    (0..count).map(|i| (i as i16).wrapping_mul(327)).collect()
}

/// 创建大规模i32测试数据
fn create_large_i32_data(count: usize) -> Vec<i32> {
    (0..count)
        .map(|i| (i as i32).wrapping_mul(12345).wrapping_add(67890))
        .collect()
}

/// 测量转换操作的耗时（纳秒）
fn benchmark_conversion<F>(iterations: usize, mut f: F) -> u64
where
    F: FnMut(),
{
    // 预热
    for _ in 0..10 {
        f();
    }

    // 正式测量
    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let elapsed = start.elapsed();

    elapsed.as_nanos() as u64 / iterations as u64
}

// ============================================================================
// 测试1: SIMD效率统计
// ============================================================================

#[test]
fn test_simd_efficiency_stats() {
    let converter = SampleConverter::new();

    log("\nSIMD效率统计:", "\nSIMD efficiency stats:");
    log(
        format!(
            "{:<10} {:<15} {:<15} {:<10}",
            "长度", "SIMD样本", "标量样本", "SIMD%"
        ),
        format!(
            "{:<10} {:<15} {:<15} {:<10}",
            "Length", "SIMD samples", "Scalar samples", "SIMD%"
        ),
    );
    log(format!("{:-<55}", ""), format!("{:-<55}", ""));

    // 测试不同长度的SIMD利用率
    for &len in &[5, 10, 15, 20, 32, 50, 100, 500, 1000, 10000] {
        let input = create_large_i16_data(len);
        let mut output = Vec::new();

        let stats = converter.convert_i16_to_f32(&input, &mut output).unwrap();

        log(
            format!(
                "{:<10} {:<15} {:<15} {:<10.1}%",
                len,
                stats.simd_samples,
                stats.scalar_samples,
                stats.simd_efficiency()
            ),
            format!(
                "{:<10} {:<15} {:<15} {:<10.1}%",
                len,
                stats.simd_samples,
                stats.scalar_samples,
                stats.simd_efficiency()
            ),
        );

        // 验证样本数一致
        assert_eq!(
            stats.simd_samples + stats.scalar_samples,
            len,
            "样本数统计错误"
        );

        // 大数据集应该有高SIMD效率
        if len >= 1000 {
            assert!(
                stats.simd_efficiency() >= 75.0,
                "大数据集SIMD效率不足，len={}, 效率={:.1}%",
                len,
                stats.simd_efficiency()
            );
        }
    }
}

// ============================================================================
// 测试2: 吞吐量测试
// ============================================================================

#[test]
#[ignore] // Debug模式下极慢（10M样本 × 10次迭代），仅在Release性能验证时运行
fn test_throughput() {
    let converter = SampleConverter::new();

    // 测试1秒内能处理多少样本
    let input = create_large_i16_data(10_000_000); // 10M样本

    let start = Instant::now();
    let iterations = 10;
    for _ in 0..iterations {
        let mut output = Vec::new();
        converter.convert_i16_to_f32(&input, &mut output).unwrap();
    }
    let elapsed = start.elapsed();

    let total_samples = input.len() * iterations;
    let samples_per_sec = total_samples as f64 / elapsed.as_secs_f64();
    let mb_per_sec = (total_samples * 2) as f64 / elapsed.as_secs_f64() / 1_000_000.0; // i16=2字节

    log("\n吞吐量测试:", "\nThroughput test:");
    log(
        format!("   总样本: {total_samples} (x{iterations})"),
        format!("   Total samples: {total_samples} (x{iterations})"),
    );
    log(
        format!("   耗时: {:.2} 秒", elapsed.as_secs_f64()),
        format!("   Elapsed: {:.2} s", elapsed.as_secs_f64()),
    );
    log(
        format!("   吞吐量: {:.2} M样本/秒", samples_per_sec / 1_000_000.0),
        format!(
            "   Throughput: {:.2} M samples/s",
            samples_per_sec / 1_000_000.0
        ),
    );
    log(
        format!("   带宽: {mb_per_sec:.2} MB/秒"),
        format!("   Bandwidth: {mb_per_sec:.2} MB/s"),
    );

    // 期望吞吐量 >= 40M样本/秒（保守估计，考虑Docker虚拟环境开销）
    // 本地环境可达700-800M/s，Docker环境约40-45M/s
    assert!(
        samples_per_sec >= 40_000_000.0,
        "吞吐量不足，期望>=40M样本/秒，实际={:.2}M/秒",
        samples_per_sec / 1_000_000.0
    );
}

// ============================================================================
// 测试3: 不同数据规模的性能表现
// ============================================================================

#[test]
#[ignore] // Debug模式下极慢（包含1M样本规模测试），仅在Release性能验证时运行
fn test_varying_data_sizes() {
    let converter = SampleConverter::new();

    log(
        "\n不同数据规模性能测试:",
        "\nPerformance across input sizes:",
    );
    log(
        format!(
            "{:<12} {:<15} {:<15} {:<10}",
            "样本数", "耗时(ms)", "吞吐(M/s)", "SIMD%"
        ),
        format!(
            "{:<12} {:<15} {:<15} {:<10}",
            "Samples", "Time (ms)", "Throughput (M/s)", "SIMD%"
        ),
    );
    log(format!("{:-<60}", ""), format!("{:-<60}", ""));

    let sizes = vec![
        100,       // 极小数据集
        1_000,     // 小数据集
        10_000,    // 中等数据集
        100_000,   // 大数据集
        1_000_000, // 超大数据集
    ];

    for size in sizes {
        let input = create_large_i16_data(size);

        let time_ns = benchmark_conversion(10, || {
            let mut output = Vec::new();
            converter.convert_i16_to_f32(&input, &mut output).unwrap();
        });

        let throughput = (size as f64) / (time_ns as f64 / 1_000_000_000.0) / 1_000_000.0;

        // 获取SIMD效率
        let mut output = Vec::new();
        let stats = converter.convert_i16_to_f32(&input, &mut output).unwrap();

        log(
            format!(
                "{:<12} {:<15.3} {:<15.2} {:<10.1}%",
                size,
                time_ns as f64 / 1_000_000.0,
                throughput,
                stats.simd_efficiency()
            ),
            format!(
                "{:<12} {:<15.3} {:<15.2} {:<10.1}%",
                size,
                time_ns as f64 / 1_000_000.0,
                throughput,
                stats.simd_efficiency()
            ),
        );
    }
}

// ============================================================================
// 测试4: 平台特性检测
// ============================================================================

#[test]
fn test_simd_capabilities() {
    let converter = SampleConverter::new();

    log("\nSIMD能力检测:", "\nSIMD capability detection:");
    log(
        format!("   SIMD支持: {}", converter.has_simd_support()),
        format!("   SIMD supported: {}", converter.has_simd_support()),
    );

    let caps = converter.simd_capabilities();
    log(
        format!("   SSE2: {}", caps.sse2),
        format!("   SSE2: {}", caps.sse2),
    );
    log(
        format!("   SSE3: {}", caps.sse3),
        format!("   SSE3: {}", caps.sse3),
    );
    log(
        format!("   SSSE3: {}", caps.ssse3),
        format!("   SSSE3: {}", caps.ssse3),
    );
    log(
        format!("   SSE4.1: {}", caps.sse4_1),
        format!("   SSE4.1: {}", caps.sse4_1),
    );
    log(
        format!("   AVX: {}", caps.avx),
        format!("   AVX: {}", caps.avx),
    );
    log(
        format!("   AVX2: {}", caps.avx2),
        format!("   AVX2: {}", caps.avx2),
    );
    log(
        format!("   FMA: {}", caps.fma),
        format!("   FMA: {}", caps.fma),
    );
    log(
        format!("   NEON: {}", caps.neon),
        format!("   NEON: {}", caps.neon),
    );
    log(
        format!("   NEON_FP16: {}", caps.neon_fp16),
        format!("   NEON_FP16: {}", caps.neon_fp16),
    );
    log(
        format!("   SVE: {}", caps.sve),
        format!("   SVE: {}", caps.sve),
    );

    // 至少应该有一种SIMD支持（x86_64的SSE2或ARM的NEON）
    #[cfg(target_arch = "x86_64")]
    assert!(caps.sse2, "x86_64平台应该支持SSE2");

    #[cfg(target_arch = "aarch64")]
    assert!(caps.neon, "ARM64平台应该支持NEON");
}

// ============================================================================
// 测试5: i32转换性能
// ============================================================================

#[test]
#[ignore] // Debug模式下极慢（500k样本 × 20次迭代），仅在Release性能验证时运行
fn test_i32_conversion_performance() {
    let converter = SampleConverter::new();

    let input = create_large_i32_data(500_000);

    let time_ns = benchmark_conversion(20, || {
        let mut output = Vec::new();
        converter.convert_i32_to_f32(&input, &mut output).unwrap();
    });

    let throughput = (input.len() as f64) / (time_ns as f64 / 1_000_000_000.0) / 1_000_000.0;

    // 获取SIMD效率
    let mut output = Vec::new();
    let stats = converter.convert_i32_to_f32(&input, &mut output).unwrap();

    log(
        "\ni32性能测试 [500k样本]:",
        "\ni32 throughput test [500k samples]:",
    );
    log(
        format!("   耗时: {:.2} ms", time_ns as f64 / 1_000_000.0),
        format!("   Time: {:.2} ms", time_ns as f64 / 1_000_000.0),
    );
    log(
        format!("   吞吐量: {throughput:.2} M样本/秒"),
        format!("   Throughput: {throughput:.2} M samples/s"),
    );
    log(
        format!("   SIMD效率: {:.1}%", stats.simd_efficiency()),
        format!("   SIMD efficiency: {:.1}%", stats.simd_efficiency()),
    );

    // 大数据集应该有高效率
    assert!(
        stats.simd_efficiency() >= 75.0,
        "i32 SIMD效率不足，实际={:.1}%",
        stats.simd_efficiency()
    );
}

// ============================================================================
// 测试6: 对齐vs非对齐性能对比
// ============================================================================

#[test]
#[ignore] // Debug模式下极慢（100k样本 × 50次迭代 × 2组测试），仅在Release性能验证时运行
fn test_aligned_vs_unaligned_performance() {
    let converter = SampleConverter::new();

    let size = 100_000;

    // 测试对齐数据（长度是SIMD向量大小的倍数）
    let aligned_input = create_large_i32_data(size); // size是4的倍数

    // 测试非对齐数据
    let unaligned_input = create_large_i32_data(size + 3); // +3导致非对齐

    let aligned_time = benchmark_conversion(50, || {
        let mut output = Vec::new();
        converter
            .convert_i32_to_f32(&aligned_input, &mut output)
            .unwrap();
    });

    let unaligned_time = benchmark_conversion(50, || {
        let mut output = Vec::new();
        converter
            .convert_i32_to_f32(&unaligned_input, &mut output)
            .unwrap();
    });

    let overhead = (unaligned_time as f64 / aligned_time as f64 - 1.0) * 100.0;

    log("\n对齐vs非对齐性能:", "\nAligned vs unaligned performance:");
    log(
        format!("   对齐耗时: {:.2} ms", aligned_time as f64 / 1_000_000.0),
        format!(
            "   Aligned time: {:.2} ms",
            aligned_time as f64 / 1_000_000.0
        ),
    );
    log(
        format!(
            "   非对齐耗时: {:.2} ms",
            unaligned_time as f64 / 1_000_000.0
        ),
        format!(
            "   Unaligned time: {:.2} ms",
            unaligned_time as f64 / 1_000_000.0
        ),
    );
    log(
        format!("   Overhead: {overhead:.1}%"),
        format!("   Overhead: {overhead:.1}%"),
    );

    // 非对齐overhead应该 < 15%
    assert!(
        overhead < 15.0,
        "非对齐overhead过大，期望<15%，实际={overhead:.1}%"
    );
}

// ============================================================================
// 测试7: 小数据集性能
// ============================================================================

#[test]
fn test_small_data_performance() {
    let converter = SampleConverter::new();

    log("\n小数据集性能测试:", "\nSmall dataset performance:");
    log(
        format!("{:<10} {:<15} {:<10}", "长度", "耗时(ns)", "SIMD%"),
        format!("{:<10} {:<15} {:<10}", "Length", "Time (ns)", "SIMD%"),
    );
    log(format!("{:-<40}", ""), format!("{:-<40}", ""));

    // 测试极小数据集
    let small_sizes = vec![1, 2, 3, 4, 5, 8, 10, 16, 32, 64];

    for size in small_sizes {
        let input = create_large_i16_data(size);

        let time_ns = benchmark_conversion(1000, || {
            let mut output = Vec::new();
            converter.convert_i16_to_f32(&input, &mut output).unwrap();
        });

        let mut output = Vec::new();
        let stats = converter.convert_i16_to_f32(&input, &mut output).unwrap();

        log(
            format!(
                "{:<10} {:<15} {:<10.1}%",
                size,
                time_ns,
                stats.simd_efficiency()
            ),
            format!(
                "{:<10} {:<15} {:<10.1}%",
                size,
                time_ns,
                stats.simd_efficiency()
            ),
        );
    }
}

// ============================================================================
// 测试8: 内存带宽测试（长时间测试，默认忽略）
// ============================================================================

#[test]
#[ignore]
fn test_memory_bandwidth() {
    let converter = SampleConverter::new();

    // 测试极大数据集（100MB）
    let input = create_large_i32_data(25_000_000); // 100MB

    let start = Instant::now();
    let mut output = Vec::new();
    converter.convert_i32_to_f32(&input, &mut output).unwrap();
    let elapsed = start.elapsed();

    let mb_processed = (input.len() * 4) as f64 / 1_000_000.0; // i32=4字节
    let bandwidth = mb_processed / elapsed.as_secs_f64();

    log("\n内存带宽测试:", "\nMemory bandwidth test:");
    log(
        format!("   数据量: {mb_processed:.2} MB"),
        format!("   Data size: {mb_processed:.2} MB"),
    );
    log(
        format!("   耗时: {:.2} 秒", elapsed.as_secs_f64()),
        format!("   Elapsed: {:.2} s", elapsed.as_secs_f64()),
    );
    log(
        format!("   带宽: {bandwidth:.2} MB/秒"),
        format!("   Bandwidth: {bandwidth:.2} MB/s"),
    );

    // 现代系统应该能达到 >= 300 MB/秒
    assert!(
        bandwidth >= 300.0,
        "内存带宽过低，期望>=300MB/秒，实际={bandwidth:.2}MB/秒"
    );
}

// ============================================================================
// 测试9: ConversionStats准确性验证
// ============================================================================

#[test]
fn test_conversion_stats_accuracy() {
    let converter = SampleConverter::new();

    // 测试100个样本
    let input = create_large_i16_data(100);
    let mut output = Vec::new();

    let stats = converter.convert_i16_to_f32(&input, &mut output).unwrap();

    log("\nConversionStats验证:", "\nConversionStats validation:");
    log(
        format!("   输入样本: {}", stats.input_samples),
        format!("   Input samples: {}", stats.input_samples),
    );
    log(
        format!("   输出样本: {}", stats.output_samples),
        format!("   Output samples: {}", stats.output_samples),
    );
    log(
        format!("   SIMD样本: {}", stats.simd_samples),
        format!("   SIMD samples: {}", stats.simd_samples),
    );
    log(
        format!("   标量样本: {}", stats.scalar_samples),
        format!("   Scalar samples: {}", stats.scalar_samples),
    );
    log(
        format!("   SIMD效率: {:.1}%", stats.simd_efficiency()),
        format!("   SIMD efficiency: {:.1}%", stats.simd_efficiency()),
    );
    log(
        format!("   使用SIMD: {}", stats.used_simd),
        format!("   Used SIMD: {}", stats.used_simd),
    );
    log(
        format!("   耗时: {} ns", stats.duration_ns),
        format!("   Duration: {} ns", stats.duration_ns),
    );

    // 基本一致性检查
    assert_eq!(stats.input_samples, 100);
    assert_eq!(stats.output_samples, 100);
    assert_eq!(stats.simd_samples + stats.scalar_samples, 100);
    assert_eq!(output.len(), 100);

    // SIMD标志应该正确
    if converter.has_simd_support() {
        assert!(stats.used_simd || stats.simd_samples > 0);
    }
}
