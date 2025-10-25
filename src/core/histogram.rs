//! foobar2000兼容的DR直方图和20%采样算法
//!
//! 基于foobar2000 DR Meter逆向分析的精确直方图实现，专注于窗口级RMS分析和20%采样算法。
//!
//! ## 核心特性
//!
//! - **WindowRmsAnalyzer**: 基于master分支的正确窗口级RMS分析
//! - **3秒窗口处理**: 按照DR测量标准的窗口长度
//! - **20%采样算法**: 逆向遍历选择最响20%窗口
//! - **精确峰值选择**: 主峰/次峰智能切换机制
//! - **🚀 SIMD优化**: 平方和计算使用SSE2并行加速

use crate::processing::simd_core::SimdProcessor;
use crate::tools::constants::dr_analysis::PEAK_EQUALITY_EPSILON;

/// WindowRmsAnalyzer - 基于master分支的正确20%采样算法
///
/// 这是从master分支移植的正确算法实现，使用窗口RMS值的20%采样
/// 而不是样本级直方图的20%采样，能够产生与master分支完全一致的结果。
#[derive(Debug, Clone)]
pub struct WindowRmsAnalyzer {
    /// 窗口长度（样本数）- 符合 Measuring_DR_ENv3.md 标准
    window_len: usize,
    /// 当前窗口的平方和累积
    current_sum_sq: f64,
    /// 当前窗口的最大Peak值
    current_peak: f64,
    /// 当前窗口的样本计数
    current_count: usize,
    /// 所有窗口RMS值的直方图
    histogram: DrHistogram,
    /// 所有窗口的Peak值集合（用于排序和选择第二大Peak值）
    window_peaks: Vec<f64>,
    /// 🔧 **关键修复**: 直接存储窗口RMS值以避免直方图量化损失
    window_rms_values: Vec<f64>,
    /// 处理的样本总数（用于虚拟零窗逻辑）
    total_samples_processed: usize,
    /// 最后一个样本值（用于尾窗处理）
    last_sample: f64,
    /// 🚀 **流式双峰跟踪**: 当前窗口的最大值出现次数（用于尾窗Peak调整）
    current_peak_count: usize,
    /// 🚀 **流式双峰跟踪**: 当前窗口的次大Peak值（用于尾窗Peak调整）
    current_second_peak: f64,
    /// 🚀 **SIMD优化**: SIMD处理器用于平方和计算加速
    simd_processor: SimdProcessor,
}

#[derive(Debug, Clone)]
struct DrHistogram {
    /// 10000个bin，索引0对应RMS=0，索引9999对应RMS=0.9999
    bins: Vec<u32>,
    /// 总窗口数
    total_windows: u64,
}

impl WindowRmsAnalyzer {
    /// 计算符合官方DR测量标准的3秒窗口样本数
    fn calculate_standard_window_size(sample_rate: u32) -> usize {
        match sample_rate {
            44100 => 132480,                 // 官方标准：44.1kHz使用132480样本
            _ => (3 * sample_rate) as usize, // 其他采样率：标准3秒窗口
        }
    }

    /// 创建3秒窗口RMS分析器
    ///
    /// # 参数
    /// * `sample_rate` - 音频采样率，用于计算3秒窗口长度
    /// * `_sum_doubling` - 预留参数，当前foobar2000兼容模式固定启用Sum Doubling。
    ///   该参数暂未使用，未来如需可配置再接入RMS计算逻辑。
    ///   固定行为参见`process_samples()`中的`sum_sq * 2.0`计算。
    pub fn new(sample_rate: u32, _sum_doubling: bool) -> Self {
        let window_len = Self::calculate_standard_window_size(sample_rate);
        Self {
            window_len,
            current_sum_sq: 0.0,
            current_peak: 0.0,
            current_count: 0,
            histogram: DrHistogram::new(),
            window_peaks: Vec::new(),
            window_rms_values: Vec::new(),
            total_samples_processed: 0,
            last_sample: 0.0,
            current_peak_count: 0,
            current_second_peak: 0.0,
            simd_processor: SimdProcessor::new(),
        }
    }

