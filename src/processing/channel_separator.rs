//! 声道样本分离引擎
//!
//! 负责1-2声道音频的高性能样本分离，支持单声道直通和立体声SIMD优化。
//! 结合SSE2/NEON向量化技术，为ProcessingCoordinator提供专业化的技术实现服务。

use super::simd_core::SimdProcessor;

#[cfg(debug_assertions)]
macro_rules! debug_performance {
    ($($arg:tt)*) => {
        eprintln!("[CHANNEL_DEBUG] {}", format_args!($($arg)*));
    };
}

#[cfg(not(debug_assertions))]
macro_rules! debug_performance {
    ($($arg:tt)*) => {};
}

/// 声道样本分离引擎
///
/// 负责1-2声道音频的高性能样本分离：
/// - 单声道：零开销直通
/// - 立体声：SIMD向量化优化
/// - 提供跨平台的SIMD实现(SSE2/NEON)和标量回退
pub struct ChannelSeparator {
    /// SIMD处理器实例
    simd_processor: SimdProcessor,
}

impl ChannelSeparator {
    /// 创建新的立体声分离引擎
    ///
    /// 自动检测硬件SIMD能力并初始化最优配置。
    ///
    /// # 示例
    ///
    /// ```ignore
    /// use macinmeter_dr_tool::processing::ChannelSeparator;
    ///
    /// let separator = ChannelSeparator::new();
    /// println!("SIMD支持: {}", separator.has_simd_support());
    /// ```
    pub fn new() -> Self {
        Self {
            simd_processor: SimdProcessor::new(),
        }
    }

    /// 检查是否支持SIMD加速
    pub fn has_simd_support(&self) -> bool {
        self.simd_processor.capabilities().has_basic_simd()
    }

    /// 获取SIMD处理器能力
    pub fn simd_capabilities(&self) -> &super::simd_core::SimdCapabilities {
        self.simd_processor.capabilities()
    }

    /// 智能样本分离（写入预分配缓冲区，优化内存）
    ///
    /// 根据声道数量自动选择最优分离策略：
    /// - 单声道：零开销直通
    /// - 立体声：SIMD向量化分离
    ///
    /// # 参数
    ///
    /// * `samples` - 交错的音频样本数据
    /// * `channel_idx` - 要提取的声道索引
    /// * `channel_count` - 总声道数量（1或2）
    /// * `output` - 预分配的输出缓冲区（会被清空并填充）
    ///
    /// # 优势
    ///
    /// 相比 `extract_channel_samples_optimized`，此方法避免每次调用都分配新 Vec，
    /// 在循环中复用缓冲区可显著降低内存峰值和分配开销。
    pub fn extract_channel_into(
        &self,
        samples: &[f32],
        channel_idx: usize,
        channel_count: usize,
        output: &mut Vec<f32>,
    ) {
        debug_performance!(
            "智能提取声道{} (into): 总样本={}, 声道数={}",
            channel_idx,
            samples.len(),
            channel_count
        );

        // 清空输出缓冲区，保留容量
        output.clear();

        // 智能优化（单声道和立体声自适应）
        debug_assert!(channel_count <= 2, "ChannelSeparator只应处理1-2声道文件");

        if channel_count == 1 {
            // 单声道：直接复制所有样本
            output.extend_from_slice(samples);
        } else {
            // 立体声：使用SIMD优化
            self.extract_stereo_samples_into(samples, channel_idx, output);
        }
    }

    /// 智能样本分离（自适应单声道/立体声）
    ///
    /// 根据声道数量自动选择最优分离策略：
    /// - 单声道：零开销直通
    /// - 立体声：SIMD向量化分离
    ///
    /// # 参数
    ///
    /// * `samples` - 交错的音频样本数据
    /// * `channel_idx` - 要提取的声道索引
    /// * `channel_count` - 总声道数量（1或2）
    ///
    /// # 返回值
    ///
    /// 返回指定声道的样本数据
    ///
    /// # 实现说明
    ///
    /// 此方法是 `extract_channel_into` 的便捷包裹器，内部分配Vec并调用into版本。
    /// 推荐在循环中使用 `extract_channel_into` 以复用缓冲区，获得更好的内存性能。
    pub fn extract_channel_samples_optimized(
        &self,
        samples: &[f32],
        channel_idx: usize,
        channel_count: usize,
    ) -> Vec<f32> {
        debug_performance!(
            "智能提取声道{} (包裹器): 总样本={}, 声道数={}",
            channel_idx,
            samples.len(),
            channel_count
        );

        // 优化：复用into版本的实现，避免代码重复
        let mut result = Vec::new();
        self.extract_channel_into(samples, channel_idx, channel_count, &mut result);
        result
    }

