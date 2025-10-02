//! 高性能音频处理模块
//!
//! 实现SIMD向量化、并行处理等性能优化技术，
//! 目标实现6-7倍性能提升同时保持高精度一致性。

pub mod channel_separator;
pub mod dr_channel_state;
pub mod performance_metrics;
pub mod processing_coordinator;
pub mod sample_conversion;
pub mod simd_core;

// 重新导出公共接口
pub use processing_coordinator::ProcessingCoordinator; // 外部API

// 内部类型（crate内部使用）
pub(crate) use channel_separator::ChannelSeparator;
pub(crate) use dr_channel_state::ChannelData;

// 样本转换类型（用于测试，需要pub以便lib.rs重新导出）
pub use sample_conversion::{SampleConversion, SampleConverter};