    /// 处理单声道样本，按3秒窗口计算RMS并填入直方图
    pub fn process_samples(&mut self, samples: &[f32]) {
        // 🚀 **长曲目优化**: 首次调用时预估窗口数，减少realloc
        if self.total_samples_processed == 0 && !samples.is_empty() {
            let estimated_windows = samples.len() / self.window_len + 1;
            self.window_rms_values.reserve(estimated_windows);
            self.window_peaks.reserve(estimated_windows);
        }

        // 记录总样本数
        self.total_samples_processed += samples.len();

        for &sample in samples {
            let sample_f64 = sample as f64;
            let abs_sample = sample_f64.abs();

            // 🔧 **dr14兼容性**: 保存当前样本作为潜在的"最后样本"
            self.last_sample = sample_f64;

            // 🚀 **流式双峰跟踪**: 更新Peak和次Peak
            if abs_sample > self.current_peak {
                // 新样本是新最大值
                self.current_second_peak = self.current_peak; // 旧最大值变成次大值
                self.current_peak = abs_sample;
                self.current_peak_count = 1;
            } else if (abs_sample - self.current_peak).abs() < PEAK_EQUALITY_EPSILON {
                // 新样本等于最大值（使用浮点数容差比较）
                self.current_peak_count += 1;
            } else if abs_sample > self.current_second_peak {
                // 新样本大于次大值但小于最大值
                self.current_second_peak = abs_sample;
            }

            // 更新当前窗口的平方和
            self.current_sum_sq += sample_f64 * sample_f64;
            self.current_count += 1;

            // 窗口满了，计算窗口RMS和Peak并添加到直方图
            if self.current_count >= self.window_len {
                // ✅ 官方标准RMS公式：RMS = sqrt(2 * sum(smp_i^2) / n)
                // 💡 Sum Doubling（系数2.0）固定启用，与foobar2000 DR Meter兼容
                // 📌 这是foobar2000的固定行为，不受new()参数控制
                let window_rms = (2.0 * self.current_sum_sq / self.current_count as f64).sqrt();
                self.histogram.add_window_rms(window_rms);

                // ✅ 记录窗口Peak值用于后续排序
                self.window_peaks.push(self.current_peak);

                // 🔧 **关键修复**: 直接存储RMS值避免量化损失
                self.window_rms_values.push(window_rms);

                // 重置窗口
                self.current_sum_sq = 0.0;
                self.current_peak = 0.0;
                self.current_peak_count = 0;
                self.current_second_peak = 0.0;
                self.current_count = 0;
            }
        }

        // 处理不足一个窗口的剩余样本
        if self.current_count > 0 {
            // 排除最后一个样本
            if self.current_count > 1 {
                // 排除最后一个样本：从平方和中减去最后样本的平方，样本数-1
                let adjusted_sum_sq = self.current_sum_sq - (self.last_sample * self.last_sample);
                let adjusted_count = self.current_count - 1;

                // RMS公式：RMS = sqrt(2 * sum(smp_i^2) / (n-1))
                // 💡 Sum Doubling（系数2.0）固定启用，与foobar2000 DR Meter兼容
                let window_rms = (2.0 * adjusted_sum_sq / adjusted_count as f64).sqrt();
                self.histogram.add_window_rms(window_rms);
                self.window_rms_values.push(window_rms);

                // 🚀 **流式双峰跟踪**: 使用O(1)算法调整Peak值，排除最后一个样本
                let last_abs = self.last_sample.abs();
                let adjusted_peak = if (last_abs - self.current_peak).abs() < PEAK_EQUALITY_EPSILON
                {
                    // 最后样本是最大值
                    if self.current_peak_count > 1 {
                        // 还有其他最大值，Peak不变
                        self.current_peak
                    } else {
                        // 最后样本是唯一最大值，使用次大值
                        self.current_second_peak
                    }
                } else {
                    // 最后样本不是最大值，Peak不变
                    self.current_peak
                };
                self.window_peaks.push(adjusted_peak);
            } else {
                // 尾窗只有1个样本时会完全跳过
            }

            // 重置状态
            self.current_sum_sq = 0.0;
            self.current_peak = 0.0;
            self.current_peak_count = 0;
            self.current_second_peak = 0.0;
            self.current_count = 0;
        }
    }

    /// 计算"最响20%窗口"的加权RMS值
    ///
    /// - 若恰好整除3秒窗：seg_cnt = 实际窗口数 + 1（添加1个0窗）
    /// - 若有尾部不满窗：seg_cnt = 实际窗口数（不添加0窗）
    /// - 使用seg_cnt计算n_blk，选择最高20%的RMS值
    pub fn calculate_20_percent_rms(&self) -> f64 {
        if self.window_rms_values.is_empty() {
            return 0.0;
        }

        // 🎯 **关键修复**: 判断是否需要虚拟0窗
        let has_virtual_zero = self.total_samples_processed.is_multiple_of(self.window_len);
        let seg_cnt = if has_virtual_zero {
            self.window_rms_values.len() + 1 // 恰好整除：添加0窗
        } else {
            self.window_rms_values.len() // 有尾窗：不添加0窗
        };

        // 步骤2: 🚀 **Phase 3优化**: 构建RMS数组（容量预留+extend避免realloc）
        let mut rms_array = Vec::with_capacity(self.window_rms_values.len() + 1);
        rms_array.extend_from_slice(&self.window_rms_values);
        if has_virtual_zero {
            rms_array.push(0.0);
        }

        // 🚀 **性能优化**: 部分选择算法 O(n log n) → O(n)
        // 步骤3: 计算20%采样窗口数
        let cut_best_bins = 0.2;
        let n_blk = ((seg_cnt as f64 * cut_best_bins).floor() as usize).max(1);

        // 步骤4: 使用部分选择找到最高20%的RMS值
        let start_index = seg_cnt - n_blk;

        // 使用select_nth_unstable进行O(n)部分选择
        // 这会将数组重新排列，使得index≥start_index的元素都是最大的n_blk个
        // 使用total_cmp安全处理NaN：NaN会被排序到最后
        rms_array.select_nth_unstable_by(start_index, |a: &f64, b: &f64| a.total_cmp(b));

        // 步骤5: 🚀 **SIMD优化**: 计算最高20%RMS值的平方和
        let top_20_values = &rms_array[start_index..start_index + n_blk];
        let rms_sum = self.simd_processor.calculate_square_sum(top_20_values);

        // 步骤6: 开方平均
        (rms_sum / n_blk as f64).sqrt()
    }