    /// 立体声样本分离优化入口（写入预分配缓冲区）
    fn extract_stereo_samples_into(
        &self,
        samples: &[f32],
        channel_idx: usize,
        output: &mut Vec<f32>,
    ) {
        self.extract_stereo_samples_simd_into(samples, channel_idx, output);
    }

    /// SSE2优化的立体声样本分离（x86_64专用，写入预分配缓冲区）
    #[cfg(target_arch = "x86_64")]
    fn extract_stereo_samples_simd_into(
        &self,
        samples: &[f32],
        channel_idx: usize,
        output: &mut Vec<f32>,
    ) {
        if !self.simd_processor.capabilities().has_basic_simd() {
            Self::extract_channel_samples_scalar_into(samples, channel_idx, 2, output);
            return;
        }

        let samples_per_channel = samples.len() / 2;
        // 确保输出缓冲区有足够容量
        if output.capacity() < samples_per_channel {
            output.reserve(samples_per_channel - output.capacity());
        }

        // SAFETY: extract_stereo_samples_sse2_unsafe需要SSE2支持，已通过capabilities检查验证。
        // samples生命周期有效，output已预分配容量，函数内部会正确处理数组边界。
        unsafe { self.extract_stereo_samples_sse2_unsafe(samples, channel_idx, output) }

        debug_performance!(
            "SSE2 stereo separation complete (into): extracted {0}=>{1} samples / SSE2立体声分离完成 (into): 提取{0}=>{1}个样本",
            samples.len(),
            output.len()
        );
    }

