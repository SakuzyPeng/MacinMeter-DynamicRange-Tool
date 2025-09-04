//! 10000-bin直方图和20%采样算法
//!
//! 基于 Measuring_DR_ENv3.md 标准实现的高精度直方图统计和采样算法。
//! 以 dr14_t.meter 作为参考实现，使用3秒窗口RMS分布统计

/// 3秒窗口RMS分析器
///
/// 实现 Measuring_DR_ENv3.md 标准的"上位20%"RMS统计：
/// - 以3秒为窗口累计平方和，计算窗口RMS
/// - 把窗口RMS值填入直方图进行统计
/// - "上位20%"指RMS最高的20%窗口，用于DR计算
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

    /// 所有窗口的Peak值集合（用于排序和选择第二大Peak值，符合标准公式4）
    window_peaks: Vec<f64>,

    /// 🔧 **紧急修复**: 直接存储窗口RMS值以避免直方图量化损失
    /// 当RMS > 0.9999时，直方图量化会造成严重误差
    /// 对于小窗口数量的情况，直接存储更准确
    window_rms_values: Vec<f64>,

    /// 🎯 **精确对齐dr14_t.meter**: 记录整轨样本总数
    /// 用于判断是否需要虚拟0窗口（仅在恰好整除时添加）
    total_samples_processed: usize,

    /// 🔧 **dr14兼容性**: 保存当前窗口的最后一个样本，用于尾窗"丢弃最后采样"逻辑
    last_sample: f64,

    /// 🔧 **方案A**: 当前窗口样本缓存，用于尾窗Peak值精确重新计算
    current_window_samples: Vec<f64>,
}

/// 10000-bin直方图容器
///
/// 实现 Measuring_DR_ENv3.md 标准的直方图统计：
/// - 覆盖索引0-9999，对应RMS值0.0000-0.9999（精度0.0001）
/// - 每个bin统计落在该RMS范围内的**窗口**数量（不是样本数量）
/// - 支持加权均值+开方的20%RMS计算
#[derive(Debug, Clone)]
pub struct DrHistogram {
    /// 10000个bin的窗口计数器（索引0-9999）
    bins: Vec<u64>,

    /// 总窗口数量
    total_windows: u64,

    /// RMS值到索引的映射缓存
    rms_to_index_cache: Option<Vec<u16>>,
}

impl WindowRmsAnalyzer {
    /// 计算符合官方DR测量标准的3秒窗口样本数
    ///
    /// 根据官方DR测量标准 (Measuring_DR_ENv3):
    /// - 44.1kHz 采样率使用 132480 样本 (3 * (44100 + 60))  
    /// - 其他采样率使用标准的 3 * sample_rate
    ///
    /// # 参数
    ///
    /// * `sample_rate` - 采样率（Hz）
    ///
    /// # 返回
    ///
    /// 符合官方标准的窗口样本数
    fn calculate_standard_window_size(sample_rate: u32) -> usize {
        match sample_rate {
            44100 => 132480,                 // 官方标准：44.1kHz使用132480样本
            _ => (3 * sample_rate) as usize, // 其他采样率：标准3秒窗口
        }
    }

    /// 创建3秒窗口RMS分析器
    ///
    /// # 参数
    ///
    /// * `sample_rate` - 采样率（Hz）
    pub fn new(sample_rate: u32) -> Self {
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
    ///
    /// # 参数
    ///
    /// * `samples` - 单声道f32样本数组
    pub fn process_channel(&mut self, samples: &[f32]) {
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
            // 参考: dr14_t.meter/dr14tmeter/compute_dr14.py:68-71
            if self.current_count > 1 {
                // 排除最后一个样本：从平方和中减去最后样本的平方，样本数-1
                let adjusted_sum_sq = self.current_sum_sq - (self.last_sample * self.last_sample);
                let adjusted_count = self.current_count - 1;

                // ✅ dr14兼容RMS公式：RMS = sqrt(2 * sum(smp_i^2) / (n-1))
                let window_rms = (2.0 * adjusted_sum_sq / adjusted_count as f64).sqrt();
                self.histogram.add_window_rms(window_rms);
                self.window_rms_values.push(window_rms);

                // 🎯 **方案A**: 精确重新计算Peak值，排除最后一个样本
                // 重新遍历尾窗样本（除了最后一个）来求真实峰值，与dr14_t.meter完全一致
                let adjusted_peak = if self.current_window_samples.len() > 1 {
                    // 排除最后一个样本，重新计算峰值 (等价于 dr14 的 np.max(abs(Y[curr_sam:s[0]-1, :])))
                    self.current_window_samples[..self.current_window_samples.len() - 1]
                        .iter()
                        .map(|&s| s.abs())
                        .fold(0.0, f64::max)
                } else {
                    // 只有1个样本的情况，Peak应该是0（因为被排除了）
                    0.0
                };
                self.window_peaks.push(adjusted_peak);
            } else {
                // 尾窗只有1个样本时，dr14_t.meter会完全跳过（因为s[0]-1导致空区间）
                // 我们也跳过这种情况，不添加任何窗口数据
            }

            // 重置状态
            self.current_sum_sq = 0.0;
            self.current_peak = 0.0;
            self.current_count = 0;
            self.current_window_samples.clear(); // 清理样本缓存
        }
    }

