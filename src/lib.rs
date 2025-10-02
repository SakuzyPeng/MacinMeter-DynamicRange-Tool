//! MacinMeter Dynamic Range (DR) Analysis Tool
//!
//! 基于对foobar2000 DR Meter算法逻辑的独立分析和Rust原创实现。
//!
//! ## 技术实现说明
//! - 通过IDA Pro逆向工程理解算法行为，未使用任何原始源代码
//! - 完全使用Rust语言原创实现，独立的架构设计  
//! - 基于数学公式和算法逻辑的重新实现
//! - 致谢原作者Janne Hyvärinen的foobar2000 DR Meter插件
//!
//! 实现24字节ChannelData结构、Sum Doubling补偿机制和双Peak回退系统。

pub mod audio;
pub mod core;
pub mod error;
pub mod processing;
pub mod tools;

// 重新导出核心类型 - 统一解码器
pub use audio::universal_decoder::AudioFormat;
pub use core::dr_calculator::DrResult;
pub use error::{AudioError, AudioResult};
pub use processing::ProcessingCoordinator;
pub use tools::{AppConfig, process_streaming_decoder};

// 重新导出样本转换类型（用于SIMD精度和性能测试）
pub use processing::sample_conversion::{
    ConversionStats, SampleConversion, SampleConverter, SampleFormat,
};
