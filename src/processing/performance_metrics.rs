//! 性能评估和统计模块
//!
//! 负责音频处理性能的评估、统计和报告，为BatchProcessor提供专业化的性能分析服务。
//! 包含SIMD加速比估算、处理速度统计等功能。

use super::simd_core::SimdCapabilities;
use crate::core::DrResult;

// 跨平台性能常量（基于SIMD指令集的典型加速比）
const DEFAULT_SIMD_SPEEDUP_BASELINE: f64 = 1.0;

// x86_64 SIMD加速因子
const SSE2_TYPICAL_SPEEDUP_FACTOR: f64 = 3.5; // SSE2基线（保守估计）
const SSE4_1_SPEEDUP_BONUS: f64 = 1.1; // SSE4.1额外加成
const AVX_TYPICAL_SPEEDUP_FACTOR: f64 = 5.5; // AVX基线（保守估计）
const AVX2_SPEEDUP_BONUS: f64 = 1.0; // AVX2完整支持（无折扣）

// ARM SIMD加速因子（独立建模，避免与x86混淆）
const NEON_TYPICAL_SPEEDUP_FACTOR: f64 = 3.8; // ARM NEON基线（Apple Silicon实测）
const NEON_FP16_SPEEDUP_BONUS: f64 = 1.1; // NEON FP16额外加成

// 数据量阈值常量（按每声道样本数维度，跨采样率稳定）
const SMALL_DATASET_THRESHOLD: usize = 1000; // 小数据集：<1000样本/声道（~21ms@48kHz）
const LARGE_DATASET_THRESHOLD: usize = 100000; // 大数据集：>100k样本/声道（~2.1s@48kHz）

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

impl PerformanceStats {
    /// 计算每秒吞吐量（MB/s）
    ///
    /// 基于样本数、采样率、位深度计算出MB/s吞吐量，
    /// 便于与I/O、解码链路的性能指标对齐。
    ///
    /// # 参数
    ///
    /// * `bits_per_sample` - 位深度（如16、24、32）
    ///
    /// # 返回值
    ///
    /// 返回MB/s吞吐量（按 1 MiB = 1024×1024 字节计算，非SI单位的1000×1000）
    ///
    /// # 示例
    ///
    /// ```ignore
    /// let stats = PerformanceStats { /* ... */ };
    /// let throughput_mbps = stats.throughput_mb_per_second(16); // 16位音频
    /// println!("处理速度: {:.2} MB/s", throughput_mbps);
    /// ```
    pub fn throughput_mb_per_second(&self, bits_per_sample: u32) -> f64 {
        if self.total_duration_us == 0 {
            return 0.0;
        }

        // 计算总字节数：样本数 × (位深度/8)
        let total_bytes = self.total_samples as f64 * (bits_per_sample as f64 / 8.0);

        // 计算秒数
        let duration_seconds = self.total_duration_us as f64 / 1_000_000.0;

        // MB/s = 总字节数 / (1024*1024) / 秒数
        (total_bytes / (1024.0 * 1024.0)) / duration_seconds
    }