    /// 获取DR14标准Peak值（精确复现dr14_t.meter的peaks[seg_cnt-2]算法）
    ///
    /// 🎯 **精确对齐dr14_t.meter的Peak选择逻辑**:
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

    /// 获取总窗口数
    pub fn total_windows(&self) -> u64 {
        self.histogram.total_windows()
    }

    /// 获取存储的窗口RMS值（用于调试和验证）
    pub fn get_window_rms_values(&self) -> &[f64] {
        &self.window_rms_values
    }

    /// 获取存储的窗口Peak值（用于全局最大峰值计算）
    pub fn get_window_peaks(&self) -> &[f64] {
        &self.window_peaks
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

    /// 获取总窗口数（供WindowRmsAnalyzer使用）
    pub(crate) fn total_windows(&self) -> u64 {
        self.total_windows
    }

    /// 添加窗口RMS到直方图
    ///
    /// 根据窗口RMS值计算对应的bin索引并增加窗口计数。
    /// bin索引计算：index = (rms * 10000.0).round().min(9999.0) as usize
    ///
    /// # 参数
    ///
    /// * `window_rms` - 3秒窗口的RMS值
    fn add_window_rms(&mut self, window_rms: f64) {
        if window_rms < 0.0 || !window_rms.is_finite() {
            return; // 忽略无效窗口
        }

        // 计算bin索引：RMS映射到0-9999范围
        let index = (window_rms * 10000.0).round().min(9999.0) as usize;

        self.bins[index] += 1;
        self.total_windows += 1;
    }

    // 实现Measuring_DR_ENv3.md标准的20%RMS计算
    //
    // 基于dr14_t.meter的标准算法：
    // 1. 创建包含虚拟窗口的RMS数组（seg_cnt = actual_windows + 1）
    // 2. 对数组进行排序（升序）
    // 3. 选择最高20%的RMS值进行平方和计算
    // 4. 计算均方根：sqrt(sum_squares / count)
    //
    // # 返回值
    //
    // 返回加权计算的20%RMS值，如果直方图为空则返回0.0
    //
    // # 算法核心
    //
    // ```text
    // need = (total_windows * 0.2 + 0.5) as u64  // 标准精确舍入
    // sum_sq = 0; selected = 0;
    // for idx from 9999 down to 0:
    //   take = min(bins[idx], need - selected)
    //   if take > 0:
    //     sum_sq += take * 1e-8 * (idx * idx)
    //     selected += take
    //   if selected >= need: break
    //
    // dr14_t.meter兼容的20%采样算法（基于seg_cnt）
    //
    // 🚨 **关键修复**: 复现dr14_t.meter的完整seg_cnt逻辑
    //
    // dr14_t.meter的实际行为：
    // 1. seg_cnt = 实际窗口数 + 1 （总是+1）
    // 2. 创建大小为seg_cnt的RMS数组
    // 3. 未使用的位置填0（虚拟窗口）
    // 4. 对整个数组排序（0值会排在前面）
    // 5. 基于seg_cnt计算20%窗口数
    // 6. 从排序后的数组选择最高20%

    /// 清空直方图
    fn clear(&mut self) {
        self.bins.fill(0);
        self.total_windows = 0;
        self.rms_to_index_cache = None;
    }
}

impl Default for DrHistogram {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_analyzer_creation() {
        let analyzer = WindowRmsAnalyzer::new(48000);
        assert_eq!(analyzer.total_windows(), 0);
        assert_eq!(analyzer.window_len, 144000); // 3 * 48000
    }

    #[test]
    fn test_window_rms_processing() {
        let mut analyzer = WindowRmsAnalyzer::new(100); // 100Hz采样率，窗口=300样本

        // 创建300个样本的测试数据（正好一个3秒窗口）
        let samples: Vec<f32> = (0..300).map(|i| (i as f32) / 300.0).collect();

        analyzer.process_channel(&samples);

        assert_eq!(analyzer.total_windows(), 1); // 应该生成1个窗口

        let rms_20 = analyzer.calculate_20_percent_rms();
        assert!(rms_20 > 0.0); // 应该有有效的20%RMS值
    }

