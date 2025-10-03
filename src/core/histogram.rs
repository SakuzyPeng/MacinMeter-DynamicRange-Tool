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
    /// 当前窗口样本缓存（用于尾窗Peak重新计算）
    current_window_samples: Vec<f64>,
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
            current_window_samples: Vec::new(),
            simd_processor: SimdProcessor::new(),
        }
    }

    /// 处理单声道样本，按3秒窗口计算RMS并填入直方图
    pub fn process_samples(&mut self, samples: &[f32]) {
        // 记录总样本数
        self.total_samples_processed += samples.len();

        for &sample in samples {
            let sample_f64 = sample as f64;
            let abs_sample = sample_f64.abs();

            // 🔧 **dr14兼容性**: 保存当前样本作为潜在的"最后样本"
            self.last_sample = sample_f64;

            // 🔧 **方案A**: 维护当前窗口样本缓存，用于尾窗Peak重新计算
            self.current_window_samples.push(sample_f64);

            // 更新当前窗口的平方和和Peak值
            self.current_sum_sq += sample_f64 * sample_f64;
            self.current_peak = self.current_peak.max(abs_sample);
            self.current_count += 1;

            // 窗口满了，计算窗口RMS和Peak并添加到直方图
            if self.current_count >= self.window_len {
                // ✅ 官方标准RMS公式：RMS = sqrt(2 * sum(smp_i^2) / n)
                let window_rms = (2.0 * self.current_sum_sq / self.current_count as f64).sqrt();
                self.histogram.add_window_rms(window_rms);

                // ✅ 记录窗口Peak值用于后续排序
                self.window_peaks.push(self.current_peak);

                // 🔧 **关键修复**: 直接存储RMS值避免量化损失
                self.window_rms_values.push(window_rms);

                // 重置窗口
                self.current_sum_sq = 0.0;
                self.current_peak = 0.0;
                self.current_count = 0;
                self.current_window_samples.clear(); // 清理样本缓存
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
                let window_rms = (2.0 * adjusted_sum_sq / adjusted_count as f64).sqrt();
                self.histogram.add_window_rms(window_rms);
                self.window_rms_values.push(window_rms);

                // 🎯 **方案A**: 精确重新计算Peak值，排除最后一个样本
                let adjusted_peak = if self.current_window_samples.len() > 1 {
                    self.current_window_samples[..self.current_window_samples.len() - 1]
                        .iter()
                        .map(|&s| s.abs())
                        .fold(0.0, f64::max)
                } else {
                    0.0
                };
                self.window_peaks.push(adjusted_peak);
            } else {
                // 尾窗只有1个样本时会完全跳过
            }

            // 重置状态
            self.current_sum_sq = 0.0;
            self.current_peak = 0.0;
            self.current_count = 0;
            self.current_window_samples.clear(); // 清理样本缓存
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

        // 步骤2: 构建RMS数组
        let mut rms_array = vec![0.0; seg_cnt];
        // 复制实际RMS值
        for (i, &rms) in self.window_rms_values.iter().enumerate() {
            rms_array[i] = rms;
        }
        // 如果has_virtual_zero为true，最后一个位置保持0.0

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

        // 🎯 **关键修复**: 判断是否需要虚拟0窗
        let has_virtual_zero = self.total_samples_processed.is_multiple_of(self.window_len);
        let seg_cnt = if has_virtual_zero {
            self.window_peaks.len() + 1 // 恰好整除：添加0窗
        } else {
            self.window_peaks.len() // 有尾窗：不添加0窗
        };

        // 步骤2: 创建peaks数组
        let mut peaks_array = vec![0.0; seg_cnt];
        for (i, &peak) in self.window_peaks.iter().enumerate() {
            peaks_array[i] = peak;
        }
        // 如果has_virtual_zero为true，最后一个位置保持为0.0

        // 步骤3: 升序排序
        // 使用total_cmp安全处理NaN：NaN会被排序到最后
        peaks_array.sort_by(|a, b| a.total_cmp(b));

        // 步骤4: 选择peaks[seg_cnt-1]位置的值（最大值）
        peaks_array[seg_cnt - 1]
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

        // 🎯 **关键修复**: 判断是否需要虚拟0窗
        let has_virtual_zero = self.total_samples_processed.is_multiple_of(self.window_len);
        let seg_cnt = if has_virtual_zero {
            self.window_peaks.len() + 1 // 恰好整除：添加0窗
        } else {
            self.window_peaks.len() // 有尾窗：不添加0窗
        };

        // 步骤2: 创建peaks数组
        let mut peaks_array = vec![0.0; seg_cnt];
        for (i, &peak) in self.window_peaks.iter().enumerate() {
            peaks_array[i] = peak;
        }
        // 如果has_virtual_zero为true，最后一个位置保持为0.0

        // 步骤3: 升序排序
        // 使用total_cmp安全处理NaN：NaN会被排序到最后
        peaks_array.sort_by(|a, b| a.total_cmp(b));

        // 步骤4: 选择peaks[seg_cnt-2]位置的值
        if seg_cnt >= 2 {
            peaks_array[seg_cnt - 2]
        } else {
            // 只有1个Peak时，使用该Peak
            peaks_array[0]
        }
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
        self.current_window_samples.clear();
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
}
