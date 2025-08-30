//! 音频解码模块
//!
//! 提供多格式音频文件的解码支持。

pub mod multi_decoder;
pub mod wav_decoder;

// 重新导出公共接口
pub use multi_decoder::MultiDecoder;
pub use wav_decoder::{AudioFormat, WavDecoder};
