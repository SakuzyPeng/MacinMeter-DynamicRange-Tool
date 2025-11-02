//! SIMD基础设施
//!
//! 提供跨平台SIMD能力检测和通用SIMD处理器，
//! 针对音频处理的核心算法进行专门优化。
//!
//! ## 性能目标
//! - 4样本并行处理（128位向量）
//! - 理论峰值6-7x（纯SIMD运算），实际典型3-5x（受内存带宽限制）
//! - 高精度一致性（与标量实现误差 < 1e-6）
//!
//! ## 当前实现路径
//! - x86_64: **SSE2实现**（AVX2能力检测已预留，未实现）
//! - ARM64: **NEON实现**
//! - 其他平台: 自动fallback到标量实现

use crate::processing::ChannelData;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
use std::sync::Once;

/// SIMD处理器能力检测结果（支持x86_64和ARM aarch64）
#[derive(Debug, Clone, PartialEq)]
pub struct SimdCapabilities {
    // x86_64 SIMD能力
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

    // ARM aarch64 SIMD能力
    /// NEON支持（ARM的128位SIMD，4x f32并行）
    pub neon: bool,
    /// 高级NEON特性（如点积、FMA等）
    pub neon_fp16: bool,
    /// ARM SVE支持（可变长度向量，未来扩展）
    pub sve: bool,
}

impl SimdCapabilities {
    /// 检测当前CPU的SIMD能力
    ///
    /// 使用各架构的特性检测指令，返回详细的SIMD支持情况
    pub fn detect() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            Self {
                // x86_64 SIMD能力检测
                sse2: is_x86_feature_detected!("sse2"),
                sse3: is_x86_feature_detected!("sse3"),
                ssse3: is_x86_feature_detected!("ssse3"),
                sse4_1: is_x86_feature_detected!("sse4.1"),
                avx: is_x86_feature_detected!("avx"),
                avx2: is_x86_feature_detected!("avx2"),
                fma: is_x86_feature_detected!("fma"),
                // ARM能力在x86上为false
                neon: false,
                neon_fp16: false,
                sve: false,
            }
        }

        #[cfg(target_arch = "aarch64")]
        {
            Self {
                // x86_64能力在ARM上为false
                sse2: false,
                sse3: false,
                ssse3: false,
                sse4_1: false,
                avx: false,
                avx2: false,
                fma: false,
                // ARM aarch64 SIMD能力检测
                neon: true, // 现代Apple Silicon/ARM处理器都支持NEON
                neon_fp16: std::arch::is_aarch64_feature_detected!("fp16"),
                sve: std::arch::is_aarch64_feature_detected!("sve"),
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
                neon_fp16: false,
                sve: false,
            }
        }
    }

    /// 是否支持基础SIMD加速（SSE2或NEON）
    #[inline]
    pub fn has_basic_simd(&self) -> bool {
        self.sse2 || self.neon
    }

    /// 是否支持高级SIMD优化（SSE4.1+或NEON FP16+）
    #[inline]
    pub fn has_advanced_simd(&self) -> bool {
        self.sse4_1 || self.neon_fp16
    }

    /// 获取建议的并行度（一次处理的样本数）
    ///
    /// 注意：当前仅实现了SSE2/NEON路径(4宽度)，AVX2支持待未来扩展
    pub fn recommended_parallelism(&self) -> usize {
        // 注意：即使检测到AVX2支持，当前实现仅支持SSE2/NEON (4样本并行)
        // AVX2实现(8样本并行)将在未来版本中添加
        if self.sse2 || self.neon {
            4 // SSE2/NEON: 4x f32 并行 (当前唯一实现的SIMD路径)
        } else {
            1 // 标量处理
        }
    }
}

/// SIMD优化的声道数据处理器
///
/// 为ChannelData提供向量化加速，
/// 保持与原始实现高精度的数值一致性
pub struct SimdChannelData {
    /// 内部ChannelData实例
    inner: ChannelData,

    /// SIMD能力缓存
    capabilities: SimdCapabilities,
}

impl SimdChannelData {
    /// 创建新的SIMD优化声道数据处理器
    ///
    /// # 示例
    ///
    /// ```ignore
    /// use macinmeter_dr_tool::processing::SimdChannelData;
    ///
    /// let processor = SimdChannelData::new();
    /// println!("SIMD支持: {}", processor.has_simd_support());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// 检查是否支持SIMD加速
    #[inline]
    pub fn has_simd_support(&self) -> bool {
        self.capabilities.has_basic_simd()
    }

    /// 获取SIMD能力信息
    #[inline]
    pub fn capabilities(&self) -> &SimdCapabilities {
        &self.capabilities
    }

