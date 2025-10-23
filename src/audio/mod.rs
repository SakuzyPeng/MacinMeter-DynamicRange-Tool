//! 音频解码模块
//!
//! 提供多格式音频文件的解码支持。
//!
//! # 推荐 API
//!
//! **使用 [`UniversalDecoder`] + [`UniversalStreamingDecoder`]** - 统一解码器架构，支持所有格式并具备可扩展性。
//!
//! ## 快速开始
//!
//! ```rust,no_run
//! use macinmeter_dr_tool::audio::{UniversalDecoder, UniversalStreamingDecoder};
//!
//! // 创建解码器工厂
//! let universal_decoder = UniversalDecoder::new();
//!
//! // 创建流式解码器（自动选择最佳解码策略）
//! let mut decoder: Box<dyn UniversalStreamingDecoder> =
//!     universal_decoder.create_streaming("audio.flac")?;
//!
//! // 获取格式信息
//! let format = decoder.format();
//!
//! // 流式读取音频数据
//! while let Some(samples) = decoder.next_chunk()? {
//!     // 处理音频样本...
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## 核心类型
//!
//! - [`UniversalDecoder`][] - 解码器工厂，提供 `create_streaming()` 等方法
//! - [`UniversalStreamingDecoder`][] - 统一的流式解码器接口（trait 别名）
//! - [`AudioFormat`][] - 音频格式信息（采样率、声道数、位深度等）
//! - [`StreamingDecoder`][] - 底层流式解码器 trait（通常不需要直接使用）

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
