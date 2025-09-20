//! 性能评估和统计模块
//!
//! 负责音频处理性能的评估、统计和报告，为BatchProcessor提供专业化的性能分析服务。
//! 包含SIMD加速比估算、处理速度统计等功能。

use super::simd_channel_data::SimdCapabilities;
use crate::core::DrResult;

// 跨平台性能常量（动态检测替代硬编码）
const DEFAULT_SIMD_SPEEDUP_BASELINE: f64 = 1.0;
const SSE2_TYPICAL_SPEEDUP_FACTOR: f64 = 3.5; // 保守估计，适配不同硬件
const AVX_TYPICAL_SPEEDUP_FACTOR: f64 = 5.5; // 保守估计，适配不同硬件

// 数据量阈值常量（用于性能优化判断）
const SMALL_DATASET_THRESHOLD: usize = 1000; // 小数据集阈值
const LARGE_DATASET_THRESHOLD: usize = 100000; // 大数据集阈值

#[cfg(debug_assertions)]
macro_rules! debug_performance {
    ($($arg:tt)*) => {
        eprintln!("[METRICS_DEBUG] {}", format_args!($($arg)*));
    };
}

#[cfg(not(debug_assertions))]
macro_rules! debug_performance {
    ($($arg:tt)*) => {};
}

/// 高性能处理结果
#[derive(Debug, Clone)]
pub struct PerformanceResult {
    /// DR计算结果
    pub dr_results: Vec<DrResult>,

    /// 处理性能统计
    pub performance_stats: PerformanceStats,

    /// SIMD使用情况
    pub simd_usage: SimdUsageStats,
}

/// 高性能处理统计
#[derive(Debug, Clone)]
pub struct PerformanceStats {
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

/// 性能评估器
///
/// 专门负责音频处理性能的评估和统计计算，
/// 提供SIMD加速比估算、处理速度分析等功能。
pub struct PerformanceEvaluator {
    /// SIMD能力缓存
    capabilities: SimdCapabilities,
}

impl PerformanceEvaluator {
    /// 创建新的性能评估器
    ///
    /// 自动检测硬件SIMD能力并缓存用于性能估算。
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::processing::PerformanceEvaluator;
    ///
    /// let evaluator = PerformanceEvaluator::new();
    /// let speedup = evaluator.estimate_simd_speedup(10000);
    /// println!("预期SIMD加速比: {:.1}x", speedup);
    /// ```
    pub fn new() -> Self {
        Self {
            capabilities: SimdCapabilities::detect(),
        }
    }

    /// 基于SIMD能力创建性能评估器
    pub fn with_capabilities(capabilities: SimdCapabilities) -> Self {
        Self { capabilities }
    }

    /// 获取SIMD能力信息
    pub fn capabilities(&self) -> &SimdCapabilities {
        &self.capabilities
    }