    /// 🚀 **O(n)优化**: 单遍扫描找出最大值和次大值
    ///
    /// 用O(n)单遍扫描代替O(n log n)排序，语义与排序后取最后两个元素一致：
    /// - 对于重复值，自然保留（例如多个最大值时，次大值就是该最大值）
    /// - 无NaN数据（peak值总是非负的），直接用普通比较更快
    /// - 支持虚拟0窗语义：若has_virtual_zero=true，考虑虚拟0值的排序影响
    ///
    /// # 返回值
    ///
    /// 返回 (最大值, 次大值)
    #[inline(always)]
    fn find_top_two(values: &[f64], has_virtual_zero: bool) -> (f64, f64) {
        if values.is_empty() {
            return (0.0, 0.0);
        }

        if values.len() == 1 {
            let v = values[0];
            // 单元素：最大和次大相同，除非有虚拟0
            if has_virtual_zero && 0.0 > v {
                return (0.0, v);
            }
            return (v, v);
        }

        // 多元素：用第一个元素初始化
        let mut max = values[0];
        let mut second = 0.0; // 次大初始为0，会在循环中更新

        for &val in values.iter().skip(1) {
            if val > max {
                second = max;
                max = val;
            } else if val > second {
                second = val;
            }
        }

        // 处理虚拟0窗的影响（若存在）
        if has_virtual_zero {
            let virtual_zero = 0.0;
            if virtual_zero > max {
                second = max;
                max = virtual_zero;
            } else if virtual_zero > second {
                second = virtual_zero;
            }
        }

        (max, second)
    }

    /// 获取最大窗口Peak值（主峰）
    ///
    /// 实现窗口级最大Peak选择算法：
    /// - 若恰好整除3秒窗：seg_cnt = 实际窗口数 + 1（添加1个0窗）
    /// - 若有尾部不满窗：seg_cnt = 实际窗口数（不添加0窗）
    /// - peaks[seg_cnt-1] 选择排序后的最大值
    ///
    /// # 返回值
    ///
    /// 返回窗口级最大Peak值
    pub fn get_largest_peak(&self) -> f64 {
        if self.window_peaks.is_empty() {
            return 0.0;
        }

        let has_virtual_zero = self.total_samples_processed.is_multiple_of(self.window_len);

        // 🚀 **微优化**: 直接扫描window_peaks，无临时Vec分配
        // find_top_two 内部处理虚拟0窗语义
        let (max, _second) = Self::find_top_two(&self.window_peaks, has_virtual_zero);
        max
    }

    /// 获取第二大窗口Peak值
    ///
    /// 实现与master分支相同的窗口级Peak选择算法：
    /// - 若恰好整除3秒窗：seg_cnt = 实际窗口数 + 1（添加1个0窗）
    /// - 若有尾部不满窗：seg_cnt = 实际窗口数（不添加0窗）
    /// - peaks[seg_cnt-2] 选择排序后的第二大值
    ///
    /// # 返回值
    ///
    /// 返回选择的Peak值
    pub fn get_second_largest_peak(&self) -> f64 {
        if self.window_peaks.is_empty() {
            return 0.0;
        }

        let has_virtual_zero = self.total_samples_processed.is_multiple_of(self.window_len);

        // 🚀 **微优化**: 直接扫描window_peaks，无临时Vec分配
        // find_top_two 内部处理虚拟0窗语义
        let (_max, second) = Self::find_top_two(&self.window_peaks, has_virtual_zero);
        second
    }

    /// 清空分析器状态
    pub fn clear(&mut self) {
        self.current_sum_sq = 0.0;
        self.current_peak = 0.0;
        self.current_count = 0;
        self.histogram.clear();
        self.window_peaks.clear();
        self.window_rms_values.clear();
        self.total_samples_processed = 0;
        self.last_sample = 0.0;
        self.current_peak_count = 0;
        self.current_second_peak = 0.0;
    }
}

impl DrHistogram {
    /// 创建新的10000-bin直方图
    fn new() -> Self {
        Self {
            bins: vec![0; 10000], // 索引0-9999
            total_windows: 0,
        }
    }

    /// 添加窗口RMS到直方图
    fn add_window_rms(&mut self, window_rms: f64) {
        if window_rms < 0.0 || !window_rms.is_finite() {
            return; // 忽略无效窗口
        }

        // 计算bin索引：RMS映射到0-9999范围
        let index = (window_rms * 10000.0).round().min(9999.0) as usize;

        self.bins[index] += 1;
        self.total_windows += 1;
    }

