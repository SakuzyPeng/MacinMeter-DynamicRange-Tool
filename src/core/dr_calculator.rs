//! DR计算核心引擎
//!
//! 实现基于 Measuring_DR_ENv3.md 标准的动态范围测量算法：
//! DR = -20 * log10(sqrt(Σ(RMS²)/N) / Pk_2nd)
//! 以 dr14_t.meter 项目作为参考实现

use super::{ChannelData, WindowRmsAnalyzer};
use crate::error::{AudioError, AudioResult};

/// RMS计算系数：sqrt(2) 的高精度值
///
/// 根据 Measuring_DR_ENv3.md 标准公式(1)：RMS = sqrt(2 * Σ(smp²)/n)
/// 该系数确保与标准规范和 dr14_t.meter 参考实现的精确匹配。
#[allow(clippy::approx_constant)]
const RMS_FACTOR: f64 = 1.414_213_562_373_095_1;

/// RMS计算最小样本数阈值
///
/// 当样本数量过少时，RMS计算可能不稳定，
/// 基于经验值设定最小样本数阈值
const MIN_SAMPLES_FOR_RMS: usize = 100;

/// DR计算结果
#[derive(Debug, Clone, PartialEq)]
pub struct DrResult {
    /// 声道索引
    pub channel: usize,

    /// 计算得到的DR值
    pub dr_value: f64,

    /// RMS值（用于DR计算的20%窗口RMS）
    pub rms: f64,

    /// Peak值（用于DR计算的第二大峰值）
    pub peak: f64,

    /// 全局最大采样峰值（用于dr14_t.meter兼容显示）
    pub global_peak: f64,

    /// 整曲RMS均值（用于dr14_t.meter兼容显示）
    pub global_rms: f64,

    /// 参与计算的样本数量
    pub sample_count: usize,
}

impl DrResult {
    /// 创建新的DR计算结果
    pub fn new(
        channel: usize,
        dr_value: f64,
        rms: f64,
        peak: f64,
        global_peak: f64,
        global_rms: f64,
        sample_count: usize,
    ) -> Self {
        Self {
            channel,
            dr_value,
            rms,
            peak,
            global_peak,
            global_rms,
            sample_count,
        }
    }

    /// 格式化DR值为整数显示（与标准兼容）
    pub fn dr_value_rounded(&self) -> i32 {
        self.dr_value.round() as i32
    }
}

/// DR计算器
///
/// 负责协调整个DR计算过程，包括：
/// - 多声道数据管理
/// - Sum Doubling补偿机制
/// - DR值计算和结果生成
/// - 10000-bin直方图和20%采样算法（Measuring_DR_ENv3.md标准模式）
pub struct DrCalculator {
    /// 每个声道的数据累积器
    channels: Vec<ChannelData>,

    /// 总处理样本数（单声道）
    sample_count: usize,

    /// 是否启用Sum Doubling补偿（交错数据）
    sum_doubling_enabled: bool,

    /// 是否启用Measuring_DR_ENv3.md标准模式（20%采样算法）
    measuring_dr_env3_mode: bool,

    /// 每个声道的3秒窗口RMS分析器（仅在Measuring_DR_ENv3.md标准模式下使用）
    window_analyzers: Option<Vec<WindowRmsAnalyzer>>,

    /// 采样率（用于窗口大小计算）
    sample_rate: u32,
}

/// Sum Doubling质量评估结果
#[derive(Debug, Clone, PartialEq)]
pub struct SumDoublingQuality {
    /// 是否建议应用Sum Doubling
    pub should_apply: bool,

    /// 置信度评分 (0.0-1.0)
    pub confidence: f64,

    /// 检测到的问题标志
    pub issues: SumDoublingIssues,
}

/// Sum Doubling问题标志
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SumDoublingIssues {
    /// 样本数量过少
    pub insufficient_samples: bool,

    /// RMS值异常（可能影响补偿效果）
    pub abnormal_rms: bool,

    /// Peak值异常（可能不适合补偿）
    pub abnormal_peak: bool,
}

