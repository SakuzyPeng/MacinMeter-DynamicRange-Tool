//! 错误处理模块
//!
//! 提供统一的symphonia错误处理宏
//! 注意：此模块仅供universal_decoder协调器内部使用

// use crate::error::AudioError; // 由宏内部使用，无需显式导入

/// 🔧 统一的symphonia错误处理宏
///
/// 消除重复的错误处理模式，提高代码可维护性。
/// 此宏仅供协调器内部使用。
macro_rules! handle_symphonia_error {
    ($result:expr, $decoder:expr) => {
        match $result {
            Ok(value) => Ok(value),
            Err(symphonia::core::errors::Error::ResetRequired) => {
                $decoder.reset();
                Err(AudioError::FormatError("解码器重置".to_string()))
            }
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                Ok(None) // 文件结束是正常情况
            }
            Err(symphonia::core::errors::Error::DecodeError(_)) => {
                Ok(None) // 解码错误，跳过这个包
            }
            Err(e) => Err(AudioError::FormatError(format!("symphonia错误: {e}"))),
        }
    };

    // 🔥 专用于packet处理的版本
    ($result:expr, $decoder:expr, continue_on_reset) => {
        match $result {
            Ok(value) => Some(value),
            Err(symphonia::core::errors::Error::ResetRequired) => {
                $decoder.reset();
                None // 信号继续循环
            }
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                return Ok(None); // 文件结束
            }
            Err(symphonia::core::errors::Error::DecodeError(_)) => {
                None // 跳过错误包，继续循环
            }
            Err(e) => return Err(AudioError::FormatError(format!("symphonia错误: {e}"))),
        }
    };
}

// 使宏在当前模块可见，但不对外暴露
pub(super) use handle_symphonia_error;