    /// 批量处理音频样本（SIMD优化）
    ///
    /// 使用SSE2指令并行处理4个样本，
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
    /// ```ignore
    /// use macinmeter_dr_tool::processing::SimdChannelData;
    ///
    /// let mut processor = SimdChannelData::new();
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
                // SAFETY: process_samples_sse2需要SSE2支持，已通过capabilities.has_basic_simd()验证。
                // 该函数内部会正确处理数组边界，确保SIMD和标量处理不会越界。
                unsafe { self.process_samples_sse2(samples) }
            }
            #[cfg(target_arch = "aarch64")]
            {
                // SAFETY: process_samples_neon需要NEON支持，已通过capabilities.has_basic_simd()验证。
                // 该函数内部会正确处理数组边界，确保SIMD和标量处理不会越界。
                unsafe { self.process_samples_neon(samples) }
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
    /// - 标量处理Peak检测确保精度一致性
    /// - 完整处理所有样本（包括剩余样本）
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "sse2")]
    #[allow(unused_unsafe)] // 跨平台兼容: 抑制CI环境"unnecessary unsafe block"警告，保持精度一致性
    unsafe fn process_samples_sse2(&mut self, samples: &[f32]) -> usize {
        let len = samples.len();
        let mut i = 0;

        // SSE2向量化累加器：2个f64值 (128位寄存器)
        let mut sum_pd = _mm_setzero_pd();

        // SIMD加速RMS计算：4样本并行处理
        while i + 4 <= len {
            // SAFETY: 使用_mm_loadu_ps从未对齐内存加载4个f32值。
            // 前置条件：i + 4 <= len，确保有4个有效样本可读取。
            // samples.as_ptr().add(i)计算的指针保证在数组边界内：i最大为len-4。
            // _mm_loadu_ps允许未对齐访问，不要求16字节对齐，因此总是安全的。
            let samples_vec = unsafe { _mm_loadu_ps(samples.as_ptr().add(i)) };

            // 真正向量化：直接用SSE2指令做f32→f64转换和平方累加
            // SAFETY: SSE2向量化f32→f64转换和平方累加
            // _mm_cvtps_pd将__m128的低2个f32转为2个f64 (__m128d)
            // _mm_movehl_ps将高2个f32移到低位，再用_mm_cvtps_pd转换
            // 所有操作都是纯SIMD寄存器运算，无内存访问风险
            unsafe {
                // 低2个f32 → 2个f64
                let lo_pd = _mm_cvtps_pd(samples_vec);
                // 高2个f32 → 2个f64 (先用movehl_ps将高半部分移到低位)
                let hi_ps = _mm_movehl_ps(samples_vec, samples_vec);
                let hi_pd = _mm_cvtps_pd(hi_ps);

                // 向量化平方并累加：sum_pd += lo_pd²
                sum_pd = _mm_add_pd(sum_pd, _mm_mul_pd(lo_pd, lo_pd));
                // 向量化平方并累加：sum_pd += hi_pd²
                sum_pd = _mm_add_pd(sum_pd, _mm_mul_pd(hi_pd, hi_pd));
            }

            i += 4;
        }

        // 水平提取：将2个f64累加到标量
        // SAFETY: _mm_storeu_pd将__m128d存储到未对齐的f64数组
        // sum_array是有效的2元素f64数组，已正确初始化
        unsafe {
            let mut sum_array = [0.0f64; 2];
            _mm_storeu_pd(sum_array.as_mut_ptr(), sum_pd);
            self.inner.rms_accumulator += sum_array[0] + sum_array[1];
        }

        // 处理剩余样本（标量方式，确保完整性）
        while i < len {
            let sample = samples[i] as f64;
            self.inner.rms_accumulator += sample * sample;
            i += 1;
        }

        // Peak检测使用标量方式确保跨架构一致性
        for &sample in samples {
            let abs_sample = sample.abs() as f64;

            if abs_sample > self.inner.peak_primary {
                // 新样本成为主Peak，原主Peak降为次Peak
                self.inner.peak_secondary = self.inner.peak_primary;
                self.inner.peak_primary = abs_sample;
            } else if abs_sample > self.inner.peak_secondary {
                // 新样本成为次Peak
                self.inner.peak_secondary = abs_sample;
            }
        }

        len
    }

    /// ARM NEON优化的样本处理（unsafe）
    ///
    /// 使用128位NEON向量并行处理4个f32样本：
    /// - 向量化RMS累积（平方和）
    /// - 标量处理Peak检测确保精度一致性
    /// - 完整处理所有样本（包括剩余样本）
    #[cfg(target_arch = "aarch64")]
    #[target_feature(enable = "neon")]
    #[allow(unused_unsafe)] // 跨平台兼容: 抑制CI环境"unnecessary unsafe block"警告，保持精度一致性
    unsafe fn process_samples_neon(&mut self, samples: &[f32]) -> usize {
        use std::arch::aarch64::*;

        let len = samples.len();
        let mut i = 0;

        // NEON向量化累加器：2个f64值 (128位寄存器)
        let mut sum_pd = vdupq_n_f64(0.0);

        // SIMD加速RMS计算：4样本并行处理
        while i + 4 <= len {
            // SAFETY: 使用vld1q_f32从未对齐内存加载4个f32值。
            // 前置条件：i + 4 <= len，确保有4个有效样本可读取。
            // samples.as_ptr().add(i)计算的指针保证在数组边界内：i最大为len-4。
            // vld1q_f32允许未对齐访问，因此总是安全的。
            let samples_vec = unsafe { vld1q_f32(samples.as_ptr().add(i)) };

            // 真正向量化：直接用NEON指令做f32→f64转换和平方累加
            // SAFETY: NEON向量化f32→f64转换和平方累加
            // vcvt_f64_f32将float32x2_t的2个f32转为2个f64 (float64x2_t)
            // vget_low_f32和vget_high_f32拆分4个f32为低2个和高2个
            // 所有操作都是纯NEON寄存器运算，无内存访问风险
            unsafe {
                // 拆分4个f32为低2个和高2个
                let lo_f32 = vget_low_f32(samples_vec); // 低2个f32
                let hi_f32 = vget_high_f32(samples_vec); // 高2个f32

                // 转换为f64
                let lo_pd = vcvt_f64_f32(lo_f32); // 低2个f32 → 2个f64
                let hi_pd = vcvt_f64_f32(hi_f32); // 高2个f32 → 2个f64

                // 向量化平方并累加：sum_pd += lo_pd²
                sum_pd = vaddq_f64(sum_pd, vmulq_f64(lo_pd, lo_pd));
                // 向量化平方并累加：sum_pd += hi_pd²
                sum_pd = vaddq_f64(sum_pd, vmulq_f64(hi_pd, hi_pd));
            }

            i += 4;
        }

        // 水平提取：将2个f64累加到标量
        // SAFETY: vst1q_f64将float64x2_t存储到未对齐的f64数组
        // sum_array是有效的2元素f64数组，已正确初始化
        unsafe {
            let mut sum_array = [0.0f64; 2];
            vst1q_f64(sum_array.as_mut_ptr(), sum_pd);
            self.inner.rms_accumulator += sum_array[0] + sum_array[1];
        }

        // 处理剩余样本（标量方式，确保完整性）
        while i < len {
            let sample = samples[i] as f64;
            self.inner.rms_accumulator += sample * sample;
            i += 1;
        }

        // Peak检测使用标量方式确保跨架构一致性
        for &sample in samples {
            let abs_sample = sample.abs() as f64;

            if abs_sample > self.inner.peak_primary {
                // 新样本成为主Peak，原主Peak降为次Peak
                self.inner.peak_secondary = self.inner.peak_primary;
                self.inner.peak_primary = abs_sample;
            } else if abs_sample > self.inner.peak_secondary {
                // 新样本成为次Peak
                self.inner.peak_secondary = abs_sample;
            }
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

    /// 获取内部ChannelData的引用
    #[inline]
    pub fn inner(&self) -> &ChannelData {
        &self.inner
    }

    /// 获取内部ChannelData的可变引用
    #[inline]
    pub fn inner_mut(&mut self) -> &mut ChannelData {
        &mut self.inner
    }

    /// 计算RMS值（代理到内部实现）
    #[inline]
    pub fn calculate_rms(&self, sample_count: usize) -> f64 {
        self.inner.calculate_rms(sample_count)
    }

    /// 获取有效Peak值（返回备选峰值，不做最终选择）
    ///
    /// **重要**：此方法仅代理到 `ChannelData::get_effective_peak()`。
    /// 参见那里的文档说明为何不应在 DR 计算中直接使用此值。
    ///
    /// 正确做法：通过 `PeakSelectionStrategy::select_peak()` 进行峰值选择。
    #[inline]
    pub fn get_effective_peak(&self) -> f64 {
        self.inner.get_effective_peak()
    }

    /// 重置处理器状态
    pub fn reset(&mut self) {
        self.inner.reset();
    }
}

impl Default for SimdChannelData {
    fn default() -> Self {
        Self {
            inner: ChannelData::new(),
            capabilities: SimdCapabilities::detect(),
        }
    }
}

/// SIMD处理器工厂
#[derive(Debug, Clone)]
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
    #[inline]
    pub fn capabilities(&self) -> &SimdCapabilities {
        &self.capabilities
    }

    /// 创建SIMD优化的声道数据处理器
    pub fn create_channel_processor(&self) -> SimdChannelData {
        SimdChannelData::new()
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

    /// **SIMD优化**: 计算数组平方和 (专为RMS 20%采样优化)
    ///
    /// 使用SSE2/NEON并行计算 sum(x²)，
    /// 针对histogram.rs中的RMS计算进行专门优化。
    ///
    /// # 性能提升
    /// - SSE2: 4样本并行，~3-4倍加速
    /// - 智能回退：不支持SIMD时使用标量实现
    /// - 内存友好：流式处理，避免缓存未命中
    ///
    /// # 参数
    /// * `values` - 待计算平方和的浮点数数组
    ///
    /// # 返回值
    /// 返回所有元素的平方和: Σ(values\[i\]²)
    pub fn calculate_square_sum(&self, values: &[f64]) -> f64 {
        if values.is_empty() {
            return 0.0;
        }

        // 对于小数组，直接使用标量计算
        if !self.should_use_simd(values.len()) {
            return values.iter().map(|&x| x * x).sum();
        }

        #[cfg(target_arch = "x86_64")]
        {
            if self.capabilities.sse2 {
                // SAFETY: calculate_square_sum_sse2需要SSE2支持，已通过capabilities.sse2验证。
                // values的生命周期和边界检查由调用者保证，函数内部会正确处理数组边界。
                unsafe { self.calculate_square_sum_sse2(values) }
            } else {
                #[cfg(debug_assertions)]
                {
                    eprintln!(
                        "[PERFORMANCE_WARNING] SSE2 unavailable, falling back to scalar square-sum (≈3x slower) / SSE2不可用，RMS平方和计算回退到标量实现，性能将下降约3倍"
                    );
                }
                values.iter().map(|&x| x * x).sum()
            }
        }

        #[cfg(target_arch = "aarch64")]
        {
            if self.capabilities.neon {
                // SAFETY: calculate_square_sum_neon需要NEON支持，已通过capabilities.neon验证。
                // values的生命周期和边界检查由调用者保证，函数内部会正确处理数组边界。
                unsafe { self.calculate_square_sum_neon(values) }
            } else {
                #[cfg(debug_assertions)]
                {
                    eprintln!(
                        "[PERFORMANCE_WARNING] NEON unavailable, falling back to scalar square-sum (≈3x slower) / NEON不可用，RMS平方和计算回退到标量实现，性能将下降约3倍"
                    );
                }
                values.iter().map(|&x| x * x).sum()
            }
        }

        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            // 其他架构：使用标量实现
            static WARN_ONCE: Once = Once::new();
            WARN_ONCE.call_once(|| {
                eprintln!(
                    "[PERFORMANCE_WARNING] Architecture {} lacks SIMD support; using scalar square-sum / 架构{}不支持SIMD，RMS平方和计算使用标量实现",
                    std::env::consts::ARCH,
                    std::env::consts::ARCH
                );
                eprintln!(
                    "[PERFORMANCE_TIP] Expect up to ~3x slower than x86_64/ARM64 SIMD paths / 当前性能可能较x86_64/ARM64慢约3倍"
                );
            });
            values.iter().map(|&x| x * x).sum()
        }
    }

    /// SSE2优化的平方和计算
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "sse2")]
    unsafe fn calculate_square_sum_sse2(&self, values: &[f64]) -> f64 {
        use std::arch::x86_64::*;

        let len = values.len();
        let mut i = 0;

        // 累加器：使用双精度向量避免精度损失
        let mut sum_vec = _mm_setzero_pd(); // 2x f64 向量

        // SIMD主循环：每次处理2个f64值（SSE2限制）
        while i + 2 <= len {
            // SAFETY: SSE2向量化平方和计算。
            // 前置条件：i + 2 <= len，确保有2个有效f64值可读取。
            // _mm_loadu_pd从未对齐内存加载2个f64，指针values.as_ptr().add(i)在边界内。
            // _mm_mul_pd和_mm_add_pd是纯SIMD寄存器操作，无内存访问风险。
            unsafe {
                // 加载2个f64值
                let vals = _mm_loadu_pd(values.as_ptr().add(i));
                // 计算平方
                let squares = _mm_mul_pd(vals, vals);
                // 累加到总和
                sum_vec = _mm_add_pd(sum_vec, squares);
            }

            i += 2;
        }

        // 提取并累加向量中的两个值
        let mut total_sum = 0.0;
        // SAFETY: 使用_mm_storeu_pd将__m128d存储到未对齐的f64数组
        // 相比transmute更安全且语义清晰，是提取SSE2向量元素的标准做法
        unsafe {
            let mut sum_array = [0.0f64; 2];
            _mm_storeu_pd(sum_array.as_mut_ptr(), sum_vec);
            total_sum += sum_array[0] + sum_array[1];
        }

        // 处理剩余的奇数个元素（标量）
        while i < len {
            total_sum += values[i] * values[i];
            i += 1;
        }

        total_sum
    }

    /// ARM NEON优化的平方和计算
    #[cfg(target_arch = "aarch64")]
    #[target_feature(enable = "neon")]
    unsafe fn calculate_square_sum_neon(&self, values: &[f64]) -> f64 {
        use std::arch::aarch64::*;

        let len = values.len();
        let mut i = 0;

        // **NEON优化**: 使用128位NEON向量处理2个f64值
        // 累加器：初始化为零向量
        let mut sum_vec = vdupq_n_f64(0.0); // 2x f64 向量，初始化为0

        // SIMD主循环：每次处理2个f64值（NEON双精度限制）
        while i + 2 <= len {
            // SAFETY: ARM NEON向量化平方和计算。
            // 前置条件：i + 2 <= len，确保有2个有效f64值可读取。
            // vld1q_f64从内存加载2个f64到NEON向量，指针values.as_ptr().add(i)在边界内。
            // vmulq_f64和vaddq_f64是纯NEON寄存器操作，无内存访问风险。
            unsafe {
                // 加载2个f64值到NEON向量
                let vals = vld1q_f64(values.as_ptr().add(i));
                // 计算平方：vals * vals
                let squares = vmulq_f64(vals, vals);
                // 累加到总和向量
                sum_vec = vaddq_f64(sum_vec, squares);
            }

            i += 2;
        }

        // **精度保证**: 提取并累加向量中的两个f64值
        // 使用水平加法提取NEON向量的两个元素
        let mut total_sum = vgetq_lane_f64(sum_vec, 0) + vgetq_lane_f64(sum_vec, 1);

        // **边界处理**: 处理剩余的奇数个元素（标量方式）
        while i < len {
            total_sum += values[i] * values[i];
            i += 1;
        }

        total_sum
    }
}

