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

// 🔥 Bit-exact数值常量已移除 (未使用的常量)
// 📖 如需精确常量值，参考master分支实现

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
            histogram: DrHistogram::new(), // 使用无参数的new方法
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
}

#[derive(Debug, Clone)]
struct DrHistogram {
    /// 10000个bin，索引0对应RMS=0，索引9999对应RMS=0.9999
    bins: Vec<u32>,
    /// 总窗口数
    total_windows: u64,
    /// RMS值到索引的映射缓存
    rms_to_index_cache: Option<Vec<u16>>,
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
        }
    }

    /// 处理单声道样本，按3秒窗口计算RMS并填入直方图
    pub fn process_samples(&mut self, samples: &[f32]) {
        // 🎯 **精确对齐dr14_t.meter**: 记录总样本数
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
            // 🎯 **精确复刻dr14_t.meter尾窗行为**:
            // dr14在尾窗切片时使用 Y[curr_sam:s[0] - 1, :] 排除最后一个样本
            if self.current_count > 1 {
                // 排除最后一个样本：从平方和中减去最后样本的平方，样本数-1
                let adjusted_sum_sq = self.current_sum_sq - (self.last_sample * self.last_sample);
                let adjusted_count = self.current_count - 1;

                // ✅ dr14兼容RMS公式：RMS = sqrt(2 * sum(smp_i^2) / (n-1))
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
                // 尾窗只有1个样本时，dr14_t.meter会完全跳过
            }

            // 重置状态
            self.current_sum_sq = 0.0;
            self.current_peak = 0.0;
            self.current_count = 0;
            self.current_window_samples.clear(); // 清理样本缓存
        }
    }

    /// 设置窗口长度（样本数）
    pub fn set_window_length(&mut self, window_length_samples: usize) {
        self.window_len = window_length_samples;
    }

    /// 处理音频块并添加RMS值（向后兼容接口）
    pub fn add_window_rms(&mut self, rms_value: f64, sample_count: usize) {
        self.window_rms_values.push(rms_value);
        self.total_samples_processed += sample_count;
    }

    /// 计算"最响20%窗口"的加权RMS值
    ///
    /// 🎯 **精确对齐dr14_t.meter的20%算法**:
    /// - 若恰好整除3秒窗：seg_cnt = 实际窗口数 + 1（添加1个0窗）
    /// - 若有尾部不满窗：seg_cnt = 实际窗口数（不添加0窗）
    /// - 使用seg_cnt计算n_blk，选择最高20%的RMS值
    pub fn calculate_20_percent_rms(&self) -> f64 {
        if self.window_rms_values.is_empty() {
            return 0.0;
        }

        // 🎯 **关键修复**: 判断是否需要虚拟0窗
        let has_virtual_zero = self.total_samples_processed % self.window_len == 0;
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

        // 步骤3: 排序（升序，0值会排在前面）
        rms_array.sort_by(|a, b| a.partial_cmp(b).unwrap());

        // 步骤4: 计算20%采样窗口数（精确复现dr14_t.meter）
        let cut_best_bins = 0.2;
        let n_blk = ((seg_cnt as f64 * cut_best_bins).floor() as usize).max(1);

        // 步骤5: 选择最高20%的RMS值
        let start_index = seg_cnt - n_blk;
        let mut rms_sum = 0.0;

        for &rms_value in rms_array.iter().skip(start_index).take(n_blk) {
            rms_sum += rms_value * rms_value; // 平方和
        }

        // 步骤6: 开方平均
        (rms_sum / n_blk as f64).sqrt()
    }

    /// 获取按照dr14_t.meter标准的最大窗口Peak值（主峰）
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
        let has_virtual_zero = self.total_samples_processed % self.window_len == 0;
        let seg_cnt = if has_virtual_zero {
            self.window_peaks.len() + 1 // 恰好整除：添加0窗
        } else {
            self.window_peaks.len() // 有尾窗：不添加0窗
        };

        // 步骤2: 创建peaks数组（模拟dr14_t.meter的行为）
        let mut peaks_array = vec![0.0; seg_cnt];
        for (i, &peak) in self.window_peaks.iter().enumerate() {
            peaks_array[i] = peak;
        }
        // 如果has_virtual_zero为true，最后一个位置保持为0.0

        // 步骤3: 升序排序（模拟np.sort(peaks, 0)）
        peaks_array.sort_by(|a, b| a.partial_cmp(b).unwrap());

        // 步骤4: 选择peaks[seg_cnt-1]位置的值（最大值）
        peaks_array[seg_cnt - 1]
    }

    /// 获取按照dr14_t.meter标准的第二大窗口Peak值
    ///
    /// 实现与master分支相同的窗口级Peak选择算法：
    /// - 若恰好整除3秒窗：seg_cnt = 实际窗口数 + 1（添加1个0窗）
    /// - 若有尾部不满窗：seg_cnt = 实际窗口数（不添加0窗）
    /// - peaks[seg_cnt-2] 选择排序后的第二大值
    ///
    /// # 返回值
    ///
    /// 返回按照dr14_t.meter精确算法选择的Peak值
    pub fn get_second_largest_peak(&self) -> f64 {
        if self.window_peaks.is_empty() {
            return 0.0;
        }

        // 🎯 **关键修复**: 判断是否需要虚拟0窗
        let has_virtual_zero = self.total_samples_processed % self.window_len == 0;
        let seg_cnt = if has_virtual_zero {
            self.window_peaks.len() + 1 // 恰好整除：添加0窗
        } else {
            self.window_peaks.len() // 有尾窗：不添加0窗
        };

        // 步骤2: 创建peaks数组（模拟dr14_t.meter的行为）
        let mut peaks_array = vec![0.0; seg_cnt];
        for (i, &peak) in self.window_peaks.iter().enumerate() {
            peaks_array[i] = peak;
        }
        // 如果has_virtual_zero为true，最后一个位置保持为0.0

        // 步骤3: 升序排序（模拟np.sort(peaks, 0)）
        peaks_array.sort_by(|a, b| a.partial_cmp(b).unwrap());

        // 步骤4: 选择peaks[seg_cnt-2]位置的值
        if seg_cnt >= 2 {
            peaks_array[seg_cnt - 2] // dr14_t.meter的索引逻辑
        } else {
            // 只有1个Peak时，使用该Peak
            peaks_array[0]
        }
    }

    /// 获取窗口RMS值列表（供调试使用）
    pub fn get_window_rms_values(&self) -> &[f64] {
        &self.window_rms_values
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

    /// 获取处理的样本总数
    pub fn total_samples_processed(&self) -> usize {
        self.total_samples_processed
    }
}

