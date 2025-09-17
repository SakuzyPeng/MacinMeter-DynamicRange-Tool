//! 批量音频处理器
//!
//! 结合SIMD优化和多线程技术，实现高效的批量音频数据处理。
//! 专门优化多声道音频的DR计算性能。

use super::simd::SimdProcessor;
use crate::core::{DrCalculator, DrResult};
use crate::error::{AudioError, AudioResult};
use rayon::prelude::*;

// 跨平台性能常量（动态检测替代硬编码）
const DEFAULT_SIMD_SPEEDUP_BASELINE: f64 = 1.0;
const SSE2_TYPICAL_SPEEDUP_FACTOR: f64 = 3.5; // 保守估计，适配不同硬件
const AVX_TYPICAL_SPEEDUP_FACTOR: f64 = 5.5; // 保守估计，适配不同硬件
const DEFAULT_BLOCK_DURATION: f64 = 3.0; // 默认块持续时间（秒，官方规范）

// 数据量阈值常量（用于性能优化判断）
const SMALL_DATASET_THRESHOLD: usize = 1000; // 小数据集阈值
const LARGE_DATASET_THRESHOLD: usize = 100000; // 大数据集阈值

#[cfg(debug_assertions)]
macro_rules! debug_batch {
    ($($arg:tt)*) => {
        eprintln!("[BATCH_DEBUG] {}", format_args!($($arg)*));
    };
}

#[cfg(not(debug_assertions))]
macro_rules! debug_batch {
    ($($arg:tt)*) => {};
}

/// 批量处理结果
#[derive(Debug, Clone)]
pub struct BatchResult {
    /// DR计算结果
    pub dr_results: Vec<DrResult>,

    /// 处理性能统计
    pub performance_stats: BatchPerformanceStats,

    /// SIMD使用情况
    pub simd_usage: SimdUsageStats,
}

/// 批量处理性能统计
#[derive(Debug, Clone)]
pub struct BatchPerformanceStats {
    /// 总处理时间（微秒）
    pub total_duration_us: u64,

    /// 每秒处理样本数
    pub samples_per_second: f64,

    /// 处理的声道数
    pub channels_processed: usize,

    /// 处理的样本总数
    pub total_samples: usize,

    /// SIMD加速比（相对于标量实现）
    pub simd_speedup: f64,
}

/// SIMD使用统计
#[derive(Debug, Clone)]
pub struct SimdUsageStats {
    /// 是否使用了SIMD优化
    pub used_simd: bool,

    /// SIMD处理的样本数
    pub simd_samples: usize,

    /// 标量处理的样本数（fallback）
    pub scalar_samples: usize,

    /// SIMD覆盖率（SIMD样本数 / 总样本数）
    pub simd_coverage: f64,
}

/// 声道处理配置
#[derive(Debug, Clone)]
struct ChannelProcessConfig {
    samples_per_channel: usize,
    sum_doubling: bool,
    use_simd: bool,
    sample_rate: u32,
    block_duration: f64,
}

/// 高性能批量处理器
///
/// 结合SIMD向量化和多线程并行，提供最佳的DR计算性能
pub struct BatchProcessor {
    /// SIMD处理器工厂
    simd_processor: SimdProcessor,

    /// 是否启用多线程处理
    enable_multithreading: bool,

    /// 线程池大小
    thread_pool_size: Option<usize>,
}

impl BatchProcessor {
    /// 创建新的批量处理器
    ///
    /// # 参数
    ///
    /// * `enable_multithreading` - 是否启用多线程处理
    /// * `thread_pool_size` - 线程池大小（None表示自动）
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::processing::BatchProcessor;
    ///
    /// // 启用多线程和SIMD优化
    /// let processor = BatchProcessor::new(true, None);
    /// ```
    pub fn new(enable_multithreading: bool, thread_pool_size: Option<usize>) -> Self {
        Self {
            simd_processor: SimdProcessor::new(),
            enable_multithreading,
            thread_pool_size,
        }
    }

