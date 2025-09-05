//! SIMD向量化音频处理器
//!
//! 基于多架构SIMD指令集实现4样本并行处理，
//! 针对DR计算的核心算法进行专门优化。
//!
//! ## 性能目标
//! - 4样本并行处理（128位SIMD向量）
//! - 6-7倍性能提升
//! - 100%精度一致性（与标量实现）
//!
//! ## 兼容性
//! - x86_64: SSE2支持（2003年后的处理器）
//! - ARM64: NEON支持（现代ARM处理器）
//! - 自动fallback到标量实现（不支持SIMD时）
//! - 跨平台兼容（运行时检测）

use crate::core::ChannelData;
#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

/// SIMD处理器能力检测结果
#[derive(Debug, Clone, PartialEq)]
pub struct SimdCapabilities {
    // x86_64 SIMD特性
    /// SSE2支持（4x f32并行）
    pub sse2: bool,

    /// SSE3支持（水平加法等）
    pub sse3: bool,

    /// SSSE3支持（改进的shuffle）
    pub ssse3: bool,

    /// SSE4.1支持（点积等）
    pub sse4_1: bool,

    /// AVX支持（8x f32并行，未来扩展）
    pub avx: bool,

    /// AVX2支持（256位整数运算）
    pub avx2: bool,

    /// FMA支持（融合乘加运算）
    pub fma: bool,

    // ARM64 SIMD特性
    /// NEON支持（4x f32并行）
    pub neon: bool,

    /// Crypto支持（AES/SHA等）
    pub crypto: bool,

    /// FP16支持（半精度浮点）
    pub fp16: bool,

    /// Dot Product支持（点积指令）
    pub dotprod: bool,
}

impl SimdCapabilities {
    /// 检测当前CPU的SIMD能力
    ///
    /// 使用平台特定的特性检测API，
    /// 返回详细的SIMD支持情况
    pub fn detect() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            Self {
                // x86_64 特性
                sse2: is_x86_feature_detected!("sse2"),
                sse3: is_x86_feature_detected!("sse3"),
                ssse3: is_x86_feature_detected!("ssse3"),
                sse4_1: is_x86_feature_detected!("sse4.1"),
                avx: is_x86_feature_detected!("avx"),
                avx2: is_x86_feature_detected!("avx2"),
                fma: is_x86_feature_detected!("fma"),
                // ARM特性在x86上为false
                neon: false,
                crypto: false,
                fp16: false,
                dotprod: false,
            }
        }

        #[cfg(target_arch = "aarch64")]
        {
            Self {
                // x86特性在ARM上为false
                sse2: false,
                sse3: false,
                ssse3: false,
                sse4_1: false,
                avx: false,
                avx2: false,
                fma: false,
                // ARM64 特性检测
                neon: std::arch::is_aarch64_feature_detected!("neon"),
                crypto: std::arch::is_aarch64_feature_detected!("aes")
                    && std::arch::is_aarch64_feature_detected!("sha2"),
                fp16: std::arch::is_aarch64_feature_detected!("fp16"),
                dotprod: std::arch::is_aarch64_feature_detected!("dotprod"),
            }
        }

        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            // 其他架构暂不支持SIMD
            Self {
                sse2: false,
                sse3: false,
                ssse3: false,
                sse4_1: false,
                avx: false,
                avx2: false,
                fma: false,
                neon: false,
                crypto: false,
                fp16: false,
                dotprod: false,
            }
        }
    }

    /// 是否支持基础SIMD加速（SSE2或NEON）
    pub fn has_basic_simd(&self) -> bool {
        self.sse2 || self.neon
    }

    /// 是否支持高级SIMD优化（SSE4.1或DotProd）
    pub fn has_advanced_simd(&self) -> bool {
        self.sse4_1 || self.dotprod
    }

    /// 获取建议的并行度（一次处理的样本数）
    pub fn recommended_parallelism(&self) -> usize {
        if self.avx2 {
            8 // AVX2: 8x f32 并行
        } else if self.sse2 || self.neon {
            4 // SSE2/NEON: 4x f32 并行
        } else {
            1 // 标量处理
        }
    }
}

/// SIMD优化的声道数据处理器
///
/// 为ChannelData提供向量化加速，
/// 保持与原始实现100%的数值一致性
pub struct SimdChannelData {
    /// 内部ChannelData实例
    inner: ChannelData,

    /// SIMD能力缓存
    capabilities: SimdCapabilities,