    #[test]
    fn test_multiple_windows() {
        let mut analyzer = WindowRmsAnalyzer::new(100); // 窗口=300样本

        // 创建900个样本（3个完整窗口）
        let samples: Vec<f32> = (0..900).map(|_| 0.5).collect(); // 恒定幅度0.5

        analyzer.process_channel(&samples);

        assert_eq!(analyzer.total_windows(), 3); // 应该生成3个窗口

        let rms_20 = analyzer.calculate_20_percent_rms();
        // ✅ 官方标准：恒定0.5幅度，RMS = √(2) × 0.5 ≈ 0.707
        assert!((rms_20 - 0.5 * 2.0_f64.sqrt()).abs() < 0.01);
    }

    #[test]
    fn test_partial_window() {
        let mut analyzer = WindowRmsAnalyzer::new(100); // 窗口=300样本

        // 创建450个样本（1个完整窗口+150个部分窗口）
        let samples: Vec<f32> = (0..450).map(|_| 0.3).collect();

        analyzer.process_channel(&samples);

        assert_eq!(analyzer.total_windows(), 2); // 1个完整+1个部分窗口
    }

    #[test]
    fn test_weighted_20_percent_calculation() {
        let mut analyzer = WindowRmsAnalyzer::new(100);

        // 创建多个不同RMS值的窗口
        // 窗口1: 高RMS值（0.9）
        let high_samples: Vec<f32> = (0..300).map(|_| 0.9).collect();
        analyzer.process_channel(&high_samples);

        // 窗口2-5: 低RMS值（0.1）
        for _ in 0..4 {
            let low_samples: Vec<f32> = (0..300).map(|_| 0.1).collect();
            analyzer.process_channel(&low_samples);
        }

        assert_eq!(analyzer.total_windows(), 5);

        let rms_20 = analyzer.calculate_20_percent_rms();

        // 20%的窗口（1个窗口）应该是高RMS值0.9
        // 加权计算应该接近0.9
        assert!(rms_20 > 0.8); // 应该接近最高的RMS值
    }

    #[test]
    fn test_standard_rounding() {
        let mut analyzer = WindowRmsAnalyzer::new(100);

        // 创建11个窗口，20%应该是(11*0.2+0.5)=2.7->3个窗口
        for i in 0..11 {
            let amplitude = (10 - i) as f32 / 10.0; // 递减的RMS值
            let samples: Vec<f32> = (0..300).map(|_| amplitude).collect();
            analyzer.process_channel(&samples);
        }

        assert_eq!(analyzer.total_windows(), 11);

        let rms_20 = analyzer.calculate_20_percent_rms();
        // 前3个最高RMS窗口：1.0, 0.9, 0.8
        // 加权平均后开方应该接近这个范围的值
        assert!(rms_20 > 0.8);
    }

    #[test]
    fn test_clear() {
        let mut analyzer = WindowRmsAnalyzer::new(100);

        let samples: Vec<f32> = (0..300).map(|_| 0.5).collect();
        analyzer.process_channel(&samples);
        assert_eq!(analyzer.total_windows(), 1);

        analyzer.clear();
        assert_eq!(analyzer.total_windows(), 0);
        assert_eq!(analyzer.current_count, 0);
        assert_eq!(analyzer.current_sum_sq, 0.0);
    }

    #[test]
    fn test_window_size_calculation() {
        // 测试44.1kHz的特殊补偿机制
        assert_eq!(
            WindowRmsAnalyzer::calculate_standard_window_size(44100),
            132480, // 官方标准：44.1kHz使用132480样本
            "44.1kHz should use 132480 samples (3 * (44100 + 60))"
        );

        // 测试其他常见采样率使用标准计算
        assert_eq!(
            WindowRmsAnalyzer::calculate_standard_window_size(48000),
            144000, // 3 * 48000
            "48kHz should use standard 3 * sample_rate calculation"
        );

        assert_eq!(
            WindowRmsAnalyzer::calculate_standard_window_size(96000),
            288000, // 3 * 96000
            "96kHz should use standard 3 * sample_rate calculation"
        );

        assert_eq!(
            WindowRmsAnalyzer::calculate_standard_window_size(192000),
            576000, // 3 * 192000
            "192kHz should use standard 3 * sample_rate calculation"
        );

        // 测试创建的分析器确实使用了正确的窗口大小
        let analyzer_44k = WindowRmsAnalyzer::new(44100);
        assert_eq!(analyzer_44k.window_len, 132480);

        let analyzer_48k = WindowRmsAnalyzer::new(48000);
        assert_eq!(analyzer_48k.window_len, 144000);
    }
}
