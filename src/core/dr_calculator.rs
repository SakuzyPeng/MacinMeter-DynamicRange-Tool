//! DR计算核心引擎
//!
//! 实现foobar2000 DR Meter的核心算法：DR = log10(RMS / Peak) * -20.0

use super::{ChannelData, SimpleHistogramAnalyzer};
use crate::error::{AudioError, AudioResult};

// 早期版本：已移除Sum Doubling相关常量，不再使用RMS补偿机制

/// DR计算结果
#[derive(Debug, Clone, PartialEq)]
pub struct DrResult {
    /// 声道索引
    pub channel: usize,

    /// 计算得到的DR值
    pub dr_value: f64,

    /// RMS值（用于调试和验证）
    pub rms: f64,

    /// Peak值（用于调试和验证）
    pub peak: f64,

    /// 参与计算的样本数量
    pub sample_count: usize,
}

impl DrResult {
    /// 创建新的DR计算结果
    pub fn new(channel: usize, dr_value: f64, rms: f64, peak: f64, sample_count: usize) -> Self {
        Self {
            channel,
            dr_value,
            rms,
            peak,
            sample_count,
        }
    }

    /// 格式化DR值为整数显示（与foobar2000兼容）
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
/// - 10001-bin直方图和20%采样算法（foobar2000兼容模式）
pub struct DrCalculator {
    /// 每个声道的数据累积器
    channels: Vec<ChannelData>,

    /// 总处理样本数（单声道）
    sample_count: usize,

    /// 是否启用Sum Doubling补偿（交错数据）
    sum_doubling_enabled: bool,

    /// 是否启用foobar2000兼容模式（20%采样算法）
    foobar2000_mode: bool,

    /// 每个声道的简单直方图分析器（仅在foobar2000模式下使用）
    histogram_analyzers: Option<Vec<SimpleHistogramAnalyzer>>,

    /// 采样率（用于窗口大小计算）
    sample_rate: u32,

    /// 🏷️ FEATURE_ADDITION: 精确权重实验控制开关
    /// 📅 添加时间: 2025-08-31  
    /// 🎯 目的: 控制是否使用精确权重公式计算20% RMS
    /// 🔄 回退: 如需回退，删除此字段，相关逻辑改为直接使用简单算法
    weighted_rms_enabled: bool,
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

