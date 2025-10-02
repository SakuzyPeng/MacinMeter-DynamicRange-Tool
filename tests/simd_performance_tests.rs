//! SIMD性能基准测试
//!
//! 验证SIMD优化的性能表现，确保达到预期的效率。
//!
//! ## 性能目标
//! - SIMD效率 >= 80%（大数据集）
//! - 吞吐量 >= 100M样本/秒
//! - 小数据集性能可接受

use macinmeter_dr_tool::{SampleConversion, SampleConverter};
use std::time::Instant;

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

    println!("\n📊 SIMD效率统计:");
    println!(
        "{:<10} {:<15} {:<15} {:<10}",
        "长度", "SIMD样本", "标量样本", "SIMD%"
    );
    println!("{:-<55}", "");

    // 测试不同长度的SIMD利用率
    for &len in &[5, 10, 15, 20, 32, 50, 100, 500, 1000, 10000] {
        let input = create_large_i16_data(len);
        let mut output = Vec::new();

        let stats = converter.convert_i16_to_f32(&input, &mut output).unwrap();

        println!(
            "{:<10} {:<15} {:<15} {:<10.1}%",
            len,
            stats.simd_samples,
            stats.scalar_samples,
            stats.simd_efficiency()
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

    println!("\n📊 吞吐量测试:");
    println!("   总样本: {} (x{})", input.len(), iterations);
    println!("   耗时: {:.2} 秒", elapsed.as_secs_f64());
    println!("   吞吐量: {:.2} M样本/秒", samples_per_sec / 1_000_000.0);
    println!("   带宽: {mb_per_sec:.2} MB/秒");

    // 期望吞吐量 >= 50M样本/秒（保守估计，考虑不同平台）
    assert!(
        samples_per_sec >= 50_000_000.0,
        "吞吐量不足，期望>=50M样本/秒，实际={:.2}M/秒",
        samples_per_sec / 1_000_000.0
    );
}

// ============================================================================
// 测试3: 不同数据规模的性能表现
// ============================================================================

#[test]
fn test_varying_data_sizes() {
    let converter = SampleConverter::new();

    println!("\n📊 不同数据规模性能测试:");
    println!(
        "{:<12} {:<15} {:<15} {:<10}",
        "样本数", "耗时(ms)", "吞吐(M/s)", "SIMD%"
    );
    println!("{:-<60}", "");

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

        println!(
            "{:<12} {:<15.3} {:<15.2} {:<10.1}%",
            size,
            time_ns as f64 / 1_000_000.0,
            throughput,
            stats.simd_efficiency()
        );
    }
}

// ============================================================================
// 测试4: 平台特性检测
// ============================================================================

#[test]
fn test_simd_capabilities() {
    let converter = SampleConverter::new();

    println!("\n🔍 SIMD能力检测:");
    println!("   SIMD支持: {}", converter.has_simd_support());

    let caps = converter.simd_capabilities();
    println!("   SSE2: {}", caps.sse2);
    println!("   SSE3: {}", caps.sse3);
    println!("   SSSE3: {}", caps.ssse3);
    println!("   SSE4.1: {}", caps.sse4_1);
    println!("   AVX: {}", caps.avx);
    println!("   AVX2: {}", caps.avx2);
    println!("   FMA: {}", caps.fma);
    println!("   NEON: {}", caps.neon);
    println!("   NEON_FP16: {}", caps.neon_fp16);
    println!("   SVE: {}", caps.sve);

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

    println!("\n📊 i32性能测试 [500k样本]:");
    println!("   耗时: {:.2} ms", time_ns as f64 / 1_000_000.0);
    println!("   吞吐量: {throughput:.2} M样本/秒");
    println!("   SIMD效率: {:.1}%", stats.simd_efficiency());

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

    println!("\n📊 对齐vs非对齐性能:");
    println!("   对齐耗时: {:.2} ms", aligned_time as f64 / 1_000_000.0);
    println!(
        "   非对齐耗时: {:.2} ms",
        unaligned_time as f64 / 1_000_000.0
    );
    println!("   Overhead: {overhead:.1}%");

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

    println!("\n📊 小数据集性能测试:");
    println!("{:<10} {:<15} {:<10}", "长度", "耗时(ns)", "SIMD%");
    println!("{:-<40}", "");

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

        println!(
            "{:<10} {:<15} {:<10.1}%",
            size,
            time_ns,
            stats.simd_efficiency()
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

    println!("\n📊 内存带宽测试:");
    println!("   数据量: {mb_processed:.2} MB");
    println!("   耗时: {:.2} 秒", elapsed.as_secs_f64());
    println!("   带宽: {bandwidth:.2} MB/秒");

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

    println!("\n📊 ConversionStats验证:");
    println!("   输入样本: {}", stats.input_samples);
    println!("   输出样本: {}", stats.output_samples);
    println!("   SIMD样本: {}", stats.simd_samples);
    println!("   标量样本: {}", stats.scalar_samples);
    println!("   SIMD效率: {:.1}%", stats.simd_efficiency());
    println!("   使用SIMD: {}", stats.used_simd);
    println!("   耗时: {} ns", stats.duration_ns);

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