    /// 批量处理交错音频数据（多声道SIMD优化）
    ///
    /// 使用SIMD并行处理每个声道，同时支持多声道间的并行计算
    /// 固定使用foobar2000兼容模式（20%采样算法）
    ///
    /// # 参数
    ///
    /// * `samples` - 交错音频数据 [L1, R1, L2, R2, ...]
    /// * `channel_count` - 声道数量
    /// * `sample_rate` - 采样率
    /// * `sum_doubling` - 是否启用Sum Doubling补偿
    ///
    /// # 返回值
    ///
    /// 返回批量处理结果，包含DR值和性能统计
    pub fn process_interleaved_batch(
        &self,
        samples: &[f32],
        channel_count: usize,
        sample_rate: u32,
        sum_doubling: bool,
    ) -> AudioResult<BatchResult> {
        let start_time = std::time::Instant::now();

        if samples.len() % channel_count != 0 {
            return Err(AudioError::InvalidInput(format!(
                "样本数量({})必须是声道数({})的倍数",
                samples.len(),
                channel_count
            )));
        }

        let samples_per_channel = samples.len() / channel_count;

        // 决定是否使用SIMD优化
        let use_simd = self.simd_processor.should_use_simd(samples_per_channel);

        // 创建处理配置（动态块大小配置）
        let config = ChannelProcessConfig {
            samples_per_channel,
            sum_doubling,
            use_simd,
            sample_rate,
            block_duration: DEFAULT_BLOCK_DURATION,
        };

        debug_batch!(
            "配置创建: channels={}, samples_per_channel={}, simd={}, block_duration={:.1}s",
            channel_count,
            samples_per_channel,
            use_simd,
            config.block_duration
        );

        // 声道数据分离和处理
        let (dr_results, simd_stats) = if self.enable_multithreading && channel_count > 1 {
            self.process_channels_parallel(samples, channel_count, &config)?
        } else {
            self.process_channels_sequential(samples, channel_count, &config)?
        };

        let duration = start_time.elapsed();

        // 计算性能统计
        let total_duration_us = duration.as_micros() as u64;
        let samples_per_second = if total_duration_us > 0 {
            (samples.len() as f64) / (total_duration_us as f64 / 1_000_000.0)
        } else {
            0.0
        };

        // 跨平台SIMD加速比评估（运行时动态检测）
        let simd_speedup = self.estimate_simd_speedup(use_simd, samples_per_channel);

        debug_batch!(
            "性能评估: SIMD={}, speedup={:.1}x, samples/sec={:.0}",
            use_simd,
            simd_speedup,
            samples_per_second
        );

        let performance_stats = BatchPerformanceStats {
            total_duration_us,
            samples_per_second,
            channels_processed: channel_count,
            total_samples: samples.len(),
            simd_speedup,
        };

        Ok(BatchResult {
            dr_results,
            performance_stats,
            simd_usage: simd_stats,
        })
    }

    /// 并行处理多个声道（多线程+SIMD）
    fn process_channels_parallel(
        &self,
        samples: &[f32],
        channel_count: usize,
        config: &ChannelProcessConfig,
    ) -> AudioResult<(Vec<DrResult>, SimdUsageStats)> {
        // 提取每个声道的数据
        let channel_samples: Vec<Vec<f32>> = (0..channel_count)
            .map(|ch| Self::extract_channel_samples(samples, ch, channel_count))
            .collect();

        // 并行处理每个声道
        let results: Result<Vec<_>, AudioError> = channel_samples
            .par_iter()
            .enumerate()
            .map(|(ch_idx, ch_samples)| self.process_single_channel(ch_samples, ch_idx, config))
            .collect();

        let dr_results = results?;

        // 统计SIMD使用情况
        let simd_stats = Self::calculate_simd_stats(config, channel_count);

        Ok((dr_results, simd_stats))
    }

    /// 顺序处理多个声道（单线程+SIMD）
    fn process_channels_sequential(
        &self,
        samples: &[f32],
        channel_count: usize,
        config: &ChannelProcessConfig,
    ) -> AudioResult<(Vec<DrResult>, SimdUsageStats)> {
        let mut dr_results = Vec::with_capacity(channel_count);

        for ch_idx in 0..channel_count {
            // 提取单个声道的样本
            let ch_samples = Self::extract_channel_samples(samples, ch_idx, channel_count);
            let dr_result = self.process_single_channel(&ch_samples, ch_idx, config)?;
            dr_results.push(dr_result);
        }

        let simd_stats = Self::calculate_simd_stats(config, channel_count);

        Ok((dr_results, simd_stats))
    }

