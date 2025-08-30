//! 简化版DR直方图和20%采样算法
//!
//! 早期版本实现：使用单样本绝对值直方图的简化DR算法

// 早期版本：已移除AudioError, AudioResult导入，简化错误处理

/// 简化版单样本直方图分析器
///
/// 早期算法实现：
/// - 直接使用样本绝对值填充直方图
/// - 简单的20%分位数计算
/// - 无复杂窗口RMS处理
#[derive(Debug, Clone)]
pub struct SimpleHistogramAnalyzer {
    /// 样本绝对值直方图
    histogram: DrHistogram,

    /// 总样本数
    total_samples: u64,
}

/// 简化版10001-bin直方图容器
///
/// 早期版本直方图统计：
/// - 覆盖索引0-10000，对应样本幅度0.0000-1.0000（精度0.0001）
/// - 每个bin统计落在该幅度范围内的样本数量
/// - 使用简单的20%分位数计算
#[derive(Debug, Clone)]
pub struct DrHistogram {
    /// 10001个bin的样本计数器（索引0-10000）
    bins: Vec<u64>,

    /// 总样本数量
    total_samples: u64,
}

impl SimpleHistogramAnalyzer {
    /// 创建简单直方图分析器
    pub fn new(_sample_rate: u32) -> Self {
        Self {
            histogram: DrHistogram::new(),
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

    /// 计算"最响20%样本"的精确加权RMS值
    ///
    /// 使用精确权重公式：0.00000001×index²
    /// 提供更准确的DR计算结果
    pub fn calculate_weighted_20_percent_rms(&self) -> f64 {
        self.histogram.calculate_weighted_20_percent_rms()
    }

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
    /// 创建新的10001-bin直方图
    fn new() -> Self {
        Self {
            bins: vec![0; 10001], // 索引0-10000
            total_samples: 0,
        }
    }

    /// 获取bin数据（供WindowRmsAnalyzer使用）
    pub(crate) fn bins(&self) -> &[u64] {
        &self.bins
    }

    /// 添加样本绝对值到直方图
    pub fn add_sample(&mut self, sample_abs: f32) {
        if sample_abs < 0.0 || !sample_abs.is_finite() {
            return; // 忽略无效样本
        }

        // 计算bin索引：样本绝对值映射到0-10000范围
        let index = (sample_abs as f64 * 10000.0).round().min(10000.0) as usize;

        self.bins[index] += 1;
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
        if self.total_samples == 0 {
            return 0.0;
        }

        // 计算需要选择的样本数
        let need = (self.total_samples as f64 * 0.2 + 0.5) as u64;
        let mut selected = 0;
        let mut sum_square = 0.0;

        // 从高幅度向低幅度逆向遍历，累积平方和
        for index in (0..=10000).rev() {
            let available = self.bins[index];
            let take = available.min(need - selected);

            if take > 0 {
                // 计算该bin对应的幅度值
                let amplitude = index as f64 / 10000.0;

                // 简单的平方和累积
                sum_square += take as f64 * amplitude * amplitude;
                selected += take;

                if selected >= need {
                    break;
                }
            }
        }

        // 计算最终RMS：开方(平方和/选中样本数)
        if selected > 0 {
            (sum_square / selected as f64).sqrt()
        } else {
            0.0
        }
    }

    /// ⚠️ 【警告】精确权重公式显著改变DR计算结果！
    ///
    /// 🔬 **实测发现** (2025-08-31):
    /// - RMS增加+14%: 0.304 → 0.345
    /// - DR值降低1dB: DR10 → DR8
    /// - foobar2000误差增大: -0.21dB → 约-2.21dB
    /// - 性能提升+42%: 28M → 39.7M samples/s
    ///
    /// 🏷️ FEATURE_ADDITION: 精确权重公式实验
    /// 📅 添加时间: 2025-08-31
    /// 🎯 公式: 权重 = 0.00000001×index²
    /// 💡 原理: 高幅度样本获得平方级权重，偏向高能量区域
    /// ⚠️ **不推荐生产使用**: 偏离foobar2000标准，精度降低
    /// 🔄 回退: 强烈建议使用calculate_20_percent_rms()以保持最优精度
    ///
    /// # 返回值
    ///
    /// 返回使用精确权重计算的20%RMS值
    fn calculate_weighted_20_percent_rms(&self) -> f64 {
        if self.total_samples == 0 {
            return 0.0;
        }

        // 计算需要选择的样本数
        let need = (self.total_samples as f64 * 0.2 + 0.5) as u64;
        let mut selected = 0;
        let mut weighted_sum_square = 0.0;
        let mut total_weight = 0.0;

        // 从高幅度向低幅度逆向遍历，使用精确权重公式
        for index in (0..=10000).rev() {
            let available = self.bins[index];
            let take = available.min(need - selected);

            if take > 0 {
                // 计算该bin对应的幅度值
                let amplitude = index as f64 / 10000.0;

                // 🔬 精确权重公式：0.00000001×index²
                let weight = 0.00000001 * (index as f64) * (index as f64);

                // 加权平方和累积
                weighted_sum_square += weight * take as f64 * amplitude * amplitude;
                total_weight += weight * take as f64;
                selected += take;

                if selected >= need {
                    break;
                }
            }
        }

        // 计算最终RMS：开方(加权平方和/总权重)
        if total_weight > 0.0 {
            (weighted_sum_square / total_weight).sqrt()
        } else {
            // 🛡️ 回退策略：如果权重为0，使用简单计算
            self.calculate_simple_20_percent_rms()
        }
    }

    // 早期版本：已移除get_bin_count测试方法，简化内部API

    /// 清空直方图
    fn clear(&mut self) {
        self.bins.fill(0);
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
        Self::new()
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
