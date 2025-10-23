//! 音频解码模块
//!
//! 提供多格式音频文件的解码支持。
//!
//! **推荐使用 `UniversalDecoder`** - 统一解码器架构，支持所有格式并具备可扩展性。
//! 别名 `UniversalStreamingDecoder` 指向统一的流式解码器接口。

// 内部子模块（仅供universal_decoder协调器使用）
mod format;
mod stats;
mod streaming;

// Opus音频支持模块（使用songbird专用解码器）
mod opus_decoder;

// 🚀 有序并行解码器 - 攻击解码瓶颈的核心性能优化
pub mod parallel_decoder;

// 统一解码器架构 - 唯一推荐的解码器
pub mod universal_decoder;

// 导出核心类型（直接从定义模块导出，避免间接依赖）
pub use format::{AudioFormat, FormatSupport};
pub use stats::ChunkSizeStats;
pub use streaming::StreamingDecoder;

// 导出统一解码器（推荐使用）
pub use universal_decoder::{
    StreamingDecoder as UniversalStreamingDecoder, // 统一流式接口别名
    UniversalDecoder,                              // 统一解码器工厂
};

// 导出Opus解码器（⚠️ 仅用于测试和特殊场景，生产环境请使用UniversalDecoder）
pub use opus_decoder::SongbirdOpusDecoder;