    /// SSE2立体声样本分离的核心实现（unsafe）
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "sse2")]
    unsafe fn extract_stereo_samples_sse2_unsafe(
        &self,
        samples: &[f32],
        channel_idx: usize,
        result: &mut Vec<f32>,
    ) {
        use std::arch::x86_64::*;

        let len = samples.len();
        let mut i = 0;

        // SSE2批量处理：一次处理8个样本（4对立体声）
        while i + 8 <= len {
            // SAFETY: SSE2向量化立体声声道分离。
            // 前置条件：i + 8 <= len确保有8个有效f32样本（32字节）可读取。
            // _mm_loadu_ps从未对齐内存加载4个f32，两次加载共8个样本。
            // _mm_shuffle_ps是纯SSE2寄存器操作，通过位掩码重排向量元素。
            // _mm_storeu_ps直接写入Vec尾部，避免临时数组开销。
            // result已预分配容量，set_len+直接存储完全安全。
            unsafe {
                // 加载8个样本: [L0, R0, L1, R1, L2, R2, L3, R3]
                let samples1 = _mm_loadu_ps(samples.as_ptr().add(i));
                let samples2 = _mm_loadu_ps(samples.as_ptr().add(i + 4));

                if channel_idx == 0 {
                    // 提取左声道: [L0, L1, L2, L3]
                    // samples1 = [L0, R0, L1, R1], samples2 = [L2, R2, L3, R3]
                    // 使用shuffle提取偶数位置的样本
                    let left1 = _mm_shuffle_ps(samples1, samples1, 0b10_00_10_00); // [L0, L1, L0, L1]
                    let left2 = _mm_shuffle_ps(samples2, samples2, 0b10_00_10_00); // [L2, L3, L2, L3]
                    // 组合成 [L0, L1, L2, L3] - 修复：使用正确的shuffle掩码
                    let final_left = _mm_shuffle_ps(left1, left2, 0b01_00_01_00);

                    // 直接写入Vec尾部（无临时数组）
                    let current = result.len();
                    result.set_len(current + 4);
                    _mm_storeu_ps(result.as_mut_ptr().add(current), final_left);
                } else {
                    // 提取右声道: [R0, R1, R2, R3]
                    // 使用shuffle提取奇数位置的样本
                    let right1 = _mm_shuffle_ps(samples1, samples1, 0b11_01_11_01); // [R0, R1, R0, R1]
                    let right2 = _mm_shuffle_ps(samples2, samples2, 0b11_01_11_01); // [R2, R3, R2, R3]
                    // 组合成 [R0, R1, R2, R3] - 修复：使用正确的shuffle掩码
                    let final_right = _mm_shuffle_ps(right1, right2, 0b01_00_01_00);

                    // 直接写入Vec尾部（无临时数组）
                    let current = result.len();
                    result.set_len(current + 4);
                    _mm_storeu_ps(result.as_mut_ptr().add(current), final_right);
                }
            }

            i += 8;
        }

        // 处理剩余样本（标量方式）
        while i < len {
            if i % 2 == channel_idx {
                result.push(samples[i]);
            }
            i += 1;
        }
    }

    /// ARM NEON优化的立体声样本分离（ARM NEON (aarch64)，写入预分配缓冲区）
    #[cfg(target_arch = "aarch64")]
    fn extract_stereo_samples_simd_into(
        &self,
        samples: &[f32],
        channel_idx: usize,
        output: &mut Vec<f32>,
    ) {
        if !self.simd_processor.capabilities().has_basic_simd() {
            Self::extract_channel_samples_scalar_into(samples, channel_idx, 2, output);
            return;
        }

        let samples_per_channel = samples.len() / 2;
        // 确保输出缓冲区有足够容量
        if output.capacity() < samples_per_channel {
            output.reserve(samples_per_channel - output.capacity());
        }

        // SAFETY: extract_stereo_samples_neon_unsafe需要NEON支持，已通过capabilities检查验证。
        // samples生命周期有效，output已预分配容量，函数内部会正确处理数组边界。
        unsafe { self.extract_stereo_samples_neon_unsafe(samples, channel_idx, output) }

        debug_performance!(
            "NEON stereo separation complete (into): extracted {0}=>{1} samples (ARM NEON aarch64) / NEON立体声分离完成 (into): 提取{0}=>{1}个样本 (ARM NEON aarch64)",
            samples.len(),
            output.len()
        );
    }

    /// ARM NEON立体声样本分离的核心实现（unsafe）
    #[cfg(target_arch = "aarch64")]
    #[target_feature(enable = "neon")]
    unsafe fn extract_stereo_samples_neon_unsafe(
        &self,
        samples: &[f32],
        channel_idx: usize,
        result: &mut Vec<f32>,
    ) {
        use std::arch::aarch64::*;

        let len = samples.len();
        let mut i = 0;

        // NEON批量处理：一次处理8个样本（4对立体声）
        while i + 8 <= len {
            // SAFETY: ARM NEON向量化立体声声道分离。
            // 前置条件：i + 8 <= len确保有8个有效f32样本（32字节）可读取。
            // vld1q_f32从内存加载4个f32到NEON向量，两次加载共8个样本。
            // vuzpq_f32是NEON的unzip指令，高效地将交错数据分离为偶/奇元素。
            // vst1q_f32直接写入Vec尾部，避免临时数组开销。
            // result已预分配容量，set_len+直接存储完全安全。
            unsafe {
                // 加载8个样本: [L0, R0, L1, R1, L2, R2, L3, R3]
                let samples1 = vld1q_f32(samples.as_ptr().add(i)); // [L0,R0,L1,R1]
                let samples2 = vld1q_f32(samples.as_ptr().add(i + 4)); // [L2,R2,L3,R3]

                // 使用NEON的unzip指令解交错
                // deinterleaved.0 = [L0, L1, L2, L3] (偶数位置=左声道)
                // deinterleaved.1 = [R0, R1, R2, R3] (奇数位置=右声道)
                let deinterleaved = vuzpq_f32(samples1, samples2);

                // 根据channel_idx选择对应声道并直接写入Vec尾部
                let channel_data = if channel_idx == 0 {
                    deinterleaved.0 // 左声道
                } else {
                    deinterleaved.1 // 右声道
                };

                let current = result.len();
                result.set_len(current + 4);
                vst1q_f32(result.as_mut_ptr().add(current), channel_data);
            }

            i += 8;
        }

        // 处理剩余样本（标量方式）
        while i < len {
            if i % 2 == channel_idx {
                result.push(samples[i]);
            }
            i += 1;
        }
    }

    /// 其他架构的立体声分离回退实现（写入预分配缓冲区）
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    fn extract_stereo_samples_simd_into(
        &self,
        samples: &[f32],
        channel_idx: usize,
        output: &mut Vec<f32>,
    ) {
        debug_performance!(
            "未支持架构回退到标量实现 (into): arch={}",
            std::env::consts::ARCH
        );
        Self::extract_channel_samples_scalar_into(samples, channel_idx, 2, output);
    }

    /// 标量声道样本分离（写入预分配缓冲区）
    ///
    /// 使用迭代器的高效标量实现，适用于所有平台和声道配置。
    pub fn extract_channel_samples_scalar_into(
        samples: &[f32],
        channel_idx: usize,
        channel_count: usize,
        output: &mut Vec<f32>,
    ) {
        debug_performance!(
            "标量提取声道{} (into): 总样本={}, 声道数={}",
            channel_idx,
            samples.len(),
            channel_count
        );

        // 预估所需容量
        let estimated_capacity = samples.len().div_ceil(channel_count);
        if output.capacity() < estimated_capacity {
            output.reserve(estimated_capacity - output.capacity());
        }

        // 使用 extend 将分离的样本添加到输出缓冲区
        output.extend(
            samples
                .iter()
                .skip(channel_idx)
                .step_by(channel_count)
                .copied(),
        );
    }

    /// 标量声道样本分离（通用回退实现）
    ///
    /// 使用迭代器的高效标量实现，适用于所有平台和声道配置。
    ///
    /// # 实现说明
    ///
    /// 此方法是 `extract_channel_samples_scalar_into` 的便捷包裹器。
    /// 推荐在循环中使用 `*_into` 版本以复用缓冲区。
    pub fn extract_channel_samples_scalar(
        samples: &[f32],
        channel_idx: usize,
        channel_count: usize,
    ) -> Vec<f32> {
        debug_performance!(
            "标量提取声道{} (包裹器): 总样本={}, 声道数={}",
            channel_idx,
            samples.len(),
            channel_count
        );

        // 优化：复用into版本的实现，避免代码重复
        let mut result = Vec::new();
        Self::extract_channel_samples_scalar_into(samples, channel_idx, channel_count, &mut result);
        result
    }
}

