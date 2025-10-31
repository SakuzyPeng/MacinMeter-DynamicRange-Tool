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
/// 关键精度修复：使用与foobar2000相同的SSE2 _mm_sqrt_pd指令
///
/// 注意：在非x86_64架构上自动回退到标准sqrt()
#[cfg(target_arch = "x86_64")]
#[inline]
fn foobar2000_sse_sqrt(value: f64) -> f64 {
    // SAFETY: SSE2平方根计算，与foobar2000行为完全一致。
    // _mm_set_pd将value打包到SSE向量的低64位（高64位填0）。
    // _mm_sqrt_pd计算向量中两个f64的平方根（仅使用低64位结果）。
    // _mm_cvtsd_f64提取低64位的f64结果到标量。
    // 所有操作都是纯SSE2寄存器操作，无内存访问，完全安全。
    // 此实现复现foobar2000的SSE sqrt精度特性，确保DR计算结果一致。
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

    /// 主Peak值：样本的最大绝对值
    pub peak_primary: f64,

    /// 次Peak值：第二大绝对值，用于削波容错
    pub peak_secondary: f64,
}

impl ChannelData {
    /// 创建新的空ChannelData实例
    ///
    /// 所有累积值初始化为0.0，符合foobar2000标准。
    ///
    /// # 示例
    ///
    /// ```ignore
    /// use macinmeter_dr_tool::processing::ChannelData;
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

    /// 处理单个音频样本
    ///
    /// 这是核心的样本级处理函数，更新RMS累积值和双Peak跟踪：
    /// 1. 累积样本的平方到RMS accumulator
    /// 2. 更新Primary Peak（如果当前样本更大）
    /// 3. 智能更新Secondary Peak（保持第二大值）
    ///
    /// # 参数
    ///
    /// * `sample` - 单个音频样本值（f32格式）
    ///
    /// # 示例
    ///
    /// ```ignore
    /// use macinmeter_dr_tool::processing::ChannelData;
    ///
    /// let mut data = ChannelData::new();
    /// data.process_sample(0.5);
    /// data.process_sample(0.8);
    ///
    /// assert!((data.peak_primary - 0.8).abs() < 1e-5);
    /// assert!((data.peak_secondary - 0.5).abs() < 1e-5);
    /// ```
    #[inline]
    pub fn process_sample(&mut self, sample: f32) {
        let sample_f64 = sample as f64;

        // 累积RMS平方和
        self.rms_accumulator += sample_f64 * sample_f64;

        // 使用绝对值进行Peak检测
        let abs_sample = sample_f64.abs();

        // 双Peak智能更新逻辑
        if abs_sample > self.peak_primary {
            // 新的最大Peak：当前Primary降为Secondary
            self.peak_secondary = self.peak_primary;
            self.peak_primary = abs_sample;
        } else if abs_sample > self.peak_secondary {
            // 新的第二大Peak：只更新Secondary
            self.peak_secondary = abs_sample;
        }
        // 如果abs_sample <= secondary，则不更新（保持现有Peak值）
    }

    /// 计算最终的RMS值
    ///
    /// 使用累积的平方和计算RMS，应用foobar2000兼容的SSE平方根计算。
    ///
    /// **重要**：调用此方法前必须确保已处理了正确数量的样本。
    ///
    /// # 参数
    ///
    /// * `sample_count` - 已处理的样本总数，用于平均化
    ///
    /// # 返回值
    ///
    /// 返回计算得到的RMS值
    ///
    /// # 示例
    ///
    /// ```ignore
    /// use macinmeter_dr_tool::processing::ChannelData;
    ///
    /// let mut data = ChannelData::new();
    /// data.process_sample(0.5);
    /// data.process_sample(0.5);
    ///
    /// let rms = data.calculate_rms(2);
    /// assert!((rms - 0.5).abs() < 1e-10);
    /// ```
    #[inline]
    pub fn calculate_rms(&self, sample_count: usize) -> f64 {
        if sample_count == 0 {
            return 0.0;
        }

        // 计算平均平方值
        let mean_square = self.rms_accumulator / (sample_count as f64);

        // 使用foobar2000兼容的SSE平方根计算
        if mean_square <= 0.0 {
            0.0
        } else {
            foobar2000_sse_sqrt(mean_square)
        }
    }

