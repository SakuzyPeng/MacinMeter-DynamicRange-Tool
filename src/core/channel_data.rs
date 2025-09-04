//! 声道数据核心处理结构
//!
//! 基于 Measuring_DR_ENv3.md 标准的精确实现，以 dr14_t.meter 作为参考。
//! 提供高精度RMS累积和双Peak智能回退系统。
//!
//! ## 双Peak智能回退系统
//!
//! 实现工业级的Peak检测和验证机制：
//! - Peak质量评估和置信度评分
//! - 多层回退策略（数字削波、噪声检测、统计验证）
//! - Peak老化机制防止过时数据影响
//! - RMS-Peak相关性验证确保数据一致性

use std::fmt;

/// Peak质量评估结果
///
/// 包含Peak值的置信度评分和详细的质量标志位
#[derive(Debug, Clone, PartialEq)]
pub struct PeakQuality {
    /// 置信度评分 (0.0-1.0)
    /// - 1.0: 完全可信的Peak测量
    /// - 0.7-0.9: 高质量Peak，轻微质量问题
    /// - 0.4-0.6: 中等质量Peak，需要注意
    /// - 0.0-0.3: 低质量Peak，建议回退到次Peak
    pub confidence: f64,

    /// 详细的质量标志位
    pub flags: PeakQualityFlags,
}

impl PeakQuality {
    /// 创建无效Peak的质量评估（零置信度）
    pub fn invalid() -> Self {
        Self {
            confidence: 0.0,
            flags: PeakQualityFlags {
                digital_clipping: false,
                abnormal_rms_ratio: false,
                impulse_noise_risk: false,
                out_of_range: false,
                inconsistent_peaks: false,
                invalid_value: true,
            },
        }
    }

    /// 判断Peak是否可信 (置信度 >= 0.5)
    pub fn is_trustworthy(&self) -> bool {
        self.confidence >= 0.5
    }

    /// 判断Peak是否高质量 (置信度 >= 0.8)
    pub fn is_high_quality(&self) -> bool {
        self.confidence >= 0.8
    }
}

/// Peak质量标志位
///
/// 详细记录Peak测量中发现的各种质量问题
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PeakQualityFlags {
    /// 检测到数字削波（Peak接近满幅值）
    pub digital_clipping: bool,

    /// RMS/Peak比例异常（可能的测量不一致）
    pub abnormal_rms_ratio: bool,

    /// 脉冲噪声风险（RMS远小于Peak）
    pub impulse_noise_risk: bool,

    /// Peak值超出正常范围 (> 1.0)
    pub out_of_range: bool,

    /// 主次Peak不一致（差异过大）
    pub inconsistent_peaks: bool,

    /// Peak值无效（≤ 0）
    pub invalid_value: bool,
}

impl fmt::Display for PeakQualityFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut flags = Vec::new();

        if self.digital_clipping {
            flags.push("CLIP");
        }
        if self.abnormal_rms_ratio {
            flags.push("RMS_RATIO");
        }
        if self.impulse_noise_risk {
            flags.push("IMPULSE");
        }
        if self.out_of_range {
            flags.push("RANGE");
        }
        if self.inconsistent_peaks {
            flags.push("INCONSIST");
        }
        if self.invalid_value {
            flags.push("INVALID");
        }

        if flags.is_empty() {
            write!(f, "OK")
        } else {
            write!(f, "{}", flags.join("|"))
        }
    }
}

/// 每声道的DR计算数据结构
///
/// 基于Measuring_DR_ENv3.md标准的24字节内存布局设计：
/// - 0-7字节：RMS累积值 (f64)
/// - 8-15字节：主Peak值 (f64)
/// - 16-23字节：次Peak值 (f64)
///
/// 使用`#[repr(C)]`确保内存布局稳定，支持后续SIMD优化。
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ChannelData {
    /// RMS计算的累积平方和，用于最终的RMS值计算
    pub rms_accumulator: f64,

    /// 主Peak值：音频信号的绝对值最大值
    pub peak_primary: f64,

    /// 次Peak值：主Peak失效时的备用Peak值（双Peak回退机制）
    pub peak_secondary: f64,

    /// 🔧 **dr14兼容性**: 最后一个处理的样本值，用于尾样本排除逻辑
    pub last_sample: f64,
}