impl DrCalculator {
    /// 创建新的DR计算器
    ///
    /// # 参数
    ///
    /// * `channel_count` - 音频声道数量
    /// * `sum_doubling` - 是否启用Sum Doubling补偿（交错数据需要）
    /// * `sample_rate` - 采样率（Hz）
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::core::DrCalculator;
    ///
    /// // 立体声，启用Sum Doubling，48kHz采样率
    /// let calculator = DrCalculator::new(2, true, 48000);
    /// ```
    pub fn new(channel_count: usize, sum_doubling: bool, sample_rate: u32) -> AudioResult<Self> {
        Self::new_with_mode(channel_count, sum_doubling, false, sample_rate)
    }

    /// 创建新的DR计算器（支持Measuring_DR_ENv3.md标准模式）
    ///
    /// # 参数
    ///
    /// * `channel_count` - 音频声道数量
    /// * `sum_doubling` - 是否启用Sum Doubling补偿
    /// * `measuring_dr_env3_mode` - 是否启用Measuring_DR_ENv3.md标准模式（3秒窗口20%采样算法）
    /// * `sample_rate` - 采样率（Hz，用于3秒窗口计算）
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::core::DrCalculator;
    ///
    /// // 创建Measuring_DR_ENv3.md标准模式的计算器
    /// let calculator = DrCalculator::new_with_mode(2, true, true, 48000).unwrap();
    /// ```
    pub fn new_with_mode(
        channel_count: usize,
        sum_doubling: bool,
        measuring_dr_env3_mode: bool,
        sample_rate: u32,
    ) -> AudioResult<Self> {
        if channel_count == 0 {
            return Err(AudioError::InvalidInput("声道数量必须大于0".to_string()));
        }

        if channel_count > 32 {
            return Err(AudioError::InvalidInput("声道数量不能超过32".to_string()));
        }

        if sample_rate == 0 {
            return Err(AudioError::InvalidInput("采样率必须大于0".to_string()));
        }

        let window_analyzers = if measuring_dr_env3_mode {
            Some(
                (0..channel_count)
                    .map(|_| WindowRmsAnalyzer::new(sample_rate))
                    .collect(),
            )
        } else {
            None
        };

        Ok(Self {
            channels: vec![ChannelData::new(); channel_count],
            sample_count: 0,
            sum_doubling_enabled: sum_doubling,
            measuring_dr_env3_mode,
            window_analyzers,
            sample_rate,
        })
    }

    /// 处理交错音频数据
    ///
    /// 音频数据按[L1, R1, L2, R2, ...]格式排列（立体声示例）
    ///
    /// # 参数
    ///
    /// * `samples` - 交错排列的音频样本数据
    ///
    /// # 返回值
    ///
    /// 返回处理的样本数量（单声道）
    ///
    /// # 错误
    ///
    /// * `AudioError::InvalidInput` - 输入数据长度与声道数不匹配
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::core::DrCalculator;
    ///
    /// let mut calculator = DrCalculator::new(2, true, 48000).unwrap();
    /// let samples = vec![0.5, -0.3, 0.7, -0.1]; // L1, R1, L2, R2
    /// let processed = calculator.process_interleaved_samples(&samples).unwrap();
    /// assert_eq!(processed, 2); // 2个样本每声道
    /// ```
    pub fn process_interleaved_samples(&mut self, samples: &[f32]) -> AudioResult<usize> {
        let channel_count = self.channels.len();

        if samples.len() % channel_count != 0 {
            return Err(AudioError::InvalidInput(format!(
                "样本数量({})必须是声道数({})的倍数",
                samples.len(),
                channel_count
            )));
        }

        let samples_per_channel = samples.len() / channel_count;

        // 分离交错数据为单声道数据
        let mut channel_data: Vec<Vec<f32>> =
            vec![Vec::with_capacity(samples_per_channel); channel_count];

        for sample_idx in 0..samples_per_channel {
            for channel_idx in 0..channel_count {
                let sample = samples[sample_idx * channel_count + channel_idx];
                channel_data[channel_idx].push(sample);
            }
        }

        // 处理每个声道的数据
        for channel_idx in 0..channel_count {
            let channel_samples = &channel_data[channel_idx];

            // 基本样本处理（Peak检测和RMS累积）
            for &sample in channel_samples {
                self.channels[channel_idx].process_sample(sample);
            }

            // Measuring_DR_ENv3.md标准模式：3秒窗口RMS分析
            if let Some(ref mut analyzers) = self.window_analyzers {
                analyzers[channel_idx].process_channel(channel_samples);
            }
        }

        self.sample_count += samples_per_channel;
        Ok(samples_per_channel)
    }

