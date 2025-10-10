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

/// 解码器性能优化常量
pub mod decoder_performance {
    /// BatchPacketReader批量预读包数
    ///
    /// 通过批量预读减少系统调用次数，64包是经过性能测试的最优值：
    /// - 减少99%的I/O系统调用
    /// - 良好的缓存局部性（完全fit进L2缓存）
    /// - 与并行解码器batch_size保持一致
    pub const BATCH_PACKET_SIZE: usize = 64;

    /// BatchPacketReader预读触发阈值
    ///
    /// 当缓冲区包数量低于此阈值时触发批量预读，
    /// 20个包的阈值确保缓冲区始终有足够数据，避免解码器空等
    pub const PREFETCH_THRESHOLD: usize = 20;

    /// 并行解码器批量处理大小
    ///
    /// 用于OrderedParallelDecoder的批量解码配置，
    /// 64包批量提供最佳的性能/内存平衡
    pub const PARALLEL_DECODE_BATCH_SIZE: usize = 64;

    /// 并行解码器线程数
    ///
    /// 用于OrderedParallelDecoder的工作线程数，
    /// 4线程在多数CPU上提供最佳性能
    pub const PARALLEL_DECODE_THREADS: usize = 4;
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

/// 缓冲区内存优化常量（阶段D）
pub mod buffers {
    /// 样本缓冲区容量预分配倍数
    ///
    /// 为 sample_buffer 预分配 window_size_samples * BUFFER_CAPACITY_MULTIPLIER 容量，
    /// 减少扩容次数和内存抖动。值为 3 可在多数场景下避免扩容：
    /// - 窗口重叠处理时有足够预留空间
    /// - 避免频繁的 Vec reallocation
    /// - 对内存峰值影响可控（≈ +2-3 MB per worker）
    pub const BUFFER_CAPACITY_MULTIPLIER: usize = 3;

    /// 样本缓冲区硬上限比例
    ///
    /// 当 sample_buffer.len() 超过 window_size_samples * MAX_BUFFER_RATIO 时，
    /// 触发强制 compact 操作，防止缓冲区无限增长。值为 3.5：
    /// - 提供足够的弹性空间（50% 预留）
    /// - 及时回收过度累积的内存
    /// - 与预分配倍数配合形成"预分配→正常使用→硬上限压缩"的完整循环
    pub const MAX_BUFFER_RATIO: f64 = 3.5;

    /// 窗口对齐优化开关（内部策略，面向开发者）
    ///
    /// **默认行为（Release）**：固定返回 true，启用阶段D优化（预分配+硬上限）
    ///
    /// **调试模式（Debug/Test）**：读取环境变量 `DR_DISABLE_WINDOW_ALIGN`
    /// - `DR_DISABLE_WINDOW_ALIGN=1` 或 `=true` → 禁用优化（用于A/B对比测试）
    /// - 未设置或其他值 → 启用优化（默认）
    ///
    /// **设计原则**：
    /// - 不污染用户CLI，保持接口简洁
    /// - Release行为确定性（固定启用），避免环境变量引入非确定性
    /// - Debug模式提供灵活性，便于性能对比和回归测试
    ///
    /// **使用示例**：
    /// ```bash
    /// # Debug模式：禁用优化进行对比测试（注意：不加--release）
    /// DR_DISABLE_WINDOW_ALIGN=1 cargo run -- /path/to/audio
    ///
    /// # Release二进制：环境变量被忽略，始终启用优化
    /// DR_DISABLE_WINDOW_ALIGN=1 ./target/release/MacinMeter-... /path  # 无效
    /// ```
    #[inline]
    pub fn window_alignment_enabled() -> bool {
        #[cfg(any(test, debug_assertions))]
        {
            // Debug/Test模式：支持环境变量控制
            std::env::var("DR_DISABLE_WINDOW_ALIGN")
                .map(|v| v != "1" && v != "true")
                .unwrap_or(true) // 默认启用
        }
        #[cfg(not(any(test, debug_assertions)))]
        {
            // Release模式：固定启用
            true
        }
    }
}
