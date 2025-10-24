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

/// 性能优化工具函数
pub mod performance {
    use thread_priority::ThreadPriority;

    /// 设置当前线程为高优先级（Intel混合架构P-core优先）
    ///
    /// 在Intel 12代及以后的混合架构CPU上，高优先级线程更可能被调度到P-core（性能核心）
    /// 而非E-core（效能核心），从而提升计算密集型任务的性能。
    ///
    /// # 策略
    /// - Windows: 使用THREAD_PRIORITY_HIGHEST (优先级2)，避免过度抢占
    /// - Unix/macOS: 使用相对高优先级值
    ///
    /// # 返回
    /// - `Ok(())`: 成功设置优先级
    /// - `Err(msg)`: 设置失败（非致命，静默失败）
    ///
    /// # 注意
    /// - 失败不会影响程序运行，只是可能无法获得P-core优先权
    /// - macOS/Linux可能需要sudo权限才能设置高于normal的优先级
    /// - Windows通常可以成功设置THREAD_PRIORITY_HIGHEST
    pub fn set_high_priority() -> Result<(), String> {
        thread_priority::set_current_thread_priority(ThreadPriority::Max)
            .map_err(|e| format!("设置线程优先级失败: {e}"))
    }

    /// 为Rayon线程池配置高优先级spawn handler
    ///
    /// 创建一个自定义的线程spawn handler，使所有Rayon工作线程
    /// 自动设置为高优先级，从而在混合架构CPU上优先运行在P-core。
    ///
    /// # 使用示例
    /// ```rust,no_run
    /// use macinmeter_dr_tool::tools::utils::performance;
    ///
    /// // 在Rayon初始化前调用
    /// if let Err(e) = performance::setup_rayon_high_priority() {
    ///     eprintln!("⚠️ 无法设置Rayon高优先级: {}", e);
    /// }
    /// ```
    ///
    /// # 返回
    /// - `Ok(())`: 成功配置Rayon线程池
    /// - `Err(msg)`: 配置失败（通常是因为Rayon已初始化）
    ///
    /// # 注意
    /// - 必须在首次使用Rayon之前调用（否则会失败）
    /// - 建议在main()函数开头调用
    pub fn setup_rayon_high_priority() -> Result<(), String> {
        rayon::ThreadPoolBuilder::new()
            .spawn_handler(|thread| {
                let mut builder = std::thread::Builder::new();
                if let Some(name) = thread.name() {
                    builder = builder.name(name.to_owned());
                }
                if let Some(stack_size) = thread.stack_size() {
                    builder = builder.stack_size(stack_size);
                }

                builder.spawn(move || {
                    // 尝试设置高优先级，失败静默（不影响功能）
                    let _ = set_high_priority();
                    thread.run()
                })?;

                Ok(())
            })
            .build_global()
            .map_err(|e| format!("Rayon线程池初始化失败: {e}"))
    }

    /// 智能性能优化初始化（推荐使用）
    ///
    /// 根据平台特性自动应用最佳性能优化策略：
    /// 1. 为主线程设置高优先级
    /// 2. 配置Rayon线程池为高优先级
    ///
    /// # 返回
    /// - `Ok(())`: 至少一项优化成功
    /// - `Err(msg)`: 所有优化均失败（极少发生）
    ///
    /// # 使用建议
    /// 在main()函数开头调用：
    /// ```rust,no_run
    /// use macinmeter_dr_tool::tools::utils::performance;
    ///
    /// if let Err(e) = performance::optimize_for_performance() {
    ///     eprintln!("⚠️ 性能优化失败: {}", e);
    ///     // 继续运行，性能可能受影响但功能正常
    /// }
    /// // ... 其余程序逻辑
    /// ```
    pub fn optimize_for_performance() -> Result<(), String> {
        let mut errors = Vec::new();

        // 1. 优化主线程优先级
        if let Err(e) = set_high_priority() {
            errors.push(format!("主线程: {e}"));
        }

        // 2. 优化Rayon线程池
        if let Err(e) = setup_rayon_high_priority() {
            errors.push(format!("Rayon: {e}"));
        }

        // 如果所有优化都失败才报错
        if errors.len() >= 2 {
            Err(format!("所有性能优化均失败: {}", errors.join(", ")))
        } else {
            Ok(())
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
pub use performance::{optimize_for_performance, set_high_priority, setup_rayon_high_priority};
