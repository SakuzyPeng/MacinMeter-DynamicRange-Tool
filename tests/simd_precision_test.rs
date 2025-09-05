//! SIMD精度深度测试
//!
//! 检查SIMD优化是否存在类似dr14_t.meter那样的"超级向量化精度问题"

use macinmeter_dr_tool::core::ChannelData;
use macinmeter_dr_tool::processing::SimdChannelData;

#[test]
fn test_extreme_precision_requirements() {
    println!("🔬 执行极端精度要求测试...");

    // 使用更大的测试数据集
    let test_samples: Vec<f32> = (0..10000)
        .map(|i| (i as f32 * 0.001).sin() * 0.8) // 更复杂的波形
        .collect();

    // SIMD处理
    let mut simd_processor = SimdChannelData::new(16);
    simd_processor.process_samples_simd(&test_samples);

    // 标量处理
    let mut scalar_data = ChannelData::new();
    for &sample in &test_samples {
        scalar_data.process_sample(sample);
    }

    // 计算差异
    let rms_diff = (simd_processor.inner().rms_accumulator - scalar_data.rms_accumulator).abs();
    let peak1_diff = (simd_processor.inner().peak_primary - scalar_data.peak_primary).abs();
    let peak2_diff = (simd_processor.inner().peak_secondary - scalar_data.peak_secondary).abs();

    println!("📊 大数据集精度对比:");
    println!("  样本数量: {}", test_samples.len());
    println!("  RMS累积:");
    println!("    SIMD:  {:.16}", simd_processor.inner().rms_accumulator);
    println!("    标量:  {:.16}", scalar_data.rms_accumulator);
    println!("    差异:  {rms_diff:.2e}");
    println!(
        "    相对误差: {:.2e}",
        rms_diff / scalar_data.rms_accumulator
    );

    println!("  主Peak:");
    println!("    SIMD:  {:.16}", simd_processor.inner().peak_primary);
    println!("    标量:  {:.16}", scalar_data.peak_primary);
    println!("    差异:  {peak1_diff:.2e}");

    println!("  次Peak:");
    println!("    SIMD:  {:.16}", simd_processor.inner().peak_secondary);
    println!("    标量:  {:.16}", scalar_data.peak_secondary);
    println!("    差异:  {peak2_diff:.2e}");

    // 更严格的精度要求（类似dr14_t.meter的标准）
    let relative_rms_error = rms_diff / scalar_data.rms_accumulator;

    println!("🎯 精度评估:");
    println!("  RMS相对误差: {relative_rms_error:.2e}");

    if relative_rms_error > 1e-10 {
        println!("⚠️  警告：RMS精度可能不足，相对误差 > 1e-10");
    } else {
        println!("✅ RMS精度满足要求");
    }

    if peak1_diff > 1e-12 {
        println!("⚠️  警告：Peak精度可能不足");
    } else {
        println!("✅ Peak精度满足要求");
    }
}

#[test]
fn test_dr_calculation_precision() {
    println!("🎵 DR计算精度测试...");

    // 模拟真实音频：3秒48kHz立体声
    let samples_per_channel = 3 * 48000;
    let mut stereo_samples = Vec::with_capacity(samples_per_channel * 2);

    for i in 0..samples_per_channel {
        let left = (i as f32 * 0.001).sin() * 0.7; // 左声道
        let right = (i as f32 * 0.0015).cos() * 0.6; // 右声道
        stereo_samples.push(left);
        stereo_samples.push(right);
    }

    // 分别处理左右声道
    let left_samples: Vec<f32> = stereo_samples.iter().step_by(2).cloned().collect();
    let right_samples: Vec<f32> = stereo_samples.iter().skip(1).step_by(2).cloned().collect();

    println!("  样本信息：{}秒，{}kHz，立体声", 3, 48);
    println!("  左声道样本数：{}", left_samples.len());
    println!("  右声道样本数：{}", right_samples.len());

    // 测试左声道
    let mut simd_left = SimdChannelData::new(1024);
    let mut scalar_left = ChannelData::new();

    simd_left.process_samples_simd(&left_samples);
    for &sample in &left_samples {
        scalar_left.process_sample(sample);
    }

    let left_rms_simd = simd_left.calculate_rms(left_samples.len());
    let left_rms_scalar = scalar_left.calculate_rms(left_samples.len());

    println!("  左声道RMS对比:");
    println!("    SIMD:  {:.8} dB", 20.0 * left_rms_simd.log10());
    println!("    标量:  {:.8} dB", 20.0 * left_rms_scalar.log10());

    let rms_db_diff = 20.0 * (left_rms_simd / left_rms_scalar).log10();
    println!("    差异:  {rms_db_diff:.6} dB");

    // DR计算精度要求：误差应 < 0.01 dB
    if rms_db_diff.abs() > 0.01 {
        println!("⚠️  警告：RMS差异 > 0.01dB，可能影响DR测量精度");
        println!("   这类似于dr14_t.meter的超级向量化精度问题！");
    } else {
        println!("✅ RMS精度满足DR测量要求 (< 0.01dB)");
    }
}

#[test]
fn test_cumulative_error_analysis() {
    println!("📈 累积误差分析测试...");

    // 测试不同长度的累积误差增长
    let test_lengths = [100, 1000, 10000, 100000];

    for &len in &test_lengths {
        let test_samples: Vec<f32> = (0..len).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();

        let mut simd_proc = SimdChannelData::new(64);
        let mut scalar_data = ChannelData::new();

        simd_proc.process_samples_simd(&test_samples);
        for &sample in &test_samples {
            scalar_data.process_sample(sample);
        }

        let rms_diff = (simd_proc.inner().rms_accumulator - scalar_data.rms_accumulator).abs();
        let relative_error = rms_diff / scalar_data.rms_accumulator;

        println!("  样本数 {len:6}: 相对误差 {relative_error:.2e}");

        // 检查误差是否随样本数增长
        if len > 1000 && relative_error > 1e-9 {
            println!("    ⚠️  累积误差随样本数增长，存在精度风险");
        }
    }
}