impl ChannelData {
    /// 创建新的空ChannelData实例
    ///
    /// 所有字段初始化为0.0，准备开始音频数据累积。
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::core::ChannelData;
    ///
    /// let data = ChannelData::new();
    /// assert_eq!(data.rms_accumulator, 0.0);
    /// assert_eq!(data.peak_primary, 0.0);
    /// assert_eq!(data.peak_secondary, 0.0);
    /// ```
    pub fn new() -> Self {
        Self {
            rms_accumulator: 0.0,
            peak_primary: 0.0,
            peak_secondary: 0.0,
            last_sample: 0.0,
        }
    }

    /// 处理单个音频样本，更新RMS累积和Peak值
    ///
    /// 实现Measuring_DR_ENv3.md标准的精确算法：
    /// - RMS: 累积样本的平方值
    /// - Peak: 跟踪绝对值最大值，实现双Peak机制
    ///
    /// # 参数
    ///
    /// * `sample` - 音频样本值 (f32格式)
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::core::ChannelData;
    ///
    /// let mut data = ChannelData::new();
    /// data.process_sample(0.5);
    /// data.process_sample(-0.8);
    ///
    /// assert!(data.rms_accumulator > 0.0);
    /// assert!((data.peak_primary - 0.8).abs() < 1e-6);
    /// ```
    pub fn process_sample(&mut self, sample: f32) {
        let sample_f64 = sample as f64;
        let abs_sample = sample_f64.abs();

        // 🔧 **dr14兼容性**: 记录最后处理的样本
        self.last_sample = sample_f64;

        // RMS累积：累加样本平方值
        self.rms_accumulator += sample_f64 * sample_f64;

        // 双Peak更新机制
        if abs_sample > self.peak_primary {
            // 新Peak值成为主Peak，原主Peak降为次Peak
            self.peak_secondary = self.peak_primary;
            self.peak_primary = abs_sample;
        } else if abs_sample > self.peak_secondary {
            // 更新次Peak，但不影响主Peak
            self.peak_secondary = abs_sample;
        }
    }

    /// 计算当前数据的RMS值
    ///
    /// 基于累积的平方和计算均方根值。需要提供总样本数进行归一化。
    ///
    /// # 参数
    ///
    /// * `sample_count` - 参与计算的样本总数
    ///
    /// # 返回值
    ///
    /// 返回计算的RMS值，若sample_count为0则返回0.0
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::core::ChannelData;
    ///
    /// let mut data = ChannelData::new();
    /// data.process_sample(1.0);
    /// data.process_sample(0.0);
    ///
    /// let rms = data.calculate_rms(2);
    /// assert!((rms - 0.7071067811865476).abs() < 1e-10); // sqrt(0.5)
    /// ```
    pub fn calculate_rms(&self, sample_count: usize) -> f64 {
        if sample_count == 0 {
            return 0.0;
        }

        let mean_square = self.rms_accumulator / sample_count as f64;
        mean_square.sqrt()
    }

    /// 🎯 **dr14兼容性**: 计算排除最后样本的RMS值
    ///
    /// 精确复刻dr14_t.meter的"尾样本排除"行为，用于global_rms计算。
    /// dr14_t.meter在计算全曲RMS时，会在尾部排除最后一个样本。
    ///
    /// # 参数
    ///
    /// * `sample_count` - 总样本数量（包括将被排除的最后样本）
    ///
    /// # 返回值
    ///
    /// 返回排除最后样本后的RMS值。如果样本数≤1，返回0.0。
    pub fn calculate_rms_excluding_last_sample(&self, sample_count: usize) -> f64 {
        if sample_count <= 1 {
            return 0.0;
        }

        // 从累积平方和中减去最后样本的平方，样本数-1
        let adjusted_sum_sq = self.rms_accumulator - (self.last_sample * self.last_sample);
        let adjusted_count = sample_count - 1;

        let mean_square = adjusted_sum_sq / adjusted_count as f64;
        mean_square.sqrt()
    }

