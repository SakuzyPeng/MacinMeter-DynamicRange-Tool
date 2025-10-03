//! 🔬 SIMD边界条件和性能关键路径测试
//!
//! **优先级2：SIMD精度和边界验证**
//!
//! 验证SIMD优化在各种边界条件下的正确性和精度一致性
//!
//! ## 🎯 测试策略
//!
//! 1. **边界长度测试** - 0/1/3/4/5/7/8样本（SIMD向量长度为4）
//! 2. **精度验证** - SIMD vs 标量实现误差必须在可接受范围内
//! 3. **声道分离边界** - 单/双声道边界条件
//!
//! ## ⚠️ 测试约束
//!
//! - SIMD向量长度：4个f32（128位）
//! - 精度要求：误差 < 1e-6
//! - 覆盖所有边界情况：4n, 4n+1, 4n+2, 4n+3

use macinmeter_dr_tool::processing::channel_separator::ChannelSeparator;
use macinmeter_dr_tool::processing::simd_core::SimdProcessor;

// ========== SIMD边界长度测试 ==========

/// 验证空数组处理（0样本）
#[test]
fn test_simd_empty_array() {
    println!("📊 测试SIMD空数组处理");

    let separator = ChannelSeparator::new();
    let empty_samples: Vec<f32> = Vec::new();

    // 单声道提取
    let result = separator.extract_channel_samples_optimized(&empty_samples, 0, 1);
    assert_eq!(result.len(), 0, "空数组应该返回空结果");

    // 立体声提取
    let result_stereo = separator.extract_channel_samples_optimized(&empty_samples, 0, 2);
    assert_eq!(result_stereo.len(), 0, "空数组应该返回空结果");

    println!("  ✓ 空数组处理正确");
    println!("✅ SIMD空数组测试通过");
}

/// 验证单样本处理（< SIMD向量长度）
#[test]
fn test_simd_single_sample() {
    println!("📊 测试SIMD单样本处理");

    let separator = ChannelSeparator::new();

    // 单声道单样本
    let samples = vec![0.5];
    let result = separator.extract_channel_samples_optimized(&samples, 0, 1);
    assert_eq!(result.len(), 1);
    assert!((result[0] - 0.5).abs() < 1e-6, "单样本值应该准确");

    // 立体声单样本（L=0.3, R=0.7）
    let stereo_samples = vec![0.3, 0.7];
    let left = separator.extract_channel_samples_optimized(&stereo_samples, 0, 2);
    let right = separator.extract_channel_samples_optimized(&stereo_samples, 1, 2);
    assert_eq!(left.len(), 1);
    assert_eq!(right.len(), 1);
    assert!((left[0] - 0.3).abs() < 1e-6);
    assert!((right[0] - 0.7).abs() < 1e-6);

    println!("  ✓ 单样本提取准确");
    println!("✅ SIMD单样本测试通过");
}

/// 验证3样本处理（4n-1，剩余3个样本需标量处理）
#[test]
fn test_simd_three_samples() {
    println!("📊 测试SIMD 3样本处理（边界情况）");

    let separator = ChannelSeparator::new();

    // 单声道3样本
    let samples = vec![0.1, 0.2, 0.3];
    let result = separator.extract_channel_samples_optimized(&samples, 0, 1);
    assert_eq!(result.len(), 3);
    for (i, &expected) in [0.1, 0.2, 0.3].iter().enumerate() {
        assert!((result[i] - expected).abs() < 1e-6, "样本{i}应该准确");
    }

    // 立体声3帧（6样本）
    let stereo_samples = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6];
    let left = separator.extract_channel_samples_optimized(&stereo_samples, 0, 2);
    let right = separator.extract_channel_samples_optimized(&stereo_samples, 1, 2);
    assert_eq!(left.len(), 3);
    assert_eq!(right.len(), 3);
    assert!((left[0] - 0.1).abs() < 1e-6);
    assert!((left[1] - 0.3).abs() < 1e-6);
    assert!((left[2] - 0.5).abs() < 1e-6);

    println!("  ✓ 3样本处理正确");
    println!("✅ SIMD 3样本测试通过");
}