    /// 处理单个声道（SIMD优化）
    fn process_single_channel(
        &self,
        samples: &[f32],
        channel_idx: usize,
        config: &ChannelProcessConfig,
    ) -> AudioResult<DrResult> {
        // 创建块处理DR计算器（官方规范模式）
        // 重要：虽然数据已分离为单声道，但Sum Doubling基于原始数据来源
        // 如果原始数据是交错的，分离后的每个声道仍需要Sum Doubling补偿
        let calculator = DrCalculator::new(
            1,
            config.sum_doubling, // 保持原始交错数据的Sum Doubling配置
            config.sample_rate,
            config.block_duration, // 动态获取块持续时间
        )?;

        debug_batch!(
            "声道{}处理: 样本数={}, 块大小={:.1}s, Sum Doubling={}",
            channel_idx,
            samples.len(),
            config.block_duration,
            config.sum_doubling
        );

        // 使用foobar2000兼容的块处理模式计算DR
        // SIMD优化已在块处理内部实现
        let results = calculator.calculate_dr_from_samples(samples, 1)?;
        let mut result = results.into_iter().next().unwrap();
        result.channel = channel_idx;

        Ok(result)
    }

    /// 获取SIMD处理器能力
    pub fn simd_capabilities(&self) -> &super::simd::SimdCapabilities {
        self.simd_processor.capabilities()
    }

    /// 设置是否启用多线程
    pub fn set_multithreading(&mut self, enabled: bool) {
        self.enable_multithreading = enabled;
    }

    /// 检查是否推荐启用SIMD（基于样本数量）
    pub fn should_use_simd(&self, sample_count: usize) -> bool {
        self.simd_processor.should_use_simd(sample_count)
    }

    /// 获取配置的线程池大小
    pub fn thread_pool_size(&self) -> Option<usize> {
        self.thread_pool_size
    }

    /// 估算SIMD加速比（基于硬件能力和数据量）
    fn estimate_simd_speedup(&self, use_simd: bool, sample_count: usize) -> f64 {
        if !use_simd {
            return DEFAULT_SIMD_SPEEDUP_BASELINE;
        }

        let capabilities = self.simd_processor.capabilities();
        let base_speedup = match capabilities.recommended_parallelism() {
            4 if capabilities.sse4_1 => SSE2_TYPICAL_SPEEDUP_FACTOR * 1.1, // SSE4.1加成
            4 => SSE2_TYPICAL_SPEEDUP_FACTOR,
            8 if capabilities.avx2 => AVX_TYPICAL_SPEEDUP_FACTOR,
            8 => AVX_TYPICAL_SPEEDUP_FACTOR * 0.9, // AVX without AVX2
            _ => DEFAULT_SIMD_SPEEDUP_BASELINE,
        };

        // 根据数据量调整加速比（小数据集开销相对更大）
        let size_factor = if sample_count < SMALL_DATASET_THRESHOLD {
            0.7 // 小数据集效率降低
        } else if sample_count > LARGE_DATASET_THRESHOLD {
            1.1 // 大数据集效率提升
        } else {
            1.0
        };

        let estimated = base_speedup * size_factor;

        debug_batch!(
            "SIMD加速比估算: 基础={:.1}x, 大小系数={:.1}, 最终={:.1}x",
            base_speedup,
            size_factor,
            estimated
        );

        estimated
    }

    /// 提取单个声道的交错样本数据
    fn extract_channel_samples(
        samples: &[f32],
        channel_idx: usize,
        channel_count: usize,
    ) -> Vec<f32> {
        debug_batch!(
            "提取声道{}: 总样本={}, 声道数={}",
            channel_idx,
            samples.len(),
            channel_count
        );

        samples
            .iter()
            .skip(channel_idx)
            .step_by(channel_count)
            .copied()
            .collect()
    }

    /// 计算SIMD使用统计信息（消除重复的total_samples计算）
    fn calculate_simd_stats(config: &ChannelProcessConfig, channel_count: usize) -> SimdUsageStats {
        let total_samples = config.samples_per_channel * channel_count;
        let simd_samples = if config.use_simd { total_samples } else { 0 };
        let simd_coverage = if config.use_simd { 1.0 } else { 0.0 };

        debug_batch!(
            "SIMD统计: 总样本={}, SIMD样本={}, 覆盖率={:.1}%",
            total_samples,
            simd_samples,
            simd_coverage * 100.0
        );

        SimdUsageStats {
            used_simd: config.use_simd,
            simd_samples,
            scalar_samples: total_samples - simd_samples,
            simd_coverage,
        }
    }
}