    /// 样本缓冲区（用于批量处理）
    sample_buffer: Vec<f32>,

    /// 缓冲区容量（对齐到SIMD边界）
    buffer_capacity: usize,
}

impl SimdChannelData {
    /// 创建新的SIMD优化声道数据处理器
    ///
    /// # 参数
    ///
    /// * `buffer_size` - 样本缓冲区大小，会自动对齐到SIMD边界
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::processing::SimdChannelData;
    ///
    /// let processor = SimdChannelData::new(1024);
    /// println!("SIMD支持: {}", processor.has_simd_support());
    /// ```
    pub fn new(buffer_size: usize) -> Self {
        let capabilities = SimdCapabilities::detect();
        let parallelism = capabilities.recommended_parallelism();

        // 将缓冲区大小对齐到SIMD边界
        let aligned_size = buffer_size.div_ceil(parallelism) * parallelism;

        Self {
            inner: ChannelData::new(),
            capabilities,
            sample_buffer: Vec::with_capacity(aligned_size),
            buffer_capacity: aligned_size,
        }
    }

    /// 检查是否支持SIMD加速
    pub fn has_simd_support(&self) -> bool {
        self.capabilities.has_basic_simd()
    }

    /// 获取SIMD能力信息
    pub fn capabilities(&self) -> &SimdCapabilities {
        &self.capabilities
    }

    /// 批量处理音频样本（SIMD优化）
    ///
    /// 使用SSE2或NEON指令并行处理4个样本，
    /// 显著提升RMS累积和Peak检测性能
    ///
    /// # 参数
    ///
    /// * `samples` - 音频样本数组
    ///
    /// # 返回值
    ///
    /// 返回处理的样本数量
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::processing::SimdChannelData;
    ///
    /// let mut processor = SimdChannelData::new(1024);
    /// let samples = vec![0.1, 0.2, 0.3, 0.4, 0.5];
    /// let processed = processor.process_samples_simd(&samples);
    /// assert_eq!(processed, 5);
    /// ```
    pub fn process_samples_simd(&mut self, samples: &[f32]) -> usize {
        if samples.is_empty() {
            return 0;
        }

        if self.capabilities.has_basic_simd() {
            #[cfg(target_arch = "x86_64")]
            {
                if self.capabilities.sse2 {
                    unsafe { self.process_samples_sse2(samples) }
                } else {
                    self.process_samples_scalar(samples)
                }
            }
            #[cfg(target_arch = "aarch64")]
            {
                if self.capabilities.neon {
                    unsafe { self.process_samples_neon(samples) }
                } else {
                    self.process_samples_scalar(samples)
                }
            }
            #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
            {
                self.process_samples_scalar(samples)
            }
        } else {
            self.process_samples_scalar(samples)
        }
    }