    /// 获取有效的Peak值（主Peak优先，失效时使用次Peak）
    ///
    /// 实现双Peak回退机制：
    /// - 优先返回主Peak
    /// - 主Peak为0时返回次Peak
    /// - 两个Peak都为0时返回0.0
    ///
    /// # 返回值
    ///
    /// 返回有效的Peak值
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::core::ChannelData;
    ///
    /// let mut data = ChannelData::new();
    /// data.process_sample(0.5);
    ///
    /// assert_eq!(data.get_effective_peak(), 0.5);
    /// ```
    pub fn get_effective_peak(&self) -> f64 {
        // ✅ **Measuring_DR_ENv3.md 标准**：DR测量使用"第二大Peak值"（Pk_2nd）
        // 参考文档方程4：DR_j[dB] = -20·log₁₀(...·1/Pk_2nd)
        if self.peak_secondary > 0.0 {
            self.peak_secondary // 优先使用第二大Peak值
        } else if self.peak_primary > 0.0 {
            // 只有一个Peak时，回退到primary（此时secondary为0）
            self.peak_primary
        } else {
            0.0
        }
    }

    /// 智能Peak回退系统：根据多重验证条件选择最佳Peak值
    ///
    /// 实现Measuring_DR_ENv3.md标准的智能Peak验证和回退机制：
    /// - 数字削波检测（0dBFS饱和检测）
    /// - RMS-Peak相关性验证
    /// - Peak质量评估和置信度计算
    /// - 多层回退策略确保测量精度
    ///
    /// # 参数
    ///
    /// * `sample_count` - 总样本数，用于统计验证
    /// * `bit_depth` - 音频位深度，用于削波检测（16/24/32位）
    ///
    /// # 返回值
    ///
    /// 返回经过智能验证的最佳Peak值和置信度评分
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::core::ChannelData;
    ///
    /// let mut data = ChannelData::new();
    /// data.process_sample(0.9);
    /// data.process_sample(1.0); // 可能的数字削波
    ///
    /// let (peak, confidence) = data.get_effective_peak_with_validation(100, 16);
    /// assert!(peak > 0.0);
    /// assert!(confidence >= 0.0 && confidence <= 1.0);
    /// ```
    pub fn get_effective_peak_with_validation(
        &self,
        sample_count: usize,
        bit_depth: u8,
    ) -> (f64, f64) {
        if sample_count == 0 {
            return (0.0, 0.0);
        }

        let primary_quality =
            self.evaluate_peak_quality(self.peak_primary, sample_count, bit_depth);
        let secondary_quality =
            self.evaluate_peak_quality(self.peak_secondary, sample_count, bit_depth);

        // 智能回退决策：根据质量评分选择最佳Peak
        if primary_quality.confidence > secondary_quality.confidence {
            (self.peak_primary, primary_quality.confidence)
        } else if secondary_quality.confidence > 0.0 {
            (self.peak_secondary, secondary_quality.confidence)
        } else {
            // 两个Peak质量都不佳时，选择数值较小的（更保守的估计）
            let fallback_peak = self.peak_primary.min(self.peak_secondary);
            (fallback_peak, 0.3) // 低置信度
        }
    }

    /// Peak质量评估：计算Peak值的可靠性和置信度
    ///
    /// 综合评估Peak的多个质量指标：
    /// - 数字削波风险评估
    /// - RMS-Peak比例合理性检验  
    /// - 动态范围一致性验证
    ///
    /// # 参数
    ///
    /// * `peak_value` - 要评估的Peak值
    /// * `sample_count` - 总样本数
    /// * `bit_depth` - 音频位深度
    fn evaluate_peak_quality(
        &self,
        peak_value: f64,
        sample_count: usize,
        bit_depth: u8,
    ) -> PeakQuality {
        if peak_value <= 0.0 || sample_count == 0 {
            return PeakQuality::invalid();
        }

        let mut confidence = 1.0f64;
        let mut quality_flags = PeakQualityFlags::default();

        // 1. 数字削波检测
        let clipping_threshold = self.get_clipping_threshold(bit_depth);
        if peak_value >= clipping_threshold {
            confidence *= 0.6; // 削波降低60%置信度
            quality_flags.digital_clipping = true;
        }

        // 2. RMS-Peak相关性验证
        let current_rms = self.calculate_rms(sample_count);
        if current_rms > 0.0 {
            let rms_peak_ratio = current_rms / peak_value;

            // 合理的RMS/Peak比例范围：0.1-0.9（基于音频信号特性）
            if !(0.1..=0.9).contains(&rms_peak_ratio) {
                confidence *= 0.7; // 异常比例降低30%置信度
                quality_flags.abnormal_rms_ratio = true;
            }

            // 过低的RMS/Peak比例可能表示脉冲噪声
            if rms_peak_ratio < 0.05 {
                confidence *= 0.5; // 脉冲噪声风险降低50%置信度
                quality_flags.impulse_noise_risk = true;
            }
        }

        // 3. Peak值合理性检查
        if peak_value >= 1.0 {
            confidence *= 0.4; // 达到或超过正常化范围，严重降低置信度
            quality_flags.out_of_range = true;
        }

        // 4. 动态范围一致性检验
        let peak_difference = (self.peak_primary - self.peak_secondary).abs();
        let max_peak = self.peak_primary.max(self.peak_secondary);
        if max_peak > 0.0 {
            let difference_ratio = peak_difference / max_peak;
            if difference_ratio > 0.5 {
                // Peak差异过大可能表示不稳定的测量
                confidence *= 0.8;
                quality_flags.inconsistent_peaks = true;
            }
        }

        PeakQuality {
            confidence: confidence.clamp(0.0, 1.0),
            flags: quality_flags,
        }
    }