    /// 计算每秒吞吐量（MB/s），自动推断为32位浮点
    ///
    /// 默认使用32位浮点（内部处理格式）计算吞吐量。
    ///
    /// # 返回值
    ///
    /// 返回MB/s吞吐量（基于f32样本，按 1 MiB = 1024×1024 字节计算）
    pub fn throughput_mb_per_second_f32(&self) -> f64 {
        self.throughput_mb_per_second(32)
    }
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
    /// ```ignore
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
    /// **重要**: 直接依据SIMD能力位判断，而非recommended_parallelism()，
    /// 以支持未来AVX2实现且独立建模ARM NEON。
    ///
    /// **实现状态**: AVX/AVX2估算是面向未来的预估值。当前实现仅支持SSE2/NEON
    /// (4宽并行)，但估算已按AVX因子计算以反映硬件潜力。实际运行时会降级到
    /// SSE2/NEON路径。
    ///
    /// # 参数
    ///
    /// * `sample_count` - 处理的样本数量（每声道）
    ///
    /// # 返回值
    ///
    /// 返回预期的SIMD加速比（倍数），保证 >= 1.0
    ///
    /// # 示例
    ///
    /// ```ignore
    /// use macinmeter_dr_tool::processing::PerformanceEvaluator;
    ///
    /// let evaluator = PerformanceEvaluator::new();
    /// let speedup = evaluator.estimate_simd_speedup(48000); // 1秒48kHz音频
    /// assert!(speedup >= 1.0); // 至少不会比标量慢
    /// ```
    pub fn estimate_simd_speedup(&self, sample_count: usize) -> f64 {
        // 基于SIMD能力位直接判断基线加速比（支持未来AVX2扩展）
        let base_speedup = if self.capabilities.avx2 {
            // AVX2: 当前未实现8宽SIMD，但为未来预留估算路径
            AVX_TYPICAL_SPEEDUP_FACTOR * AVX2_SPEEDUP_BONUS
        } else if self.capabilities.avx {
            // AVX: 当前未实现，但检测到硬件支持时使用AVX估算
            AVX_TYPICAL_SPEEDUP_FACTOR
        } else if self.capabilities.neon {
            // ARM NEON: 独立建模，避免与x86 SSE2混淆
            let base = NEON_TYPICAL_SPEEDUP_FACTOR;
            if self.capabilities.neon_fp16 {
                base * NEON_FP16_SPEEDUP_BONUS
            } else {
                base
            }
        } else if self.capabilities.sse2 {
            // x86 SSE2: 当前主要实现路径（4样本并行）
            let base = SSE2_TYPICAL_SPEEDUP_FACTOR;
            if self.capabilities.sse4_1 {
                base * SSE4_1_SPEEDUP_BONUS
            } else {
                base
            }
        } else {
            // 无SIMD支持：标量实现
            DEFAULT_SIMD_SPEEDUP_BASELINE
        };

        // 根据数据量调整加速比（小数据集开销相对更大）
        let size_factor = if sample_count < SMALL_DATASET_THRESHOLD {
            0.7 // 小数据集效率降低
        } else if sample_count > LARGE_DATASET_THRESHOLD {
            1.1 // 大数据集效率提升
        } else {
            1.0
        };

        // 最终加速比计算（标量路径不应因数据集大小"加速"）
        let final_speedup = if base_speedup == 1.0 {
            // 标量实现：无论数据集大小，加速比恒为1.0
            1.0
        } else {
            // SIMD实现：应用数据集大小系数，保证 >= 1.0
            (base_speedup * size_factor).max(1.0)
        };

        debug_performance!(
            "SIMD加速比估算: 基础={:.1}x, 大小系数={:.1}, 最终={:.1}x (能力={})",
            base_speedup,
            size_factor,
            final_speedup,
            if self.capabilities.avx2 {
                "AVX2"
            } else if self.capabilities.avx {
                "AVX"
            } else if self.capabilities.neon {
                "NEON"
            } else if self.capabilities.sse2 {
                "SSE2"
            } else {
                "Scalar"
            }
        );

        final_speedup
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
    /// **重要**: `used_simd` 字段由 `simd_samples > 0` 自动推导，
    /// 避免调用方传入值与实际计数不一致。
    ///
    /// # 参数
    ///
    /// * `simd_samples` - SIMD处理的样本数
    /// * `scalar_samples` - 标量处理的样本数
    ///
    /// # 返回值
    ///
    /// 返回SIMD使用统计信息
    pub fn create_simd_usage_stats(
        &self,
        simd_samples: usize,
        scalar_samples: usize,
    ) -> SimdUsageStats {
        let total_samples = simd_samples + scalar_samples;
        let simd_coverage = if total_samples > 0 {
            simd_samples as f64 / total_samples as f64
        } else {
            0.0
        };

        // 自动推导 used_simd：只要有SIMD样本就认为使用了SIMD
        let used_simd = simd_samples > 0;

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

        // 测试有SIMD样本的情况
        let stats = evaluator.create_simd_usage_stats(9000, 1000);

        assert!(stats.used_simd); // 自动推导：simd_samples > 0
        assert_eq!(stats.simd_samples, 9000);
        assert_eq!(stats.scalar_samples, 1000);
        assert!((stats.simd_coverage - 0.9).abs() < 1e-6);

        // 测试无SIMD样本的情况
        let stats_no_simd = evaluator.create_simd_usage_stats(0, 1000);
        assert!(!stats_no_simd.used_simd); // 自动推导：simd_samples == 0

        println!("SIMD使用统计测试通过:");
        println!("  SIMD覆盖率: {:.1}%", stats.simd_coverage * 100.0);
        println!("  无SIMD时used_simd={}", stats_no_simd.used_simd);
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
