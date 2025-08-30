//! 高性能音频处理模块
//!
//! 实现SIMD向量化、并行处理等性能优化技术，
//! 目标实现6-7倍性能提升同时保持100%精度一致性。

pub mod batch;
pub mod simd;

// 重新导出公共接口
pub use batch::{BatchProcessor, BatchResult};
pub use simd::{SimdCapabilities, SimdChannelData, SimdProcessor};