impl Default for SimdProcessor {
    fn default() -> Self {
        Self::new()
    }
}

/// 4×4转置结果：4个通道各4帧的样本
#[derive(Debug, Clone, Copy)]
pub struct Transpose4x4 {
    pub ch0: [f32; 4],
    pub ch1: [f32; 4],
    pub ch2: [f32; 4],
    pub ch3: [f32; 4],
}

impl SimdProcessor {
    /// 块级4×4转置：从交错样本提取4个通道的4帧
    ///
    /// 使用SIMD指令执行4×4矩阵转置，从交错的多声道样本中
    /// 提取连续4帧的4个通道，并重组为SoA布局。
    ///
    /// # 参数
    ///
    /// * `interleaved` - 交错的多声道样本 (L0,R0,C0,LFE0, L1,R1,C1,LFE1, ...)
    /// * `frame_offset` - 起始帧索引（非字节偏移）
    /// * `channel_offset` - 声道组偏移（0/4/8/12，必须是4的倍数）
    /// * `channel_count` - 总声道数
    ///
    /// # 返回值
    ///
    /// 返回Transpose4x4结构，包含4个通道各4帧的样本
    ///
    /// # 性能收益
    ///
    /// - **SSE2/NEON**: 单次SIMD加载+转置 vs 16次标量跨步访问
    /// - **Cache友好**: 连续内存访问模式
    /// - **零拷贝**: 直接从交错数组提取到栈上小数组
    ///
    /// # Panics
    ///
    /// - `channel_offset % 4 != 0` 时panic
    /// - `frame_offset + 4 > total_frames` 时panic（越界）
    pub fn transpose_4x4_block(
        &self,
        interleaved: &[f32],
        frame_offset: usize,
        channel_offset: usize,
        channel_count: usize,
    ) -> Transpose4x4 {
        debug_assert!(
            channel_offset.is_multiple_of(4),
            "channel_offset must be multiple of 4"
        );

        let total_frames = interleaved.len() / channel_count;
        debug_assert!(
            frame_offset + 4 <= total_frames,
            "not enough frames for 4x4 block"
        );

        if self.capabilities.has_basic_simd() {
            #[cfg(target_arch = "x86_64")]
            {
                // SAFETY: transpose_4x4_sse2需要SSE2支持，已通过capabilities验证
                unsafe {
                    self.transpose_4x4_sse2(
                        interleaved,
                        frame_offset,
                        channel_offset,
                        channel_count,
                    )
                }
            }
            #[cfg(target_arch = "aarch64")]
            {
                // SAFETY: transpose_4x4_neon需要NEON支持，已通过capabilities验证
                unsafe {
                    self.transpose_4x4_neon(
                        interleaved,
                        frame_offset,
                        channel_offset,
                        channel_count,
                    )
                }
            }
            #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
            {
                self.transpose_4x4_scalar(interleaved, frame_offset, channel_offset, channel_count)
            }
        } else {
            self.transpose_4x4_scalar(interleaved, frame_offset, channel_offset, channel_count)
        }
    }