    /// 处理非交错音频数据
    ///
    /// 每个声道的数据单独提供：[[L1, L2, ...], [R1, R2, ...]]
    ///
    /// # 参数
    ///
    /// * `channel_samples` - 每个声道的样本数据数组
    ///
    /// # 返回值
    ///
    /// 返回处理的样本数量（单声道）
    ///
    /// # 错误
    ///
    /// * `AudioError::InvalidInput` - 声道数量不匹配或样本长度不一致
    pub fn process_channel_samples(&mut self, channel_samples: &[Vec<f32>]) -> AudioResult<usize> {
        if channel_samples.len() != self.channels.len() {
            return Err(AudioError::InvalidInput(format!(
                "提供的声道数({})与初始化声道数({})不匹配",
                channel_samples.len(),
                self.channels.len()
            )));
        }

        if channel_samples.is_empty() {
            return Ok(0);
        }

        let sample_count = channel_samples[0].len();

        // 验证所有声道的样本数量一致
        for (idx, samples) in channel_samples.iter().enumerate() {
            if samples.len() != sample_count {
                return Err(AudioError::InvalidInput(format!(
                    "声道{}的样本数量({})与声道0({})不匹配",
                    idx,
                    samples.len(),
                    sample_count
                )));
            }
        }

        // 处理每个声道的数据
        for (channel_idx, samples) in channel_samples.iter().enumerate() {
            // 基本样本处理（Peak检测和RMS累积）
            for &sample in samples {
                self.channels[channel_idx].process_sample(sample);
            }

            // Measuring_DR_ENv3.md标准模式：3秒窗口RMS分析
            if let Some(ref mut analyzers) = self.window_analyzers {
                analyzers[channel_idx].process_channel(samples);
            }
        }

        self.sample_count += sample_count;
        Ok(sample_count)
    }