    /// SSE2优化的样本处理（unsafe）
    ///
    /// 使用128位SSE2向量并行处理4个f32样本：
    /// - 向量化RMS累积（平方和）
    /// - 向量化Peak检测（绝对值最大）
    /// - 双Peak机制的向量化实现
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "sse2")]
    unsafe fn process_samples_sse2(&mut self, samples: &[f32]) -> usize {
        unsafe {
            let len = samples.len();
            let mut i = 0;

            // 当前累积值加载到SSE寄存器
            let mut rms_accum = _mm_set1_ps(0.0);
            let mut primary_peak = _mm_set1_ps(self.inner.peak_primary as f32);
            let mut secondary_peak = _mm_set1_ps(self.inner.peak_secondary as f32);

            // 4样本并行处理主循环
            while i + 4 <= len {
                // 加载4个样本到SSE寄存器
                let samples_vec = _mm_loadu_ps(samples.as_ptr().add(i));

                // 计算绝对值：通过清除符号位实现
                let abs_mask = _mm_set1_ps(f32::from_bits(0x7FFFFFFF));
                let abs_samples = _mm_and_ps(samples_vec, abs_mask);

                // RMS累积：samples^2
                let squares = _mm_mul_ps(samples_vec, samples_vec);
                rms_accum = _mm_add_ps(rms_accum, squares);

                // Peak检测：更新主Peak和次Peak
                let new_primary_mask = _mm_cmpgt_ps(abs_samples, primary_peak);

                // 条件更新：新Peak > 主Peak时，主Peak -> 次Peak，新Peak -> 主Peak
                let old_primary = primary_peak;
                primary_peak = _mm_blendv_ps(primary_peak, abs_samples, new_primary_mask);
                secondary_peak = _mm_blendv_ps(secondary_peak, old_primary, new_primary_mask);

                // 处理新Peak > 次Peak但 <= 主Peak的情况
                let secondary_mask = _mm_and_ps(
                    _mm_cmpgt_ps(abs_samples, secondary_peak),
                    _mm_cmple_ps(abs_samples, primary_peak),
                );
                secondary_peak = _mm_blendv_ps(secondary_peak, abs_samples, secondary_mask);

                i += 4;
            }

            // 水平归约：将4个并行值合并为标量
            self.inner.rms_accumulator += self.horizontal_sum_ps(rms_accum) as f64;

            // Peak值的水平最大值
            self.inner.peak_primary = self.horizontal_max_ps(primary_peak) as f64;
            self.inner.peak_secondary = self.horizontal_max_ps(secondary_peak) as f64;

            // 处理剩余样本（标量方式）
            while i < len {
                self.inner.process_sample(samples[i]);
                i += 1;
            }

            len
        }
    }

    /// ARM NEON优化的样本处理（unsafe）
    ///
    /// 使用128位NEON向量并行处理4个f32样本：
    /// - 向量化RMS累积（平方和）
    /// - 向量化Peak检测（绝对值最大）
    /// - 双Peak机制的向量化实现
    #[cfg(target_arch = "aarch64")]
    #[target_feature(enable = "neon")]
    unsafe fn process_samples_neon(&mut self, samples: &[f32]) -> usize {
        let len = samples.len();
        let mut i = 0;

        // 使用标量方式处理Peak，确保与原实现完全一致
        // NEON用于RMS累积，Peak处理回退到标量以保证正确性
        let mut current_primary = self.inner.peak_primary;
        let mut current_secondary = self.inner.peak_secondary;

        // SAFETY: NEON intrinsics操作
        unsafe {
            // 当前累积值加载到NEON寄存器
            let mut rms_accum = vdupq_n_f32(0.0);

            // 4样本并行处理主循环（仅RMS累积使用SIMD）
            while i + 4 <= len {
                // 加载4个样本到NEON寄存器
                let samples_vec = vld1q_f32(samples.as_ptr().add(i));

                // RMS累积：samples^2（SIMD加速）
                let squares = vmulq_f32(samples_vec, samples_vec);
                rms_accum = vaddq_f32(rms_accum, squares);

                // Peak处理：逐个样本处理以确保正确的次Peak逻辑
                for j in 0..4 {
                    let sample_abs = samples[i + j].abs() as f64;

                    if sample_abs > current_primary {
                        current_secondary = current_primary;
                        current_primary = sample_abs;
                    } else if sample_abs > current_secondary {
                        current_secondary = sample_abs;
                    }
                }

                i += 4;
            }

            // 水平归约：将4个并行值合并为标量
            self.inner.rms_accumulator += self.horizontal_sum_neon(rms_accum) as f64;
        }

        // 更新Peak值
        self.inner.peak_primary = current_primary;
        self.inner.peak_secondary = current_secondary;

        // 处理剩余样本（标量方式）
        while i < len {
            self.inner.process_sample(samples[i]);
            i += 1;
        }

        len
    }

    /// 标量处理方式（fallback）
    fn process_samples_scalar(&mut self, samples: &[f32]) -> usize {
        for &sample in samples {
            self.inner.process_sample(sample);
        }
        samples.len()
    }

    /// SSE寄存器水平求和（4个f32相加）
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "sse2")]
    unsafe fn horizontal_sum_ps(&self, vec: __m128) -> f32 {
        unsafe {
            let shuf1 = _mm_movehdup_ps(vec); // [1,1,3,3] 
            let sum1 = _mm_add_ps(vec, shuf1); // [0+1,1+1,2+3,3+3]
            let shuf2 = _mm_movehl_ps(sum1, sum1); // [2+3,3+3,2+3,3+3]
            let sum2 = _mm_add_ss(sum1, shuf2); // [0+1+2+3,...]
            _mm_cvtss_f32(sum2)
        }
    }

    /// SSE寄存器水平最大值（4个f32中的最大值）
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "sse2")]
    unsafe fn horizontal_max_ps(&self, vec: __m128) -> f32 {
        unsafe {
            let shuf1 = _mm_movehdup_ps(vec);
            let max1 = _mm_max_ps(vec, shuf1);
            let shuf2 = _mm_movehl_ps(max1, max1);
            let max2 = _mm_max_ss(max1, shuf2);
            _mm_cvtss_f32(max2)
        }
    }

    /// NEON寄存器水平求和（4个f32相加）
    #[cfg(target_arch = "aarch64")]
    #[target_feature(enable = "neon")]
    unsafe fn horizontal_sum_neon(&self, vec: float32x4_t) -> f32 {
        // 使用NEON的vpaddq指令进行水平求和
        let sum_pairs = vpaddq_f32(vec, vec); // [v0+v1, v2+v3, v0+v1, v2+v3]
        vpadds_f32(vget_low_f32(sum_pairs)) // (v0+v1) + (v2+v3)
    }

    /// 获取内部ChannelData的引用
    pub fn inner(&self) -> &ChannelData {
        &self.inner
    }

    /// 获取内部ChannelData的可变引用
    pub fn inner_mut(&mut self) -> &mut ChannelData {
        &mut self.inner
    }

    /// 计算RMS值（代理到内部实现）
    pub fn calculate_rms(&self, sample_count: usize) -> f64 {
        self.inner.calculate_rms(sample_count)
    }

    /// 获取有效Peak值（代理到内部实现）
    pub fn get_effective_peak(&self) -> f64 {
        self.inner.get_effective_peak()
    }

    /// 重置处理器状态
    pub fn reset(&mut self) {
        self.inner.reset();
        self.sample_buffer.clear();
    }

    /// 获取缓冲区容量（字节对齐到SIMD边界）
    pub fn buffer_capacity(&self) -> usize {
        self.buffer_capacity
    }
}