    /// 清空直方图
    fn clear(&mut self) {
        self.bins.fill(0);
        self.total_windows = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_size_calculation() {
        // 44.1kHz特殊情况
        assert_eq!(
            WindowRmsAnalyzer::calculate_standard_window_size(44100),
            132480
        );

        // 其他采样率：3秒标准窗口
        assert_eq!(
            WindowRmsAnalyzer::calculate_standard_window_size(48000),
            144000
        );
        assert_eq!(
            WindowRmsAnalyzer::calculate_standard_window_size(96000),
            288000
        );
        assert_eq!(
            WindowRmsAnalyzer::calculate_standard_window_size(192000),
            576000
        );
    }

    #[test]
    fn test_window_rms_analyzer_creation() {
        let analyzer = WindowRmsAnalyzer::new(44100, false);
        assert_eq!(analyzer.window_len, 132480);
        assert_eq!(analyzer.current_count, 0);
        assert_eq!(analyzer.total_samples_processed, 0);
        assert_eq!(analyzer.window_rms_values.len(), 0);
        assert_eq!(analyzer.window_peaks.len(), 0);
    }

    #[test]
    fn test_process_samples_single_window() {
        let mut analyzer = WindowRmsAnalyzer::new(48000, false);

        // 创建一个完整的3秒窗口（144000样本）
        let samples = vec![0.5f32; 144000];
        analyzer.process_samples(&samples);

        // 应该产生1个完整窗口
        assert_eq!(analyzer.window_rms_values.len(), 1);
        assert_eq!(analyzer.window_peaks.len(), 1);
        assert_eq!(analyzer.total_samples_processed, 144000);

        // 验证Peak值
        assert!((analyzer.window_peaks[0] - 0.5).abs() < 1e-10);

        // 验证RMS计算（0.5的样本，RMS = sqrt(2 * 0.5^2) ≈ 0.707）
        let expected_rms = (2.0 * 0.5 * 0.5_f64).sqrt();
        assert!((analyzer.window_rms_values[0] - expected_rms).abs() < 1e-10);
    }

    #[test]
    fn test_process_samples_multiple_windows() {
        let mut analyzer = WindowRmsAnalyzer::new(48000, false);

        // 创建2.5个窗口的样本
        let samples = vec![0.3f32; 360000]; // 2.5 * 144000 = 360000
        analyzer.process_samples(&samples);

        // 应该产生3个窗口（2个完整+1个尾窗）
        assert_eq!(analyzer.window_rms_values.len(), 3);
        assert_eq!(analyzer.window_peaks.len(), 3);
    }

    #[test]
    fn test_process_samples_with_tail_window() {
        let mut analyzer = WindowRmsAnalyzer::new(48000, false);

        // 1个完整窗口 + 小于1个窗口的尾部
        let full_window = vec![0.5f32; 144000];
        let tail = vec![0.3f32; 72000]; // 0.5个窗口

        analyzer.process_samples(&full_window);
        analyzer.process_samples(&tail);

        // 应该有2个窗口（1个完整+1个尾窗）
        assert_eq!(analyzer.window_rms_values.len(), 2);
        assert_eq!(analyzer.window_peaks.len(), 2);
    }

    #[test]
    fn test_process_samples_single_sample_tail() {
        let mut analyzer = WindowRmsAnalyzer::new(48000, false);

        // 1个完整窗口 + 1个样本的尾部
        let full_window = vec![0.5f32; 144000];
        let tail = vec![0.8f32]; // 只有1个样本

        analyzer.process_samples(&full_window);
        analyzer.process_samples(&tail);

        // 只有1个样本的尾窗应该被跳过
        assert_eq!(analyzer.window_rms_values.len(), 1);
        assert_eq!(analyzer.window_peaks.len(), 1);
    }

    #[test]
    fn test_calculate_20_percent_rms_empty() {
        let analyzer = WindowRmsAnalyzer::new(44100, false);
        assert_eq!(analyzer.calculate_20_percent_rms(), 0.0);
    }

    #[test]
    fn test_calculate_20_percent_rms_with_virtual_zero() {
        let mut analyzer = WindowRmsAnalyzer::new(48000, false);

        // 恰好1个完整窗口（应该添加虚拟0窗）
        let samples = vec![0.5f32; 144000];
        analyzer.process_samples(&samples);

        let rms_20 = analyzer.calculate_20_percent_rms();
        assert!(rms_20 > 0.0, "RMS应该大于0");
    }

    #[test]
    fn test_calculate_20_percent_rms_without_virtual_zero() {
        let mut analyzer = WindowRmsAnalyzer::new(48000, false);

        // 不完整的窗口（不应该添加虚拟0窗）
        let samples = vec![0.5f32; 145000]; // 144000 + 1000
        analyzer.process_samples(&samples);

        let rms_20 = analyzer.calculate_20_percent_rms();
        assert!(rms_20 > 0.0);
    }

    #[test]
    fn test_get_largest_peak_empty() {
        let analyzer = WindowRmsAnalyzer::new(44100, false);
        assert_eq!(analyzer.get_largest_peak(), 0.0);
    }

    #[test]
    fn test_get_largest_peak() {
        let mut analyzer = WindowRmsAnalyzer::new(48000, false);

        // 创建多个窗口，峰值递增
        let window1 = vec![0.3f32; 144000];
        let window2 = vec![0.7f32; 144000];
        let window3 = vec![0.5f32; 144000];

        analyzer.process_samples(&window1);
        analyzer.process_samples(&window2);
        analyzer.process_samples(&window3);

        let largest_peak = analyzer.get_largest_peak();
        // f32精度限制，使用1e-6精度
        assert!(
            (largest_peak - 0.7).abs() < 1e-6,
            "应该选择最大Peak: actual={largest_peak}"
        );
    }

    #[test]
    fn test_get_second_largest_peak_empty() {
        let analyzer = WindowRmsAnalyzer::new(44100, false);
        assert_eq!(analyzer.get_second_largest_peak(), 0.0);
    }

    #[test]
    fn test_get_second_largest_peak_single() {
        let mut analyzer = WindowRmsAnalyzer::new(48000, false);

        // 创建1个窗口+小尾窗（避免虚拟0窗）
        let window1 = vec![0.6f32; 144000];
        let tail = vec![0.1f32; 100]; // 小尾窗，避免虚拟0窗
        analyzer.process_samples(&window1);
        analyzer.process_samples(&tail);

        let second_peak = analyzer.get_second_largest_peak();

        // 有2个窗口（1个完整+1个尾窗），第二大Peak应该是较小的那个
        // 因为尾窗会排除最后一个样本重新计算Peak，所以会比较小
        // 第二大Peak应该是尾窗的Peak（约0.1左右）
        assert!(
            (0.0..0.6).contains(&second_peak),
            "第二大Peak应该小于最大Peak: actual={second_peak}"
        );
    }

    #[test]
    fn test_get_second_largest_peak() {
        let mut analyzer = WindowRmsAnalyzer::new(48000, false);

        // 创建多个窗口，峰值不同
        let window1 = vec![0.3f32; 144000];
        let window2 = vec![0.8f32; 144000];
        let window3 = vec![0.6f32; 144000];

        analyzer.process_samples(&window1);
        analyzer.process_samples(&window2);
        analyzer.process_samples(&window3);

        let second_peak = analyzer.get_second_largest_peak();
        // f32精度限制，使用1e-6精度
        assert!((second_peak - 0.6).abs() < 1e-6, "应该选择第二大Peak");
    }

    #[test]
    fn test_clear() {
        let mut analyzer = WindowRmsAnalyzer::new(48000, false);

        // 添加一些数据
        let samples = vec![0.5f32; 288000]; // 2个窗口
        analyzer.process_samples(&samples);

        assert!(!analyzer.window_rms_values.is_empty());
        assert!(!analyzer.window_peaks.is_empty());
        assert!(analyzer.total_samples_processed > 0);

        // 清空
        analyzer.clear();

        assert_eq!(analyzer.window_rms_values.len(), 0);
        assert_eq!(analyzer.window_peaks.len(), 0);
        assert_eq!(analyzer.total_samples_processed, 0);
        assert_eq!(analyzer.current_count, 0);
        assert_eq!(analyzer.current_sum_sq, 0.0);
        assert_eq!(analyzer.current_peak, 0.0);
    }

    #[test]
    fn test_dr_histogram_creation() {
        let hist = DrHistogram::new();
        assert_eq!(hist.bins.len(), 10000);
        assert_eq!(hist.total_windows, 0);
    }

    #[test]
    fn test_dr_histogram_add_window_rms() {
        let mut hist = DrHistogram::new();

        // 添加有效RMS值
        hist.add_window_rms(0.5);
        assert_eq!(hist.total_windows, 1);

        hist.add_window_rms(0.8);
        assert_eq!(hist.total_windows, 2);

        // 添加无效值（负数）
        hist.add_window_rms(-0.1);
        assert_eq!(hist.total_windows, 2, "负数RMS应该被忽略");

        // 添加无效值（NaN）
        hist.add_window_rms(f64::NAN);
        assert_eq!(hist.total_windows, 2, "NaN应该被忽略");

        // 添加无效值（无穷）
        hist.add_window_rms(f64::INFINITY);
        assert_eq!(hist.total_windows, 2, "无穷值应该被忽略");
    }

    #[test]
    fn test_dr_histogram_clear() {
        let mut hist = DrHistogram::new();

        hist.add_window_rms(0.5);
        hist.add_window_rms(0.8);
        assert_eq!(hist.total_windows, 2);

        hist.clear();
        assert_eq!(hist.total_windows, 0);
        assert!(hist.bins.iter().all(|&bin| bin == 0));
    }

    #[test]
    fn test_virtual_zero_window_logic() {
        let mut analyzer = WindowRmsAnalyzer::new(48000, false);

        // 恰好144000样本（1个完整窗口）
        let samples = vec![0.5f32; 144000];
        analyzer.process_samples(&samples);

        // 验证虚拟0窗逻辑
        assert!(
            analyzer
                .total_samples_processed
                .is_multiple_of(analyzer.window_len)
        );

        // 145000样本（1个完整窗口+尾窗）
        let mut analyzer2 = WindowRmsAnalyzer::new(48000, false);
        let samples2 = vec![0.5f32; 145000];
        analyzer2.process_samples(&samples2);

        assert!(
            !analyzer2
                .total_samples_processed
                .is_multiple_of(analyzer2.window_len)
        );
    }

    #[test]
    fn test_rms_calculation_accuracy() {
        let mut analyzer = WindowRmsAnalyzer::new(48000, false);

        // 使用已知值测试RMS计算精度
        // 样本值为0.3，预期RMS = sqrt(2 * 0.3^2) = sqrt(0.18) ≈ 0.424264
        let samples = vec![0.3f32; 144000];
        analyzer.process_samples(&samples);

        assert!(!analyzer.window_rms_values.is_empty(), "应该有至少1个RMS值");

        let expected_rms = (2.0 * 0.3 * 0.3_f64).sqrt();
        let actual_rms = analyzer.window_rms_values[0];

        eprintln!(
            "Expected RMS: {}, Actual RMS: {}, Diff: {}",
            expected_rms,
            actual_rms,
            (actual_rms - expected_rms).abs()
        );

        assert!(
            (actual_rms - expected_rms).abs() < 1e-5,
            "RMS计算误差过大: expected={expected_rms}, actual={actual_rms}"
        );
    }

    #[test]
    fn test_peak_selection_with_varying_values() {
        let mut analyzer = WindowRmsAnalyzer::new(48000, false);

        // 创建3个窗口，峰值分别为0.2, 0.9, 0.5
        let window1 = vec![0.2f32; 144000];
        let window2 = vec![0.9f32; 144000];
        let window3 = vec![0.5f32; 144000];

        analyzer.process_samples(&window1);
        analyzer.process_samples(&window2);
        analyzer.process_samples(&window3);

        // 最大Peak应该是0.9，f32精度限制使用1e-6
        assert!((analyzer.get_largest_peak() - 0.9).abs() < 1e-6);

        // 第二大Peak应该是0.5
        assert!((analyzer.get_second_largest_peak() - 0.5).abs() < 1e-6);
    }

    /// 🚀 **Phase 1回归测试**: 尾窗最后样本是唯一最大值
    ///
    /// 验证流式双峰跟踪在尾窗排除唯一最大值时使用次大值的正确性
    #[test]
    fn test_tail_window_peak_adjustment_unique_max() {
        let mut analyzer = WindowRmsAnalyzer::new(48000, false);

        // 完整窗口 + 尾窗（最后样本是唯一最大值）
        let full_window = vec![0.5f32; 144000];
        analyzer.process_samples(&full_window);

        // 尾窗：除最后一个样本外都是0.3，最后一个是0.8（唯一最大值）
        let mut tail = vec![0.3f32; 1000];
        tail.push(0.8f32); // 最后样本是最大值
        analyzer.process_samples(&tail);

        // 应该有2个窗口（1个完整+1个尾窗）
        assert_eq!(analyzer.window_peaks.len(), 2);

        // 尾窗Peak应该是0.3（排除最后的0.8后，次大值是0.3）
        let tail_peak = analyzer.window_peaks[1];
        assert!(
            (tail_peak - 0.3).abs() < 1e-6,
            "尾窗Peak应该是0.3（次大值），实际={tail_peak}"
        );
    }

    /// 🚀 **Phase 1回归测试**: 尾窗最后样本是最大值但出现多次
    ///
    /// 验证流式双峰跟踪在尾窗排除重复最大值时保持最大值的正确性
    #[test]
    fn test_tail_window_peak_adjustment_duplicate_max() {
        let mut analyzer = WindowRmsAnalyzer::new(48000, false);

        // 完整窗口
        let full_window = vec![0.5f32; 144000];
        analyzer.process_samples(&full_window);

        // 尾窗：有多个0.7的样本（包括最后一个）
        let mut tail = vec![0.3f32; 500];
        tail.extend_from_slice(&[0.7f32; 500]); // 添加多个最大值
        tail.push(0.7f32); // 最后样本也是最大值
        analyzer.process_samples(&tail);

        // 应该有2个窗口
        assert_eq!(analyzer.window_peaks.len(), 2);

        // 尾窗Peak应该仍是0.7（因为还有其他0.7的样本）
        let tail_peak = analyzer.window_peaks[1];
        assert!(
            (tail_peak - 0.7).abs() < 1e-6,
            "尾窗Peak应该保持0.7（还有其他最大值），实际={tail_peak}"
        );
    }

    /// 🚀 **Phase 1回归测试**: 尾窗最后样本不是最大值
    ///
    /// 验证流式双峰跟踪在尾窗排除非最大值样本时保持Peak不变的正确性
    #[test]
    fn test_tail_window_peak_adjustment_non_max() {
        let mut analyzer = WindowRmsAnalyzer::new(48000, false);

        // 完整窗口
        let full_window = vec![0.5f32; 144000];
        analyzer.process_samples(&full_window);

        // 尾窗：最大值在中间，最后样本较小
        let mut tail = vec![0.3f32; 500];
        tail.push(0.9f32); // 最大值在中间
        tail.extend_from_slice(&[0.3f32; 500]); // 后面都是较小值
        tail.push(0.4f32); // 最后样本不是最大值
        analyzer.process_samples(&tail);

        // 应该有2个窗口
        assert_eq!(analyzer.window_peaks.len(), 2);

        // 尾窗Peak应该是0.9（排除最后的0.4不影响，因为0.4不是最大值）
        let tail_peak = analyzer.window_peaks[1];
        assert!(
            (tail_peak - 0.9).abs() < 1e-6,
            "尾窗Peak应该保持0.9（最后样本不是最大值），实际={tail_peak}"
        );
    }

    /// 🧪 Phase 4.2: 20%采样边界测试 - 小segment计数
    ///
    /// 测试当window_rms_values非常少（1-5个）时，20%采样逻辑的正确性
    #[test]
    fn test_20_percent_sampling_small_segments() {
        let mut analyzer = WindowRmsAnalyzer::new(48000, false);

        // 测试1: seg_cnt = 1 (只有1个窗口)
        let samples_1_window = vec![0.5f32; 144000]; // 恰好1个完整窗口
        analyzer.process_samples(&samples_1_window);

        let rms_20_1 = analyzer.calculate_20_percent_rms();
        assert!(rms_20_1 > 0.0, "1个窗口时RMS应该大于0");

        // 清空analyzer
        analyzer.clear();

        // 测试2: seg_cnt = 3 (3个窗口)
        let samples_3_windows = vec![0.5f32; 432000]; // 3个完整窗口
        analyzer.process_samples(&samples_3_windows);

        let rms_20_3 = analyzer.calculate_20_percent_rms();
        assert!(rms_20_3 > 0.0, "3个窗口时RMS应该大于0");
        assert_eq!(analyzer.window_rms_values.len(), 3, "应该有3个窗口RMS值");

        // 清空analyzer
        analyzer.clear();

        // 测试3: seg_cnt = 5 (5个窗口)
        let samples_5_windows = vec![0.5f32; 720000]; // 5个完整窗口
        analyzer.process_samples(&samples_5_windows);

        let rms_20_5 = analyzer.calculate_20_percent_rms();
        assert!(rms_20_5 > 0.0, "5个窗口时RMS应该大于0");
        assert_eq!(analyzer.window_rms_values.len(), 5, "应该有5个窗口RMS值");

        // 验证20%采样逻辑：5个窗口 → ceil(5 * 0.2) = 1个窗口被选中
        // 由于所有窗口RMS相同，结果应该等于单个窗口的RMS
        let window_rms = analyzer.window_rms_values[0];
        assert!(
            (rms_20_5 - window_rms).abs() < 1e-6,
            "5个相同窗口的20%采样应该等于单个窗口RMS"
        );
    }

    /// 🧪 Phase 4.2: 20%采样边界测试 - 大segment计数
    ///
    /// 测试当window_rms_values非常多（1000+）时，20%采样逻辑的正确性和性能
    ///
    /// ⚠️ 此测试包含硬性时间门限（<10ms），在不同CI环境或低性能机器上易偶发失败。
    /// 已标记为 #[ignore] 以避免CI抖动。使用以下命令手动执行性能测试：
    /// `cargo test --release -- --ignored`
    #[test]
    #[ignore]
    fn test_20_percent_sampling_large_segments() {
        let mut analyzer = WindowRmsAnalyzer::new(48000, false);

        // 生成1000个窗口的样本数据
        // 每个窗口48000 * 3.0 = 144000样本
        let window_size = 144000;
        let num_windows = 1000;
        let total_samples = window_size * num_windows;

        // 使用不同的RMS值模拟真实音频（梯度分布）
        let mut samples = Vec::with_capacity(total_samples);
        for i in 0..num_windows {
            // 创建不同强度的窗口：RMS从0.1到1.0
            let intensity = 0.1 + (i as f32 / num_windows as f32) * 0.9;
            let window_samples = vec![intensity; window_size];
            samples.extend_from_slice(&window_samples);
        }

        analyzer.process_samples(&samples);

        // 验证窗口数量
        assert_eq!(
            analyzer.window_rms_values.len(),
            num_windows,
            "应该有1000个窗口"
        );

        // 计算20% RMS
        let rms_20 = analyzer.calculate_20_percent_rms();
        assert!(rms_20 > 0.0, "1000个窗口的20% RMS应该大于0");

        // 验证20%采样逻辑：1000个窗口 → floor(1000 * 0.2).max(1) = 200个最响窗口被选中
        let mut sorted_rms = analyzer.window_rms_values.clone();
        sorted_rms.sort_by(|a, b| b.partial_cmp(a).unwrap()); // 降序排序

        let top_20_percent_count = ((num_windows as f64 * 0.2).floor() as usize).max(1);
        assert_eq!(top_20_percent_count, 200, "应该选中200个最响窗口");

        // 计算前200个最响窗口的RMS：平方和的平均值再开方
        let top_200_square_sum: f64 = sorted_rms[0..200].iter().map(|x| x * x).sum();
        let expected_rms_20 = (top_200_square_sum / 200.0).sqrt();

        // 由于梯度分布，20% RMS应该接近高强度窗口的平方平均根
        assert!(
            (rms_20 - expected_rms_20).abs() < 0.01,
            "20% RMS应该等于前200个最响窗口的平方平均根，实际={rms_20}, 预期={expected_rms_20}"
        );

        // 性能验证：1000个窗口的排序应该非常快（<10ms）
        let start = std::time::Instant::now();
        let _rms_again = analyzer.calculate_20_percent_rms();
        let duration = start.elapsed();
        assert!(
            duration.as_millis() < 10,
            "1000个窗口的20%采样计算应该在10ms内完成，实际={duration:?}"
        );
    }

    /// 🧪 Phase 4.3: 虚拟0窗口一致性测试
    ///
    /// 测试虚拟0窗口逻辑在各种场景下的正确性和一致性
    #[test]
    fn test_virtual_zero_window_consistency() {
        let mut analyzer = WindowRmsAnalyzer::new(48000, false);
        let window_len = 144000; // 3秒 @ 48kHz

        // 场景1: 恰好1个完整窗口（应该添加虚拟0窗口）
        let samples_exact_1 = vec![0.5f32; window_len];
        analyzer.process_samples(&samples_exact_1);

        assert_eq!(
            analyzer.window_rms_values.len(),
            1,
            "恰好1个窗口应该产生1个RMS值"
        );
        assert_eq!(
            analyzer.total_samples_processed, window_len,
            "total_samples应该等于window_len"
        );

        // 验证虚拟0窗口：整除 → 添加虚拟0
        let has_virtual_zero_1 = analyzer.total_samples_processed.is_multiple_of(window_len);
        assert!(has_virtual_zero_1, "恰好整除时应该标记为需要虚拟0窗口");

        // 清空analyzer
        analyzer.clear();

        // 场景2: 恰好3个完整窗口（应该添加虚拟0窗口）
        let samples_exact_3 = vec![0.5f32; window_len * 3];
        analyzer.process_samples(&samples_exact_3);

        assert_eq!(
            analyzer.window_rms_values.len(),
            3,
            "恰好3个窗口应该产生3个RMS值"
        );

        let has_virtual_zero_3 = analyzer.total_samples_processed.is_multiple_of(window_len);
        assert!(has_virtual_zero_3, "恰好整除时应该标记为需要虚拟0窗口");

        // 清空analyzer
        analyzer.clear();

        // 场景3: 1个完整窗口 + 部分样本（不应该添加虚拟0窗口）
        let partial_samples = window_len + 1000; // 多1000个样本
        let samples_partial = vec![0.5f32; partial_samples];
        analyzer.process_samples(&samples_partial);

        assert_eq!(
            analyzer.window_rms_values.len(),
            2,
            "1个完整窗口+尾窗应该产生2个RMS值"
        );

        let has_virtual_zero_partial = analyzer.total_samples_processed.is_multiple_of(window_len);
        assert!(!has_virtual_zero_partial, "有尾部样本时不应该添加虚拟0窗口");

        // 清空analyzer
        analyzer.clear();

        // 场景4: 多次分批处理，恰好整除（应该添加虚拟0窗口）
        // 注意：每次process_samples调用都会处理尾窗，所以分批调用会产生中间尾窗RMS
        let batch1 = vec![0.5f32; window_len / 2]; // 0.5个窗口
        let batch2 = vec![0.5f32; window_len / 2]; // 0.5个窗口
        analyzer.process_samples(&batch1);
        analyzer.process_samples(&batch2);

        assert_eq!(
            analyzer.window_rms_values.len(),
            2,
            "分批处理：batch1产生1个尾窗RMS，batch2完成窗口后产生1个完整窗口RMS，共2个"
        );

        let has_virtual_zero_batched = analyzer.total_samples_processed.is_multiple_of(window_len);
        assert!(
            has_virtual_zero_batched,
            "分批处理但总样本数恰好整除时应该添加虚拟0窗口"
        );

        // 清空analyzer
        analyzer.clear();

        // 场景5: 零样本（特殊边界情况）
        assert_eq!(
            analyzer.window_rms_values.len(),
            0,
            "未处理样本时应该没有RMS值"
        );

        let rms_zero = analyzer.calculate_20_percent_rms();
        assert_eq!(rms_zero, 0.0, "空analyzer的20% RMS应该为0");
    }

    /// 🚀 **O(n)优化验证**: 验证 find_top_two 与排序方法的等价性
    ///
    /// 确保 O(n) 单遍扫描算法与 O(n log n) 排序方法返回相同的结果
    #[test]
    fn test_find_top_two_equivalence() {
        // 测试用例1: 基础情况
        let values1 = vec![0.3, 0.9, 0.5, 0.1, 0.8];
        let (max1, second1) = WindowRmsAnalyzer::find_top_two(&values1, false);
        assert!((max1 - 0.9).abs() < 1e-10, "最大值应该是0.9");
        assert!((second1 - 0.8).abs() < 1e-10, "次大值应该是0.8");

        // 测试用例2: 重复值
        let values2 = vec![0.5, 0.8, 0.8, 0.3];
        let (max2, second2) = WindowRmsAnalyzer::find_top_two(&values2, false);
        assert!((max2 - 0.8).abs() < 1e-10, "最大值应该是0.8");
        assert!((second2 - 0.8).abs() < 1e-10, "次大值也应该是0.8（重复值）");

        // 测试用例3: 单一值
        let values3 = vec![0.5];
        let (max3, second3) = WindowRmsAnalyzer::find_top_two(&values3, false);
        assert!((max3 - 0.5).abs() < 1e-10);
        assert!((second3 - 0.5).abs() < 1e-10);

        // 测试用例4: 包含0的值（测试普通比较对0.0的处理）
        let values4 = vec![0.5, 0.9, 0.3, 0.0];
        let (max4, second4) = WindowRmsAnalyzer::find_top_two(&values4, false);
        assert!((max4 - 0.9).abs() < 1e-10);
        assert!((second4 - 0.5).abs() < 1e-10);

        // 测试用例5: 所有相同值
        let values5 = vec![0.7, 0.7, 0.7];
        let (max5, second5) = WindowRmsAnalyzer::find_top_two(&values5, false);
        assert!((max5 - 0.7).abs() < 1e-10);
        assert!((second5 - 0.7).abs() < 1e-10);

        // 测试用例6: 虚拟窗语义验证
        // 当 has_virtual_zero=true 时，虚拟0被考虑进排序
        let values_vz = vec![0.5, 0.9, 0.3];
        let (max_vz, second_vz) = WindowRmsAnalyzer::find_top_two(&values_vz, true);
        assert!((max_vz - 0.9).abs() < 1e-10, "有虚拟0时最大值仍为0.9");
        assert!((second_vz - 0.5).abs() < 1e-10, "有虚拟0时次大值为0.5");

        // 测试用例7: 对比排序方法验证结果一致性
        let test_values = vec![
            vec![1.0, 2.0, 3.0, 4.0, 5.0],
            vec![5.0, 4.0, 3.0, 2.0, 1.0],
            vec![3.0, 1.0, 4.0, 1.0, 5.0],
            vec![0.0, 0.5, 0.9, 0.1],
            vec![1.0],
            vec![1.0, 1.0],
        ];

        for values in test_values {
            let (max_our, second_our) = WindowRmsAnalyzer::find_top_two(&values, false);

            // 排序方法（参考实现）
            let mut sorted = values.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let max_ref = sorted[sorted.len() - 1];
            let second_ref = if sorted.len() >= 2 {
                sorted[sorted.len() - 2]
            } else {
                sorted[0]
            };

            assert!(
                (max_our - max_ref).abs() < 1e-10,
                "最大值不匹配: our={max_our}, ref={max_ref}, values={values:?}"
            );
            assert!(
                (second_our - second_ref).abs() < 1e-10,
                "次大值不匹配: our={second_our}, ref={second_ref}, values={values:?}"
            );
        }
    }
}
