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
//! 实现24字节ChannelData结构（位于processing::dr_channel_state）、Sum Doubling补偿机制和双Peak回退系统。

pub mod audio;
pub mod core;
pub mod error;
pub mod processing;
pub mod tools;

/// 核心库版本号（与 CLI 同步）
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// 重新导出核心类型
pub use audio::AudioFormat; // 音频格式信息（从 audio 模块直接导出）
pub use core::dr_calculator::DrResult;
pub use error::{AudioError, AudioResult};
pub use processing::ProcessingCoordinator;
pub use tools::{AppConfig, process_streaming_decoder};

// 重新导出样本转换类型（用于SIMD精度和性能测试）
pub use processing::sample_conversion::{
    ConversionStats, SampleConversion, SampleConverter, SampleFormat,
};

// 高层分析输出类型（供外部UI/壳程序直接使用）
pub use tools::processor::AnalysisOutput;

/// 分析单个音频文件的便捷入口（库模式）
///
/// - `path` 为音频文件路径
/// - `config` 使用 `AppConfig` 控制并行度、静音过滤等行为
///
/// 返回的 [`AnalysisOutput`] 与内部 CLI 流程完全一致，
/// 包含官方 DR 结果、精确 DR 和可选的裁切/静音诊断信息。
pub fn analyze_file(path: &std::path::Path, config: &AppConfig) -> AudioResult<AnalysisOutput> {
    tools::processor::process_audio_file(path, config)
}