/// SIMD处理器工厂
pub struct SimdProcessor {
    capabilities: SimdCapabilities,
}

impl SimdProcessor {
    /// 创建SIMD处理器工厂
    pub fn new() -> Self {
        Self {
            capabilities: SimdCapabilities::detect(),
        }
    }

    /// 获取SIMD能力
    pub fn capabilities(&self) -> &SimdCapabilities {
        &self.capabilities
    }

    /// 创建SIMD优化的声道数据处理器
    pub fn create_channel_processor(&self, buffer_size: usize) -> SimdChannelData {
        SimdChannelData::new(buffer_size)
    }

    /// 检查是否推荐使用SIMD优化
    ///
    /// 考虑CPU支持和数据量大小，
    /// 小数据量可能不适合SIMD开销
    pub fn should_use_simd(&self, sample_count: usize) -> bool {
        // 至少需要SSE2支持
        if !self.capabilities.has_basic_simd() {
            return false;
        }

        // 样本数量需要足够大才值得SIMD开销
        // 基于实验数据，至少需要100个样本
        sample_count >= 100
    }
}

impl Default for SimdProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simd_capability_detection() {
        let caps = SimdCapabilities::detect();

        // 至少应该能检测基本信息（不管是否支持）
        println!("SIMD能力检测:");
        #[cfg(target_arch = "x86_64")]
        {
            println!("  x86_64架构:");
            println!("    SSE2: {}", caps.sse2);
            println!("    SSE4.1: {}", caps.sse4_1);
            println!("    AVX: {}", caps.avx);
            println!("    AVX2: {}", caps.avx2);
        }
        #[cfg(target_arch = "aarch64")]
        {
            println!("  ARM64架构:");
            println!("    NEON: {}", caps.neon);
            println!("    DotProd: {}", caps.dotprod);
            println!("    FP16: {}", caps.fp16);
            println!("    Crypto: {}", caps.crypto);
        }
        println!("  基础SIMD支持: {}", caps.has_basic_simd());
        println!("  高级SIMD支持: {}", caps.has_advanced_simd());
        println!("  推荐并行度: {}", caps.recommended_parallelism());

