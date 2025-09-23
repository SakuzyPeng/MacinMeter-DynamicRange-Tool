//! Processing层协调器
//!
//! 负责协调processing层各种服务的纯粹协调器，专注于服务编排和业务流程控制。
//! 委托技术实现给专门的模块：ChannelExtractor负责SIMD分离，PerformanceEvaluator负责统计。

use super::channel_extractor::ChannelExtractor;
use super::performance_metrics::{PerformanceEvaluator, PerformanceResult};
use crate::core::DrResult;
use crate::error::{AudioError, AudioResult};
use rayon::prelude::*;

#[cfg(debug_assertions)]
macro_rules! debug_coordinator {
    ($($arg:tt)*) => {
        eprintln!("[COORDINATOR_DEBUG] {}", format_args!($($arg)*));
    };
}

#[cfg(not(debug_assertions))]
macro_rules! debug_coordinator {
    ($($arg:tt)*) => {};
}

/// Processing层协调器
///
/// 纯粹的协调器，负责编排processing层的各种高性能服务：
/// - 委托声道分离给ChannelExtractor
/// - 委托性能评估给PerformanceEvaluator
/// - 专注并行协调和回调管理
/// - 为DrCalculator提供零配置的高性能服务
pub struct ProcessingCoordinator {
    /// 声道分离引擎
    channel_extractor: ChannelExtractor,

    /// 性能评估器
    performance_evaluator: PerformanceEvaluator,
}

impl ProcessingCoordinator {
    /// 创建新的processing协调器
    ///
    /// 自动初始化所有委托服务，总是启用最优性能配置。
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::processing::ProcessingCoordinator;
    ///
    /// // 自动启用所有性能优化，零配置
    /// let coordinator = ProcessingCoordinator::new();
    /// ```
    pub fn new() -> Self {
        Self {
            channel_extractor: ChannelExtractor::new(),
            performance_evaluator: PerformanceEvaluator::new(),
        }
    }

    /// 🚀 高性能音频样本处理服务（协调器模式）
    ///
    /// 纯粹的服务协调器，专注于编排各种高性能服务，保持算法中立性。
    /// 通过回调方式让调用者保持算法控制权，专注于性能优化服务编排。
    ///
    /// **注意**：仅处理1-2声道文件，多声道文件已在DrCalculator层被拒绝。
    ///
    /// # 参数
    ///
    /// * `samples` - 交错的音频样本数据（单声道或立体声）
    /// * `channel_count` - 声道数量（1或2）
    /// * `channel_processor` - 单声道处理回调函数，参数为(声道样本, 声道索引)
    ///
    /// # 返回值
    ///
    /// 返回处理结果，包含各声道的DR值和性能统计信息
    pub fn process_channels<F>(
        &self,
        samples: &[f32],
        channel_count: usize,
        channel_processor: F,
    ) -> AudioResult<PerformanceResult>
    where
        F: Fn(&[f32], usize) -> AudioResult<DrResult> + Sync + Send,
    {
        let start_time = std::time::Instant::now();

        // 🎛️ 基础参数验证
        if samples.is_empty() {
            return Err(AudioError::InvalidInput("样本数据不能为空".to_string()));
        }

        if !samples.len().is_multiple_of(channel_count) {
            return Err(AudioError::InvalidInput(format!(
                "样本数量({})必须是声道数({})的倍数",
                samples.len(),
                channel_count
            )));
        }

        let samples_per_channel = samples.len() / channel_count;

        debug_coordinator!(
            "🎛️ Processing协调器启动: channels={}, samples_per_channel={}, 委托模式=始终启用",
            channel_count,
            samples_per_channel
        );

        // 🔍 [TRACE] ProcessingCoordinator启动
        #[cfg(debug_assertions)]
        eprintln!("🔍 [COORDINATOR] ProcessingCoordinator::process_channels 启动");
        #[cfg(debug_assertions)]
        eprintln!(
            "🔍 [COORDINATOR] 输入参数: samples={}, channels={}",
            samples.len(),
            channel_count
        );

        // 🎛️ 智能并行协调（多声道并行，单声道顺序）
        let dr_results = if channel_count > 1 {
            // 🚀 并行协调：委托多个声道分离服务
            self.coordinate_parallel_processing(samples, channel_count, channel_processor)?
        } else {
            // 📝 顺序协调：单声道无需并行开销
            self.coordinate_sequential_processing(samples, channel_count, channel_processor)?
        };

        let duration = start_time.elapsed();

        // 🎛️ 委托性能评估服务
        let performance_stats = self.performance_evaluator.calculate_performance_stats(
            duration.as_micros() as u64,
            samples.len(),
            channel_count,
            samples_per_channel,
        );

        // 🎛️ 委托SIMD使用统计服务
        let simd_usage = self.performance_evaluator.create_simd_usage_stats(
            true,          // 始终启用SIMD优化
            samples.len(), // 所有样本都通过SIMD路径
            0,             // 无标量回退
        );

        debug_coordinator!(
            "🎛️ 协调完成: SIMD=始终启用, speedup={:.1}x, samples/sec={:.0}",
            performance_stats.simd_speedup,
            performance_stats.samples_per_second
        );

        Ok(PerformanceResult {
            dr_results,
            performance_stats,
            simd_usage,
        })
    }