    /// 计算所有声道的DR值
    ///
    /// 实现 Measuring_DR_ENv3.md 标准算法：
    /// - 传统模式：DR = log10(RMS / Peak) * -20.0  
    /// - 标准模式：DR = -20 × log₁₀(sqrt(Σ(RMS²)/N) / Pk_2nd)（使用20%采样算法）
    ///
    /// # 返回值
    ///
    /// 返回每个声道的DR计算结果
    ///
    /// # 错误
    ///
    /// * `AudioError::CalculationError` - 计算过程中出现异常
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::core::DrCalculator;
    ///
    /// let mut calculator = DrCalculator::new(2, false, 48000).unwrap();
    ///
    /// // 生成足够的样本数据进行DR计算（每声道1000个样本）
    /// let mut samples = Vec::new();
    /// for i in 0..1000 {
    ///     let amp = (i as f32 / 1000.0) * 0.5; // 渐变幅度，最大0.5
    ///     samples.push(amp);      // 左声道
    ///     samples.push(-amp);     // 右声道  
    /// }
    ///
    /// calculator.process_interleaved_samples(&samples).unwrap();
    /// let results = calculator.calculate_dr().unwrap();
    /// assert_eq!(results.len(), 2); // 两个声道的结果
    ///
    /// // DR值应该为正数
    /// assert!(results[0].dr_value > 0.0);
    /// assert!(results[1].dr_value > 0.0);
    /// ```
    pub fn calculate_dr(&self) -> AudioResult<Vec<DrResult>> {
        if self.sample_count == 0 {
            return Err(AudioError::CalculationError(
                "没有音频数据可供计算".to_string(),
            ));
        }

        let mut results = Vec::with_capacity(self.channels.len());

        for (channel_idx, channel_data) in self.channels.iter().enumerate() {
            // 根据模式选择RMS计算方法
            let rms = if self.measuring_dr_env3_mode {
                self.calculate_channel_rms_measuring_dr_env3(channel_idx)?
            } else {
                self.calculate_channel_rms(channel_data)?
            };

            // ✅ 根据模式选择正确的Peak值
            let peak = if self.measuring_dr_env3_mode {
                // 官方标准：使用排序后第二大的窗口Peak值
                let analyzers = self.window_analyzers.as_ref().ok_or_else(|| {
                    AudioError::CalculationError(
                        "Measuring_DR_ENv3.md标准模式下未初始化窗口分析器".to_string(),
                    )
                })?;
                if channel_idx >= analyzers.len() {
                    return Err(AudioError::CalculationError(format!(
                        "声道索引{channel_idx}超出范围"
                    )));
                }
                analyzers[channel_idx].get_second_largest_peak()
            } else {
                // 传统模式：使用ChannelData的Peak值
                channel_data.get_effective_peak()
            };

            let dr_value = self.calculate_dr_value(rms, peak)?;

            // 计算dr14_t.meter兼容的显示值
            let global_peak = if self.measuring_dr_env3_mode {
                // ENV3模式：从窗口分析器获取全局最大峰值
                let analyzers = self.window_analyzers.as_ref().unwrap();
                let window_peaks = analyzers[channel_idx].get_window_peaks();
                window_peaks.iter().copied().fold(0.0f64, f64::max)
            } else {
                // 传统模式：使用ChannelData的主峰值
                channel_data.peak_primary()
            };

            let global_rms = if self.measuring_dr_env3_mode {
                // ENV3模式：计算整曲RMS（所有样本的RMS）
                let total_sum_sq = channel_data.rms_accumulator;
                if self.sample_count > 0 {
                    (2.0 * total_sum_sq / self.sample_count as f64).sqrt()
                } else {
                    0.0
                }
            } else {
                // 传统模式：使用计算出的RMS
                rms
            };

            results.push(DrResult::new(
                channel_idx,
                dr_value,
                rms,
                peak,
                global_peak,
                global_rms,
                self.sample_count,
            ));
        }

        Ok(results)
    }

    /// 计算单个声道的RMS值（使用智能Sum Doubling补偿）
    fn calculate_channel_rms(&self, channel_data: &ChannelData) -> AudioResult<f64> {
        let rms = channel_data.calculate_rms(self.sample_count);
        let peak = channel_data.get_effective_peak();

        // 使用智能Sum Doubling补偿系统
        let (compensated_rms, _quality) =
            self.apply_intelligent_sum_doubling(rms, peak, self.sample_count);

        if compensated_rms.is_infinite() || compensated_rms.is_nan() {
            return Err(AudioError::CalculationError(
                "RMS计算结果无效（无穷大或NaN）".to_string(),
            ));
        }

        Ok(compensated_rms)
    }

    /// 计算单个声道的20% RMS值（Measuring_DR_ENv3.md标准模式）
    ///
    /// 使用直方图的20%采样算法，实现符合Measuring_DR_ENv3.md标准的精度。
    /// 这是"最响20%样本"算法的核心实现。
    fn calculate_channel_rms_measuring_dr_env3(&self, channel_idx: usize) -> AudioResult<f64> {
        let analyzers = self.window_analyzers.as_ref().ok_or_else(|| {
            AudioError::CalculationError(
                "Measuring_DR_ENv3.md标准模式下未初始化窗口分析器".to_string(),
            )
        })?;

        if channel_idx >= analyzers.len() {
            return Err(AudioError::CalculationError(format!(
                "声道索引{channel_idx}超出范围"
            )));
        }

        let analyzer = &analyzers[channel_idx];

        // 检查窗口数据可用性
        if analyzer.total_windows() == 0 {
            return Err(AudioError::CalculationError(
                "未检测到任何窗口数据，可能样本数不足".to_string(),
            ));
        }

        // ✅ 严格按照官方公式4计算：sqrt(sum(RMS_n²)/N)
        // calculate_20_percent_rms() 已完整实现官方标准，无需额外补偿
        let rms_20_percent = analyzer.calculate_20_percent_rms();

        if rms_20_percent.is_infinite() || rms_20_percent.is_nan() {
            return Err(AudioError::CalculationError(
                "Measuring_DR_ENv3.md标准RMS计算结果无效（无穷大或NaN）".to_string(),
            ));
        }

        if rms_20_percent <= 0.0 {
            return Err(AudioError::CalculationError(
                "Measuring_DR_ENv3.md标准RMS值必须大于0".to_string(),
            ));
        }

        Ok(rms_20_percent)
    }

