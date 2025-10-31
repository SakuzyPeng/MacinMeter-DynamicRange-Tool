//! Processing层协调器
//!
//! 负责协调processing层各种服务的纯粹协调器，专注于服务编排和业务流程控制。
//! 委托技术实现给专门的模块：ChannelSeparator负责SIMD分离，PerformanceEvaluator负责统计。

use super::channel_separator::ChannelSeparator;
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
/// - 委托声道分离给ChannelSeparator
/// - 委托性能评估给PerformanceEvaluator
/// - 专注并行协调和回调管理
/// - 为DrCalculator提供零配置的高性能服务
pub struct ProcessingCoordinator {
    /// 声道分离引擎
    channel_separator: ChannelSeparator,

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
            channel_separator: ChannelSeparator::new(),
            performance_evaluator: PerformanceEvaluator::new(),
        }
    }

    /// 高性能音频样本处理服务（协调器模式）
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

        // 基础参数验证
        if samples.is_empty() {
            return Err(AudioError::InvalidInput(
                "Sample data cannot be empty / 样本数据不能为空".to_string(),
            ));
        }

        if !samples.len().is_multiple_of(channel_count) {
            return Err(AudioError::InvalidInput(format!(
                "Sample count ({}) must be a multiple of channel count ({}) / 样本数量({})必须是声道数({})的倍数",
                samples.len(),
                channel_count,
                samples.len(),
                channel_count
            )));
        }

        let samples_per_channel = samples.len() / channel_count;

        debug_coordinator!(
            "Processing coordinator start: channels={}, samples_per_channel={}, total_samples={}, delegation=always on / Processing协调器启动: channels={}, samples_per_channel={}, total_samples={}, 委托模式=始终启用",
            channel_count,
            samples_per_channel,
            samples.len(),
            channel_count,
            samples_per_channel,
            samples.len()
        );

        // 智能并行协调（多声道并行，单声道顺序）
        let dr_results = if channel_count > 1 {
            // 并行协调：委托多个声道分离服务
            self.coordinate_parallel_processing(samples, channel_count, channel_processor)?
        } else {
            // 顺序协调：单声道无需并行开销
            self.coordinate_sequential_processing(samples, channel_count, channel_processor)?
        };

        let duration = start_time.elapsed();

        // 委托性能评估服务
        let performance_stats = self.performance_evaluator.calculate_performance_stats(
            duration.as_micros() as u64,
            samples.len(),
            channel_count,
            samples_per_channel,
        );

        // 委托SIMD使用统计服务
        //
        // 注意：当前假设所有样本都走SIMD路径（Processing层默认行为）。
        // 实际的SIMD覆盖情况在ChannelSeparator和SampleConverter层有更准确的统计。
        // 如果上游存在标量回退路径（如某些边界条件），应从实际转换器传入真实计数。
        //
        // used_simd 现在由 create_simd_usage_stats 内部自动推导（simd_samples > 0）
        let simd_usage = self.performance_evaluator.create_simd_usage_stats(
            samples.len(), // 假设：所有样本都通过SIMD路径
            0,             // 假设：无标量回退
        );

        debug_coordinator!(
            "Coordination finished: SIMD always on, speedup={:.1}x, samples/sec={:.0} / 协调完成: SIMD=始终启用, speedup={:.1}x, samples/sec={:.0}",
            performance_stats.simd_speedup,
            performance_stats.samples_per_second,
            performance_stats.simd_speedup,
            performance_stats.samples_per_second
        );

        Ok(PerformanceResult {
            dr_results,
            performance_stats,
            simd_usage,
        })
    }

    /// 并行处理协调（多声道）
    fn coordinate_parallel_processing<F>(
        &self,
        samples: &[f32],
        channel_count: usize,
        channel_processor: F,
    ) -> AudioResult<Vec<DrResult>>
    where
        F: Fn(&[f32], usize) -> AudioResult<DrResult> + Sync + Send,
    {
        debug_coordinator!(
            "Entering parallel coordination: {} channels / 启动并行协调模式: {} 声道",
            channel_count,
            channel_count
        );

        let results: Result<Vec<_>, _> = (0..channel_count)
            .into_par_iter()
            .map(|channel_idx| {
                // 委托声道分离服务
                let channel_samples = self.channel_separator.extract_channel_samples_optimized(
                    samples,
                    channel_idx,
                    channel_count,
                );

                debug_coordinator!(
                    "Parallel coordination channel {}: delegating {} samples / 并行协调声道{}: 委托分离{}个样本",
                    channel_idx,
                    channel_samples.len(),
                    channel_idx,
                    channel_samples.len()
                );

                // 委托算法层进行DR计算（保持算法中立）
                let result = channel_processor(&channel_samples, channel_idx);

                // 仅在调试构建下访问结果用于日志，避免 release 下未使用变量的 Clippy 警告
                #[cfg(debug_assertions)]
                {
                    if let Ok(ref dr_result) = result {
                        debug_coordinator!(
                            "Channel {} DR computed: DR={:.2} / 声道{} DR计算完成: DR={:.2}",
                            channel_idx,
                            dr_result.dr_value,
                            channel_idx,
                            dr_result.dr_value
                        );
                    }
                }

                result
            })
            .collect();

        results
    }

    /// 顺序处理协调（单声道）
    fn coordinate_sequential_processing<F>(
        &self,
        samples: &[f32],
        channel_count: usize,
        channel_processor: F,
    ) -> AudioResult<Vec<DrResult>>
    where
        F: Fn(&[f32], usize) -> AudioResult<DrResult>,
    {
        debug_coordinator!(
            "Entering sequential coordination: {} channels / 启动顺序协调模式: {} 声道",
            channel_count,
            channel_count
        );

        let mut dr_results = Vec::with_capacity(channel_count);

        for channel_idx in 0..channel_count {
            // 委托声道分离服务
            let channel_samples = self.channel_separator.extract_channel_samples_optimized(
                samples,
                channel_idx,
                channel_count,
            );

            debug_coordinator!(
                "Sequential coordination channel {}: delegating {} samples / 顺序协调声道{}: 委托分离{}个样本",
                channel_idx,
                channel_samples.len(),
                channel_idx,
                channel_samples.len()
            );

            // 委托算法层进行DR计算
            let result = channel_processor(&channel_samples, channel_idx)?;
            dr_results.push(result);
        }

        Ok(dr_results)
    }

    /// 获取委托的SIMD能力信息
    pub fn simd_capabilities(&self) -> &super::simd_core::SimdCapabilities {
        self.channel_separator.simd_capabilities()
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
        println!(
            "Processing coordinator SIMD capabilities: {caps:?} / 协调器SIMD能力: {caps:?}",
            caps = coordinator.simd_capabilities()
        );
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

        println!("Processing coordination test passed / 协调器处理测试通过");
        println!(
            "   Processing time: {time} μs / 处理时间: {time} μs",
            time = result.performance_stats.total_duration_us
        );
        println!(
            "   Throughput: {:.0} samples/s / 样本处理速度: {:.0} samples/s",
            result.performance_stats.samples_per_second,
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

        println!("Coordinator consistency check passed / 协调器一致性验证通过");
    }

    // ==================== 阶段1：参数验证与错误处理 ====================

    #[test]
    fn test_empty_samples_error() {
        let coordinator = ProcessingCoordinator::new();

        // 测试空样本应该返回错误
        let result = coordinator.process_channels(&[], 1, |_samples, _idx| {
            use crate::core::DrResult;
            Ok(DrResult {
                channel: 0,
                dr_value: 0.0,
                rms: 0.0,
                peak: 0.0,
                primary_peak: 0.0,
                secondary_peak: 0.0,
                sample_count: 0,
            })
        });

        assert!(result.is_err());
        if let Err(AudioError::InvalidInput(msg)) = result {
            assert!(msg.contains("样本数据不能为空"));
        } else {
            panic!("Expected InvalidInput error");
        }
    }

    #[test]
    fn test_sample_channel_mismatch_error() {
        let coordinator = ProcessingCoordinator::new();

        // 测试样本数不是声道数倍数的错误
        let samples = vec![0.5, 0.3, 0.7]; // 3个样本，无法整除2声道
        let result = coordinator.process_channels(&samples, 2, |_samples, _idx| {
            use crate::core::DrResult;
            Ok(DrResult {
                channel: 0,
                dr_value: 0.0,
                rms: 0.0,
                peak: 0.0,
                primary_peak: 0.0,
                secondary_peak: 0.0,
                sample_count: 0,
            })
        });

        assert!(result.is_err());
        if let Err(AudioError::InvalidInput(msg)) = result {
            assert!(msg.contains("必须是声道数"));
            assert!(msg.contains("的倍数"));
        } else {
            panic!("Expected InvalidInput error with mismatch message");
        }
    }

    #[test]
    fn test_callback_error_propagation() {
        let coordinator = ProcessingCoordinator::new();

        let samples = vec![0.5, 0.3, 0.7, 0.4]; // 2声道，2个样本每声道

        // 测试回调函数错误应该被传播
        let result = coordinator.process_channels(&samples, 2, |_samples, _idx| {
            Err(AudioError::CalculationError(
                "Simulated DR calculation failed / 模拟DR计算失败".to_string(),
            ))
        });

        assert!(result.is_err());
        if let Err(AudioError::CalculationError(msg)) = result {
            assert_eq!(msg, "Simulated DR calculation failed / 模拟DR计算失败");
        } else {
            panic!("Expected CalculationError");
        }
    }

    // ==================== 阶段2：单声道处理路径测试 ====================

    #[test]
    fn test_mono_sequential_processing() {
        let coordinator = ProcessingCoordinator::new();

        // 单声道样本数据
        let samples = vec![0.1, 0.2, 0.3, 0.5, 1.0, 0.8]; // 6个单声道样本

        let result = coordinator
            .process_channels(&samples, 1, |channel_samples, channel_idx| {
                use crate::core::DrResult;
                // 验证是单声道
                assert_eq!(channel_idx, 0);
                assert_eq!(channel_samples.len(), 6);

                Ok(DrResult {
                    channel: channel_idx,
                    dr_value: 12.0,
                    rms: 0.3,
                    peak: 1.0,
                    primary_peak: 1.0,
                    secondary_peak: 0.8,
                    sample_count: channel_samples.len(),
                })
            })
            .unwrap();

        // 验证单声道结果
        assert_eq!(result.dr_results.len(), 1);
        assert_eq!(result.performance_stats.channels_processed, 1);
        assert_eq!(result.performance_stats.total_samples, 6);
    }

    #[test]
    fn test_mono_channel_extraction() {
        let coordinator = ProcessingCoordinator::new();

        // 单声道数据，验证声道分离逻辑
        let samples = vec![0.5, 0.6, 0.7, 0.8];

        coordinator
            .process_channels(&samples, 1, |channel_samples, _idx| {
                use crate::core::DrResult;
                // 单声道应该提取所有样本
                assert_eq!(channel_samples, &samples[..]);

                Ok(DrResult {
                    channel: 0,
                    dr_value: 10.0,
                    rms: 0.5,
                    peak: 0.8,
                    primary_peak: 0.8,
                    secondary_peak: 0.7,
                    sample_count: channel_samples.len(),
                })
            })
            .unwrap();
    }

    #[test]
    fn test_mono_vs_stereo_performance_stats() {
        let coordinator = ProcessingCoordinator::new();

        // 单声道样本
        let mono_samples = vec![0.5; 100];
        let mono_result = coordinator
            .process_channels(&mono_samples, 1, |samples, idx| {
                use crate::core::DrResult;
                Ok(DrResult {
                    channel: idx,
                    dr_value: 10.0,
                    rms: 0.3,
                    peak: 0.5,
                    primary_peak: 0.5,
                    secondary_peak: 0.4,
                    sample_count: samples.len(),
                })
            })
            .unwrap();

        // 立体声样本（相同总样本数）
        let stereo_samples = vec![0.5; 100];
        let stereo_result = coordinator
            .process_channels(&stereo_samples, 2, |samples, idx| {
                use crate::core::DrResult;
                Ok(DrResult {
                    channel: idx,
                    dr_value: 10.0,
                    rms: 0.3,
                    peak: 0.5,
                    primary_peak: 0.5,
                    secondary_peak: 0.4,
                    sample_count: samples.len(),
                })
            })
            .unwrap();

        // 验证统计信息差异
        assert_eq!(mono_result.performance_stats.channels_processed, 1);
        assert_eq!(stereo_result.performance_stats.channels_processed, 2);
        assert_eq!(mono_result.performance_stats.total_samples, 100);
        assert_eq!(stereo_result.performance_stats.total_samples, 100);

        // 验证每声道样本数通过DrResult获得
        assert_eq!(mono_result.dr_results[0].sample_count, 100);
        assert_eq!(stereo_result.dr_results[0].sample_count, 50);
    }

    // ==================== 阶段3：辅助方法与报告生成测试 ====================

    #[test]
    fn test_simd_capabilities_access() {
        let coordinator = ProcessingCoordinator::new();

        // 访问委托的SIMD能力
        let capabilities = coordinator.simd_capabilities();

        // 验证SIMD能力信息存在
        assert!(std::mem::size_of_val(capabilities) > 0);
        println!("SIMD capabilities: {capabilities:?} / SIMD能力: {capabilities:?}");
    }

    #[test]
    fn test_performance_evaluator_access() {
        let coordinator = ProcessingCoordinator::new();

        // 访问委托的性能评估器
        let evaluator = coordinator.performance_evaluator();

        // 验证评估器存在
        assert!(std::mem::size_of_val(evaluator) > 0);
    }

    #[test]
    fn test_performance_report_generation() {
        let coordinator = ProcessingCoordinator::new();

        let samples = vec![0.5; 100];
        let result = coordinator
            .process_channels(&samples, 2, |samples, idx| {
                use crate::core::DrResult;
                Ok(DrResult {
                    channel: idx,
                    dr_value: 12.0,
                    rms: 0.4,
                    peak: 0.5,
                    primary_peak: 0.5,
                    secondary_peak: 0.45,
                    sample_count: samples.len(),
                })
            })
            .unwrap();

        // 生成性能报告
        let report = coordinator.generate_performance_report(&result);

        // 验证报告包含关键信息
        assert!(!report.is_empty());
        assert!(report.contains("SIMD") || report.contains("性能") || report.contains("samples"));
        println!("Performance report:\n{report}\n性能报告:\n{report}");
    }

    // ==================== 阶段4：高级功能测试 ====================

    #[test]
    fn test_default_trait() {
        // 测试Default trait实现
        let coordinator = ProcessingCoordinator::default();

        // 验证通过default创建的协调器功能正常
        let samples = vec![0.5; 10];
        let result = coordinator.process_channels(&samples, 1, |samples, idx| {
            use crate::core::DrResult;
            Ok(DrResult {
                channel: idx,
                dr_value: 10.0,
                rms: 0.3,
                peak: 0.5,
                primary_peak: 0.5,
                secondary_peak: 0.4,
                sample_count: samples.len(),
            })
        });

        assert!(result.is_ok());
    }

    #[test]
    fn test_large_sample_processing() {
        let coordinator = ProcessingCoordinator::new();

        // 测试大样本处理（模拟真实场景）
        let large_samples = vec![0.5; 48000 * 2]; // 1秒立体声@48kHz

        let result = coordinator
            .process_channels(&large_samples, 2, |samples, idx| {
                use crate::core::DrResult;
                // 验证每声道样本数正确
                assert_eq!(samples.len(), 48000);

                Ok(DrResult {
                    channel: idx,
                    dr_value: 15.0,
                    rms: 0.2,
                    peak: 0.5,
                    primary_peak: 0.5,
                    secondary_peak: 0.45,
                    sample_count: samples.len(),
                })
            })
            .unwrap();

        // 验证大样本处理结果
        assert_eq!(result.dr_results.len(), 2);
        assert_eq!(result.performance_stats.total_samples, 96000);
        assert_eq!(result.dr_results[0].sample_count, 48000); // 每声道样本数
        assert!(result.performance_stats.samples_per_second > 0.0);
    }

    #[test]
    fn test_simd_usage_stats() {
        let coordinator = ProcessingCoordinator::new();

        let samples = vec![0.5; 1000];
        let result = coordinator
            .process_channels(&samples, 2, |samples, idx| {
                use crate::core::DrResult;
                Ok(DrResult {
                    channel: idx,
                    dr_value: 12.0,
                    rms: 0.3,
                    peak: 0.5,
                    primary_peak: 0.5,
                    secondary_peak: 0.4,
                    sample_count: samples.len(),
                })
            })
            .unwrap();

        // 平台健壮化断言（支持无SIMD的罕见平台）
        // 在大多数平台（x86_64, aarch64）上使用SIMD，在罕见的无SIMD平台上则使用scalar路径

        // 获取SIMD支持状态
        use crate::processing::simd_core::SimdProcessor;
        let simd_processor = SimdProcessor::new();
        let has_simd_support = simd_processor.capabilities().has_basic_simd();

        if has_simd_support {
            // 常见平台（x86_64/aarch64）：期望使用SIMD
            assert!(
                result.simd_usage.used_simd,
                "Expected SIMD to be used on common platforms (x86_64/aarch64)"
            );
            assert_eq!(
                result.simd_usage.simd_samples, 1000,
                "All samples should be processed with SIMD"
            );
            assert_eq!(
                result.simd_usage.scalar_samples, 0,
                "Scalar samples should be 0 when SIMD is available"
            );
            // 验证SIMD覆盖率为1.0（即100%，允许浮点误差）
            assert!(
                (result.simd_usage.simd_coverage - 1.0).abs() < 0.01,
                "SIMD coverage should be ~1.0 (100%), got {}",
                result.simd_usage.simd_coverage
            );
        } else {
            // 罕见平台（无SIMD支持）：期望使用scalar路径
            assert!(
                !result.simd_usage.used_simd,
                "SIMD should not be used on platforms without SIMD support"
            );
            assert_eq!(
                result.simd_usage.simd_samples, 0,
                "SIMD samples should be 0 on non-SIMD platforms"
            );
            assert_eq!(
                result.simd_usage.scalar_samples, 1000,
                "All samples should be processed with scalar on non-SIMD platforms"
            );
            assert_eq!(
                result.simd_usage.simd_coverage, 0.0,
                "SIMD coverage should be 0.0 on non-SIMD platforms"
            );
        }
    }
}
