//! DR计算核心引擎
//!
//! 基于对foobar2000 DR Meter算法的独立分析实现核心DR计算公式：DR = log10(RMS / Peak) * -20.0
//!
//! 注：本实现通过IDA Pro逆向分析理解算法逻辑，所有代码均为Rust原创实现

use crate::core::SilenceFilterConfig;
use crate::core::histogram::WindowRmsAnalyzer;
use crate::core::peak_selection::{PeakSelectionStrategy, PeakSelector};
use crate::error::{AudioError, AudioResult};
use crate::processing::ProcessingCoordinator;
#[allow(unused_imports)]
use crate::tools::constants::dr_analysis;

// 配置常量：集中管理默认值，提高可维护性
/// 标准音频采样率（CD质量）
const DEFAULT_SAMPLE_RATE: u32 = 44100;

/// DR测量标准的3秒块持续时间
const DEFAULT_BLOCK_DURATION: f64 = 3.0;

/// 默认峰值选择策略（优先次峰，抗尖峰干扰）
const DEFAULT_PEAK_STRATEGY: PeakSelectionStrategy = PeakSelectionStrategy::PreferSecondary;

/// 默认启用Sum Doubling确保foobar2000兼容性
const DEFAULT_SUM_DOUBLING: bool = true;

/// 支持的最大声道数（架构限制）
const MAX_SUPPORTED_CHANNELS: usize = 32;

// foobar2000专属模式：使用累加器级别Sum Doubling，移除了+3dB RMS补偿机制

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

    /// 主峰值
    pub primary_peak: f64,

    /// 次峰值
    pub secondary_peak: f64,

    /// 参与计算的样本数量
    pub sample_count: usize,
}

