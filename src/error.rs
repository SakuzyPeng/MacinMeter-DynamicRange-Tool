//! 统一错误处理框架
//!
//! 实现8层防御性异常处理机制的核心错误类型定义。

use std::fmt;
use std::io;

/// 音频处理相关的统一错误类型
#[derive(Debug)]
pub enum AudioError {
    /// 输入验证错误 - 第1层防护
    InvalidInput(String),

    /// 文件I/O错误 - 第2层防护  
    IoError(io::Error),

    /// 音频格式错误 - 第3层防护
    FormatError(String),

    /// 解码错误 - 第4层防护
    DecodingError(String),

    /// 计算异常 - 第5层防护
    CalculationError(String),

    /// 内存不足错误 - 第6层防护
    OutOfMemory,

    /// 数值溢出错误 - 第7层防护  
    NumericOverflow(String),

    /// 资源访问错误 - 第8层防护
    ResourceError(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::InvalidInput(msg) => write!(f, "输入验证失败: {msg}"),
            AudioError::IoError(err) => write!(f, "文件I/O错误: {err}"),
            AudioError::FormatError(msg) => write!(f, "音频格式错误: {msg}"),
            AudioError::DecodingError(msg) => write!(f, "音频解码失败: {msg}"),
            AudioError::CalculationError(msg) => write!(f, "计算异常: {msg}"),
            AudioError::OutOfMemory => write!(f, "内存不足"),
            AudioError::NumericOverflow(msg) => write!(f, "数值溢出: {msg}"),
            AudioError::ResourceError(msg) => write!(f, "资源访问错误: {msg}"),
        }
    }
}

impl std::error::Error for AudioError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AudioError::IoError(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for AudioError {
    fn from(err: io::Error) -> Self {
        AudioError::IoError(err)
    }
}

impl From<hound::Error> for AudioError {
    fn from(err: hound::Error) -> Self {
        AudioError::DecodingError(format!("WAV解码错误: {err}"))
    }
}

/// 音频处理操作的标准Result类型
pub type AudioResult<T> = Result<T, AudioError>;
