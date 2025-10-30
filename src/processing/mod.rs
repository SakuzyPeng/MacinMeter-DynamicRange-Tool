//! 高性能音频处理模块
//!
//! 实现SIMD向量化、并行处理等性能优化技术。
//!
//! ## 性能目标
//! - **SIMD优化**: 理论峰值6-7x（纯SIMD运算），实际典型3-5x（受内存带宽限制）
//! - **当前实现**: ARM NEON / x86 SSE2，针对f32平方和计算优化
//! - **平台相关**: 向量宽度和内存架构会影响实际加速比

pub mod channel_separator;
pub mod dr_channel_state;
pub mod edge_trimmer;
pub mod performance_metrics;
pub mod processing_coordinator;
pub mod sample_conversion;
pub mod simd_core;

// 重新导出公共接口
pub use processing_coordinator::ProcessingCoordinator; // 外部API

// 便捷导出（保留模块路径兼容性）
pub use channel_separator::ChannelSeparator;

// 内部类型（crate内部使用）
pub(crate) use dr_channel_state::ChannelData;

// 样本转换类型（便于测试与外部直接从 processing 引用）
pub use sample_conversion::{SampleConversion, SampleConverter};

// SIMD能力检测（统一对外引入路径）
pub use simd_core::SimdCapabilities;

// 性能指标类型（统一对外引入路径）
pub use performance_metrics::{
    PerformanceEvaluator, PerformanceResult, PerformanceStats, SimdUsageStats,
};

// 边缘裁切类型（P0阶段实验性功能）
pub use edge_trimmer::{EdgeTrimConfig, EdgeTrimReport, EdgeTrimmer, TrimStats};

/// 静音窗口过滤报告（供输出模块使用）
#[derive(Debug, Clone)]
pub struct SilenceFilterChannelReport {
    pub channel_index: usize,
    pub valid_windows: usize,
    pub filtered_windows: usize,
    pub total_windows: usize,
}

impl SilenceFilterChannelReport {
    #[inline]
    pub fn filtered_percent(&self) -> f64 {
        if self.total_windows == 0 {
            0.0
        } else {
            (self.filtered_windows as f64 / self.total_windows as f64) * 100.0
        }
    }
}

#[derive(Debug, Clone)]
pub struct SilenceFilterReport {
    pub threshold_db: f64,
    pub channels: Vec<SilenceFilterChannelReport>,
}

impl SilenceFilterReport {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.channels
            .iter()
            .all(|c| c.total_windows == 0 || c.filtered_windows == 0)
    }
}