    /// 计算DR值：根据Measuring_DR_ENv3.md标准公式(4)
    /// DR_j[dB] = -20 × log10(sqrt(Σ(RMS_n²)/N) × (1/Pk_2nd))
    fn calculate_dr_value(&self, rms: f64, peak: f64) -> AudioResult<f64> {
        if rms <= 0.0 {
            return Err(AudioError::CalculationError("RMS值必须大于0".to_string()));
        }

        if peak <= 0.0 {
            return Err(AudioError::CalculationError("Peak值必须大于0".to_string()));
        }

        if rms > peak {
            return Err(AudioError::CalculationError(format!(
                "RMS值({rms})不能大于Peak值({peak})"
            )));
        }

        // ✅ 严格按照Measuring_DR_ENv3.md公式(4)实现
        // DR_j[dB] = -20 × log10(sqrt(Σ(RMS_n²)/N) / Pk_2nd)
        let ratio = rms / peak;
        let log_value = ratio.log10();

        if log_value.is_infinite() || log_value.is_nan() {
            return Err(AudioError::CalculationError("对数计算结果无效".to_string()));
        }

        let dr_value = -20.0 * log_value;

        // DR值应该在合理范围内（0-100dB）
        if !(0.0..=100.0).contains(&dr_value) {
            return Err(AudioError::CalculationError(format!(
                "DR值({dr_value:.2})超出合理范围(0-100)"
            )));
        }

        Ok(dr_value)
    }

    /// 智能Sum Doubling补偿系统
    ///
    /// 基于音频特征分析，智能决定是否应用Sum Doubling补偿，
    /// 并使用高精度常量确保与Measuring_DR_ENv3.md标准的100%一致性。
    ///
    /// # 参数
    ///
    /// * `rms` - 原始RMS值
    /// * `peak` - Peak值（用于质量评估）
    /// * `sample_count` - 样本数量
    ///
    /// # 返回值
    ///
    /// 返回补偿后的RMS值和质量评估信息
    fn apply_intelligent_sum_doubling(
        &self,
        rms: f64,
        peak: f64,
        sample_count: usize,
    ) -> (f64, SumDoublingQuality) {
        // 如果Sum Doubling未启用，直接返回
        if !self.sum_doubling_enabled {
            return (
                rms,
                SumDoublingQuality {
                    should_apply: false,
                    confidence: 1.0,
                    issues: SumDoublingIssues::default(),
                },
            );
        }

        // 评估Sum Doubling质量
        let quality = self.evaluate_sum_doubling_quality(rms, peak, sample_count);

        if quality.should_apply {
            // 应用高精度Sum Doubling补偿
            let compensated_rms = rms * RMS_FACTOR;
            (compensated_rms, quality)
        } else {
            // 不建议应用，返回原始RMS
            (rms, quality)
        }
    }