impl Default for ChannelSeparator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stereo_extractor_creation() {
        let separator = ChannelSeparator::new();
        println!(
            "Stereo separator SIMD capabilities: {caps:?} / 立体声分离器SIMD能力: {caps:?}",
            caps = separator.simd_capabilities()
        );
    }

    #[test]
    fn test_mono_channel_extraction() {
        let separator = ChannelSeparator::new();
        let samples = vec![0.1, 0.2, 0.3, 0.4, 0.5];

        // 单声道：应该返回全部样本
        let result = separator.extract_channel_samples_optimized(&samples, 0, 1);
        assert_eq!(result, samples);
    }

    #[test]
    fn test_stereo_channel_separation() {
        let separator = ChannelSeparator::new();

        // 立体声测试数据
        let samples = vec![
            0.1, 0.2, // L0, R0
            0.3, 0.4, // L1, R1
            0.5, 0.6, // L2, R2
        ];

        // 提取左声道
        let left = separator.extract_channel_samples_optimized(&samples, 0, 2);
        assert_eq!(left, vec![0.1, 0.3, 0.5]);

        // 提取右声道
        let right = separator.extract_channel_samples_optimized(&samples, 1, 2);
        assert_eq!(right, vec![0.2, 0.4, 0.6]);
    }

    #[test]
    fn test_scalar_vs_simd_consistency() {
        let separator = ChannelSeparator::new();

        // 足够触发SIMD的样本数量
        let mut samples = Vec::new();
        for i in 0..100 {
            samples.push(i as f32); // 左声道
            samples.push((i + 1000) as f32); // 右声道
        }

        // SIMD优化提取
        let simd_left = separator.extract_channel_samples_optimized(&samples, 0, 2);
        let simd_right = separator.extract_channel_samples_optimized(&samples, 1, 2);

        // 标量提取
        let scalar_left = ChannelSeparator::extract_channel_samples_scalar(&samples, 0, 2);
        let scalar_right = ChannelSeparator::extract_channel_samples_scalar(&samples, 1, 2);

        // 验证一致性
        assert_eq!(simd_left.len(), scalar_left.len());
        assert_eq!(simd_right.len(), scalar_right.len());

        for (simd_val, scalar_val) in simd_left.iter().zip(scalar_left.iter()) {
            assert!((simd_val - scalar_val).abs() < 1e-6);
        }

        for (simd_val, scalar_val) in simd_right.iter().zip(scalar_right.iter()) {
            assert!((simd_val - scalar_val).abs() < 1e-6);
        }

        println!(
            "SIMD vs scalar stereo separation consistency verified / SIMD与标量立体声分离一致性验证通过"
        );
    }
}
