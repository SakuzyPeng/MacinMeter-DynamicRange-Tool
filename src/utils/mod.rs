//! 工具模块
//!
//! 提供安全性保障和辅助功能。

pub mod safety;

// 重新导出公共接口
pub use safety::SafetyGuard;
