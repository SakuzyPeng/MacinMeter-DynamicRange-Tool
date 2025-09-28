//! 音频解码模块
//!
//! 提供多格式音频文件的解码支持。
//!
//! **使用 `UniversalDecoder`** - 统一解码器架构，支持所有格式并具备可扩展性

// 内部子模块（仅供universal_decoder协调器使用）
mod format;
mod stats;
mod streaming;

// Opus音频支持模块
mod opus_decoder;

// 统一解码器架构 - 唯一推荐的解码器
pub mod universal_decoder;

// 导出新的统一解码器（推荐使用）
pub use universal_decoder::{
    AudioFormat, ChunkSizeStats, FormatSupport, StreamingDecoder as UniversalStreamingDecoder,
    UniversalDecoder,
};

// 导出流式解码器接口（供外部使用）
pub use streaming::StreamingDecoder;
