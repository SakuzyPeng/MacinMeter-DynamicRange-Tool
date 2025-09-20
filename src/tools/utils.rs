//! 工具函数模块
//!
//! 提供音频值转换、文件路径处理等通用工具函数。

/// 音频值转换工具函数
pub mod audio {
    /// 将线性值转换为dB值
    #[inline]
    pub fn linear_to_db(value: f64) -> f64 {
        if value > 0.0 {
            20.0 * value.log10()
        } else {
            -f64::INFINITY
        }
    }

    /// 将线性值转换为格式化的dB字符串（用于表格输出）
    #[inline]
    pub fn linear_to_db_string(value: f64) -> String {
        if value > 0.0 {
            format!("{:.2}", 20.0 * value.log10())
        } else {
            "-1.#J".to_string()
        }
    }
}

/// 文件路径处理工具函数
pub mod path {
    use std::path::Path;

    /// 提取文件名（统一处理路径提取逻辑）
    #[inline]
    pub fn extract_filename(path: &Path) -> &str {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
    }

    /// 提取文件stem（不含扩展名）
    #[inline]
    pub fn extract_file_stem(path: &Path) -> &str {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("audio")
    }

    /// 提取文件名（返回String，用于日志显示）
    #[inline]
    pub fn extract_filename_lossy(path: &Path) -> String {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    }

    /// 获取父目录，如果不存在则返回当前目录
    #[inline]
    pub fn get_parent_dir(path: &Path) -> &Path {
        path.parent().unwrap_or_else(|| Path::new("."))
    }

    /// 提取文件扩展名（用于编解码器识别）
    #[inline]
    pub fn extract_extension_uppercase(path: &Path) -> String {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.to_uppercase())
            .unwrap_or_else(|| "Unknown".to_string())
    }

    /// 安全提取文件stem（返回String）
    #[inline]
    pub fn extract_file_stem_string(path: &Path) -> String {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("audio")
            .to_string()
    }
}

// 重新导出为平级函数，保持向后兼容
pub use audio::{linear_to_db, linear_to_db_string};
pub use path::{
    extract_extension_uppercase, extract_file_stem, extract_file_stem_string, extract_filename,
    extract_filename_lossy, get_parent_dir,
};
