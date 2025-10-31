//! 统一错误处理框架
//!
//! 实现7层防御性异常处理机制的核心错误类型定义：
//! 1. InvalidInput - 输入验证错误
//! 2. IoError - 文件I/O错误
//! 3. FormatError - 音频格式错误
//! 4. DecodingError - 音频解码错误
//! 5. CalculationError - 计算异常
//! 6. OutOfMemory - 内存不足（主要用于容量守卫，Rust OOM默认abort）
//! 7. ResourceError - 资源访问错误

use std::fmt;
use std::io;

/// 音频处理相关的统一错误类型
///
/// 标记为 `#[non_exhaustive]` 以便在库演进时新增变体而不破坏外部 match。
#[derive(Debug)]
#[non_exhaustive]
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
            AudioError::InvalidInput(msg) => {
                write!(f, "输入验证失败 / Input validation failed: {msg}")
            }
            AudioError::IoError(err) => write!(f, "文件I/O错误 / File I/O error: {err}"),
            AudioError::FormatError(msg) => write!(f, "音频格式错误 / Audio format error: {msg}"),
            AudioError::DecodingError(msg) => {
                write!(f, "音频解码失败 / Audio decoding failed: {msg}")
            }
            AudioError::CalculationError(msg) => write!(f, "计算异常 / Calculation error: {msg}"),
            AudioError::OutOfMemory => write!(f, "内存不足 / Out of memory"),
            AudioError::ResourceError(msg) => {
                write!(f, "资源访问错误 / Resource access error: {msg}")
            }
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
        AudioError::DecodingError(format!("WAV解码错误 / WAV decoding error: {err}"))
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
            Self::Format => "FORMAT/格式错误",
            Self::Decoding => "DECODING/解码错误",
            Self::Io => "I/O错误",
            Self::Calculation => "CALCULATION/计算错误",
            Self::Other => "OTHER/其他错误",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::io::{Error as IoError, ErrorKind};

    #[test]
    fn test_audio_error_display() {
        // 测试所有错误类型的Display实现
        let errors = vec![
            (
                AudioError::InvalidInput("无效参数".to_string()),
                "输入验证失败 / Input validation failed: 无效参数",
            ),
            (
                AudioError::IoError(IoError::new(ErrorKind::NotFound, "文件未找到")),
                "文件I/O错误 / File I/O error: 文件未找到",
            ),
            (
                AudioError::FormatError("不支持的格式".to_string()),
                "音频格式错误 / Audio format error: 不支持的格式",
            ),
            (
                AudioError::DecodingError("解码失败".to_string()),
                "音频解码失败 / Audio decoding failed: 解码失败",
            ),
            (
                AudioError::CalculationError("除零错误".to_string()),
                "计算异常 / Calculation error: 除零错误",
            ),
            (AudioError::OutOfMemory, "内存不足 / Out of memory"),
            (
                AudioError::ResourceError("资源不可用".to_string()),
                "资源访问错误 / Resource access error: 资源不可用",
            ),
        ];

        for (error, expected_msg) in errors {
            let msg = format!("{error}");
            assert_eq!(
                msg, expected_msg,
                "错误消息格式不匹配 / Error message format mismatch"
            );
        }
    }

    #[test]
    fn test_audio_error_source() {
        // IoError应该有source
        let io_err = IoError::new(ErrorKind::PermissionDenied, "权限不足");
        let audio_err = AudioError::IoError(io_err);
        assert!(
            audio_err.source().is_some(),
            "IoError应该有source / IoError should have source"
        );

        // 其他错误类型没有source
        let errors_without_source = vec![
            AudioError::InvalidInput("test".to_string()),
            AudioError::FormatError("test".to_string()),
            AudioError::DecodingError("test".to_string()),
            AudioError::CalculationError("test".to_string()),
            AudioError::OutOfMemory,
            AudioError::ResourceError("test".to_string()),
        ];

        for err in errors_without_source {
            assert!(
                err.source().is_none(),
                "错误 {err:?} 不应该有source / Error {err:?} should not have source"
            );
        }
    }

    #[test]
    fn test_from_io_error() {
        let io_err = IoError::new(ErrorKind::NotFound, "测试文件");
        let audio_err: AudioError = io_err.into();

        match audio_err {
            AudioError::IoError(_) => {
                assert!(format!("{audio_err}").contains("文件I/O错误"));
            }
            _ => panic!("From<IoError>转换失败 / From<IoError> conversion failed"),
        }
    }

    #[test]
    fn test_from_hound_error() {
        // 创建一个hound错误并转换
        let hound_err = hound::Error::FormatError("测试WAV错误");
        let audio_err: AudioError = hound_err.into();

        match audio_err {
            AudioError::DecodingError(msg) => {
                assert!(msg.contains("WAV解码错误"));
                assert!(msg.contains("测试WAV错误"));
            }
            _ => panic!("From<hound::Error>转换失败 / From<hound::Error> conversion failed"),
        }
    }

    #[test]
    fn test_helper_format_error() {
        let err = format_error("解析头部", "无效magic number");

        match err {
            AudioError::FormatError(msg) => {
                assert!(msg.contains("解析头部"));
                assert!(msg.contains("无效magic number"));
            }
            _ => panic!("format_error返回了错误的类型 / format_error returned wrong type"),
        }
    }

    #[test]
    fn test_helper_decoding_error() {
        let err = decoding_error("FLAC解码", "帧头损坏");

        match err {
            AudioError::DecodingError(msg) => {
                assert!(msg.contains("FLAC解码"));
                assert!(msg.contains("帧头损坏"));
            }
            _ => panic!("decoding_error返回了错误的类型 / decoding_error returned wrong type"),
        }
    }

    #[test]
    fn test_helper_calculation_error() {
        let err = calculation_error("RMS计算", "样本数为0");

        match err {
            AudioError::CalculationError(msg) => {
                assert!(msg.contains("RMS计算"));
                assert!(msg.contains("样本数为0"));
            }
            _ => {
                panic!("calculation_error返回了错误的类型 / calculation_error returned wrong type")
            }
        }
    }

    #[test]
    fn test_error_category_from_audio_error() {
        let test_cases = vec![
            (
                AudioError::FormatError("test".into()),
                ErrorCategory::Format,
            ),
            (
                AudioError::DecodingError("test".into()),
                ErrorCategory::Decoding,
            ),
            (
                AudioError::IoError(IoError::new(ErrorKind::NotFound, "test")),
                ErrorCategory::Io,
            ),
            (
                AudioError::CalculationError("test".into()),
                ErrorCategory::Calculation,
            ),
            (AudioError::OutOfMemory, ErrorCategory::Calculation),
            (
                AudioError::InvalidInput("test".into()),
                ErrorCategory::Other,
            ),
            (
                AudioError::ResourceError("test".into()),
                ErrorCategory::Other,
            ),
        ];

        for (error, expected_category) in test_cases {
            let category = ErrorCategory::from_audio_error(&error);
            assert_eq!(
                category, expected_category,
                "错误 {error:?} 的分类不正确 / Error {error:?} categorization is incorrect"
            );
        }
    }

    #[test]
    fn test_error_category_display_name() {
        let categories = vec![
            (ErrorCategory::Format, "FORMAT/格式错误"),
            (ErrorCategory::Decoding, "DECODING/解码错误"),
            (ErrorCategory::Io, "I/O错误"),
            (ErrorCategory::Calculation, "CALCULATION/计算错误"),
            (ErrorCategory::Other, "OTHER/其他错误"),
        ];

        for (category, expected_name) in categories {
            assert_eq!(category.display_name(), expected_name);
        }
    }

    #[test]
    fn test_error_category_traits() {
        // 测试Clone
        let cat1 = ErrorCategory::Format;
        let cat2 = cat1;
        assert_eq!(cat1, cat2);

        // 测试Hash (通过HashMap)
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert(ErrorCategory::Format, 1);
        map.insert(ErrorCategory::Decoding, 2);
        assert_eq!(map.get(&ErrorCategory::Format), Some(&1));
    }

    #[test]
    fn test_audio_result_usage() {
        // 测试AudioResult类型别名的使用
        fn ok_result() -> AudioResult<i32> {
            Ok(42)
        }

        fn err_result() -> AudioResult<i32> {
            Err(AudioError::InvalidInput("测试错误".into()))
        }

        assert!(ok_result().is_ok());
        assert!(err_result().is_err());

        match err_result() {
            Err(AudioError::InvalidInput(msg)) => {
                assert_eq!(msg, "测试错误");
            }
            _ => panic!("AudioResult错误类型不匹配 / AudioResult error type mismatch"),
        }
    }

    #[test]
    fn test_error_chain() {
        // 测试错误链
        let io_err = IoError::new(ErrorKind::PermissionDenied, "无权限");
        let audio_err = AudioError::IoError(io_err);

        // 验证可以获取底层错误
        if let Some(source) = audio_err.source() {
            let io_source = source.downcast_ref::<IoError>().unwrap();
            assert_eq!(io_source.kind(), ErrorKind::PermissionDenied);
        } else {
            panic!("应该有source / Should have source");
        }
    }

    #[test]
    fn test_error_debug_format() {
        // 测试Debug格式化
        let err = AudioError::FormatError("测试".into());
        let debug_str = format!("{err:?}");
        assert!(debug_str.contains("FormatError"));
        assert!(debug_str.contains("测试"));
    }
}