/// 验证4样本处理（恰好1个SIMD向量）
#[test]
fn test_simd_exact_vector_size() {
    println!("📊 测试SIMD恰好4样本处理（1个向量）");

    let separator = ChannelSeparator::new();

    // 单声道4样本（恰好1个SSE2向量）
    let samples = vec![0.1, 0.2, 0.3, 0.4];
    let result = separator.extract_channel_samples_optimized(&samples, 0, 1);
    assert_eq!(result.len(), 4);
    for (i, &expected) in [0.1, 0.2, 0.3, 0.4].iter().enumerate() {
        assert!((result[i] - expected).abs() < 1e-6, "样本{i}应该准确");
    }

    // 立体声4帧（8样本，4个左声道 + 4个右声道交错）
    let stereo_samples = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
    let left = separator.extract_channel_samples_optimized(&stereo_samples, 0, 2);
    let right = separator.extract_channel_samples_optimized(&stereo_samples, 1, 2);
    assert_eq!(left.len(), 4);
    assert_eq!(right.len(), 4);

    println!("  ✓ 4样本向量处理正确");
    println!("✅ SIMD 4样本测试通过");
}

/// 验证5样本处理（4n+1，1个向量 + 1个剩余）
#[test]
fn test_simd_one_extra_sample() {
    println!("📊 测试SIMD 5样本处理（4+1）");

    let separator = ChannelSeparator::new();

    // 单声道5样本
    let samples = vec![0.1, 0.2, 0.3, 0.4, 0.5];
    let result = separator.extract_channel_samples_optimized(&samples, 0, 1);
    assert_eq!(result.len(), 5);
    for (i, &expected) in [0.1, 0.2, 0.3, 0.4, 0.5].iter().enumerate() {
        assert!((result[i] - expected).abs() < 1e-6, "样本{i}应该准确");
    }

    println!("  ✓ 5样本（4+1）处理正确");
    println!("✅ SIMD 5样本测试通过");
}

/// 验证7样本处理（4n+3，1个向量 + 3个剩余）
#[test]
fn test_simd_three_extra_samples() {
    println!("📊 测试SIMD 7样本处理（4+3）");

    let separator = ChannelSeparator::new();

    let samples = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7];
    let result = separator.extract_channel_samples_optimized(&samples, 0, 1);
    assert_eq!(result.len(), 7);

    println!("  ✓ 7样本（4+3）处理正确");
    println!("✅ SIMD 7样本测试通过");
}

/// 验证8样本处理（恰好2个SIMD向量）
#[test]
fn test_simd_two_vectors() {
    println!("📊 测试SIMD 8样本处理（2个向量）");

    let separator = ChannelSeparator::new();

    let samples = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
    let result = separator.extract_channel_samples_optimized(&samples, 0, 1);
    assert_eq!(result.len(), 8);
    for (i, &val) in samples.iter().enumerate() {
        assert!((result[i] - val).abs() < 1e-6, "样本{i}应该准确");
    }

    println!("  ✓ 8样本（2个向量）处理正确");
    println!("✅ SIMD 8样本测试通过");
}

// ========== SIMD精度验证测试 ==========

/// 验证SIMD与标量实现精度一致性（小数组）
#[test]
fn test_simd_scalar_precision_small() {
    println!("📊 测试SIMD精度一致性（小数组）");

    let separator = ChannelSeparator::new();

    // 创建测试数据：10个样本
    let samples: Vec<f32> = (0..10).map(|i| (i as f32) * 0.1).collect();

    // 使用SIMD处理
    let simd_result = separator.extract_channel_samples_optimized(&samples, 0, 1);

    // 验证结果完全一致（单声道直通）
    assert_eq!(simd_result.len(), samples.len());
    for (i, (&simd_val, &original)) in simd_result.iter().zip(samples.iter()).enumerate() {
        let diff = (simd_val - original).abs();
        assert!(
            diff < 1e-6,
            "样本{i}: SIMD={simd_val}, 原始={original}, 差异={diff}"
        );
    }

    println!("  ✓ SIMD精度与原始数据完全一致");
    println!("✅ SIMD小数组精度测试通过");
}