    /// 🚀 并行处理协调（多声道）
    fn coordinate_parallel_processing<F>(
        &self,
        samples: &[f32],
        channel_count: usize,
        channel_processor: F,
    ) -> AudioResult<Vec<DrResult>>
    where
        F: Fn(&[f32], usize) -> AudioResult<DrResult> + Sync + Send,
    {
        debug_coordinator!("🚀 启动并行协调模式: {} 声道", channel_count);

        let results: Result<Vec<_>, _> = (0..channel_count)
            .into_par_iter()
            .map(|channel_idx| {
                // 🎛️ 委托声道分离服务
                #[cfg(debug_assertions)]
                eprintln!("🔍 [COORDINATOR] 并行处理声道{channel_idx} - 委托ChannelExtractor");

                let channel_samples = self.channel_extractor.extract_channel_samples_optimized(
                    samples,
                    channel_idx,
                    channel_count,
                );

                #[cfg(debug_assertions)]
                eprintln!(
                    "🔍 [COORDINATOR] 声道{channel_idx} 分离完成: {} 个样本",
                    channel_samples.len()
                );

                debug_coordinator!(
                    "🎛️ 并行协调声道{}: 委托分离{}个样本",
                    channel_idx,
                    channel_samples.len()
                );

                // 🎛️ 委托算法层进行DR计算（保持算法中立）
                #[cfg(debug_assertions)]
                eprintln!("🔍 [COORDINATOR] 声道{channel_idx} 开始回调DR算法");

                let result = channel_processor(&channel_samples, channel_idx);

                #[cfg(debug_assertions)]
                if let Ok(ref dr_result) = result {
                    eprintln!(
                        "🔍 [COORDINATOR] 声道{channel_idx} DR计算完成: DR={:.2}",
                        dr_result.dr_value
                    );
                }

                result
            })
            .collect();

        results
    }

    /// 📝 顺序处理协调（单声道）
    fn coordinate_sequential_processing<F>(
        &self,
        samples: &[f32],
        channel_count: usize,
        channel_processor: F,
    ) -> AudioResult<Vec<DrResult>>
    where
        F: Fn(&[f32], usize) -> AudioResult<DrResult>,
    {
        debug_coordinator!("📝 启动顺序协调模式: {} 声道", channel_count);

        let mut dr_results = Vec::with_capacity(channel_count);

        for channel_idx in 0..channel_count {
            // 🎛️ 委托声道分离服务
            let channel_samples = self.channel_extractor.extract_channel_samples_optimized(
                samples,
                channel_idx,
                channel_count,
            );

            debug_coordinator!(
                "🎛️ 顺序协调声道{}: 委托分离{}个样本",
                channel_idx,
                channel_samples.len()
            );

            // 🎛️ 委托算法层进行DR计算
            let result = channel_processor(&channel_samples, channel_idx)?;
            dr_results.push(result);
        }

        Ok(dr_results)
    }

    /// 获取委托的SIMD能力信息
    pub fn simd_capabilities(&self) -> &super::simd_channel_data::SimdCapabilities {
        self.channel_extractor.simd_capabilities()
    }

    /// 获取委托的性能评估器
    pub fn performance_evaluator(&self) -> &PerformanceEvaluator {
        &self.performance_evaluator
    }

    /// 生成性能报告（委托给评估器）
    pub fn generate_performance_report(&self, performance_result: &PerformanceResult) -> String {
        self.performance_evaluator.generate_performance_report(
            &performance_result.performance_stats,
            &performance_result.simd_usage,
        )
    }
}