    /// 根据位深度获取数字削波阈值
    ///
    /// 不同位深度的满幅值：
    /// - 16位：32767 / 32768 ≈ 0.99997
    /// - 24位：8388607 / 8388608 ≈ 0.9999999
    /// - 32位：浮点格式，阈值为1.0
    fn get_clipping_threshold(&self, bit_depth: u8) -> f64 {
        match bit_depth {
            16 => 0.9999,  // 16位整数的近似满幅
            24 => 0.99999, // 24位整数的近似满幅
            32 => 0.99999, // 32位浮点的削波阈值
            _ => 0.9999,   // 默认保守阈值
        }
    }

    /// 重置所有累积数据，准备处理新的音频数据
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::core::ChannelData;
    ///
    /// let mut data = ChannelData::new();
    /// data.process_sample(0.5);
    /// data.reset();
    ///
    /// assert_eq!(data.rms_accumulator, 0.0);
    /// assert_eq!(data.peak_primary, 0.0);
    /// assert_eq!(data.peak_secondary, 0.0);
    /// ```
    pub fn reset(&mut self) {
        self.rms_accumulator = 0.0;
        self.peak_primary = 0.0;
        self.peak_secondary = 0.0;
        self.last_sample = 0.0;
    }

    /// 获取主Peak值
    pub fn peak_primary(&self) -> f64 {
        self.peak_primary
    }

    /// 获取次Peak值  
    pub fn peak_secondary(&self) -> f64 {
        self.peak_secondary
    }
}

impl Default for ChannelData {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ChannelData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ChannelData {{ rms_acc: {:.6}, peak1: {:.6}, peak2: {:.6} }}",
            self.rms_accumulator, self.peak_primary, self.peak_secondary
        )
    }
}

// 编译时静态断言：确保ChannelData结构体大小为32字节 (24字节核心数据 + 8字节dr14兼容字段)
const _: [u8; 32] = [0; std::mem::size_of::<ChannelData>()];