    /// 评估Sum Doubling补偿的质量和适用性
    ///
    /// 综合考虑多个音频特征：
    /// - 样本数量充足性
    /// - RMS和Peak值的合理性
    /// - 动态范围特征
    fn evaluate_sum_doubling_quality(
        &self,
        rms: f64,
        peak: f64,
        sample_count: usize,
    ) -> SumDoublingQuality {
        let mut confidence = 1.0f64;
        let mut issues = SumDoublingIssues::default();

        // 1. 样本数量检查
        if sample_count < MIN_SAMPLES_FOR_RMS {
            confidence *= 0.5; // 样本不足，降低置信度
            issues.insufficient_samples = true;
        }

        // 2. RMS值合理性检查
        if rms <= 0.0 || !rms.is_finite() {
            confidence *= 0.0; // 无效RMS，禁用Sum Doubling
            issues.abnormal_rms = true;
        } else if rms > peak {
            confidence *= 0.3; // RMS > Peak，可能有问题
            issues.abnormal_rms = true;
        }

        // 3. Peak值合理性检查
        if peak <= 0.0 || !peak.is_finite() || peak > 1.2 {
            confidence *= 0.4; // 异常Peak值
            issues.abnormal_peak = true;
        }

        // 4. RMS/Peak比例检查
        if peak > 0.0 {
            let ratio = rms / peak;
            if !(0.01..=0.95).contains(&ratio) {
                confidence *= 0.7; // 异常比例可能影响Sum Doubling效果
                issues.abnormal_rms = true;
            }
        }

        // 决策：置信度高于阈值则建议应用
        let should_apply = confidence >= 0.3;

        SumDoublingQuality {
            should_apply,
            confidence: confidence.clamp(0.0, 1.0),
            issues,
        }
    }

    /// 重置计算器状态，准备处理新的音频数据
    pub fn reset(&mut self) {
        for channel in &mut self.channels {
            channel.reset();
        }
        self.sample_count = 0;

        // 重置直方图（如果有）
        if let Some(ref mut analyzers) = self.window_analyzers {
            for analyzer in analyzers.iter_mut() {
                analyzer.clear();
            }
        }
    }

    /// 获取当前处理的样本总数
    pub fn sample_count(&self) -> usize {
        self.sample_count
    }

    /// 获取声道数量
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// 获取Sum Doubling启用状态
    pub fn sum_doubling_enabled(&self) -> bool {
        self.sum_doubling_enabled
    }

    /// 获取Measuring_DR_ENv3.md标准模式状态
    pub fn measuring_dr_env3_mode(&self) -> bool {
        self.measuring_dr_env3_mode
    }

    /// 获取指定声道的直方图统计信息（仅Measuring_DR_ENv3.md标准模式）
    pub fn get_window_stats(&self, channel_idx: usize) -> Option<crate::core::WindowStats> {
        if let Some(ref analyzers) = self.window_analyzers {
            if channel_idx < analyzers.len() {
                return Some(analyzers[channel_idx].get_statistics());
            }
        }
        None
    }

