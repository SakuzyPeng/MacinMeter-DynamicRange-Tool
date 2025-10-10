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
        // 复用 extract_file_stem，避免重复实现与潜在漂移
        super::path::extract_file_stem(path).to_string()
    }

    /// 清理文件名中的不合法字符（跨平台兼容）
    ///
    /// 将不合法的文件名字符替换为下划线，确保跨平台兼容性：
    /// - Windows 禁止字符: `< > : " / \ | ? *` 以及控制字符
    /// - Unix 禁止字符: `/` 和 null 字符
    /// - 特殊处理：空格和点号也替换为下划线以提高可读性
    ///
    /// # 参数
    /// - `filename`: 需要清理的文件名
    ///
    /// # 返回
    /// 清理后的安全文件名
    ///
    /// # 示例
    /// ```
    /// use macinmeter_dr_tool::tools::utils::path::sanitize_filename;
    ///
    /// assert_eq!(sanitize_filename("test file.txt"), "test_file_txt");
    /// assert_eq!(sanitize_filename("test<>file"), "test__file");
    /// assert_eq!(sanitize_filename("test/file\\name"), "test_file_name");
    /// ```
    pub fn sanitize_filename(filename: &str) -> String {
        filename
            .chars()
            .map(|c| match c {
                // Windows 禁止字符
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
                // 空格和点号（提高可读性）
                ' ' | '.' => '_',
                // 控制字符和 null
                c if c.is_control() || c == '\0' => '_',
                // 其他字符保留
                c => c,
            })
            .collect()
    }
}

/// 并行处理工具函数
pub mod parallel {
    use super::super::constants::parallel_limits::{MAX_PARALLEL_DEGREE, MIN_PARALLEL_DEGREE};

    /// 计算有效并发度（统一并发度计算逻辑）
    ///
    /// 将用户配置的并发度应用以下限制规则：
    /// 1. 最小值为 1（至少需要一个工作单元）
    /// 2. 最大值为 16（避免过度并发）
    /// 3. 如果提供了工作项数量，不超过实际工作项数量（避免无意义的线程浪费）
    ///
    /// # 参数
    /// - `requested_degree`: 用户请求的并发度
    /// - `max_items`: 可选的工作项数量限制（如文件数量）
    ///
    /// # 返回
    /// 应用限制规则后的有效并发度
    ///
    /// # 示例
    /// ```
    /// use macinmeter_dr_tool::tools::utils::parallel::effective_parallel_degree;
    ///
    /// // 基本限制（1-16范围）
    /// assert_eq!(effective_parallel_degree(0, None), 1);     // 最小值限制
    /// assert_eq!(effective_parallel_degree(8, None), 8);     // 正常值
    /// assert_eq!(effective_parallel_degree(32, None), 16);   // 最大值限制
    ///
    /// // 工作项数量限制
    /// assert_eq!(effective_parallel_degree(8, Some(3)), 3);  // 不超过文件数
    /// assert_eq!(effective_parallel_degree(8, Some(10)), 8); // 保持原值
    /// ```
    #[inline]
    pub fn effective_parallel_degree(requested_degree: usize, max_items: Option<usize>) -> usize {
        // 1️⃣ 应用基本限制（1-16范围）
        let clamped = requested_degree.clamp(MIN_PARALLEL_DEGREE, MAX_PARALLEL_DEGREE);

        // 2️⃣ 如果提供了工作项数量，不超过实际数量
        match max_items {
            Some(count) if count > 0 => clamped.min(count),
            _ => clamped,
        }
    }
}

// 重新导出为平级函数，保持向后兼容
pub use audio::{linear_to_db, linear_to_db_string};
pub use parallel::effective_parallel_degree;
pub use path::{
    extract_extension_uppercase, extract_file_stem, extract_file_stem_string, extract_filename,
    extract_filename_lossy, get_parent_dir, sanitize_filename,
};