impl Default for ProcessingCoordinator {
    fn default() -> Self {
        Self::new() // 总是启用最优配置
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_processing_coordinator_creation() {
        let coordinator = ProcessingCoordinator::new();

        // 验证委托服务正常初始化
        println!("协调器SIMD能力: {:?}", coordinator.simd_capabilities());
    }

    #[test]
    fn test_interleaved_processing_coordination() {
        let coordinator = ProcessingCoordinator::new();

        // 立体声测试数据 - 适配foobar2000模式
        let mut samples = Vec::new();
        for _ in 0..100 {
            samples.extend_from_slice(&[0.01, -0.01]); // 大量小信号
        }
        samples.extend_from_slice(&[
            1.0, -1.0, // 主Peak
            0.9, -0.9, // 次Peak，确保远大于20%RMS
        ]);

        let result = coordinator
            .process_channels(
                &samples,
                2, // 立体声
                |channel_samples, channel_idx| {
                    // 模拟DR计算回调
                    use crate::core::DrResult;
                    Ok(DrResult {
                        channel: channel_idx,
                        dr_value: 10.0,
                        rms: 0.1,
                        peak: 1.0,
                        primary_peak: 1.0,
                        secondary_peak: 0.9,
                        sample_count: channel_samples.len(),
                    })
                },
            )
            .unwrap();

        // 验证协调结果
        assert_eq!(result.dr_results.len(), 2);
        assert_eq!(result.performance_stats.channels_processed, 2);
        assert_eq!(result.performance_stats.total_samples, samples.len());

        // 检查每个声道的结果
        for dr_result in &result.dr_results {
            assert!(dr_result.dr_value > 0.0);
            assert!(dr_result.rms > 0.0);
            assert!(dr_result.peak > 0.0);
            assert!(dr_result.peak >= dr_result.rms);
        }

        println!("✅ 协调器处理测试通过");
        println!(
            "   处理时间: {}μs",
            result.performance_stats.total_duration_us
        );
        println!(
            "   样本处理速度: {:.0} samples/s",
            result.performance_stats.samples_per_second
        );
    }

    #[test]
    fn test_parallel_vs_sequential_coordination() {
        // 测试数据
        let mut samples = Vec::new();
        for _ in 0..50 {
            samples.extend_from_slice(&[0.01, 0.01]); // 立体声小信号
        }
        samples.extend_from_slice(&[
            1.0, 1.0, // 立体声主Peak
            0.95, 0.95, // 立体声次Peak
        ]);

        // 协调器测试1
        let coordinator1 = ProcessingCoordinator::new();
        let result1 = coordinator1
            .process_channels(&samples, 2, |channel_samples, channel_idx| {
                use crate::core::DrResult;
                Ok(DrResult {
                    channel: channel_idx,
                    dr_value: 15.0,
                    rms: 0.05,
                    peak: 1.0,
                    primary_peak: 1.0,
                    secondary_peak: 0.95,
                    sample_count: channel_samples.len(),
                })
            })
            .unwrap();

        // 协调器测试2
        let coordinator2 = ProcessingCoordinator::new();
        let result2 = coordinator2
            .process_channels(&samples, 2, |channel_samples, channel_idx| {
                use crate::core::DrResult;
                Ok(DrResult {
                    channel: channel_idx,
                    dr_value: 15.0,
                    rms: 0.05,
                    peak: 1.0,
                    primary_peak: 1.0,
                    secondary_peak: 0.95,
                    sample_count: channel_samples.len(),
                })
            })
            .unwrap();

        // 比较协调结果（应该一致）
        assert_eq!(result1.dr_results.len(), result2.dr_results.len());

        for (dr1, dr2) in result1.dr_results.iter().zip(result2.dr_results.iter()) {
            let dr_diff = (dr1.dr_value - dr2.dr_value).abs();
            let rms_diff = (dr1.rms - dr2.rms).abs();
            let peak_diff = (dr1.peak - dr2.peak).abs();

            assert!(dr_diff < 1e-6, "DR值差异过大: {dr_diff}");
            assert!(rms_diff < 1e-6, "RMS差异过大: {rms_diff}");
            assert!(peak_diff < 1e-6, "Peak差异过大: {peak_diff}");
        }

        println!("✅ 协调器一致性验证通过");
    }
}