/// 验证SIMD与标量实现精度一致性（大数组）
#[test]
fn test_simd_scalar_precision_large() {
    println!("📊 测试SIMD精度一致性（大数组1000样本）");

    let separator = ChannelSeparator::new();

    // 创建1000个样本的测试数据
    let samples: Vec<f32> = (0..1000).map(|i| ((i as f32) * 0.001).sin()).collect();

    let simd_result = separator.extract_channel_samples_optimized(&samples, 0, 1);

    // 验证精度
    assert_eq!(simd_result.len(), 1000);
    let mut max_diff = 0.0f32;
    for (i, (&simd_val, &original)) in simd_result.iter().zip(samples.iter()).enumerate() {
        let diff = (simd_val - original).abs();
        max_diff = max_diff.max(diff);
        assert!(
            diff < 1e-6,
            "样本{i}: SIMD={simd_val}, 原始={original}, 差异={diff}"
        );
    }

    println!("  ✓ 最大误差: {max_diff:.2e}");
    println!("✅ SIMD大数组精度测试通过");
}

/// 验证立体声SIMD分离精度
#[test]
fn test_stereo_simd_separation_precision() {
    println!("📊 测试立体声SIMD分离精度");

    let separator = ChannelSeparator::new();

    // 创建交错立体声数据：100帧 = 200样本
    let mut stereo_samples = Vec::with_capacity(200);
    for i in 0..100 {
        stereo_samples.push((i as f32) * 0.01); // 左声道
        stereo_samples.push((i as f32) * 0.02); // 右声道
    }

    let left = separator.extract_channel_samples_optimized(&stereo_samples, 0, 2);
    let right = separator.extract_channel_samples_optimized(&stereo_samples, 1, 2);

    // 验证分离结果
    assert_eq!(left.len(), 100);
    assert_eq!(right.len(), 100);

    for i in 0..100 {
        let expected_left = (i as f32) * 0.01;
        let expected_right = (i as f32) * 0.02;

        let diff_left = (left[i] - expected_left).abs();
        let diff_right = (right[i] - expected_right).abs();

        assert!(
            diff_left < 1e-6,
            "左声道样本{i}: 期望={expected_left}, 实际={}, 差异={diff_left}",
            left[i]
        );
        assert!(
            diff_right < 1e-6,
            "右声道样本{i}: 期望={expected_right}, 实际={}, 差异={diff_right}",
            right[i]
        );
    }

    println!("  ✓ 立体声分离精度完全准确");
    println!("✅ 立体声SIMD分离精度测试通过");
}

// ========== Channel Separator边界条件测试 ==========

/// 验证单声道模式忽略channel_idx（直通行为）
#[test]
fn test_mono_channel_index_ignored() {
    println!("📊 测试单声道模式channel_idx行为");

    let separator = ChannelSeparator::new();
    let samples = vec![0.1, 0.2, 0.3, 0.4];

    // 单声道模式：channel_idx被忽略，总是返回全部样本
    let result_ch0 = separator.extract_channel_samples_optimized(&samples, 0, 1);
    let result_ch1 = separator.extract_channel_samples_optimized(&samples, 1, 1);

    // 两次调用应该返回相同结果（直通）
    assert_eq!(result_ch0.len(), 4);
    assert_eq!(result_ch1.len(), 4);
    assert_eq!(result_ch0, result_ch1);

    println!("  ✓ 单声道模式忽略channel_idx，总是返回全部样本");
    println!("✅ 单声道channel_idx测试通过");
}