    /// 创建新的DR计算器（支持foobar2000兼容模式）
    ///
    /// # 参数
    ///
    /// * `channel_count` - 音频声道数量
    /// * `sum_doubling` - 是否启用Sum Doubling补偿
    /// * `foobar2000_mode` - 是否启用foobar2000兼容模式（3秒窗口20%采样算法）
    /// * `sample_rate` - 采样率（Hz，用于3秒窗口计算）
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::core::DrCalculator;
    ///
    /// // 创建foobar2000兼容模式的计算器
    /// let calculator = DrCalculator::new_with_mode(2, true, true, 48000).unwrap();
    /// ```
    pub fn new_with_mode(
        channel_count: usize,
        sum_doubling: bool,
        foobar2000_mode: bool,
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

        let window_analyzers = if foobar2000_mode {
            Some(
                (0..channel_count)
                    .map(|_| SimpleHistogramAnalyzer::new(sample_rate))
                    .collect(),
            )
        } else {
            None
        };

        Ok(Self {
            channels: vec![ChannelData::new(); channel_count],
            sample_count: 0,
            sum_doubling_enabled: sum_doubling,
            foobar2000_mode,
            histogram_analyzers: window_analyzers,
            sample_rate,
            // 🏷️ FEATURE_ADDITION: 精确权重实验控制开关初始化
            // 📅 添加时间: 2025-08-31
            // 🎯 默认禁用精确权重公式，确保向后兼容性
            // 🔄 回退: 如需回退，删除此行初始化
            weighted_rms_enabled: false,
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

            // foobar2000模式：3秒窗口RMS分析
            if let Some(ref mut analyzers) = self.histogram_analyzers {
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

            // foobar2000模式：3秒窗口RMS分析
            if let Some(ref mut analyzers) = self.histogram_analyzers {
                analyzers[channel_idx].process_channel(samples);
            }
        }

        self.sample_count += sample_count;
        Ok(sample_count)
    }

    /// 计算所有声道的DR值
    ///
    /// 实现foobar2000的核心算法：
    /// - 传统模式：DR = log10(RMS / Peak) * -20.0  
    /// - foobar2000模式：DR = log10(20%_RMS / Peak) * -20.0（使用20%采样算法）
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
    /// let samples = vec![0.1, -0.1, 0.2, -0.2, 1.0, -1.0];
    /// calculator.process_interleaved_samples(&samples).unwrap();
    ///
    /// let results = calculator.calculate_dr().unwrap();
    /// assert_eq!(results.len(), 2);
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
            let rms = if self.foobar2000_mode {
                self.calculate_channel_rms_foobar2000(channel_idx)?
            } else {
                self.calculate_channel_rms(channel_data)?
            };

            let peak = channel_data.get_effective_peak();
            let dr_value = self.calculate_dr_value_with_fallback(rms, channel_data)?;

            results.push(DrResult::new(
                channel_idx,
                dr_value,
                rms,
                peak,
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

    /// 计算单个声道的20% RMS值（foobar2000兼容模式）
    ///
    /// 使用10001-bin直方图的20%采样算法，实现与foobar2000完全一致的精度。
    /// 这是foobar2000 "最响20%样本"算法的核心实现。
    fn calculate_channel_rms_foobar2000(&self, channel_idx: usize) -> AudioResult<f64> {
        let analyzers = self.histogram_analyzers.as_ref().ok_or_else(|| {
            AudioError::CalculationError("foobar2000模式下未初始化窗口分析器".to_string())
        })?;

        if channel_idx >= analyzers.len() {
            return Err(AudioError::CalculationError(format!(
                "声道索引{channel_idx}超出范围"
            )));
        }

        let analyzer = &analyzers[channel_idx];

        // 检查窗口数据可用性
        if analyzer.total_samples() == 0 {
            return Err(AudioError::CalculationError(
                "未检测到任何窗口数据，可能样本数不足".to_string(),
            ));
        }

        // 🏷️ FEATURE_ADDITION: 精确权重公式条件选择
        // 📅 添加时间: 2025-08-31
        // 🎯 根据weighted_rms_enabled标志选择RMS计算方法
        // 🔄 回退: 如需回退，改为直接使用analyzer.calculate_20_percent_rms()
        let rms_20_percent = if self.weighted_rms_enabled {
            // 使用精确权重公式：0.00000001×index²
            analyzer.calculate_weighted_20_percent_rms()
        } else {
            // 使用简化的20%采样算法（默认，与基线版本兼容）
            analyzer.calculate_20_percent_rms()
        };

        // 获取对应声道的Peak值（用于智能Sum Doubling评估）
        let peak = self.channels[channel_idx].get_effective_peak();

        // 使用智能Sum Doubling补偿系统
        let (compensated_rms, _quality) =
            self.apply_intelligent_sum_doubling(rms_20_percent, peak, self.sample_count);

        if compensated_rms.is_infinite() || compensated_rms.is_nan() {
            return Err(AudioError::CalculationError(
                "foobar2000 RMS计算结果无效（无穷大或NaN）".to_string(),
            ));
        }

        if compensated_rms <= 0.0 {
            return Err(AudioError::CalculationError(
                "foobar2000 RMS值必须大于0".to_string(),
            ));
        }

        Ok(compensated_rms)
    }

    /// 简化DR计算（基础Peak选择）
    ///
    /// 🏷️ FEATURE_UPDATE: 简化Peak回退算法
    /// 📅 修改时间: 2025-08-31
    /// 🎯 移除复杂质量评估，依赖ChannelData内置的削波检测
    /// 🔄 回退: 如需复杂回退逻辑，请查看git历史中的智能Peak验证系统
    fn calculate_dr_value_with_fallback(
        &self,
        rms: f64,
        channel_data: &ChannelData,
    ) -> AudioResult<f64> {
        // 使用简化的Peak选择（内置削波检测）
        let effective_peak = channel_data.get_effective_peak();

        // 直接计算DR，信任ChannelData的Peak选择
        self.calculate_dr_value(rms, effective_peak)
    }

    /// 计算DR值：DR = log10(RMS / Peak) * -20.0
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

        let ratio = rms / peak;
        let log_value = ratio.log10();

        if log_value.is_infinite() || log_value.is_nan() {
            return Err(AudioError::CalculationError("对数计算结果无效".to_string()));
        }

        let dr_value = log_value * -20.0;

        // DR值应该在合理范围内（0-100dB）
        if !(0.0..=100.0).contains(&dr_value) {
            return Err(AudioError::CalculationError(format!(
                "DR值({dr_value:.2})超出合理范围(0-100)"
            )));
        }

        Ok(dr_value)
    }

    /// 简化Sum Doubling补偿系统
    ///
    /// 🏷️ FEATURE_UPDATE: 移除复杂质量评估逻辑
    /// 📅 修改时间: 2025-08-31
    /// 🎯 Early Version模式：禁用Sum Doubling，保持原始RMS
    /// 🔄 回退: 如需复杂质量评估，查看git历史
    ///
    /// # 参数
    ///
    /// * `rms` - 原始RMS值
    /// * `_peak` - Peak值（未使用，保留接口兼容性）
    /// * `_sample_count` - 样本数量（未使用，保留接口兼容性）
    ///
    /// # 返回值
    ///
    /// 返回原始RMS值和默认质量信息
    fn apply_intelligent_sum_doubling(
        &self,
        rms: f64,
        _peak: f64,
        _sample_count: usize,
    ) -> (f64, SumDoublingQuality) {
        // Early Version模式：始终禁用Sum Doubling，确保最高精度
        (
            rms,
            SumDoublingQuality {
                should_apply: false,
                confidence: 1.0, // 对不使用Sum Doubling有最高信心
                issues: SumDoublingIssues::default(),
            },
        )
    }

    // 🏷️ FEATURE_REMOVAL: 复杂质量评估系统已移除
    // 📅 移除时间: 2025-08-31
    // 🎯 Early Version简化：移除evaluate_sum_doubling_quality()复杂逻辑
    // 💡 原因: 用户要求只保留削波检测，移除复杂质量评估
    // 🔄 回退: 如需复杂质量评估，查看git历史中的evaluate_sum_doubling_quality()方法

    /// 重置计算器状态，准备处理新的音频数据
    pub fn reset(&mut self) {
        for channel in &mut self.channels {
            channel.reset();
        }
        self.sample_count = 0;

        // 重置直方图（如果有）
        if let Some(ref mut analyzers) = self.histogram_analyzers {
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

    /// 获取foobar2000兼容模式状态
    pub fn foobar2000_mode(&self) -> bool {
        self.foobar2000_mode
    }

    /// 获取指定声道的直方图统计信息（仅foobar2000模式）
    pub fn get_histogram_stats(&self, channel_idx: usize) -> Option<crate::core::SimpleStats> {
        if let Some(ref analyzers) = self.histogram_analyzers {
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

    /// 🏷️ FEATURE_ADDITION: 精确权重公式控制方法
    /// 📅 添加时间: 2025-08-31
    /// 🎯 启用精确权重公式（0.00000001×index²）
    /// 🔄 回退: 如需回退，删除此方法及相关调用
    pub fn enable_weighted_rms(&mut self) {
        self.weighted_rms_enabled = true;
    }

    /// 🏷️ FEATURE_ADDITION: 精确权重公式控制方法
    /// 📅 添加时间: 2025-08-31
    /// 🎯 禁用精确权重公式，回到简化算法
    /// 🔄 回退: 如需回退，删除此方法及相关调用
    pub fn disable_weighted_rms(&mut self) {
        self.weighted_rms_enabled = false;
    }

    /// 🏷️ FEATURE_ADDITION: 精确权重公式状态查询
    /// 📅 添加时间: 2025-08-31
    /// 🎯 获取当前精确权重公式启用状态
    /// 🔄 回退: 如需回退，删除此方法及相关调用
    pub fn is_weighted_rms_enabled(&self) -> bool {
        self.weighted_rms_enabled
    }

    /// 🏷️ FEATURE_ADDITION: 精确权重公式状态设置
    /// 📅 添加时间: 2025-08-31
    /// 🎯 直接设置精确权重公式启用状态
    /// 🔄 回退: 如需回退，删除此方法及相关调用
    pub fn set_weighted_rms(&mut self, enabled: bool) {
        self.weighted_rms_enabled = enabled;
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
        // 使用更合理的测试数据：小RMS，正常Peak
        let samples = vec![
            0.05, 0.05, 0.05, 0.05, // 小信号
            1.0,  // 大Peak
        ];

        calc.process_interleaved_samples(&samples).unwrap();
        let results = calc.calculate_dr().unwrap();

        let result = &results[0];

        // 基础RMS计算：sqrt((4*0.05^2 + 1*1.0^2) / 5) = sqrt(0.202) ≈ 0.449
        let base_rms = ((4.0 * 0.05_f64.powi(2) + 1.0_f64.powi(2)) / 5.0).sqrt();
        // 早期版本：不应用RMS补偿，期待原始RMS值
        let expected_rms = base_rms;

        assert!((result.rms - expected_rms).abs() < 1e-6);
        assert!((result.peak - 1.0).abs() < 1e-10); // Peak不受Sum Doubling影响
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
        let result = DrResult::new(0, 12.7, 0.1, 0.5, 1000);
        assert_eq!(result.dr_value_rounded(), 13);

        let result = DrResult::new(0, 12.3, 0.1, 0.5, 1000);
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

        // 模拟实际音频：较小的RMS，较大的Peak（典型的动态范围情况）
        let samples = vec![
            0.1, 0.1, 0.1, 0.1, // 小信号
            1.0, // 大Peak
        ];

        calc.process_interleaved_samples(&samples).unwrap();
        let results = calc.calculate_dr().unwrap();

        let result = &results[0];
        assert_eq!(result.peak, 1.0);
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

        // 验证智能Sum Doubling系统工作
        let base_rms = ((1000.0 * 0.3_f64.powi(2) + 0.8_f64.powi(2)) / 1001.0).sqrt();

        // 🏷️ FEATURE_UPDATE: 简化测试，移除质量评估调用
        // 早期版本不使用复杂质量评估，直接验证RMS值

        // 早期版本：无论系统如何决定，都应该返回原始base_rms（不应用RMS补偿）
        assert!((result.rms - base_rms).abs() < 1e-6);

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

    // 🏷️ FEATURE_REMOVAL: 质量评估测试已移除
    // 📅 移除时间: 2025-08-31
    // 🎯 Early Version简化：移除test_sum_doubling_quality_assessment()
    // 💡 原因: 对应的evaluate_sum_doubling_quality()方法已被移除
    // 🔄 回退: 如需测试质量评估，查看git历史

    #[test]
    fn test_no_rms_compensation_in_early_version() {
        // 早期版本：验证不应用任何RMS补偿
        let calc = DrCalculator::new(1, true, 48000).unwrap();

        let (result_rms, _) = calc.apply_intelligent_sum_doubling(0.5, 0.8, 1000);

        // 早期版本应该返回原始RMS值，不应用任何补偿
        assert!((result_rms - 0.5).abs() < 1e-15);
    }

    // 🏷️ FEATURE_REMOVAL: 边界情况测试已移除
    // 📅 移除时间: 2025-08-31
    // 🎯 Early Version简化：移除test_sum_doubling_edge_cases()
    // 💡 原因: 对应的evaluate_sum_doubling_quality()方法已被移除
    // 🔄 回退: 如需测试边界情况，查看git历史
}
