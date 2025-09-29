//! 高性能音频处理模块
//!
//! 实现SIMD向量化、并行处理等性能优化技术，
//! 目标实现6-7倍性能提升同时保持高精度一致性。

pub mod channel_data;
pub mod channel_extractor;
pub mod performance_metrics;
pub mod processing_coordinator;
pub mod sample_conversion;
pub mod simd_channel_data;

// 重新导出公共接口
pub use channel_data::ChannelData;
pub use channel_extractor::ChannelExtractor;
pub use performance_metrics::{
    PerformanceEvaluator, PerformanceResult, PerformanceStats, SimdUsageStats,
};
pub use processing_coordinator::ProcessingCoordinator;
pub use sample_conversion::{ConversionStats, SampleConversion, SampleConverter, SampleFormat};
pub use simd_channel_data::{SimdCapabilities, SimdChannelData, SimdProcessor};