impl DrResult {
    /// 创建带有峰值信息的DR结果
    pub fn new_with_peaks(
        channel: usize,
        dr_value: f64,
        rms: f64,
        peak: f64,
        primary_peak: f64,
        secondary_peak: f64,
        sample_count: usize,
    ) -> Self {
        Self {
            channel,
            dr_value,
            rms,
            peak,
            primary_peak,
            secondary_peak,
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
/// - 使用官方规范的3秒块级处理架构
/// - 支持流式块累积和批量处理
/// - 可配置的峰值选择策略
pub struct DrCalculator {
    /// 声道数量
    channel_count: usize,

    /// 是否启用Sum Doubling补偿（交错数据）
    sum_doubling_enabled: bool,

    /// 采样率
    sample_rate: u32,

    /// 块持续时间（秒，官方规范为3.0）
    block_duration: f64,

    /// 峰值选择策略
    peak_selection_strategy: PeakSelectionStrategy,

    /// 高性能处理协调器（提供SIMD优化的声道分离）
    processing_coordinator: ProcessingCoordinator,

    /// 实验性：静音过滤配置
    silence_filter: SilenceFilterConfig,
}

impl DrCalculator {
    /// 创建DR计算器（零配置模式）
    ///
    /// 自动检测最优配置，遵循foobar2000兼容标准：
    /// - 自动启用Sum Doubling（交错数据补偿）
    /// - 使用标准3秒窗口处理
    /// - 智能选择峰值策略（优先次峰）
    ///
    /// # 参数
    ///
    /// * `channel_count` - 音频声道数量
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::core::DrCalculator;
    ///
    /// // 零配置创建 - 自动最优设置
    /// let calculator = DrCalculator::new(2).unwrap();
    /// ```
    pub fn new(channel_count: usize) -> AudioResult<Self> {
        Self::new_with_config(channel_count, DEFAULT_SAMPLE_RATE)
    }

    /// 创建DR计算器（指定采样率）
    ///
    /// 适用于已知采样率的场景，其他参数使用智能默认值。
    ///
    /// # 参数
    ///
    /// * `channel_count` - 音频声道数量
    /// * `sample_rate` - 采样率（Hz）
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::core::DrCalculator;
    ///
    /// // 指定采样率，其他自动配置
    /// let calculator = DrCalculator::new_with_config(2, 48000).unwrap();
    /// ```
    pub fn new_with_config(channel_count: usize, sample_rate: u32) -> AudioResult<Self> {
        Self::new_advanced(
            channel_count,
            DEFAULT_SUM_DOUBLING,
            sample_rate,
            DEFAULT_BLOCK_DURATION,
            DEFAULT_PEAK_STRATEGY,
        )
    }

    /// 创建DR计算器（高级配置）
    ///
    /// 适用于需要精确控制算法参数的场景（如调试和测试）。
    ///
    /// # 参数
    ///
    /// * `channel_count` - 音频声道数量
    /// * `sum_doubling` - 是否启用Sum Doubling补偿
    /// * `sample_rate` - 采样率（Hz）
    /// * `block_duration` - 块持续时间（秒）
    /// * `peak_strategy` - 峰值选择策略
    pub fn new_advanced(
        channel_count: usize,
        sum_doubling: bool,
        sample_rate: u32,
        block_duration: f64,
        peak_strategy: PeakSelectionStrategy,
    ) -> AudioResult<Self> {
        Self::new_with_peak_strategy(
            channel_count,
            sum_doubling,
            sample_rate,
            block_duration,
            peak_strategy,
        )
    }

    /// 创建DR计算器（调试模式）
    ///
    /// 支持指定峰值选择策略，适用于算法调试和验证。
    ///
    /// # 参数
    ///
    /// * `channel_count` - 音频声道数量
    /// * `peak_strategy` - 峰值选择策略（用于调试对比）
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::core::{DrCalculator, PeakSelectionStrategy};
    ///
    /// // 调试模式 - 使用削波感知策略
    /// let calculator = DrCalculator::for_debugging(2, PeakSelectionStrategy::ClippingAware).unwrap();
    /// ```
    pub fn for_debugging(
        channel_count: usize,
        peak_strategy: PeakSelectionStrategy,
    ) -> AudioResult<Self> {
        Self::new_advanced(
            channel_count,
            DEFAULT_SUM_DOUBLING,
            DEFAULT_SAMPLE_RATE,
            DEFAULT_BLOCK_DURATION,
            peak_strategy,
        )
    }

    /// 内部构造函数（完整参数控制）
    fn new_with_peak_strategy(
        channel_count: usize,
        sum_doubling: bool,
        sample_rate: u32,
        block_duration: f64,
        peak_strategy: PeakSelectionStrategy,
    ) -> AudioResult<Self> {
        Self::new_with_peak_strategy_and_filter(
            channel_count,
            sum_doubling,
            sample_rate,
            block_duration,
            peak_strategy,
            SilenceFilterConfig::default(),
        )
    }

    /// 创建DR计算器（高级配置 + 静音过滤）
    ///
    /// # 参数
    ///
    /// * `silence_filter` - 静音过滤配置（实验性功能）
    pub fn new_with_silence_filter(
        channel_count: usize,
        sum_doubling: bool,
        sample_rate: u32,
        block_duration: f64,
        peak_strategy: PeakSelectionStrategy,
        silence_filter: SilenceFilterConfig,
    ) -> AudioResult<Self> {
        Self::new_with_peak_strategy_and_filter(
            channel_count,
            sum_doubling,
            sample_rate,
            block_duration,
            peak_strategy,
            silence_filter,
        )
    }

    fn new_with_peak_strategy_and_filter(
        channel_count: usize,
        sum_doubling: bool,
        sample_rate: u32,
        block_duration: f64,
        peak_strategy: PeakSelectionStrategy,
        silence_filter: SilenceFilterConfig,
    ) -> AudioResult<Self> {
        if channel_count == 0 {
            return Err(AudioError::InvalidInput(
                "Channel count must be greater than zero / 声道数量必须大于0".to_string(),
            ));
        }

        if channel_count > MAX_SUPPORTED_CHANNELS {
            return Err(AudioError::InvalidInput(format!(
                "Channel count cannot exceed {MAX_SUPPORTED_CHANNELS} / 声道数量不能超过{MAX_SUPPORTED_CHANNELS}"
            )));
        }

        if sample_rate == 0 {
            return Err(AudioError::InvalidInput(
                "Sample rate must be greater than zero / 采样率必须大于0".to_string(),
            ));
        }

        if block_duration <= 0.0 {
            return Err(AudioError::InvalidInput(
                "Block duration must be greater than zero / 块持续时间必须大于0".to_string(),
            ));
        }

        Ok(Self {
            channel_count,
            sum_doubling_enabled: sum_doubling,
            sample_rate,
            block_duration,
            peak_selection_strategy: peak_strategy,
            processing_coordinator: ProcessingCoordinator::new(),
            silence_filter,
        })
    }

    /// 处理交错音频数据并计算DR值（使用正确的WindowRmsAnalyzer算法）
    ///
    /// 使用从master分支移植的正确WindowRmsAnalyzer算法，
    /// 确保与master分支产生完全一致的结果。
    ///
    /// # 参数
    ///
    /// * `samples` - 交错音频样本数据
    /// * `channel_count` - 声道数量
    ///
    /// # 返回值
    ///
    /// 返回每个声道的DR计算结果
    pub fn calculate_dr_from_samples(
        &self,
        samples: &[f32],
        channel_count: usize,
    ) -> AudioResult<Vec<DrResult>> {
        // 验证输入参数
        if !samples.len().is_multiple_of(channel_count) {
            return Err(AudioError::InvalidInput(
                "Sample count must be an integer multiple of channel count / 样本数量必须是声道数的整数倍"
                    .to_string(),
            ));
        }

        if channel_count != self.channel_count {
            return Err(AudioError::InvalidInput(format!(
                "Channel count mismatch: expected {expected}, got {actual} / 声道数量不匹配：期望{expected}，实际{actual}",
                expected = self.channel_count,
                actual = channel_count
            )));
        }

        if samples.is_empty() {
            return Err(AudioError::InvalidInput(
                "Sample data cannot be empty / 样本数据不能为空".to_string(),
            ));
        }

        // 多声道支持：基于foobar2000 DR Meter实测行为
        // 每个声道独立计算DR，最终Official DR为算术平均值

        // [TRACE] 使用ProcessingCoordinator享受SIMD优化的声道分离服务
        #[cfg(debug_assertions)]
        eprintln!(
            "[DRCALC] Calling ProcessingCoordinator::process_channels / 调用ProcessingCoordinator::process_channels"
        );
        #[cfg(debug_assertions)]
        eprintln!(
            "[DRCALC] Input: samples={samples}, channels={channels} / 输入: samples={samples}, channels={channels}",
            samples = samples.len(),
            channels = channel_count
        );

        let performance_result = self.processing_coordinator.process_channels(
            samples,
            channel_count,
            |channel_samples, channel_idx| {
                // [TRACE] 回调：使用core层的DR算法计算单声道结果
                #[cfg(debug_assertions)]
                eprintln!(
                    "[DRCALC] Callback processing channel {channel_idx}: {samples} samples / 回调处理声道{channel_idx}: {samples} 个样本",
                    channel_idx = channel_idx,
                    samples = channel_samples.len()
                );

                self.calculate_single_channel_dr(channel_samples, channel_idx)
            },
        )?;

        #[cfg(debug_assertions)]
        eprintln!(
            "[DRCALC] ProcessingCoordinator returned {count} DR results / ProcessingCoordinator返回: {count} 个DR结果",
            count = performance_result.dr_results.len()
        );

        Ok(performance_result.dr_results)
    }

    /// 单声道DR计算算法（纯算法逻辑）
    fn calculate_single_channel_dr(
        &self,
        channel_samples: &[f32],
        channel_idx: usize,
    ) -> AudioResult<DrResult> {
        // [TRACE] 创建WindowRmsAnalyzer进行DR分析
        #[cfg(debug_assertions)]
        eprintln!(
            "[ANALYZER] Channel {channel_idx}: creating WindowRmsAnalyzer / 声道{channel_idx}: 创建WindowRmsAnalyzer"
        );
        #[cfg(debug_assertions)]
        eprintln!(
            "[ANALYZER] Channel {channel_idx}: processing {samples} samples / 声道{channel_idx}: 处理 {samples} 个样本",
            channel_idx = channel_idx,
            samples = channel_samples.len()
        );

        let mut analyzer = WindowRmsAnalyzer::with_silence_filter(
            self.sample_rate,
            self.sum_doubling_enabled,
            self.silence_filter,
        );

        // 关键：一次性处理所有样本，让WindowRmsAnalyzer内部创建正确的3秒窗口
        analyzer.process_samples(channel_samples);

        // 使用WindowRmsAnalyzer的20%采样算法
        let rms_20_percent = analyzer.calculate_20_percent_rms();

        // 使用配置的峰值选择策略
        let window_primary_peak = analyzer.get_largest_peak();
        let window_secondary_peak = analyzer.get_second_largest_peak();

        let peak_for_dr = self
            .peak_selection_strategy
            .select_peak(window_primary_peak, window_secondary_peak);

        // 计算DR值（官方标准公式）
        // 为了跨平台稳定性，对接近0的RMS/Peak做阈值归零处理，避免极小浮点噪声导致非零DR。
        const DR_ZERO_EPS: f64 = 1e-12;
        let dr_value = if rms_20_percent > DR_ZERO_EPS && peak_for_dr > DR_ZERO_EPS {
            let ratio = rms_20_percent / peak_for_dr;
            -20.0 * ratio.log10()
        } else {
            0.0
        };

        // 使用WindowRmsAnalyzer计算的20%RMS作为显示RMS
        // 这保持了算法的一致性，避免在DrCalculator中重复实现RMS计算
        let display_rms = rms_20_percent;

        // 创建DR结果
        let result = DrResult::new_with_peaks(
            channel_idx,
            dr_value,
            display_rms, // 使用20%RMS保持算法一致性
            peak_for_dr,
            window_primary_peak,   // 使用窗口级主峰
            window_secondary_peak, // 使用窗口级次峰
            channel_samples.len(),
        );

        Ok(result)
    }

    /// 获取声道数量
    pub fn channel_count(&self) -> usize {
        self.channel_count
    }

    /// 获取Sum Doubling启用状态
    pub fn sum_doubling_enabled(&self) -> bool {
        self.sum_doubling_enabled
    }

    /// 获取音频采样率
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// 获取块持续时间（秒）
    pub fn block_duration(&self) -> f64 {
        self.block_duration
    }

    /// 获取当前的峰值选择策略
    pub fn peak_selection_strategy(&self) -> PeakSelectionStrategy {
        self.peak_selection_strategy
    }

    /// 设置峰值选择策略
    pub fn set_peak_selection_strategy(&mut self, strategy: PeakSelectionStrategy) {
        self.peak_selection_strategy = strategy;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_calculator() {
        let calc = DrCalculator::new(2).unwrap();
        assert_eq!(calc.channel_count(), 2);
        assert!(calc.sum_doubling_enabled());
    }

    #[test]
    fn test_new_with_config() {
        let calc = DrCalculator::new_with_config(2, 48000).unwrap();
        assert_eq!(calc.channel_count(), 2);
        assert_eq!(calc.sample_rate(), 48000);
        assert!(calc.sum_doubling_enabled());
    }

    #[test]
    fn test_for_debugging() {
        let calc = DrCalculator::for_debugging(2, PeakSelectionStrategy::AlwaysPrimary).unwrap();
        assert_eq!(calc.channel_count(), 2);
        assert_eq!(
            calc.peak_selection_strategy(),
            PeakSelectionStrategy::AlwaysPrimary
        );
    }

    #[test]
    fn test_invalid_channel_count() {
        assert!(DrCalculator::new(0).is_err());
        assert!(DrCalculator::new(33).is_err());
    }

    #[test]
    fn test_invalid_interleaved_data() {
        let calc = DrCalculator::new_with_config(2, 48000).unwrap();
        let samples = vec![0.5, -0.3, 0.7]; // 不是2的倍数

        assert!(calc.calculate_dr_from_samples(&samples, 2).is_err());
    }

    #[test]
    fn test_calculate_dr_no_data() {
        let calc = DrCalculator::new(2).unwrap();
        let empty_samples: Vec<f32> = vec![];
        assert!(calc.calculate_dr_from_samples(&empty_samples, 2).is_err());
    }

    #[test]
    fn test_dr_result_rounded() {
        let result = DrResult::new_with_peaks(0, 12.7, 0.1, 0.5, 0.5, 0.0, 1000);
        assert_eq!(result.dr_value_rounded(), 13);

        let result = DrResult::new_with_peaks(0, 12.3, 0.1, 0.5, 0.5, 0.0, 1000);
        assert_eq!(result.dr_value_rounded(), 12);
    }

    #[test]
    fn test_stateless_calculation() {
        let calc = DrCalculator::new_with_config(2, 48000).unwrap();
        let samples = vec![0.5, -0.3, 0.7, -0.1];

        // 新的API是无状态的，不需要reset
        let results1 = calc.calculate_dr_from_samples(&samples, 2).unwrap();
        let results2 = calc.calculate_dr_from_samples(&samples, 2).unwrap();

        // 同样的输入应该产生同样的结果
        assert_eq!(results1.len(), results2.len());
        for (r1, r2) in results1.iter().zip(results2.iter()) {
            assert!((r1.dr_value - r2.dr_value).abs() < 1e-6);
        }
    }

    #[test]
    fn test_silent_input_dr_zero() {
        // 验证静音输入（全0）的DR归零策略
        //
        // 当RMS <= DR_ZERO_EPS(1e-12) 时，应该直接返回DR=0.0，而不进行
        // log计算。这是对无声信号的标准处理。
        let calc = DrCalculator::new_with_config(1, 48000).unwrap();

        // 创建足够长的静音样本（需要至少一个3秒窗口）
        let window_samples = 48000 * 3; // 3秒 @ 48kHz
        let silent_samples = vec![0.0f32; window_samples];

        let results = calc.calculate_dr_from_samples(&silent_samples, 1).unwrap();

        // 应该有一个结果（单声道）
        assert_eq!(results.len(), 1);

        let dr_result = &results[0];

        // 静音输入的DR应该是0.0
        assert_eq!(
            dr_result.dr_value,
            0.0,
            "Silent input should yield DR=0.0, actual value={value} / 静音输入应该产生DR=0.0，实际值={value}",
            value = dr_result.dr_value
        );

        // RMS应该非常小（接近0），使用与实现相同的容差阈值
        assert!(
            dr_result.rms < dr_analysis::DR_ZERO_EPS * 100.0, // 允许稍微宽松的容差
            "Silent input RMS should be near 0, actual value={value} / 静音输入的RMS应该接近0，实际值={value}",
            value = dr_result.rms
        );

        // Peak也应该是0（或非常小）
        assert_eq!(
            dr_result.peak,
            0.0,
            "Silent input peak should be 0, actual value={value} / 静音输入的Peak应该是0，实际值={value}",
            value = dr_result.peak
        );
    }
}