    /// 估算SIMD加速比（基于硬件能力和数据量）
    ///
    /// 根据检测到的硬件SIMD能力和数据集大小，
    /// 估算相对于标量实现的性能提升倍数。
    ///
    /// # 参数
    ///
    /// * `sample_count` - 处理的样本数量
    ///
    /// # 返回值
    ///
    /// 返回预期的SIMD加速比（倍数）
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::processing::PerformanceEvaluator;
    ///
    /// let evaluator = PerformanceEvaluator::new();
    /// let speedup = evaluator.estimate_simd_speedup(48000); // 1秒48kHz音频
    /// assert!(speedup >= 1.0); // 至少不会比标量慢
    /// ```
    pub fn estimate_simd_speedup(&self, sample_count: usize) -> f64 {
        let base_speedup = match self.capabilities.recommended_parallelism() {
            4 if self.capabilities.sse4_1 => SSE2_TYPICAL_SPEEDUP_FACTOR * 1.1, // SSE4.1加成
            4 => SSE2_TYPICAL_SPEEDUP_FACTOR,
            8 if self.capabilities.avx2 => AVX_TYPICAL_SPEEDUP_FACTOR,
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

        debug_performance!(
            "SIMD加速比估算: 基础={:.1}x, 大小系数={:.1}, 最终={:.1}x",
            base_speedup,
            size_factor,
            estimated
        );

        estimated
    }

    /// 计算性能统计信息
    ///
    /// 基于处理时间、样本数量等信息计算详细的性能统计。
    ///
    /// # 参数
    ///
    /// * `duration_us` - 处理总时间（微秒）
    /// * `total_samples` - 处理的样本总数
    /// * `channel_count` - 处理的声道数
    /// * `sample_count_per_channel` - 每声道的样本数
    ///
    /// # 返回值
    ///
    /// 返回详细的性能统计信息
    pub fn calculate_performance_stats(
        &self,
        duration_us: u64,
        total_samples: usize,
        channel_count: usize,
        sample_count_per_channel: usize,
    ) -> PerformanceStats {
        let samples_per_second = if duration_us > 0 {
            (total_samples as f64) / (duration_us as f64 / 1_000_000.0)
        } else {
            0.0
        };

        let simd_speedup = self.estimate_simd_speedup(sample_count_per_channel);

        debug_performance!(
            "性能统计计算: 样本数={}, 处理时间={}μs, 速度={:.0} samples/s, 加速比={:.1}x",
            total_samples,
            duration_us,
            samples_per_second,
            simd_speedup
        );

        PerformanceStats {
            total_duration_us: duration_us,
            samples_per_second,
            channels_processed: channel_count,
            total_samples,
            simd_speedup,
        }
    }

    /// 创建SIMD使用统计
    ///
    /// 生成SIMD优化使用情况的统计信息。
    ///
    /// # 参数
    ///
    /// * `used_simd` - 是否使用了SIMD优化
    /// * `simd_samples` - SIMD处理的样本数
    /// * `scalar_samples` - 标量处理的样本数
    ///
    /// # 返回值
    ///
    /// 返回SIMD使用统计信息
    pub fn create_simd_usage_stats(
        &self,
        used_simd: bool,
        simd_samples: usize,
        scalar_samples: usize,
    ) -> SimdUsageStats {
        let total_samples = simd_samples + scalar_samples;
        let simd_coverage = if total_samples > 0 {
            simd_samples as f64 / total_samples as f64
        } else {
            0.0
        };

        debug_performance!(
            "SIMD使用统计: 使用={}, SIMD样本={}, 标量样本={}, 覆盖率={:.1}%",
            used_simd,
            simd_samples,
            scalar_samples,
            simd_coverage * 100.0
        );

        SimdUsageStats {
            used_simd,
            simd_samples,
            scalar_samples,
            simd_coverage,
        }
    }

    /// 是否推荐使用SIMD优化
    ///
    /// 基于硬件能力和数据量大小判断是否值得启用SIMD优化。
    ///
    /// # 参数
    ///
    /// * `sample_count` - 处理的样本数量
    ///
    /// # 返回值
    ///
    /// 如果推荐使用SIMD优化返回true，否则返回false
    pub fn should_use_simd(&self, sample_count: usize) -> bool {
        // 至少需要基础SIMD支持
        if !self.capabilities.has_basic_simd() {
            return false;
        }

        // 样本数量需要足够大才值得SIMD开销
        // 基于实验数据，至少需要100个样本
        sample_count >= 100
    }

    /// 生成性能报告
    ///
    /// 为调试和分析目的生成详细的性能报告。
    ///
    /// # 参数
    ///
    /// * `stats` - 性能统计信息
    /// * `simd_stats` - SIMD使用统计
    ///
    /// # 返回值
    ///
    /// 返回格式化的性能报告字符串
    pub fn generate_performance_report(
        &self,
        stats: &PerformanceStats,
        simd_stats: &SimdUsageStats,
    ) -> String {
        format!(
            "📊 性能报告:\n\
             ⏱️  处理时间: {:.2}ms\n\
             🚀 处理速度: {:.0} samples/s\n\
             📈 SIMD加速: {:.1}x\n\
             🎯 SIMD覆盖: {:.1}%\n\
             📊 声道数量: {}\n\
             📦 样本总数: {}",
            stats.total_duration_us as f64 / 1000.0,
            stats.samples_per_second,
            stats.simd_speedup,
            simd_stats.simd_coverage * 100.0,
            stats.channels_processed,
            stats.total_samples
        )
    }
}

impl Default for PerformanceEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_evaluator_creation() {
        let evaluator = PerformanceEvaluator::new();
        println!("性能评估器SIMD能力: {:?}", evaluator.capabilities());
    }