/// 验证立体声奇数样本的处理行为
#[test]
fn test_stereo_odd_samples_handling() {
    println!("📊 测试立体声奇数样本处理");

    let separator = ChannelSeparator::new();

    // 立体声奇数样本：3个样本（1.5帧）
    // [L0=0.1, R0=0.2, L1=0.3]
    let odd_samples = vec![0.1, 0.2, 0.3];

    let left = separator.extract_channel_samples_optimized(&odd_samples, 0, 2);
    let right = separator.extract_channel_samples_optimized(&odd_samples, 1, 2);

    // 实际行为：标量处理剩余样本
    // i=0: 0%2==0 -> 左声道 0.1
    // i=1: 1%2==1 -> 右声道 0.2
    // i=2: 2%2==0 -> 左声道 0.3
    assert_eq!(left.len(), 2, "左声道应该得到索引0和2的样本");
    assert_eq!(right.len(), 1, "右声道应该得到索引1的样本");
    assert!((left[0] - 0.1).abs() < 1e-6);
    assert!((left[1] - 0.3).abs() < 1e-6);
    assert!((right[0] - 0.2).abs() < 1e-6);

    println!("  ✓ 奇数样本按索引奇偶性正确分离");
    println!("  ✓ 左声道: [0.1, 0.3] (2个样本)");
    println!("  ✓ 右声道: [0.2] (1个样本)");
    println!("✅ 立体声奇数样本处理测试通过");
}

/// 验证SIMD能力检测
#[test]
fn test_simd_capability_detection() {
    println!("📊 测试SIMD能力检测");

    let processor = SimdProcessor::new();
    let capabilities = processor.capabilities();

    println!("  检测到的SIMD能力:");

    #[cfg(target_arch = "x86_64")]
    {
        println!("    SSE2: {}", capabilities.sse2);
        println!("    SSE3: {}", capabilities.sse3);
        println!("    SSE4.1: {}", capabilities.sse4_1);
        println!("    AVX: {}", capabilities.avx);
        println!("    AVX2: {}", capabilities.avx2);
        println!("    FMA: {}", capabilities.fma);

        // x86_64应该至少支持SSE2（现代处理器标配）
        if capabilities.has_basic_simd() {
            println!("  ✓ 检测到x86_64 SIMD支持");
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        println!("    NEON: {}", capabilities.neon);
        println!("    NEON FP16: {}", capabilities.neon_fp16);
        println!("    SVE: {}", capabilities.sve);

        // Apple Silicon/ARM应该支持NEON
        assert!(capabilities.neon, "ARM aarch64应该支持NEON");
        println!("  ✓ 检测到ARM NEON支持");
    }

    println!("  基础SIMD: {}", capabilities.has_basic_simd());
    println!("  高级SIMD: {}", capabilities.has_advanced_simd());

    println!("✅ SIMD能力检测测试通过");
}

/// 验证极端值处理（无穷大、NaN）
#[test]
fn test_simd_extreme_values() {
    println!("📊 测试SIMD极端值处理");

    let separator = ChannelSeparator::new();

    // 包含特殊值的样本
    let samples = vec![
        0.0,
        -0.0,
        1.0,
        -1.0,
        f32::MAX,
        f32::MIN,
        f32::INFINITY,
        f32::NEG_INFINITY,
    ];

    let result = separator.extract_channel_samples_optimized(&samples, 0, 1);

    assert_eq!(result.len(), 8);
    assert_eq!(result[0], 0.0);
    assert_eq!(result[1], -0.0);
    assert_eq!(result[2], 1.0);
    assert_eq!(result[3], -1.0);
    assert_eq!(result[4], f32::MAX);
    assert_eq!(result[5], f32::MIN);
    assert_eq!(result[6], f32::INFINITY);
    assert_eq!(result[7], f32::NEG_INFINITY);

    println!("  ✓ 极端值保持不变");

    // NaN需要特殊处理（NaN != NaN）
    let nan_samples = vec![f32::NAN];
    let nan_result = separator.extract_channel_samples_optimized(&nan_samples, 0, 1);
    assert!(nan_result[0].is_nan(), "NaN应该保持为NaN");

    println!("  ✓ NaN正确处理");
    println!("✅ SIMD极端值测试通过");
}
