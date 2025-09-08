//! foobar2000兼容的DR直方图和20%采样算法
//!
//! 基于foobar2000 DR Meter逆向分析的精确直方图实现，专注于20%采样算法的高精度匹配。
//!
//! ## 核心特性
//!
//! - **10001-bin超高精度直方图**：覆盖0.0000-1.0000幅度范围，精度0.0001
//! - **逆向遍历20%采样**：从高幅度向低幅度遍历，精确匹配foobar2000行为  
//! - **内存布局兼容**：扁平化数组布局匹配foobar2000内存结构
//! - **Sum Doubling感知**：支持累加器级别Sum Doubling的有效样本数计算

// 早期版本：已移除AudioError, AudioResult导入，简化错误处理

// 🏷️ FEATURE_REMOVAL: SSE导入已删除，使用channel_data.rs中的统一SSE函数
// 📅 删除时间: 2025-09-08
// 🎯 原因: 删除重复的foobar2000_sse_sqrt函数定义后不再需要这些导入

// 🔥 Bit-exact数值常量 (与foobar2000完全相同的十六进制精度)
// 📖 从foobar2000反汇编中提取的精确常量值
const FOOBAR2000_0_2: f64 = f64::from_bits(0x3fc999999999999a); // 精确的0.2
// 🏷️ FEATURE_REMOVAL: FOOBAR2000_1E8常量已删除
// 📅 删除时间: 2025-09-08
// 🎯 原因: 仅用于已删除的精确权重公式，现为死代码

// 🏷️ FEATURE_REMOVAL: 重复的foobar2000_sse_sqrt函数定义已删除
// 📅 删除时间: 2025-09-08
// 🎯 原因: channel_data.rs中已有相同定义，避免代码重复
// 💡 简化效果: 移除重复代码，统一使用channel_data.rs中的版本

/// foobar2000兼容的直方图分析器
///
/// 专为foobar2000 DR Meter精确兼容设计的20%采样算法实现：
/// - 单样本绝对值直方图填充（匹配foobar2000行为）
/// - 逆向遍历20%采样算法（从高幅度向低幅度）
/// - 多声道感知的扁平化内存布局
/// - Sum Doubling有效样本数支持
#[derive(Debug, Clone)]
pub struct SimpleHistogramAnalyzer {
    /// 样本绝对值直方图
    histogram: DrHistogram,

    /// 总样本数
    total_samples: u64,
}

/// foobar2000兼容的10001-bin直方图容器
///
/// 基于foobar2000 DR Meter逆向分析的精确直方图实现：
/// - **超高精度**：10001个bin覆盖0.0000-1.0000幅度范围（精度0.0001）
/// - **foobar2000内存布局**：扁平化数组匹配原版内存结构
/// - **多声道支持**：histogram_addr = base_addr + 4 * (10001 * channel + bin_index)
/// - **20%采样算法**：支持逆向遍历的精确20%分位数计算
#[derive(Debug, Clone)]
pub struct DrHistogram {
    /// 🔥 关键修复：使用扁平化数组匹配foobar2000内存布局
    /// 每个声道占用10001个连续元素，支持多声道统一寻址
    bins: Vec<u64>,

    /// 声道数量（用于计算正确的内存偏移）
    #[allow(dead_code)] // 用于内存分配，但编译器认为未被读取
    channel_count: usize,

    /// 当前处理的声道索引
    current_channel: usize,

    /// 总样本数量
    total_samples: u64,
}

impl SimpleHistogramAnalyzer {
    /// 创建简单直方图分析器
    ///
    /// 🎯 优先级4修复：支持多声道内存布局匹配
    ///
    /// # 参数
    /// * `_sample_rate` - 采样率（保持API兼容性）
    /// * `channel_count` - 总声道数量（可选，默认1）
    /// * `current_channel` - 当前声道索引（可选，默认0）
    pub fn new(_sample_rate: u32) -> Self {
        Self {
            histogram: DrHistogram::new(1, 0), // 默认单声道兼容性
            total_samples: 0,
        }
    }

    /// 创建多声道感知的直方图分析器
    ///
    /// # 参数
    /// * `sample_rate` - 采样率
    /// * `channel_count` - 总声道数量
    /// * `current_channel` - 当前处理的声道索引
    pub fn new_multichannel(
        _sample_rate: u32,
        channel_count: usize,
        current_channel: usize,
    ) -> Self {
        Self {
            histogram: DrHistogram::new(channel_count, current_channel),
            total_samples: 0,
        }
    }

    /// 处理单声道样本，直接使用样本绝对值填充直方图
    ///
    /// # 参数
    ///
    /// * `samples` - 单声道f32样本数组
    pub fn process_channel(&mut self, samples: &[f32]) {
        for &sample in samples {
            let sample_abs = sample.abs();
            self.histogram.add_sample(sample_abs);
            self.total_samples += 1;
        }
    }