    #[test]
    fn test_simd_speedup_estimation() {
        let evaluator = PerformanceEvaluator::new();

        // 小数据集
        let small_speedup = evaluator.estimate_simd_speedup(500);
        assert!(small_speedup >= 1.0);

        // 中等数据集
        let medium_speedup = evaluator.estimate_simd_speedup(10000);
        assert!(medium_speedup >= 1.0);

        // 大数据集
        let large_speedup = evaluator.estimate_simd_speedup(200000);
        assert!(large_speedup >= 1.0);

        // 大数据集应该有更好的加速比（如果支持SIMD）
        if evaluator.capabilities().has_basic_simd() {
            assert!(large_speedup >= medium_speedup);
        }

        println!("SIMD加速比估算测试通过:");
        println!("  小数据集: {small_speedup:.1}x");
        println!("  中数据集: {medium_speedup:.1}x");
        println!("  大数据集: {large_speedup:.1}x");
    }

    #[test]
    fn test_performance_stats_calculation() {
        let evaluator = PerformanceEvaluator::new();

        let stats = evaluator.calculate_performance_stats(
            100000, // 100ms
            48000,  // 1秒48kHz样本
            2,      // 立体声
            24000,  // 每声道24k样本
        );

        assert_eq!(stats.total_duration_us, 100000);
        assert_eq!(stats.total_samples, 48000);
        assert_eq!(stats.channels_processed, 2);
        assert!(stats.samples_per_second > 0.0);
        assert!(stats.simd_speedup >= 1.0);

        println!("性能统计计算测试通过:");
        println!("  处理速度: {:.0} samples/s", stats.samples_per_second);
        println!("  SIMD加速: {:.1}x", stats.simd_speedup);
    }

    #[test]
    fn test_simd_usage_stats() {
        let evaluator = PerformanceEvaluator::new();

        let stats = evaluator.create_simd_usage_stats(true, 9000, 1000);

        assert!(stats.used_simd);
        assert_eq!(stats.simd_samples, 9000);
        assert_eq!(stats.scalar_samples, 1000);
        assert!((stats.simd_coverage - 0.9).abs() < 1e-6);

        println!("SIMD使用统计测试通过:");
        println!("  SIMD覆盖率: {:.1}%", stats.simd_coverage * 100.0);
    }

    #[test]
    fn test_simd_recommendation() {
        let evaluator = PerformanceEvaluator::new();

        // 测试SIMD推荐逻辑
        assert!(!evaluator.should_use_simd(50)); // 太少样本

        // 如果支持SIMD，足够的样本应该推荐使用
        let supports_simd = evaluator.capabilities().has_basic_simd();
        if supports_simd {
            assert!(evaluator.should_use_simd(1000)); // 足够样本且支持SIMD
        } else {
            assert!(!evaluator.should_use_simd(1000)); // 不支持SIMD
        }

        println!("SIMD推荐测试通过 (当前系统SIMD支持: {supports_simd})");
    }

    #[test]
    fn test_performance_report_generation() {
        let evaluator = PerformanceEvaluator::new();

        let stats = PerformanceStats {
            total_duration_us: 50000, // 50ms
            samples_per_second: 960000.0,
            channels_processed: 2,
            total_samples: 48000,
            simd_speedup: 3.5,
        };

        let simd_stats = SimdUsageStats {
            used_simd: true,
            simd_samples: 45000,
            scalar_samples: 3000,
            simd_coverage: 0.9375,
        };

        let report = evaluator.generate_performance_report(&stats, &simd_stats);

        assert!(report.contains("50.00ms"));
        assert!(report.contains("960000"));
        assert!(report.contains("3.5x"));
        assert!(report.contains("93.8%"));

        println!("性能报告生成测试通过:");
        println!("{report}");
    }
}
