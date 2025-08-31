//! 10001-bin直方图和20%采样算法
//!
//! 基于foobar2000 DR Meter逆向分析实现的高精度直方图统计和采样算法。
//! 核心修正：使用3秒窗口RMS分布而不是单样本幅度分布

use crate::error::{AudioError, AudioResult};

/// 3秒窗口RMS分析器
///
/// 实现foobar2000 "最响20%" 的正确统计对象：
/// - 以3秒为窗口累计平方和，计算窗口RMS
/// - 把窗口RMS值填入直方图（而不是单样本绝对值）
/// - 确保"最响20%"指的是"RMS最高的20%窗口"
#[derive(Debug, Clone)]
pub struct WindowRmsAnalyzer {
    /// 窗口长度（样本数）- 符合官方DR测量标准
    window_len: usize,

    /// 当前窗口的平方和累积
    current_sum_sq: f64,

    /// 当前窗口的样本计数
    current_count: usize,

    /// 所有窗口RMS值的直方图
    histogram: DrHistogram,
}

/// 10000-bin直方图容器
///
/// 实现foobar2000 DR Meter的官方标准直方图统计：
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
            current_count: 0,
            histogram: DrHistogram::new(),
        }
    }

    /// 处理单声道样本，按3秒窗口计算RMS并填入直方图
    ///
    /// # 参数
    ///
    /// * `samples` - 单声道f32样本数组
    pub fn process_channel(&mut self, samples: &[f32]) {
        for &sample in samples {
            let sample_f64 = sample as f64;
            self.current_sum_sq += sample_f64 * sample_f64;
            self.current_count += 1;

            // 窗口满了，计算窗口RMS并添加到直方图
            if self.current_count >= self.window_len {
                // ✅ 官方标准RMS公式：RMS = sqrt(2 * sum(smp_i^2) / n)
                let window_rms = (2.0 * self.current_sum_sq / self.current_count as f64).sqrt();
                self.histogram.add_window_rms(window_rms);

                // 重置窗口
                self.current_sum_sq = 0.0;
                self.current_count = 0;
            }
        }

        // 处理不足一个窗口的剩余样本
        if self.current_count > 0 {
            // ✅ 官方标准RMS公式：RMS = sqrt(2 * sum(smp_i^2) / n)
            let window_rms = (2.0 * self.current_sum_sq / self.current_count as f64).sqrt();
            self.histogram.add_window_rms(window_rms);

            // 重置状态
            self.current_sum_sq = 0.0;
            self.current_count = 0;
        }
    }

    /// 计算"最响20%窗口"的加权RMS值
    ///
    /// 使用foobar2000的精确算法：
    /// 1. 逆向遍历直方图找到最响20%窗口
    /// 2. 对选中窗口用1e-8×index²加权求和
    /// 3. 除以窗口数并开方得到最终RMS
    pub fn calculate_20_percent_rms(&self) -> f64 {
        self.histogram.calculate_weighted_20_percent_rms()
    }

    /// 获取总窗口数
    pub fn total_windows(&self) -> u64 {
        self.histogram.total_windows()
    }

    /// 清空分析器状态
    pub fn clear(&mut self) {
        self.current_sum_sq = 0.0;
        self.current_count = 0;
        self.histogram.clear();
    }

    /// 获取窗口统计信息
    pub fn get_statistics(&self) -> WindowStats {
        let mut non_zero_bins = 0;
        let mut min_rms = f64::INFINITY;
        let mut max_rms: f64 = 0.0;

        for (index, &count) in self.histogram.bins().iter().enumerate() {
            if count > 0 {
                non_zero_bins += 1;
                let rms = index as f64 / 10000.0;
                min_rms = min_rms.min(rms);
                max_rms = max_rms.max(rms);
            }
        }

        if min_rms == f64::INFINITY {
            min_rms = 0.0;
        }

        WindowStats {
            total_windows: self.histogram.total_windows(),
            non_zero_bins,
            min_rms,
            max_rms,
            rms_20_percent: self.calculate_20_percent_rms(),
        }
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

    /// 获取bin数据（供WindowRmsAnalyzer使用）
    pub(crate) fn bins(&self) -> &[u64] {
        &self.bins
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

    /// 实现foobar2000加权均值+开方的20%RMS计算
    ///
    /// 正确的foobar2000算法：
    /// 1. 从高RMS向低RMS逆向遍历，选取最响20%窗口
    /// 2. 对选中窗口用1e-8×index²进行加权求和
    /// 3. 除以选中窗口总数并开方得到最终RMS
    ///
    /// # 返回值
    ///
    /// 返回加权计算的20%RMS值，如果直方图为空则返回0.0
    ///
    /// # 算法核心
    ///
    /// ```text
    /// need = (total_windows * 0.2 + 0.5) as u64  // foobar精确舍入
    /// sum_sq = 0; selected = 0;
    /// for idx from 9999 down to 0:
    ///   take = min(bins[idx], need - selected)
    ///   if take > 0:
    ///     sum_sq += take * 1e-8 * (idx * idx)
    ///     selected += take
    ///   if selected >= need: break
    /// rms_20 = sqrt(sum_sq / selected)
    /// ```
    fn calculate_weighted_20_percent_rms(&self) -> f64 {
        if self.total_windows == 0 {
            return 0.0;
        }

        // 验证直方图数据完整性
        if let Err(e) = self.validate() {
            eprintln!("⚠️ 直方图验证失败: {e}");
            return 0.0;
        }

        // 计算需要选择的窗口数（foobar2000精确舍入）
        // 🔧 修复：至少选择1个窗口，避免0窗口的情况
        let need = ((self.total_windows as f64 * 0.2 + 0.5) as u64).max(1);
        let mut left = need;
        let mut weighted_sum = 0.0;

        // 从高RMS向低RMS逆向遍历，累积加权平方和
        for index in (0..=9999).rev() {
            let take = self.bins[index].min(left);
            if take > 0 {
                // 🔧 修复加权计算：将index转换回RMS值并计算平方和
                let rms_value = index as f64 / 10000.0;
                weighted_sum += take as f64 * rms_value * rms_value;
                left -= take;

                if left == 0 {
                    break;
                }
            }
        }

        // 计算最终RMS：开方(加权和/选中窗口数)
        if need > 0 {
            (weighted_sum / need as f64).sqrt()
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

    /// 验证直方图完整性
    fn validate(&self) -> AudioResult<()> {
        // 检查bin数量
        if self.bins.len() != 10000 {
            return Err(AudioError::CalculationError(format!(
                "直方图bin数量错误: 期望10000，实际{}",
                self.bins.len()
            )));
        }

        // 检查总窗口数一致性
        let computed_total: u64 = self.bins.iter().sum();
        if computed_total != self.total_windows {
            return Err(AudioError::CalculationError(format!(
                "直方图窗口数不一致: 计算值{}，记录值{}",
                computed_total, self.total_windows
            )));
        }

        Ok(())
    }
}

/// 窗口统计信息
#[derive(Debug, Clone)]
pub struct WindowStats {
    /// 总窗口数量
    pub total_windows: u64,

    /// 非零bin数量
    pub non_zero_bins: usize,

    /// 最小窗口RMS值
    pub min_rms: f64,

    /// 最大窗口RMS值  
    pub max_rms: f64,

    /// 最响20%窗口的加权RMS值
    pub rms_20_percent: f64,
}

impl Default for DrHistogram {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for WindowStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "WindowStats {{ windows: {}, bins: {}, rms_range: {:.6}-{:.6}, rms_20%: {:.6} }}",
            self.total_windows, self.non_zero_bins, self.min_rms, self.max_rms, self.rms_20_percent
        )
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
    fn test_foobar_rounding() {
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
    fn test_statistics() {
        let mut analyzer = WindowRmsAnalyzer::new(100);

        // 添加几个不同RMS的窗口
        let amplitudes = [0.1, 0.3, 0.5, 0.7, 0.9];
        for &amplitude in &amplitudes {
            let samples: Vec<f32> = (0..300).map(|_| amplitude).collect();
            analyzer.process_channel(&samples);
        }

        let stats = analyzer.get_statistics();
        assert_eq!(stats.total_windows, 5);
        assert!(stats.non_zero_bins > 0);
        assert!(stats.min_rms > 0.0);
        assert!(stats.max_rms <= 1.0);
        assert!(stats.rms_20_percent > 0.0);
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