    /// 计算"最响20%样本"的简单RMS值
    ///
    /// 早期版本的简化算法：
    /// 1. 逆向遍历直方图找到最响20%样本
    /// 2. 简单计算这些样本的平方和
    /// 3. 开方得到RMS值
    pub fn calculate_20_percent_rms(&self) -> f64 {
        self.histogram.calculate_simple_20_percent_rms()
    }

    /// 使用有效样本数计算20% RMS（考虑Sum Doubling影响）
    ///
    /// # 参数
    /// * `effective_samples` - 有效样本数，考虑Sum Doubling后的样本数
    ///
    /// # 返回值
    /// 返回基于有效样本数计算的20%RMS值
    pub fn calculate_20_percent_rms_with_effective_samples(&self, effective_samples: u64) -> f64 {
        self.histogram
            .calculate_simple_20_percent_rms_with_effective_samples(Some(effective_samples))
    }

    // 🏷️ FEATURE_REMOVAL: 精确加权RMS算法已删除
    // 📅 删除时间: 2025-09-08
    // 🎯 原因: weighted_rms功能已删除，此方法成为死代码
    // 💡 foobar2000专属模式：使用简单算法确保最优精度

    /// 获取总样本数
    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    /// 清空分析器状态
    pub fn clear(&mut self) {
        self.total_samples = 0;
        self.histogram.clear();
    }

    /// 获取样本统计信息
    pub fn get_statistics(&self) -> SimpleStats {
        let mut non_zero_bins = 0;
        let mut min_value = f64::INFINITY;
        let mut max_value: f64 = 0.0;

        for (index, &count) in self.histogram.bins().iter().enumerate() {
            if count > 0 {
                non_zero_bins += 1;
                let value = index as f64 / 10000.0;
                min_value = min_value.min(value);
                max_value = max_value.max(value);
            }
        }

        if min_value == f64::INFINITY {
            min_value = 0.0;
        }

        SimpleStats {
            total_samples: self.total_samples,
            non_zero_bins,
            min_value,
            max_value,
            rms_20_percent: self.calculate_20_percent_rms(),
        }
    }
}

impl DrHistogram {
    /// 创建新的10001-bin直方图（支持多声道扁平化布局）
    ///
    /// # 参数
    /// * `channel_count` - 声道数量，用于分配正确的内存空间
    /// * `current_channel` - 当前处理的声道索引（0-based）
    fn new(channel_count: usize, current_channel: usize) -> Self {
        Self {
            // 🔥 关键修复：分配channel_count * 10001的扁平化数组
            // 匹配foobar2000内存布局：base_addr + 4 * (10001 * channel + bin_index)
            bins: vec![0; channel_count * 10001],
            channel_count,
            current_channel,
            total_samples: 0,
        }
    }

    /// 计算foobar2000兼容的bin地址偏移
    ///
    /// 📖 对应foobar2000汇编：histogram_addr = base_addr + 4 * (10001 * channel + bin_index)
    #[inline]
    fn get_bin_offset(&self, bin_index: usize) -> usize {
        // 🎯 优先级4修复：精确匹配foobar2000的地址计算
        // 内存布局：[Ch0_Bin0..Ch0_Bin10000, Ch1_Bin0..Ch1_Bin10000, ...]
        10001 * self.current_channel + bin_index
    }

    /// 获取当前声道的bin数据（供WindowRmsAnalyzer使用）
    ///
    /// 🔥 关键修复：返回当前声道的10001个bin，而不是整个扁平化数组
    pub(crate) fn bins(&self) -> &[u64] {
        let start_offset = self.get_bin_offset(0);
        &self.bins[start_offset..start_offset + 10001]
    }

    /// 添加样本绝对值到直方图
    pub fn add_sample(&mut self, sample_abs: f32) {
        if sample_abs < 0.0 || !sample_abs.is_finite() {
            return; // 忽略无效样本
        }

        // 计算bin索引：样本绝对值映射到0-10000范围
        // 🔥 关键修复：使用foobar2000的截断方式，不是四舍五入！
        // 📖 反汇编: v47 = (int)(v46 * 10000.0) - 直接截断转换
        let bin_index = ((sample_abs as f64 * 10000.0).min(10000.0)) as usize;

        // 🎯 优先级4修复：使用foobar2000兼容的地址偏移
        let offset = self.get_bin_offset(bin_index);
        self.bins[offset] += 1;
        self.total_samples += 1;
    }

    // 早期版本：已移除add_window_rms方法，不再使用窗口RMS处理