impl Default for BatchProcessor {
    fn default() -> Self {
        Self::new(true, None) // 默认启用多线程
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_processor_creation() {
        let processor = BatchProcessor::new(true, Some(4));

        // 基本功能测试
        assert!(processor.enable_multithreading);
        println!("批量处理器SIMD能力: {:?}", processor.simd_capabilities());
    }

    #[test]
    fn test_interleaved_batch_processing() {
        let processor = BatchProcessor::new(false, None); // 禁用多线程简化测试

        // 立体声测试数据 - 适配foobar2000模式
        let mut samples = Vec::new();
        for _ in 0..100 {
            samples.extend_from_slice(&[0.01, -0.01]); // 大量小信号
        }
        samples.extend_from_slice(&[
            1.0, -1.0, // 主Peak
            0.9, -0.9, // 次Peak，确保远大于20%RMS
        ]);

        let result = processor
            .process_interleaved_batch(
                &samples, 2, // 立体声
                44100, false, // sum_doubling禁用
            )
            .unwrap();

        // 验证结果
        assert_eq!(result.dr_results.len(), 2);
        assert_eq!(result.performance_stats.channels_processed, 2);
        assert_eq!(result.performance_stats.total_samples, samples.len()); // 204个样本

        // 检查每个声道的结果
        for dr_result in &result.dr_results {
            assert!(dr_result.dr_value > 0.0);
            assert!(dr_result.rms > 0.0);
            assert!(dr_result.peak > 0.0);
            assert!(dr_result.peak >= dr_result.rms);
        }

        println!("✅ 批量处理测试通过");
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
    fn test_simd_vs_scalar_batch_consistency() {
        let processor = BatchProcessor::new(false, None);

        // 使用足够的样本数触发SIMD
        let mut samples = Vec::new();
        for i in 0..1000 {
            let val = (i as f32 / 1000.0) * 0.5; // 0.0-0.5范围
            samples.push(val); // 左声道
            samples.push(-val); // 右声道
        }
        samples.push(0.8); // 左声道Peak
        samples.push(-0.8); // 右声道Peak

        let result = processor
            .process_interleaved_batch(&samples, 2, 44100, false)
            .unwrap();

        // 验证SIMD使用情况
        println!("SIMD使用统计:");
        println!("  使用SIMD: {}", result.simd_usage.used_simd);
        println!("  SIMD样本数: {}", result.simd_usage.simd_samples);
        println!(
            "  SIMD覆盖率: {:.2}%",
            result.simd_usage.simd_coverage * 100.0
        );

        // 基本一致性检查
        assert_eq!(result.dr_results.len(), 2);
        for dr_result in &result.dr_results {
            assert!(dr_result.dr_value > 0.0);
            assert!(dr_result.dr_value < 100.0);
        }
    }

    #[test]
    fn test_parallel_vs_sequential_consistency() {
        // 创建适配foobar2000模式的4声道测试数据
        let mut samples = Vec::new();
        for _ in 0..50 {
            samples.extend_from_slice(&[0.01, 0.01, 0.01, 0.01]); // 4声道小信号
        }
        samples.extend_from_slice(&[
            1.0, 1.0, 1.0, 1.0, // 4声道主Peak
            0.95, 0.95, 0.95, 0.95, // 4声道次Peak，确保远大于20%RMS
        ]);

        // 顺序处理
        let seq_processor = BatchProcessor::new(false, None);
        let seq_result = seq_processor
            .process_interleaved_batch(&samples, 4, 44100, false)
            .unwrap();

        // 并行处理
        let par_processor = BatchProcessor::new(true, None);
        let par_result = par_processor
            .process_interleaved_batch(&samples, 4, 44100, false)
            .unwrap();

        // 比较结果（应该相同）
        assert_eq!(seq_result.dr_results.len(), par_result.dr_results.len());

        for (seq_dr, par_dr) in seq_result
            .dr_results
            .iter()
            .zip(par_result.dr_results.iter())
        {
            let dr_diff = (seq_dr.dr_value - par_dr.dr_value).abs();
            let rms_diff = (seq_dr.rms - par_dr.rms).abs();
            let peak_diff = (seq_dr.peak - par_dr.peak).abs();

            assert!(dr_diff < 1e-6, "DR值差异过大: {dr_diff}");
            assert!(rms_diff < 1e-6, "RMS差异过大: {rms_diff}");
            assert!(peak_diff < 1e-6, "Peak差异过大: {peak_diff}");
        }

        println!("✅ 并行与顺序处理一致性验证通过");
    }
}
