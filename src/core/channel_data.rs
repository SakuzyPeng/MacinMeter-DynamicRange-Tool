//! 24字节ChannelData核心数据结构
//!
//! 基于foobar2000 DR Meter逆向分析的精确实现，确保内存布局一致性。
//!
//! ## 双Peak智能回退系统
//!
//! 实现foobar2000兼容的Peak检测机制：
//! - 主Peak和次Peak的双轨跟踪
//! - 智能Peak选择算法（优先次Peak以抗尖峰干扰）
//! - 基于foobar2000反汇编分析的峰值策略

use std::fmt;

// SSE2 intrinsics仅在x86_64上可用
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::{_mm_cvtsd_f64, _mm_set_pd, _mm_sqrt_pd};

/// foobar2000兼容的SSE平方根计算
/// 🔥 关键精度修复：使用与foobar2000相同的SSE2 _mm_sqrt_pd指令
///
/// 注意：在非x86_64架构上自动回退到标准sqrt()
#[cfg(target_arch = "x86_64")]
#[inline]
fn foobar2000_sse_sqrt(value: f64) -> f64 {
    unsafe {
        let packed = _mm_set_pd(0.0, value);
        let result = _mm_sqrt_pd(packed);
        _mm_cvtsd_f64(result)
    }
}

/// 标量平方根计算（非x86_64架构的回退实现）
#[cfg(not(target_arch = "x86_64"))]
#[inline]
fn foobar2000_sse_sqrt(value: f64) -> f64 {
    value.sqrt()
}

/// 每声道的DR计算数据结构
///
/// 严格按照foobar2000 DR Meter的24字节内存布局设计：
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
        }
    }

    /// 处理单个音频样本，更新RMS累积和Peak值
    ///
    /// 实现foobar2000的精确算法：
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

        // RMS累积：累加样本平方值
        self.rms_accumulator += sample_f64 * sample_f64;

        // 🔥 关键修正：实现foobar2000的严格峰值更新条件
        // 📖 反汇编发现：if (v16 > second_peak && v16 < max_peak)
        if abs_sample > self.peak_primary {
            // 新Peak值成为主Peak，原主Peak降为次Peak
            self.peak_secondary = self.peak_primary;
            self.peak_primary = abs_sample;
        } else if abs_sample > self.peak_secondary && abs_sample < self.peak_primary {
            // ✅ foobar2000严格条件：必须同时满足 > second_peak AND < max_peak
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

        // 🔥 数据类型转换链修复：先转int再转double (与foobar2000一致)
        // 📖 对应汇编: (double)*(int *)(a1 + 20)
        let sample_count_int = sample_count as i32;
        let sample_count_f64 = sample_count_int as f64;
        let mean_square = self.rms_accumulator / sample_count_f64;

        // 🔥 关键精度修复：使用foobar2000相同的SSE平方根
        // 📖 对应汇编: *(_QWORD *)&v46 = *(_OWORD *)&_mm_sqrt_pd(v43);
        foobar2000_sse_sqrt(mean_square)
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
        // 🎯 CORRECT: foobar2000 Peak选择的真实逻辑
        // 核心原则：只要主Peak不削波就选主Peak，削波时才用次Peak

        // 步骤1：检查主Peak是否削波（达到或接近1.0）
        const CLIPPING_THRESHOLD: f64 = 1.0 - 1e-6; // 允许微小的数值误差

        if self.peak_primary > 0.0 && self.peak_primary < CLIPPING_THRESHOLD {
            // 主Peak未削波，直接使用
            self.peak_primary
        } else if self.peak_secondary > 0.0 {
            // 主Peak削波或无效，回退到次Peak
            self.peak_secondary
        } else {
            // 兜底策略：如果次Peak也无效，仍然使用主Peak
            self.peak_primary.max(0.0)
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

// 编译时静态断言：确保ChannelData结构体大小为24字节
const _: [u8; 24] = [0; std::mem::size_of::<ChannelData>()];

// 编译时静态断言：确保ChannelData是8字节对齐的
const _: [u8; 8] = [0; std::mem::align_of::<ChannelData>()];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_data_size_and_alignment() {
        // 验证24字节大小
        assert_eq!(std::mem::size_of::<ChannelData>(), 24);

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
        // 🎯 CORRECT: 削波逻辑 - 主Peak=0.8未削波，应该返回主Peak
        assert!((data.get_effective_peak() - 0.8).abs() < 1e-6); // 返回主Peak（未削波）

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
}