    /// 简化的20%RMS计算
    ///
    /// 早期算法的简化实现：
    /// 1. 从高幅度向低幅度逆向遍历，选取20%样本
    /// 2. 简单计算这些样本的平方和
    /// 3. 开方得到RMS值
    ///
    /// # 返回值
    ///
    /// 返回简化计算的20%RMS值，如果直方图为空则返回0.0
    fn calculate_simple_20_percent_rms(&self) -> f64 {
        self.calculate_simple_20_percent_rms_with_effective_samples(None)
    }

    /// 使用有效样本数计算20% RMS（考虑Sum Doubling）
    ///
    /// # 参数
    /// * `effective_samples` - 有效样本数（考虑Sum Doubling后），None则使用total_samples
    ///
    /// # 返回值
    /// 返回基于有效样本数计算的20%RMS值
    fn calculate_simple_20_percent_rms_with_effective_samples(
        &self,
        effective_samples: Option<u64>,
    ) -> f64 {
        if self.total_samples == 0 {
            return 0.0;
        }

        // 🔥 关键修正：使用有效样本数计算20%采样数量
        // 基于foobar2000反汇编分析：v14 * 0.2 + 0.5 (v14是经过Sum Doubling的样本数)
        let effective_count = effective_samples.unwrap_or(self.total_samples);

        // 🎯 优先级3修复：20%采样边界精确处理 - 三重精确边界控制
        // 📖 基于UltraThink分析：foobar2000转换链 i32 -> u32 -> u64
        let effective_count_int = effective_count as i32;
        let effective_count_f64 = effective_count_int as f64;

        // 🔥 关键修复：完全匹配foobar2000的数据类型转换链
        let samples_20_temp = (effective_count_f64 * FOOBAR2000_0_2 + 0.5) as i32; // foobar2000转换链
        let need = (samples_20_temp as u32 as u64).max(1); // 零值保护：i32 -> u32 -> u64

        let mut remaining = need;
        let mut sum_square = 0.0;

        // 🔥 从高幅度向低幅度逆向遍历，使用remaining计数器实现精确停止
        for bin_index in (0..=10000).rev() {
            if remaining == 0 {
                break;
            } // 🎯 精确停止条件

            // 🎯 优先级4修复：使用foobar2000兼容的地址偏移访问bin
            let offset = self.get_bin_offset(bin_index);
            let available = self.bins[offset];
            let use_count = available.min(remaining);

            if use_count > 0 {
                // 计算该bin对应的幅度值
                let amplitude = bin_index as f64 / 10000.0;

                // 简单的平方和累积
                sum_square += use_count as f64 * amplitude * amplitude;
                remaining -= use_count; // 🎯 精确递减remaining计数器
            }
        }

        // 计算最终RMS：开方(平方和/选中样本数)
        // 🔥 关键精度修复：使用foobar2000相同的SSE平方根
        // 📖 对应汇编: *(_QWORD *)&v46 = *(_OWORD *)&_mm_sqrt_pd(v43);
        let actually_selected = need - remaining; // 🎯 精确计算实际选中的样本数
        if actually_selected > 0 {
            // 数据类型转换链：先转int再转double
            let selected_int = actually_selected as i32;
            let selected_f64 = selected_int as f64;

            // 🎯 优先级2修复：DR计算阶段使用标量平方根（不是SSE）
            // 📖 基于UltraThink分析：音频处理用SSE，DR计算用标量
            (sum_square / selected_f64).sqrt() // 标量平方根替代SSE
        } else {
            0.0
        }
    }

    // 🏷️ FEATURE_REMOVAL: 精确权重公式已删除（60+行复杂死代码）
    // 📅 删除时间: 2025-09-08
    // 🎯 原因: weighted_rms功能已删除，该实验性算法成为死代码
    // 💡 简化效果: 删除复杂权重计算逻辑，专注foobar2000简单算法
    // 🔄 回退: 如需实验性功能，查看git历史

    // 早期版本：已移除get_bin_count测试方法，简化内部API

    /// 清空直方图（仅清空当前声道的部分）
    ///
    /// 🔥 关键修复：只清空当前声道的10001个bin，不影响其他声道
    fn clear(&mut self) {
        let start_offset = self.get_bin_offset(0);
        self.bins[start_offset..start_offset + 10001].fill(0);
        self.total_samples = 0;
    }

    // 早期版本：已移除validate方法，简化验证逻辑
}

/// 样本统计信息
#[derive(Debug, Clone)]
pub struct SimpleStats {
    /// 总样本数量
    pub total_samples: u64,

    /// 非零bin数量
    pub non_zero_bins: usize,

    /// 最小样本幅度值
    pub min_value: f64,

    /// 最大样本幅度值  
    pub max_value: f64,

    /// 最响20%样本的RMS值
    pub rms_20_percent: f64,
}

impl Default for DrHistogram {
    fn default() -> Self {
        // 🔥 默认单声道布局，兼容旧代码
        Self::new(1, 0)
    }
}

