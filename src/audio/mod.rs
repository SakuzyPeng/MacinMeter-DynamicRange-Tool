//! 音频解码模块
//!
//! 提供多格式音频文件的解码支持。
//!
//! **使用 `UniversalDecoder`** - 统一解码器架构，支持所有格式并具备可扩展性

// 统一解码器架构 - 唯一推荐的解码器
pub mod universal_decoder;

// 导出新的统一解码器（推荐使用）
pub use universal_decoder::{
    AudioDecoder as UniversalAudioDecoder, AudioFormat, FormatSupport,
    StreamingDecoder as UniversalStreamingDecoder, UniversalDecoder,
};
