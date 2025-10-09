//! 常量和默认配置集中管理
//!
//! 将所有重要常量集中定义，避免"默认值漂移"和重复定义

/// DR分析算法常量
pub mod dr_analysis {
    /// 窗口时长（秒）- foobar2000标准
    ///
    /// 固定3秒窗口与foobar2000 DR Meter保持一致，
    /// 确保分析结果的可比性和算法精度
    pub const WINDOW_DURATION_SECONDS: f64 = 3.0;
}

/// 默认配置值
pub mod defaults {
    /// 默认并行批大小
    ///
    /// 用于并行解码时的批量处理大小，
    /// 64包是经过性能测试的最优值
    pub const PARALLEL_BATCH_SIZE: usize = 64;

    /// 默认并行线程数
    ///
    /// 用于多线程并行处理的默认线程数，
    /// 4线程在多数场景下提供良好的性能/资源平衡
    pub const PARALLEL_THREADS: usize = 4;

    /// 默认多文件并行并发度
    ///
    /// 用于批量处理多个文件时的并行度，
    /// 4并发度在多数场景下提供良好的性能/资源平衡
    pub const PARALLEL_FILES_DEGREE: usize = 4;
}

/// 并发度限制常量
pub mod parallel_limits {
    /// 最小并发度
    ///
    /// 任何并行处理至少需要1个线程/工作单元
    pub const MIN_PARALLEL_DEGREE: usize = 1;

    /// 最大并发度
    ///
    /// 限制最大并发度为16，避免过度并发导致的：
    /// - 上下文切换开销
    /// - 内存占用过高
    /// - 系统资源竞争
    pub const MAX_PARALLEL_DEGREE: usize = 16;
}