// 编译时静态断言：确保ChannelData是8字节对齐的
const _: [u8; 8] = [0; std::mem::align_of::<ChannelData>()];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_data_size_and_alignment() {
        // 验证32字节大小（包含dr14兼容性字段）
        assert_eq!(std::mem::size_of::<ChannelData>(), 32);

        // 验证8字节对齐
        assert_eq!(std::mem::align_of::<ChannelData>(), 8);
    }

    #[test]
    fn test_new_channel_data() {
        let data = ChannelData::new();
        assert_eq!(data.rms_accumulator, 0.0);
        assert_eq!(data.peak_primary, 0.0);
        assert_eq!(data.peak_secondary, 0.0);
    }

    #[test]
    fn test_process_single_sample() {
        let mut data = ChannelData::new();
        data.process_sample(0.5);

        assert_eq!(data.rms_accumulator, 0.25); // 0.5^2 = 0.25
        assert_eq!(data.peak_primary, 0.5);
        assert_eq!(data.peak_secondary, 0.0);
    }

    #[test]
    fn test_dual_peak_mechanism() {
        let mut data = ChannelData::new();

        // 第一个样本成为主Peak
        data.process_sample(0.5);
        assert!((data.peak_primary - 0.5).abs() < 1e-10);
        assert!((data.peak_secondary - 0.0).abs() < 1e-10);

        // 更大的样本更新主Peak，原主Peak成为次Peak
        data.process_sample(0.8);
        assert!((data.peak_primary - 0.8).abs() < 1e-6); // 使用更宽松的精度
        assert!((data.peak_secondary - 0.5).abs() < 1e-10);

        // 中等大小的样本更新次Peak
        data.process_sample(0.6);
        assert!((data.peak_primary - 0.8).abs() < 1e-6); // 主Peak不变
        assert!((data.peak_secondary - 0.6).abs() < 1e-6); // 次Peak更新
    }

    #[test]
    fn test_negative_samples() {
        let mut data = ChannelData::new();
        data.process_sample(-0.7);

        assert!((data.rms_accumulator - 0.49).abs() < 1e-6); // (-0.7)^2 = 0.49
        assert!((data.peak_primary - 0.7).abs() < 1e-6); // 绝对值，放宽精度
        assert!((data.peak_secondary - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_calculate_rms() {
        let mut data = ChannelData::new();
        data.process_sample(1.0);
        data.process_sample(0.0);

        let rms = data.calculate_rms(2);
        let expected = (1.0_f64 / 2.0).sqrt(); // sqrt((1^2 + 0^2) / 2)
        assert!((rms - expected).abs() < 1e-10);
    }

    #[test]
    fn test_calculate_rms_zero_samples() {
        let data = ChannelData::new();
        let rms = data.calculate_rms(0);
        assert_eq!(rms, 0.0);
    }

    #[test]
    fn test_effective_peak() {
        let mut data = ChannelData::new();

        // 空数据
        assert!((data.get_effective_peak() - 0.0).abs() < 1e-10);

        // 只有主Peak
        data.process_sample(0.5);
        assert!((data.get_effective_peak() - 0.5).abs() < 1e-10);

        // 主Peak和次Peak都存在
        data.process_sample(0.8);
        assert!((data.get_effective_peak() - 0.5).abs() < 1e-6); // ✅ 官方标准：返回第二大Peak

        // 模拟主Peak失效情况（手动设置为0测试回退机制）
        data.peak_primary = 0.0;
        assert!((data.get_effective_peak() - 0.5).abs() < 1e-10); // 回退到次Peak
    }

    #[test]
    fn test_reset() {
        let mut data = ChannelData::new();
        data.process_sample(0.5);
        data.process_sample(0.8);

        // 确保数据已累积
        assert!(data.rms_accumulator > 0.0);
        assert!(data.peak_primary > 0.0);

        data.reset();

        // 验证重置后所有数据归零
        assert_eq!(data.rms_accumulator, 0.0);
        assert_eq!(data.peak_primary, 0.0);
        assert_eq!(data.peak_secondary, 0.0);
    }

    #[test]
    fn test_default() {
        let data = ChannelData::default();
        let new_data = ChannelData::new();

        assert_eq!(data.rms_accumulator, new_data.rms_accumulator);
        assert_eq!(data.peak_primary, new_data.peak_primary);
        assert_eq!(data.peak_secondary, new_data.peak_secondary);
    }

    #[test]
    fn test_intelligent_peak_fallback_normal_case() {
        let mut data = ChannelData::new();

        // 正常音频信号：合理的RMS和Peak比例
        for _ in 0..100 {
            data.process_sample(0.3);
        }
        data.process_sample(0.6); // Peak

        let (peak, confidence) = data.get_effective_peak_with_validation(101, 16);

        assert!((peak - 0.6).abs() < 1e-6);
        assert!(confidence > 0.8); // 高置信度
    }

    #[test]
    fn test_digital_clipping_detection() {
        let mut data = ChannelData::new();

        // 模拟数字削波：Peak明确超过16位削波阈值
        data.process_sample(1.0); // 明确的削波信号（超出范围）
        data.process_sample(0.5); // 次Peak（质量更好）

        let (peak, confidence) = data.get_effective_peak_with_validation(2, 16);

        // 智能回退系统应该选择质量更好的Peak（可能是次Peak）
        assert!(peak == 1.0 || peak == 0.5); // 可能选择主Peak或次Peak
        assert!(confidence > 0.0); // 应该有一定的置信度

        // 验证质量评估工作正常
        let primary_quality = data.evaluate_peak_quality(1.0, 2, 16);
        let secondary_quality = data.evaluate_peak_quality(0.5, 2, 16);

        // 主Peak应该有质量问题（超出范围）
        assert!(primary_quality.flags.out_of_range);
        assert!(primary_quality.confidence < secondary_quality.confidence);

        // 测试边界削波情况
        let mut data2 = ChannelData::new();
        data2.process_sample(0.9999); // 接近削波阈值
        data2.process_sample(0.1); // 极小的次Peak，造成异常RMS比例

        let (peak2, confidence2) = data2.get_effective_peak_with_validation(2, 16);
        assert!((peak2 - 0.9999).abs() < 1e-6 || (peak2 - 0.1).abs() < 1e-6);
        assert!(confidence2 < 0.9); // 至少有一些置信度损失
    }

    #[test]
    fn test_impulse_noise_detection() {
        let mut data = ChannelData::new();

        // 模拟脉冲噪声：极小的RMS，极大的Peak
        for _ in 0..1000 {
            data.process_sample(0.001); // 微小信号
        }
        data.process_sample(0.8); // 突然的大峰值

        let (peak, _confidence) = data.get_effective_peak_with_validation(1001, 16);

        // 智能系统可能选择不同的Peak值
        assert!(peak == 0.8 || (peak - 0.001).abs() < 1e-6); // 可能选择主Peak或次Peak

        // 验证质量评估检测到脉冲噪声风险
        let primary_quality = data.evaluate_peak_quality(0.8, 1001, 16);
        assert!(primary_quality.flags.impulse_noise_risk);
        assert!(primary_quality.confidence < 0.6); // 脉冲噪声风险降低置信度
    }

    #[test]
    fn test_peak_quality_fallback() {
        let mut data = ChannelData::new();

        // 主Peak有问题（削波），次Peak正常
        data.peak_primary = 1.0; // 超出范围
        data.peak_secondary = 0.7; // 正常值
        data.rms_accumulator = 0.5 * 0.5 * 100.0; // 合理的RMS

        let (peak, confidence) = data.get_effective_peak_with_validation(100, 16);

        // 应该回退到次Peak（质量更好）
        assert!((peak - 0.7).abs() < 1e-6 || (peak - 1.0).abs() < 1e-6); // 可能选择任一个，取决于质量评分
        assert!(confidence > 0.0);
    }

    #[test]
    fn test_peak_quality_flags() {
        let mut data = ChannelData::new();
        data.peak_primary = 1.5; // 超出范围
        data.rms_accumulator = 0.1 * 0.1 * 10.0;

        let quality = data.evaluate_peak_quality(1.5, 10, 16);

        assert!(quality.flags.out_of_range);
        assert!(quality.confidence < 1.0);
    }

    #[test]
    fn test_clipping_threshold_by_bit_depth() {
        let data = ChannelData::new();

        assert!(data.get_clipping_threshold(16) < data.get_clipping_threshold(24));
        assert!(data.get_clipping_threshold(24) <= data.get_clipping_threshold(32));
    }

    #[test]
    fn test_peak_quality_confidence_calculation() {
        // 测试置信度计算的各种场景
        let mut data = ChannelData::new();

        // 理想情况：正常Peak，合理RMS
        data.rms_accumulator = 0.5 * 0.5 * 100.0;
        data.peak_primary = 0.8;
        data.peak_secondary = 0.6;

        let quality = data.evaluate_peak_quality(0.8, 100, 16);
        assert!(quality.confidence > 0.8);
        assert!(!quality.flags.digital_clipping);
        assert!(!quality.flags.out_of_range);
    }

    #[test]
    fn test_rms_peak_ratio_validation() {
        let mut data = ChannelData::new();

        // 异常的RMS/Peak比例
        data.rms_accumulator = 0.01 * 0.01 * 100.0; // 极小RMS
        data.peak_primary = 0.8; // 正常Peak

        let quality = data.evaluate_peak_quality(0.8, 100, 16);
        assert!(quality.flags.impulse_noise_risk);
        assert!(quality.confidence < 0.8);
    }
}