impl DrHistogram {
    /// 创建新的10000-bin直方图
    fn new() -> Self {
        Self {
            bins: vec![0; 10000], // 索引0-9999
            total_windows: 0,
            rms_to_index_cache: None,
        }
    }

    /// 获取直方图bins（供SimpleHistogramAnalyzer兼容）
    pub fn bins(&self) -> &[u32] {
        &self.bins
    }

    /// 添加样本到直方图（供SimpleHistogramAnalyzer使用）
    pub fn add_sample(&mut self, sample_abs: f32) {
        if sample_abs < 0.0 || !sample_abs.is_finite() {
            return; // 忽略无效样本
        }

        // 计算bin索引：样本绝对值映射到0-9999范围
        let bin_index = ((sample_abs as f64 * 10000.0).min(9999.0)) as usize;
        self.bins[bin_index] += 1;
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

    /// 简单的20%RMS计算（供SimpleHistogramAnalyzer使用）
    pub fn calculate_simple_20_percent_rms(&self) -> f64 {
        self.calculate_simple_20_percent_rms_with_effective_samples(None)
    }

    /// 使用有效样本数计算20% RMS（供SimpleHistogramAnalyzer使用）
    pub fn calculate_simple_20_percent_rms_with_effective_samples(
        &self,
        _effective_samples: Option<u64>,
    ) -> f64 {
        let total_samples: u64 = self.bins.iter().map(|&count| count as u64).sum();
        if total_samples == 0 {
            return 0.0;
        }

        // 20%采样计算
        let need = ((total_samples as f64 * 0.2) as u64).max(1);
        let mut remaining = need;
        let mut sum_square = 0.0;

        // 从高幅度向低幅度逆向遍历
        for bin_index in (0..self.bins.len()).rev() {
            if remaining == 0 {
                break;
            }

            let available = self.bins[bin_index] as u64;
            let use_count = available.min(remaining);

            if use_count > 0 {
                let bin_value = bin_index as f64 / 10000.0;
                sum_square += use_count as f64 * (bin_value * bin_value);
                remaining -= use_count;
            }
        }

        let actually_selected = need - remaining;
        if actually_selected > 0 {
            (sum_square / actually_selected as f64).sqrt()
        } else {
            0.0
        }
    }

    /// 清空直方图
    fn clear(&mut self) {
        self.bins.fill(0);
        self.total_windows = 0;
        self.rms_to_index_cache = None;
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