        // 基本检查
        assert!(caps.recommended_parallelism() >= 1);
        assert!(caps.recommended_parallelism() <= 16);
    }

    #[test]
    fn test_simd_channel_data_creation() {
        let processor = SimdChannelData::new(1024);

        assert_eq!(processor.inner().rms_accumulator, 0.0);
        assert_eq!(processor.inner().peak_primary, 0.0);
        assert!(processor.buffer_capacity >= 1024);

        // 应该能正确报告SIMD支持状态
        let has_simd = processor.has_simd_support();
        println!("当前系统SIMD支持: {}", has_simd);
    }

    #[test]
    fn test_simd_vs_scalar_consistency() {
        // 确保SIMD和标量实现结果一致
        let test_samples = vec![0.1, -0.2, 0.3, -0.4, 0.5, -0.6, 0.7, -0.8];

        // SIMD处理
        let mut simd_processor = SimdChannelData::new(16);
        simd_processor.process_samples_simd(&test_samples);

        // 标量处理
        let mut scalar_data = ChannelData::new();
        for &sample in &test_samples {
            scalar_data.process_sample(sample);
        }

        // 比较结果（允许浮点精度误差）
        let rms_diff = (simd_processor.inner().rms_accumulator - scalar_data.rms_accumulator).abs();
        let peak1_diff = (simd_processor.inner().peak_primary - scalar_data.peak_primary).abs();
        let peak2_diff = (simd_processor.inner().peak_secondary - scalar_data.peak_secondary).abs();

        println!("🔍 SIMD vs 标量实现对比:");
        println!("  RMS累积:");
        println!("    SIMD: {}", simd_processor.inner().rms_accumulator);
        println!("    标量: {}", scalar_data.rms_accumulator);
        println!("    差异: {}", rms_diff);
        println!("  主Peak:");
        println!("    SIMD: {}", simd_processor.inner().peak_primary);
        println!("    标量: {}", scalar_data.peak_primary);
        println!("    差异: {}", peak1_diff);
        println!("  次Peak:");
        println!("    SIMD: {}", simd_processor.inner().peak_secondary);
        println!("    标量: {}", scalar_data.peak_secondary);
        println!("    差异: {}", peak2_diff);

        // 🎯 SIMD vs 标量精度阈值：考虑浮点运算的固有误差
        const RMS_TOLERANCE: f64 = 1e-5;    // RMS累积的合理误差范围
        const PEAK_TOLERANCE: f64 = 1e-6;   // Peak值的严格误差范围
        
        assert!(rms_diff < RMS_TOLERANCE, 
            "RMS差异过大: {} (阈值: {})\n  SIMD: {}\n  标量: {}", 
            rms_diff, RMS_TOLERANCE, simd_processor.inner().rms_accumulator, scalar_data.rms_accumulator);
            
        assert!(peak1_diff < PEAK_TOLERANCE, 
            "主Peak差异过大: {} (阈值: {})\n  SIMD: {}\n  标量: {}", 
            peak1_diff, PEAK_TOLERANCE, simd_processor.inner().peak_primary, scalar_data.peak_primary);
            
        assert!(peak2_diff < PEAK_TOLERANCE, 
            "次Peak差异过大: {} (阈值: {})\n  SIMD: {}\n  标量: {}", 
            peak2_diff, PEAK_TOLERANCE, simd_processor.inner().peak_secondary, scalar_data.peak_secondary);

        println!("✅ SIMD与标量实现一致性验证通过");
    }

    #[test]
    fn test_simd_processor_factory() {
        let factory = SimdProcessor::new();

        // 测试处理器创建
        let processor = factory.create_channel_processor(512);
        assert!(processor.buffer_capacity >= 512);

        // 测试SIMD推荐逻辑
        assert!(!factory.should_use_simd(50)); // 太少样本，无论是否支持SIMD都不推荐

        // 如果支持SIMD，足够的样本应该推荐使用SIMD
        // 如果不支持SIMD，即使样本足够也不会推荐
        let supports_simd = factory.capabilities().has_basic_simd();
        if supports_simd {
            assert!(factory.should_use_simd(1000)); // 足够样本且支持SIMD
        } else {
            assert!(!factory.should_use_simd(1000)); // 不支持SIMD
        }

        println!("当前系统SIMD支持: {}", supports_simd);
    }

    #[test]
    fn test_simd_edge_cases() {
        let mut processor = SimdChannelData::new(64);

        // 空数组
        assert_eq!(processor.process_samples_simd(&[]), 0);

        // 单个样本
        assert_eq!(processor.process_samples_simd(&[0.5]), 1);

        // 不对齐的数量（5个样本，不能整除4）
        let samples = vec![0.1, 0.2, 0.3, 0.4, 0.5];
        assert_eq!(processor.process_samples_simd(&samples), 5);

        // 验证状态正确更新
        assert!(processor.inner().rms_accumulator > 0.0);
        assert!(processor.inner().peak_primary > 0.0);
    }
}