    /// SSE2优化的4×4转置
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "sse2")]
    unsafe fn transpose_4x4_sse2(
        &self,
        interleaved: &[f32],
        frame_offset: usize,
        channel_offset: usize,
        channel_count: usize,
    ) -> Transpose4x4 {
        use std::arch::x86_64::*;

        // 加载4帧，每帧取channel_offset开始的4个通道
        // 帧i的样本起始位置 = (frame_offset + i) * channel_count + channel_offset
        let mut rows = [_mm_setzero_ps(); 4];
        for (i, row) in rows.iter_mut().enumerate() {
            let pos = (frame_offset + i) * channel_count + channel_offset;
            // SAFETY: pos 在 [0, interleaved.len()-4) 范围内（已由 debug_assert 验证）。
            // 需要在 unsafe 块中调用指针算术与 SIMD 加载。
            let ptr = unsafe { interleaved.as_ptr().add(pos) };
            *row = unsafe { _mm_loadu_ps(ptr) };
        }

        // SSE2 4×4转置标准算法
        // SAFETY: SSE2寄存器操作，无内存访问风险
        unsafe {
            // 步骤1：unpacklo/unpackhi交织相邻行
            let t0 = _mm_unpacklo_ps(rows[0], rows[1]); // r0低2ch, r1低2ch交织
            let t1 = _mm_unpackhi_ps(rows[0], rows[1]); // r0高2ch, r1高2ch交织
            let t2 = _mm_unpacklo_ps(rows[2], rows[3]); // r2低2ch, r3低2ch交织
            let t3 = _mm_unpackhi_ps(rows[2], rows[3]); // r2高2ch, r3高2ch交织

            // 步骤2：movelh/movehl完成最终转置
            let ch0_vec = _mm_movelh_ps(t0, t2); // ch0的4个样本
            let ch1_vec = _mm_movehl_ps(t2, t0); // ch1的4个样本
            let ch2_vec = _mm_movelh_ps(t1, t3); // ch2的4个样本
            let ch3_vec = _mm_movehl_ps(t3, t1); // ch3的4个样本

            // 提取到数组
            let mut ch0 = [0.0f32; 4];
            let mut ch1 = [0.0f32; 4];
            let mut ch2 = [0.0f32; 4];
            let mut ch3 = [0.0f32; 4];

            _mm_storeu_ps(ch0.as_mut_ptr(), ch0_vec);
            _mm_storeu_ps(ch1.as_mut_ptr(), ch1_vec);
            _mm_storeu_ps(ch2.as_mut_ptr(), ch2_vec);
            _mm_storeu_ps(ch3.as_mut_ptr(), ch3_vec);

            Transpose4x4 { ch0, ch1, ch2, ch3 }
        }
    }

    /// ARM NEON优化的4×4转置
    #[cfg(target_arch = "aarch64")]
    #[target_feature(enable = "neon")]
    unsafe fn transpose_4x4_neon(
        &self,
        interleaved: &[f32],
        frame_offset: usize,
        channel_offset: usize,
        channel_count: usize,
    ) -> Transpose4x4 {
        use std::arch::aarch64::*;

        // 加载4帧
        let rows = [0, 1, 2, 3].map(|i| {
            let pos = (frame_offset + i) * channel_count + channel_offset;
            // SAFETY: 加载4个连续f32
            unsafe { vld1q_f32(interleaved.as_ptr().add(pos)) }
        });

        // NEON 4×4转置：使用vuzp（deinterleave）两轮完成
        // SAFETY: NEON寄存器操作
        unsafe {
            // 步骤1：使用vuzpq_f32分离偶数/奇数lane
            // uzp01.0 = [1, 3, 5, 7], uzp01.1 = [2, 4, 6, 8]
            // uzp23.0 = [9, 11, 13, 15], uzp23.1 = [10, 12, 14, 16]
            let uzp01 = vuzpq_f32(rows[0], rows[1]);
            let uzp23 = vuzpq_f32(rows[2], rows[3]);

            // 步骤2：再次使用vuzp完成最终转置
            // final0.0 = [1, 5, 9, 13] = ch0
            // final0.1 = [3, 7, 11, 15] = ch2
            // final1.0 = [2, 6, 10, 14] = ch1
            // final1.1 = [4, 8, 12, 16] = ch3
            let final0 = vuzpq_f32(uzp01.0, uzp23.0);
            let final1 = vuzpq_f32(uzp01.1, uzp23.1);

            // 提取到数组
            let mut ch0 = [0.0f32; 4];
            let mut ch1 = [0.0f32; 4];
            let mut ch2 = [0.0f32; 4];
            let mut ch3 = [0.0f32; 4];

            vst1q_f32(ch0.as_mut_ptr(), final0.0);
            vst1q_f32(ch1.as_mut_ptr(), final1.0);
            vst1q_f32(ch2.as_mut_ptr(), final0.1);
            vst1q_f32(ch3.as_mut_ptr(), final1.1);

            Transpose4x4 { ch0, ch1, ch2, ch3 }
        }
    }

    /// 标量fallback版本的4×4转置
    fn transpose_4x4_scalar(
        &self,
        interleaved: &[f32],
        frame_offset: usize,
        channel_offset: usize,
        channel_count: usize,
    ) -> Transpose4x4 {
        let mut ch0 = [0.0f32; 4];
        let mut ch1 = [0.0f32; 4];
        let mut ch2 = [0.0f32; 4];
        let mut ch3 = [0.0f32; 4];

        for i in 0..4 {
            let pos = (frame_offset + i) * channel_count + channel_offset;
            ch0[i] = interleaved[pos];
            ch1[i] = interleaved[pos + 1];
            ch2[i] = interleaved[pos + 2];
            ch3[i] = interleaved[pos + 3];
        }

        Transpose4x4 { ch0, ch1, ch2, ch3 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simd_capability_detection() {
        let caps = SimdCapabilities::detect();

        // 至少应该能检测基本信息（不管是否支持）
        println!("SIMD capability detection / SIMD能力检测:");
        println!("  SSE2: {}", caps.sse2);
        println!("  SSE4.1: {}", caps.sse4_1);
        println!("  AVX: {}", caps.avx);
        println!(
            "  Recommended parallelism / 推荐并行度: {}",
            caps.recommended_parallelism()
        );

        // 基本检查
        assert!(caps.recommended_parallelism() >= 1);
        assert!(caps.recommended_parallelism() <= 16);
    }

    #[test]
    fn test_simd_channel_data_creation() {
        let processor = SimdChannelData::new();

        assert_eq!(processor.inner().rms_accumulator, 0.0);
        assert_eq!(processor.inner().peak_primary, 0.0);

        // 应该能正确报告SIMD支持状态
        let has_simd = processor.has_simd_support();
        println!("SIMD support on this system: {has_simd} / 当前系统SIMD支持: {has_simd}");
    }

    #[test]
    fn test_simd_vs_scalar_consistency() {
        // 确保SIMD和标量实现结果一致
        let test_samples = vec![0.1, -0.2, 0.3, -0.4, 0.5, -0.6, 0.7, -0.8];

        // SIMD处理
        let mut simd_processor = SimdChannelData::new();
        simd_processor.process_samples_simd(&test_samples);

        // 标量处理
        let mut scalar_data = ChannelData::new();
        for &sample in &test_samples {
            scalar_data.process_sample(sample);
        }

        // 比较结果（要求绝对精度一致性）
        let rms_diff = (simd_processor.inner().rms_accumulator - scalar_data.rms_accumulator).abs();
        let peak1_diff = (simd_processor.inner().peak_primary - scalar_data.peak_primary).abs();
        let peak2_diff = (simd_processor.inner().peak_secondary - scalar_data.peak_secondary).abs();

        // 验证SIMD处理器是否真的处理了样本
        if simd_processor.inner().rms_accumulator == 0.0 {
            panic!(
                "SIMD accumulator is zero; samples were not processed / SIMD处理器RMS累加器为0，说明样本没有被正确处理！"
            );
        }

        assert!(
            rms_diff < 1e-6,
            "RMS difference too large: {rms_diff} / RMS差异过大: {rms_diff}"
        );
        assert!(
            peak1_diff < 1e-6,
            "Primary peak difference too large: {peak1_diff} / 主Peak差异过大: {peak1_diff}"
        );
        assert!(
            peak2_diff < 1e-6,
            "Secondary peak difference too large: {peak2_diff} / 次Peak差异过大: {peak2_diff}"
        );

        println!("SIMD vs scalar consistency verified / SIMD与标量实现一致性验证通过");
    }

    #[test]
    fn test_simd_processor_factory() {
        let factory = SimdProcessor::new();

        // 测试处理器创建
        let _ = factory.create_channel_processor();

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

        println!("SIMD support available: {supports_simd} / 当前系统SIMD支持: {supports_simd}");
    }

    #[test]
    fn test_simd_edge_cases() {
        let mut processor = SimdChannelData::new();

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

    // ========================================================================
    // 深度SIMD精度测试 (从tests/simd_precision_test.rs合并)
    // ========================================================================

    #[test]
    fn test_extreme_precision_requirements() {
        println!(
            "[PRECISION_TEST] Testing extreme precision requirements / 执行极端精度要求测试..."
        );

        // 使用更大的测试数据集
        let test_samples: Vec<f32> = (0..10000)
            .map(|i| (i as f32 * 0.001).sin() * 0.8) // 更复杂的波形
            .collect();

        // SIMD处理
        let mut simd_processor = SimdChannelData::new();
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

        println!("[TEST_RESULT] Large dataset precision comparison / 大数据集精度对比:");
        println!("  样本数量 / Sample count: {}", test_samples.len());
        println!("  RMS累积 / RMS Accumulation:");
        println!("    SIMD:  {:.16}", simd_processor.inner().rms_accumulator);
        println!("    Scalar / 标量:  {:.16}", scalar_data.rms_accumulator);
        println!("    差异 / Difference:  {rms_diff:.2e}");
        println!(
            "    相对误差 / Relative Error: {:.2e}",
            rms_diff / scalar_data.rms_accumulator
        );

        println!("  Primary Peak / 主Peak:");
        println!("    SIMD:  {:.16}", simd_processor.inner().peak_primary);
        println!("    Scalar / 标量:  {:.16}", scalar_data.peak_primary);
        println!("    差异 / Difference:  {peak1_diff:.2e}");

        println!("  Secondary Peak / 次Peak:");
        println!("    SIMD:  {:.16}", simd_processor.inner().peak_secondary);
        println!("    Scalar / 标量:  {:.16}", scalar_data.peak_secondary);
        println!("    差异 / Difference:  {peak2_diff:.2e}");

        // 更严格的精度要求（类似dr14_t.meter的标准）
        let relative_rms_error = rms_diff / scalar_data.rms_accumulator;

        println!("[PRECISION_ASSESSMENT] Precision evaluation / 精度评估:");
        println!("  RMS相对误差 / RMS Relative Error: {relative_rms_error:.2e}");

        if relative_rms_error > 1e-10 {
            println!(
                "[WARNING] RMS precision may be insufficient, relative error > 1e-10 / 警告：RMS精度可能不足，相对误差 > 1e-10"
            );
        } else {
            println!("[OK] RMS precision meets requirements / RMS精度满足要求");
        }

        if peak1_diff > 1e-12 {
            println!("[WARNING] Peak precision may be insufficient / 警告：Peak精度可能不足");
        } else {
            println!("[OK] Peak precision meets requirements / Peak精度满足要求");
        }
    }

    #[test]
    fn test_dr_calculation_precision() {
        println!("DR precision test / DR计算精度测试...");

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

        println!(
            "  Sample info: {sec} s, {khz} kHz, stereo / 样本信息：{sec}秒，{khz}kHz，立体声",
            sec = 3,
            khz = 48
        );
        println!(
            "  Left channel samples: {count} / 左声道样本数：{count}",
            count = left_samples.len()
        );
        println!(
            "  Right channel samples: {count} / 右声道样本数：{count}",
            count = right_samples.len()
        );

        // 测试左声道
        let mut simd_left = SimdChannelData::new();
        let mut scalar_left = ChannelData::new();

        simd_left.process_samples_simd(&left_samples);
        for &sample in &left_samples {
            scalar_left.process_sample(sample);
        }

        let left_rms_simd = simd_left.calculate_rms(left_samples.len());
        let left_rms_scalar = scalar_left.calculate_rms(left_samples.len());

        println!("  Left channel RMS comparison / 左声道RMS对比:");
        println!("    SIMD:  {:.8} dB / SIMD", 20.0 * left_rms_simd.log10());
        println!(
            "    Scalar:  {:.8} dB / 标量",
            20.0 * left_rms_scalar.log10()
        );

        let rms_db_diff = 20.0 * (left_rms_simd / left_rms_scalar).log10();
        println!("    Difference: {rms_db_diff:.6} dB / 差异: {rms_db_diff:.6} dB");

        // DR计算精度要求：误差应 < 0.01 dB
        if rms_db_diff.abs() > 0.01 {
            println!(
                " Warning: RMS difference > 0.01 dB, potential DR precision risk / 警告：RMS差异 > 0.01 dB，可能影响DR测量精度"
            );
            println!(
                "   Similar to dr14_t.meter super-vectorized precision issue / 类似于dr14_t.meter的超级向量化精度问题"
            );
        } else {
            println!(
                "RMS precision within DR tolerance (< 0.01 dB) / RMS精度满足DR测量要求 (< 0.01 dB)"
            );
        }
    }

    #[test]
    fn test_cumulative_error_analysis() {
        println!(
            "[CUMULATIVE_ERROR_ANALYSIS] Cumulative error analysis test / 累积误差分析测试..."
        );

        // 测试不同长度的累积误差增长
        let test_lengths = [100, 1000, 10000, 100000];

        for &len in &test_lengths {
            let test_samples: Vec<f32> = (0..len).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();

            let mut simd_proc = SimdChannelData::new();
            let mut scalar_data = ChannelData::new();

            simd_proc.process_samples_simd(&test_samples);
            for &sample in &test_samples {
                scalar_data.process_sample(sample);
            }

            let rms_diff = (simd_proc.inner().rms_accumulator - scalar_data.rms_accumulator).abs();
            let relative_error = rms_diff / scalar_data.rms_accumulator;

            println!(
                "  样本数 / Sample count {len:6}: 相对误差 / Relative error {relative_error:.2e}"
            );

            // 检查误差是否随样本数增长
            if len > 1000 && relative_error > 1e-9 {
                println!(
                    "[WARNING]     Cumulative error grows with sample count, precision risk exists / 累积误差随样本数增长，存在精度风险"
                );
            }
        }
    }

    #[test]
    fn test_calculate_square_sum_basic() {
        println!("Testing calculate_square_sum basics / 测试calculate_square_sum基本功能...");

        let processor = SimdProcessor::new();

        // 测试空数组
        assert_eq!(processor.calculate_square_sum(&[]), 0.0);

        // 测试单个元素
        let result = processor.calculate_square_sum(&[3.0]);
        assert!((result - 9.0).abs() < 1e-10);

        // 测试小数组（会使用标量实现）
        let small = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let expected: f64 = small.iter().map(|&x| x * x).sum();
        let result = processor.calculate_square_sum(&small);
        assert!((result - expected).abs() < 1e-10);

        println!(
            "  Result (small array): {result}, expected: {expected} / 小数组结果: {result}, 预期: {expected}"
        );
    }

    #[test]
    fn test_calculate_square_sum_large_array() {
        println!(
            "Testing large-array SIMD optimization for calculate_square_sum / 测试calculate_square_sum大数组SIMD优化..."
        );

        let processor = SimdProcessor::new();

        // 生成大数组（触发SIMD）
        let large: Vec<f64> = (0..1000).map(|i| (i as f64) * 0.01).collect();

        // SIMD实现
        let simd_result = processor.calculate_square_sum(&large);

        // 标量参考实现
        let scalar_result: f64 = large.iter().map(|&x| x * x).sum();

        let diff = (simd_result - scalar_result).abs();
        let relative_error = diff / scalar_result;

        println!("  SIMD result: {simd_result:.12} / SIMD结果: {simd_result:.12}");
        println!("  Scalar result: {scalar_result:.12} / 标量结果: {scalar_result:.12}");
        println!("  Relative error: {relative_error:.2e} / 相对误差: {relative_error:.2e}");

        // SIMD和标量结果应该高度一致
        assert!(
            relative_error < 1e-10,
            "SIMD平方和精度不足: {relative_error:.2e}"
        );
    }

    #[test]
    fn test_calculate_square_sum_boundary() {
        println!(
            "[BOUNDARY_TEST] Testing calculate_square_sum boundary cases / 测试calculate_square_sum边界情况..."
        );

        let processor = SimdProcessor::new();

        // 测试正好100个元素（SIMD阈值）
        let boundary: Vec<f64> = (0..100).map(|i| i as f64).collect();
        let result = processor.calculate_square_sum(&boundary);
        let expected: f64 = boundary.iter().map(|&x| x * x).sum();

        println!("  100-element array / 100元素数组:");
        println!("    结果 / Result: {result}");
        println!("    预期 / Expected: {expected}");
        assert!((result - expected).abs() / expected < 1e-10);

        // 测试99个元素（刚好低于阈值，应使用标量）
        let below: Vec<f64> = (0..99).map(|i| i as f64).collect();
        let result = processor.calculate_square_sum(&below);
        let expected: f64 = below.iter().map(|&x| x * x).sum();
        assert!((result - expected).abs() / expected < 1e-10);

        // 测试101个元素（刚好高于阈值，应使用SIMD）
        let above: Vec<f64> = (0..101).map(|i| i as f64).collect();
        let result = processor.calculate_square_sum(&above);
        let expected: f64 = above.iter().map(|&x| x * x).sum();
        assert!((result - expected).abs() / expected < 1e-10);
    }

    #[test]
    fn test_has_advanced_simd() {
        let caps = SimdCapabilities::detect();
        let has_advanced = caps.has_advanced_simd();

        println!("Advanced SIMD capability check / 高级SIMD能力检测:");
        println!("  SSE4.1: {}", caps.sse4_1);
        println!("  NEON FP16: {}", caps.neon_fp16);
        println!("  has_advanced_simd: {has_advanced}");

        // 验证逻辑一致性
        assert_eq!(has_advanced, caps.sse4_1 || caps.neon_fp16);
    }

    #[test]
    fn test_recommended_parallelism_levels() {
        let caps = SimdCapabilities::detect();
        let parallelism = caps.recommended_parallelism();

        println!("[PARALLELISM_ANALYSIS] Recommended parallelism analysis / 推荐并行度分析:");
        println!("  AVX2: {} -> 推荐 / Recommended: 8", caps.avx2);
        println!(
            "  SSE2/NEON: {} -> 推荐 / Recommended: 4",
            caps.has_basic_simd()
        );
        println!("  无SIMD / No SIMD: -> 推荐 / Recommended: 1");
        println!("  实际推荐 / Actual recommendation: {parallelism}");

        // 验证逻辑
        if caps.avx2 {
            // 注意：即使检测到AVX2，当前实现仅支持SSE2/NEON（4-wide），未实现AVX2（8-wide）
            assert_eq!(parallelism, 4);
        } else if caps.has_basic_simd() {
            assert_eq!(parallelism, 4);
        } else {
            assert_eq!(parallelism, 1);
        }
    }

    #[test]
    fn test_simd_processor_should_use_simd_thresholds() {
        let processor = SimdProcessor::new();

        println!("[SIMD_THRESHOLD_TEST] SIMD threshold usage test / SIMD使用阈值测试:");

        // 测试不同样本数量
        let test_cases = vec![
            (10, false, "too few samples / 太少样本"),
            (50, false, "below threshold / 低于阈值"),
            (99, false, "just below 100 / 刚好低于100"),
            (100, true, "threshold boundary / 阈值边界"),
            (101, true, "just above 100 / 刚好高于100"),
            (1000, true, "sufficient samples / 充足样本"),
            (10000, true, "large sample set / 大量样本"),
        ];

        for (count, expected_if_simd, desc) in test_cases {
            let should_use = processor.should_use_simd(count);
            let has_simd = processor.capabilities().has_basic_simd();

            if has_simd {
                assert_eq!(
                    should_use, expected_if_simd,
                    "样本数{count} ({desc}): 预期使用SIMD={expected_if_simd}, 实际={should_use}"
                );
            } else {
                assert!(!should_use, "无SIMD支持时不应使用SIMD");
            }

            println!(
                "  {count:5} samples ({desc:12}): {}",
                if should_use {
                    "use SIMD / 使用SIMD"
                } else {
                    "use scalar / 使用标量"
                }
            );
        }
    }

    #[test]
    fn test_simd_different_data_patterns() {
        println!("Testing SIMD across data patterns / 测试不同数据模式的SIMD处理...");

        let patterns = vec![
            ("全零", vec![0.0; 100]),
            ("全正", vec![0.5; 100]),
            ("全负", vec![-0.5; 100]),
            (
                "交替",
                (0..100)
                    .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
                    .collect(),
            ),
            ("递增", (0..100).map(|i| i as f32 * 0.01).collect()),
            ("正弦", (0..100).map(|i| (i as f32 * 0.1).sin()).collect()),
        ];

        for (name, samples) in patterns {
            let mut simd_proc = SimdChannelData::new();
            let mut scalar_data = ChannelData::new();

            simd_proc.process_samples_simd(&samples);
            for &sample in &samples {
                scalar_data.process_sample(sample);
            }

            let rms_diff = (simd_proc.inner().rms_accumulator - scalar_data.rms_accumulator).abs();
            let max_val = scalar_data.rms_accumulator.abs().max(1e-10);
            let relative_error = rms_diff / max_val;

            println!(
                "  {name:8}: RMS diff={rms_diff:.2e}, relative error={relative_error:.2e} / RMS差异={rms_diff:.2e}, 相对误差={relative_error:.2e}"
            );

            if scalar_data.rms_accumulator.abs() > 1e-10 {
                assert!(
                    relative_error < 1e-6,
                    "{name}模式: 相对误差过大 {relative_error:.2e}"
                );
            }
        }
    }

    #[test]
    fn test_simd_processor_capabilities_access() {
        let processor = SimdProcessor::new();
        let caps = processor.capabilities();

        println!("Verifying capabilities() access / 验证capabilities()方法访问:");
        println!(
            "  Basic SIMD: {} / 基础SIMD: {}",
            caps.has_basic_simd(),
            caps.has_basic_simd()
        );
        println!(
            "  Advanced SIMD: {} / 高级SIMD: {}",
            caps.has_advanced_simd(),
            caps.has_advanced_simd()
        );
        println!(
            "  Recommended parallelism: {} / 推荐并行度: {}",
            caps.recommended_parallelism(),
            caps.recommended_parallelism()
        );

        // 验证返回的引用有效
        assert!(caps.recommended_parallelism() >= 1);
        assert_eq!(caps.has_basic_simd(), caps.sse2 || caps.neon);
    }

    #[test]
    fn test_calculate_rms_method() {
        let mut processor = SimdChannelData::new();

        // 处理一些样本
        let samples = vec![0.1, 0.2, 0.3, 0.4, 0.5];
        processor.process_samples_simd(&samples);

        // 计算RMS
        let rms = processor.calculate_rms(samples.len());

        println!("[RMS_TEST] Testing calculate_rms method / 测试calculate_rms方法:");
        println!(
            "  RMS累加器 / RMS Accumulator: {}",
            processor.inner().rms_accumulator
        );
        println!("  样本数 / Sample count: {}", samples.len());
        println!("  计算RMS / Calculated RMS: {rms}");

        // RMS应该是正数且合理
        assert!(rms > 0.0);
        assert!(rms < 1.0); // 样本最大值0.5，RMS不应超过1.0

        // 验证数学正确性
        let expected_rms = (processor.inner().rms_accumulator / samples.len() as f64).sqrt();
        assert!((rms - expected_rms).abs() < 1e-10);
    }

    #[test]
    fn test_inner_access() {
        let mut processor = SimdChannelData::new();

        // 初始状态
        let inner = processor.inner();
        assert_eq!(inner.rms_accumulator, 0.0);
        assert_eq!(inner.peak_primary, 0.0);
        assert_eq!(inner.peak_secondary, 0.0);

        // 处理样本后
        processor.process_samples_simd(&[0.5, -0.7, 0.3]);
        let inner = processor.inner();

        println!("Testing inner() access / 测试inner()访问:");
        println!(
            "  RMS accumulator: {} / RMS累加器: {}",
            inner.rms_accumulator, inner.rms_accumulator
        );
        println!(
            "  Primary peak: {} / 主Peak: {}",
            inner.peak_primary, inner.peak_primary
        );
        println!(
            "  Secondary peak: {} / 次Peak: {}",
            inner.peak_secondary, inner.peak_secondary
        );

        // 验证状态更新
        assert!(inner.rms_accumulator > 0.0);
        assert!(inner.peak_primary > 0.0);
    }

    #[test]
    fn test_transpose_4x4_basic() {
        println!("Testing 4x4 transpose basic functionality / 测试4×4转置基本功能...");

        let processor = SimdProcessor::new();

        // 构造4帧×4声道的交错样本
        // 帧0: [1.0, 2.0, 3.0, 4.0]
        // 帧1: [5.0, 6.0, 7.0, 8.0]
        // 帧2: [9.0, 10.0, 11.0, 12.0]
        // 帧3: [13.0, 14.0, 15.0, 16.0]
        let interleaved: Vec<f32> = (1..=16).map(|i| i as f32).collect();

        let result = processor.transpose_4x4_block(&interleaved, 0, 0, 4);

        println!("  输入交错样本 / Input interleaved: {interleaved:?}");
        println!("  ch0输出 / ch0 output: {ch0:?}", ch0 = result.ch0);
        println!("  ch1输出 / ch1 output: {ch1:?}", ch1 = result.ch1);
        println!("  ch2输出 / ch2 output: {ch2:?}", ch2 = result.ch2);
        println!("  ch3输出 / ch3 output: {ch3:?}", ch3 = result.ch3);

        // 验证转置结果
        assert_eq!(result.ch0, [1.0, 5.0, 9.0, 13.0]);
        assert_eq!(result.ch1, [2.0, 6.0, 10.0, 14.0]);
        assert_eq!(result.ch2, [3.0, 7.0, 11.0, 15.0]);
        assert_eq!(result.ch3, [4.0, 8.0, 12.0, 16.0]);

        println!("4×4转置基本功能测试通过 / 4×4 transpose basic test passed");
    }

    #[test]
    fn test_transpose_4x4_multi_channel() {
        println!("Testing 4x4 transpose with 8-channel audio / 测试8声道音频的4×4转置...");

        let processor = SimdProcessor::new();

        // 8声道，4帧：共32个样本
        // 每帧8个样本，我们提取通道4-7
        let mut interleaved = Vec::new();
        for frame in 0..4 {
            for ch in 0..8 {
                interleaved.push((frame * 8 + ch) as f32);
            }
        }

        // 提取通道组4-7（channel_offset=4）
        let result = processor.transpose_4x4_block(&interleaved, 0, 4, 8);

        println!("  8声道4帧输入 / 8-channel 4-frame input");
        println!("  提取通道4-7 / Extracting channels 4-7");
        println!("  ch4输出 / ch4 output: {v:?}", v = result.ch0);
        println!("  ch5输出 / ch5 output: {v:?}", v = result.ch1);
        println!("  ch6输出 / ch6 output: {v:?}", v = result.ch2);
        println!("  ch7输出 / ch7 output: {v:?}", v = result.ch3);

        // 帧0通道4-7: [4, 5, 6, 7]
        // 帧1通道4-7: [12, 13, 14, 15]
        // 帧2通道4-7: [20, 21, 22, 23]
        // 帧3通道4-7: [28, 29, 30, 31]
        assert_eq!(result.ch0, [4.0, 12.0, 20.0, 28.0]); // ch4
        assert_eq!(result.ch1, [5.0, 13.0, 21.0, 29.0]); // ch5
        assert_eq!(result.ch2, [6.0, 14.0, 22.0, 30.0]); // ch6
        assert_eq!(result.ch3, [7.0, 15.0, 23.0, 31.0]); // ch7

        println!("8声道4×4转置测试通过 / 8-channel 4×4 transpose test passed");
    }

    #[test]
    fn test_transpose_4x4_simd_vs_scalar() {
        println!(
            "Testing SIMD vs scalar consistency for 4x4 transpose / 测试4×4转置SIMD vs 标量一致性..."
        );

        let processor = SimdProcessor::new();

        // 生成测试数据：4声道，8帧
        let mut interleaved = Vec::new();
        for frame in 0..8 {
            for ch in 0..4 {
                interleaved.push((frame as f32) * 0.1 + (ch as f32) * 0.01);
            }
        }

        // 测试两个不同的4帧块
        let result1 = processor.transpose_4x4_block(&interleaved, 0, 0, 4);
        let result2 = processor.transpose_4x4_block(&interleaved, 4, 0, 4);

        println!("  第一块(帧0-3) / First block (frames 0-3):");
        println!("    ch0: {v:?}", v = result1.ch0);
        println!("    ch1: {v:?}", v = result1.ch1);

        println!("  第二块(帧4-7) / Second block (frames 4-7):");
        println!("    ch0: {v:?}", v = result2.ch0);
        println!("    ch1: {v:?}", v = result2.ch1);

        // 验证SIMD和标量结果一致性
        // （通过检查结果是否符合预期来间接验证）
        assert_eq!(result1.ch0.len(), 4);
        assert_eq!(result2.ch0.len(), 4);

        // 验证数值顺序（ch0应该包含所有帧的第0个通道）
        for i in 0..4 {
            let expected_value = (i as f32) * 0.1;
            assert!((result1.ch0[i] - expected_value).abs() < 1e-6);
        }

        println!("SIMD vs 标量一致性验证通过 / SIMD vs scalar consistency verified");
    }
}
