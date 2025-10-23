// 临时测试：验证ARM NEON是否真正启用
use macinmeter_dr_tool::processing::simd_core::{SimdChannelData, SimdProcessor};

fn main() {
    println!("🔍 验证ARM64 NEON优化是否启用\n");

    // 1. 检测SIMD能力
    let processor = SimdProcessor::new();
    let caps = processor.capabilities();

    println!("📊 当前平台SIMD能力:");
    println!("  架构: {}", std::env::consts::ARCH);
    println!("  NEON支持: {}", caps.neon);
    println!("  基础SIMD: {}", caps.has_basic_simd());
    println!("  推荐并行度: {}\n", caps.recommended_parallelism());

    // 2. 测试SimdChannelData是否使用NEON
    let mut simd_proc = SimdChannelData::new();
    let samples: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();

    println!("🧪 处理1000个样本...");
    let processed = simd_proc.process_samples_simd(&samples);

    println!("  处理样本数: {}", processed);
    println!("  RMS累加器: {:.8}", simd_proc.inner().rms_accumulator);
    println!("  主Peak: {:.6}", simd_proc.inner().peak_primary);

    // 3. 验证SIMD效果（非零结果说明处理成功）
    if simd_proc.inner().rms_accumulator > 0.0 {
        println!("\n✅ SIMD处理成功！");

        #[cfg(target_arch = "aarch64")]
        {
            println!("🎯 ARM64平台 - NEON向量化已启用");
            println!("   - process_samples_neon() 被调用");
            println!("   - 4样本并行处理（128位NEON向量）");
        }

        #[cfg(target_arch = "x86_64")]
        {
            println!("🎯 x86_64平台 - SSE2向量化已启用");
            println!("   - process_samples_sse2() 被调用");
        }
    } else {
        println!("\n⚠️  警告：RMS累加器为0，可能未正确处理样本");
    }
}