impl std::fmt::Display for SimpleStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SimpleStats {{ samples: {}, bins: {}, amplitude_range: {:.6}-{:.6}, rms_20%: {:.6} }}",
            self.total_samples,
            self.non_zero_bins,
            self.min_value,
            self.max_value,
            self.rms_20_percent
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_analyzer_creation() {
        let analyzer = SimpleHistogramAnalyzer::new(48000);
        assert_eq!(analyzer.total_samples(), 0);
    }

    #[test]
    fn test_simple_sample_processing() {
        let mut analyzer = SimpleHistogramAnalyzer::new(48000);

        // 创建一些测试样本
        let samples: Vec<f32> = (0..1000).map(|i| (i as f32) / 1000.0).collect();

        analyzer.process_channel(&samples);

        assert_eq!(analyzer.total_samples(), 1000); // 应该有1000个样本

        let rms_20 = analyzer.calculate_20_percent_rms();
        assert!(rms_20 > 0.0); // 应该有有效的20%RMS值
    }

    #[test]
    fn test_constant_samples() {
        let mut analyzer = SimpleHistogramAnalyzer::new(48000);

        // 创建恒定幅度的样本
        let samples: Vec<f32> = (0..1000).map(|_| 0.5).collect(); // 恒定幅度0.5

        analyzer.process_channel(&samples);

        assert_eq!(analyzer.total_samples(), 1000); // 应该有1000个样本

        let rms_20 = analyzer.calculate_20_percent_rms();
        // 恒定0.5幅度，RMS应该约等于0.5
        assert!((rms_20 - 0.5).abs() < 0.1);
    }

    #[test]
    fn test_varying_samples() {
        let mut analyzer = SimpleHistogramAnalyzer::new(48000);

        // 创建不同幅度的样本
        let samples: Vec<f32> = (0..500).map(|i| (i as f32) / 500.0).collect();

        analyzer.process_channel(&samples);

        assert_eq!(analyzer.total_samples(), 500); // 应该有500个样本

        let rms_20 = analyzer.calculate_20_percent_rms();
        assert!(rms_20 > 0.0); // 应详有有效值
    }

    #[test]
    fn test_20_percent_calculation() {
        let mut analyzer = SimpleHistogramAnalyzer::new(48000);

        // 创建多个不同幅度的样本
        // 高幅度样本（200个）
        let high_samples: Vec<f32> = (0..200).map(|_| 0.9).collect();
        analyzer.process_channel(&high_samples);

        // 低幅度样本（800个）
        let low_samples: Vec<f32> = (0..800).map(|_| 0.1).collect();
        analyzer.process_channel(&low_samples);

        assert_eq!(analyzer.total_samples(), 1000);

        let rms_20 = analyzer.calculate_20_percent_rms();

        // 20%的样本（200个）应该是高幅度值0.9
        // 简单计算应该接近0.9
        assert!(rms_20 > 0.8); // 应该接近最高的幅度值
    }

    #[test]
    fn test_percentile_calculation() {
        let mut analyzer = SimpleHistogramAnalyzer::new(48000);

        // 创建递减幅度的样本
        for i in 0..11 {
            let amplitude = (10 - i) as f32 / 10.0; // 递减的幅度值
            let samples: Vec<f32> = (0..100).map(|_| amplitude).collect();
            analyzer.process_channel(&samples);
        }

        assert_eq!(analyzer.total_samples(), 1100);

        let rms_20 = analyzer.calculate_20_percent_rms();
        // 前20%的样本应该是高幅度值
        // 简单计算应该接近高幅度值
        assert!(rms_20 > 0.8);
    }

    #[test]
    fn test_statistics() {
        let mut analyzer = SimpleHistogramAnalyzer::new(48000);

        // 添加几个不同幅度的样本
        let amplitudes = [0.1, 0.3, 0.5, 0.7, 0.9];
        for &amplitude in &amplitudes {
            let samples: Vec<f32> = (0..200).map(|_| amplitude).collect();
            analyzer.process_channel(&samples);
        }

        let stats = analyzer.get_statistics();
        assert_eq!(stats.total_samples, 1000);
        assert!(stats.non_zero_bins > 0);
        assert!(stats.min_value > 0.0);
        assert!(stats.max_value <= 1.0);
        assert!(stats.rms_20_percent > 0.0);
    }

    #[test]
    fn test_clear() {
        let mut analyzer = SimpleHistogramAnalyzer::new(48000);

        let samples: Vec<f32> = (0..100).map(|_| 0.5).collect();
        analyzer.process_channel(&samples);
        assert_eq!(analyzer.total_samples(), 100);

        analyzer.clear();
        assert_eq!(analyzer.total_samples(), 0);
    }
}
