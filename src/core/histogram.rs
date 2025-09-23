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

        // 步骤3: 排序（升序，0值会排在前面）
        rms_array.sort_by(|a, b| a.partial_cmp(b).unwrap());

        // 步骤4: 计算20%采样窗口数
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
        peaks_array.sort_by(|a, b| a.partial_cmp(b).unwrap());

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
        peaks_array.sort_by(|a, b| a.partial_cmp(b).unwrap());

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