    /// 获取有效峰值（返回备选峰值，不做最终选择）
    ///
    /// **重要**：此方法仅返回"次峰（如果可用）或主峰"的组合，
    /// **不应直接用于 DR 计算**。DR 计算应通过 `PeakSelectionStrategy::select_peak()` 进行。
    ///
    /// # 实现说明
    ///
    /// 本方法硬编码了 PreferSecondary 逻辑：
    /// - 返回次Peak（如果 > 0.0）
    /// - 否则返回主Peak
    ///
    /// 这仅是为了保留与历史代码的兼容性。**新代码不应依赖此逻辑**。
    ///
    /// # 正确用法
    ///
    /// ```ignore
    /// use macinmeter_dr_tool::{processing::ChannelData, core::peak_selection::PeakSelectionStrategy};
    ///
    /// let mut data = ChannelData::new();
    /// data.process_sample(1.0);   // 主Peak
    /// data.process_sample(0.8);   // 次Peak
    ///
    /// // 正确：通过策略选择
    /// let strategy = PeakSelectionStrategy::default();
    /// let peak = strategy.select_peak(data.peak_primary, data.peak_secondary);
    /// assert!((peak - 0.8).abs() < 1e-5);
    ///
    /// // 不推荐：直接调用 get_effective_peak()
    /// // let peak = data.get_effective_peak();  // 避免使用
    /// ```
    ///
    /// # 返回值
    ///
    /// 返回 max(secondary_peak, primary_peak)（当 secondary > 0 时）或主 Peak
    #[inline]
    pub fn get_effective_peak(&self) -> f64 {
        // 硬编码的 PreferSecondary 逻辑（仅用于兼容，不应在新代码中依赖）
        if self.peak_secondary > 0.0 {
            self.peak_secondary
        } else {
            self.peak_primary
        }
    }

    /// 重置所有累积值
    ///
    /// 将RMS累积值和双Peak值重置为0.0，准备下一轮计算。
    ///
    /// # 示例
    ///
    /// ```ignore
    /// use macinmeter_dr_tool::processing::ChannelData;
    ///
    /// let mut data = ChannelData::new();
    /// data.process_sample(0.5);
    /// data.reset();
    ///
    /// assert_eq!(data.rms_accumulator, 0.0);
    /// assert_eq!(data.peak_primary, 0.0);
    /// ```
    #[inline]
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
    fn test_memory_layout() {
        assert_eq!(std::mem::size_of::<ChannelData>(), 24);
        // 确保8字节对齐（适配SIMD要求）
        assert_eq!(std::mem::align_of::<ChannelData>(), 8);
    }

    #[test]
    fn test_new() {
        let data = ChannelData::new();
        assert_eq!(data.rms_accumulator, 0.0);
        assert_eq!(data.peak_primary, 0.0);
        assert_eq!(data.peak_secondary, 0.0);
    }

    #[test]
    fn test_process_sample() {
        let mut data = ChannelData::new();

        data.process_sample(0.5);
        assert_eq!(data.rms_accumulator, 0.25);
        assert_eq!(data.peak_primary, 0.5);
        assert_eq!(data.peak_secondary, 0.0);
    }

    #[test]
    fn test_dual_peak_system() {
        let mut data = ChannelData::new();

        // 第一个样本成为主Peak
        data.process_sample(0.6);
        assert!((data.peak_primary - 0.6).abs() < 1e-5);
        assert_eq!(data.peak_secondary, 0.0);

        // 更大样本：旧主Peak降为次Peak
        data.process_sample(0.8);
        assert!((data.peak_primary - 0.8).abs() < 1e-5);
        assert!((data.peak_secondary - 0.6).abs() < 1e-5);

        // 小样本：不影响Peak值
        data.process_sample(0.3);
        assert!((data.peak_primary - 0.8).abs() < 1e-5);
        assert!((data.peak_secondary - 0.6).abs() < 1e-5);
    }

    #[test]
    fn test_calculate_rms() {
        let mut data = ChannelData::new();
        data.process_sample(0.5);
        data.process_sample(-0.5);

        let rms = data.calculate_rms(2);
        assert!((rms - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_get_effective_peak() {
        let data = ChannelData::new();
        assert_eq!(data.get_effective_peak(), 0.0);

        let mut data = ChannelData::new();
        data.process_sample(1.0);
        data.process_sample(0.8);

        // 应该返回次Peak（0.8），而不是主Peak（1.0）
        assert!((data.get_effective_peak() - 0.8).abs() < 1e-5);
    }

    #[test]
    fn test_reset() {
        let mut data = ChannelData::new();
        data.process_sample(0.5);
        data.process_sample(0.8);

        data.reset();
        assert_eq!(data.rms_accumulator, 0.0);
        assert_eq!(data.peak_primary, 0.0);
        assert_eq!(data.peak_secondary, 0.0);
    }

    #[test]
    fn test_negative_samples() {
        let mut data = ChannelData::new();
        data.process_sample(-0.7);
        data.process_sample(0.5);

        // 负样本的绝对值应正确处理
        assert!((data.peak_primary - 0.7).abs() < 1e-5);
        assert!((data.peak_secondary - 0.5).abs() < 1e-5);
    }

    #[test]
    fn test_default_trait() {
        let data = ChannelData::default();
        let new_data = ChannelData::new();
        assert_eq!(data.rms_accumulator, new_data.rms_accumulator);
        assert_eq!(data.peak_primary, new_data.peak_primary);
        assert_eq!(data.peak_secondary, new_data.peak_secondary);
    }
}
