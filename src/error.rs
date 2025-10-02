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

    /// 资源访问错误 - 第7层防护
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

// ==================== 错误转换Helper函数 ====================
// 消除重复的 .map_err(|e| AudioError::XXX(format!(...))) 模式

/// 创建格式错误的helper函数
#[inline]
pub fn format_error<E: fmt::Display>(context: &str, err: E) -> AudioError {
    AudioError::FormatError(format!("{context}: {err}"))
}

/// 创建解码错误的helper函数
#[inline]
pub fn decoding_error<E: fmt::Display>(context: &str, err: E) -> AudioError {
    AudioError::DecodingError(format!("{context}: {err}"))
}

/// 创建计算错误的helper函数
#[inline]
pub fn calculation_error<E: fmt::Display>(context: &str, err: E) -> AudioError {
    AudioError::CalculationError(format!("{context}: {err}"))
}

// ==================== 错误分类系统 ====================
// 用于批量处理中的错误统计和分析

/// 错误类别枚举（用于批量处理统计）
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum ErrorCategory {
    /// 格式相关错误（不支持的格式、格式损坏等）
    Format,
    /// 解码相关错误（解码器失败、音频数据损坏等）
    Decoding,
    /// I/O相关错误（文件不存在、权限不足等）
    Io,
    /// 计算相关错误（数值异常、内存不足等）
    Calculation,
    /// 其他未分类错误
    Other,
}

impl ErrorCategory {
    /// 从AudioError提取错误类别
    pub fn from_audio_error(e: &AudioError) -> Self {
        match e {
            AudioError::FormatError(_) => Self::Format,
            AudioError::DecodingError(_) => Self::Decoding,
            AudioError::IoError(_) => Self::Io,
            AudioError::CalculationError(_) | AudioError::OutOfMemory => Self::Calculation,
            AudioError::InvalidInput(_) | AudioError::ResourceError(_) => Self::Other,
        }
    }

    /// 获取错误类别的显示名称
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Format => "格式错误",
            Self::Decoding => "解码错误",
            Self::Io => "I/O错误",
            Self::Calculation => "计算错误",
            Self::Other => "其他错误",
        }
    }
}