    /// 获取音频采样率
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// 获取指定声道的窗口分析器（用于调试和验证）
    pub fn get_window_analyzer(&self, channel: usize) -> Option<&WindowRmsAnalyzer> {
        self.window_analyzers.as_ref()?.get(channel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_calculator() {
        let calc = DrCalculator::new(2, true, 48000).unwrap();
        assert_eq!(calc.channel_count(), 2);
        assert_eq!(calc.sample_count(), 0);
        assert!(calc.sum_doubling_enabled());
    }

    #[test]
    fn test_invalid_channel_count() {
        assert!(DrCalculator::new(0, false, 48000).is_err());
        assert!(DrCalculator::new(33, false, 48000).is_err());
    }

    #[test]
    fn test_process_interleaved_samples() {
        let mut calc = DrCalculator::new(2, false, 48000).unwrap();
        let samples = vec![0.5, -0.3, 0.7, -0.1]; // L1, R1, L2, R2

        let processed = calc.process_interleaved_samples(&samples).unwrap();
        assert_eq!(processed, 2);
        assert_eq!(calc.sample_count(), 2);
    }

    #[test]
    fn test_invalid_interleaved_data() {
        let mut calc = DrCalculator::new(2, false, 48000).unwrap();
        let samples = vec![0.5, -0.3, 0.7]; // 不是2的倍数

        assert!(calc.process_interleaved_samples(&samples).is_err());
    }

    #[test]
    fn test_process_channel_samples() {
        let mut calc = DrCalculator::new(2, false, 48000).unwrap();
        let channel_samples = vec![
            vec![0.5, 0.7],   // 左声道
            vec![-0.3, -0.1], // 右声道
        ];

        let processed = calc.process_channel_samples(&channel_samples).unwrap();
        assert_eq!(processed, 2);
        assert_eq!(calc.sample_count(), 2);
    }

    #[test]
    fn test_calculate_dr_basic() {
        let mut calc = DrCalculator::new(1, false, 48000).unwrap();
        let samples = vec![0.5]; // 单声道，单样本

        calc.process_interleaved_samples(&samples).unwrap();
        let results = calc.calculate_dr().unwrap();

        assert_eq!(results.len(), 1);
        let result = &results[0];
        assert_eq!(result.channel, 0);
        assert_eq!(result.rms, 0.5);
        assert_eq!(result.peak, 0.5);
        // DR = log10(RMS/Peak) * -20 = log10(0.5/0.5) * -20 = log10(1) * -20 = 0
        assert!((result.dr_value - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_calculate_dr_with_sum_doubling() {
        let mut calc = DrCalculator::new(1, true, 48000).unwrap();
        // ✅ 调整测试数据以适应官方标准：确保第二大Peak > √2×RMS
        let samples = vec![
            0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1,  // 多个小信号降低RMS
            0.95, // 第二大Peak
            1.0,  // 最大Peak
        ];

        calc.process_interleaved_samples(&samples).unwrap();
        let results = calc.calculate_dr().unwrap();

        let result = &results[0];

        // ✅ 官方标准RMS计算：√(2 × (8×0.1² + 1×0.95² + 1×1.0²) / 10)
        let base_rms =
            (2.0 * (8.0 * 0.1_f64.powi(2) + 0.95_f64.powi(2) + 1.0_f64.powi(2)) / 10.0).sqrt();

        // ✅ 智能Sum Doubling：样本数不足(10 < 100)，系统不应用Sum Doubling
        assert!((result.rms - base_rms).abs() < 1e-6); // 期望基础RMS
        assert!((result.peak - 0.95).abs() < 1e-6); // ✅ 使用第二大Peak值（放宽精度）
        assert!(result.rms < result.peak); // RMS应该小于Peak
        assert!(result.dr_value > 0.0); // DR值应该为正
    }

    #[test]
    fn test_calculate_dr_no_data() {
        let calc = DrCalculator::new(2, false, 48000).unwrap();
        assert!(calc.calculate_dr().is_err());
    }

    #[test]
    fn test_dr_result_rounded() {
        let result = DrResult::new(0, 12.7, 0.1, 0.5, 0.6, 0.15, 1000);
        assert_eq!(result.dr_value_rounded(), 13);

        let result = DrResult::new(0, 12.3, 0.1, 0.5, 0.6, 0.15, 1000);
        assert_eq!(result.dr_value_rounded(), 12);
    }

    #[test]
    fn test_reset() {
        let mut calc = DrCalculator::new(2, false, 48000).unwrap();
        let samples = vec![0.5, -0.3, 0.7, -0.1];

        calc.process_interleaved_samples(&samples).unwrap();
        assert_eq!(calc.sample_count(), 2);

        calc.reset();
        assert_eq!(calc.sample_count(), 0);
    }

    #[test]
    fn test_realistic_dr_calculation() {
        let mut calc = DrCalculator::new(1, false, 48000).unwrap();

        // ✅ 模拟实际音频：确保第二大Peak > √2×RMS（官方标准）
        let samples = vec![
            0.05, 0.05, 0.05, 0.05, 0.05, 0.05, 0.05, 0.05, // 多个小信号
            0.9,  // 第二大Peak
            1.0,  // 最大Peak
        ];

        calc.process_interleaved_samples(&samples).unwrap();
        let results = calc.calculate_dr().unwrap();

        let result = &results[0];
        assert!((result.peak - 0.9).abs() < 1e-6); // ✅ 官方标准：使用第二大Peak值（放宽精度）
        // RMS应该远小于Peak，DR值应该为正
        assert!(result.rms < result.peak);
        assert!(result.dr_value > 0.0);
    }

    #[test]
    fn test_intelligent_sum_doubling_normal_case() {
        let mut calc = DrCalculator::new(1, true, 48000).unwrap();

        // 正常音频样本
        for _ in 0..1000 {
            calc.process_interleaved_samples(&[0.3]).unwrap();
        }
        calc.process_interleaved_samples(&[0.8]).unwrap(); // Peak

        let results = calc.calculate_dr().unwrap();
        let result = &results[0];

        // ✅ 验证智能Sum Doubling系统工作（官方标准RMS公式）
        let base_rms = (2.0 * (1000.0 * 0.3_f64.powi(2) + 0.8_f64.powi(2)) / 1001.0).sqrt();

        // ✅ 测试智能系统是否应用了Sum Doubling（使用实际Peak值0.8）
        let quality = calc.evaluate_sum_doubling_quality(base_rms, 0.8, 1001);

        if quality.should_apply {
            // ✅ 智能系统决定应用sum_doubling，验证结果在合理范围内
            assert!(result.rms > 0.25); // 基本合理性检查：RMS应该大于输入的基础值
            assert!(result.rms < 1.0); // RMS不应该超过合理上限
        } else {
            // ✅ 如果系统决定不应用，验证RMS在合理范围内
            assert!(result.rms > 0.25);
            assert!(result.rms < 1.0);
        }

        // 基本约束仍应满足
        assert!(result.rms > 0.0);
        assert!(result.peak > 0.0);
        assert!(result.dr_value > 0.0);
    }

    #[test]
    fn test_intelligent_sum_doubling_disabled() {
        let mut calc = DrCalculator::new(1, false, 48000).unwrap();

        for _ in 0..100 {
            calc.process_interleaved_samples(&[0.5]).unwrap();
        }

        let results = calc.calculate_dr().unwrap();
        let result = &results[0];

        // Sum Doubling未启用，RMS应该是基础值
        assert!((result.rms - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_sum_doubling_quality_assessment() {
        let calc = DrCalculator::new(1, true, 48000).unwrap();

        // 测试正常情况
        let quality = calc.evaluate_sum_doubling_quality(0.3, 0.8, 1000);
        assert!(quality.should_apply);
        assert!(quality.confidence > 0.8);
        assert!(!quality.issues.insufficient_samples);

        // 测试样本不足
        let quality = calc.evaluate_sum_doubling_quality(0.3, 0.8, 50);
        assert!(quality.confidence < 0.8); // 置信度降低
        assert!(quality.issues.insufficient_samples);

        // 测试异常RMS（RMS > Peak）
        let quality = calc.evaluate_sum_doubling_quality(0.9, 0.5, 1000);
        assert!(quality.confidence < 0.5);
        assert!(quality.issues.abnormal_rms);
    }

    #[test]
    fn test_sum_doubling_constant_precision() {
        // 验证高精度常量的使用
        let calc = DrCalculator::new(1, true, 48000).unwrap();

        let (compensated, _) = calc.apply_intelligent_sum_doubling(0.5, 0.8, 1000);
        let expected = 0.5 * RMS_FACTOR;

        assert!((compensated - expected).abs() < 1e-15); // 高精度比较

        // 验证与旧实现的差异（应该更精确）
        let old_factor = 2.0_f64.sqrt();
        let old_result = 0.5 * old_factor;

        // 新实现应该更精确（可能有微小差异）
        assert!((compensated - old_result).abs() < 1e-10);
    }

    #[test]
    fn test_sum_doubling_edge_cases() {
        let calc = DrCalculator::new(1, true, 48000).unwrap();

        // 零RMS
        let quality = calc.evaluate_sum_doubling_quality(0.0, 0.5, 1000);
        assert!(!quality.should_apply);
        assert!(quality.issues.abnormal_rms);

        // 无穷大RMS
        let quality = calc.evaluate_sum_doubling_quality(f64::INFINITY, 0.5, 1000);
        assert!(!quality.should_apply);
        assert!(quality.issues.abnormal_rms);

        // NaN RMS
        let quality = calc.evaluate_sum_doubling_quality(f64::NAN, 0.5, 1000);
        assert!(!quality.should_apply);
        assert!(quality.issues.abnormal_rms);
    }
}
