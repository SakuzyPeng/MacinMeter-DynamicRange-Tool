//! 流式处理接口模块
//!
//! 定义流式解码器trait和相关接口
//! 注意：此模块仅供universal_decoder协调器内部使用

use super::format::AudioFormat;
use super::stats::ChunkSizeStats;
use crate::error::AudioResult;

/// 流式解码器trait
///
/// 定义统一的音频解码流式接口，支持逐块读取、进度追踪和格式查询。
/// 此trait通过协调器对外提供服务，内部实现由协调器管理。
///
/// # 数据格式约定
///
/// - **交错样本**：所有返回的音频数据均为交错格式 (interleaved)
/// - **变长块**：块大小由编解码器/容器决定，不保证固定长度
/// - **使用建议**：调用方需自行累积样本到目标窗口大小（如3秒）再做分析
///
/// # 线程安全性
///
/// - **单线程消费**：trait 不要求 `Send`/`Sync`，按约定在单线程中顺序消费
/// - **内部并行**：实现内部可使用多线程（如并行解码），但对外接口保持单线程
/// - **不共享实例**：不建议在多线程间共享 `StreamingDecoder` 实例
///
/// # 使用示例
///
/// ```ignore
/// let mut decoder = /* 创建解码器 */;
///
/// while let Some(samples) = decoder.next_chunk()? {
///     // samples 为交错 f32 数组：[L0, R0, L1, R1, ...]
///     // 长度 = 帧数 × 声道数
///     process_audio_block(&samples);
///
///     // 查看进度
///     println!("进度: {:.1}%", decoder.progress() * 100.0);
/// }
/// ```
pub trait StreamingDecoder {
    /// 获取下一个音频块
    ///
    /// # 返回值
    ///
    /// - `Ok(Some(samples))` - 成功读取一块音频数据
    ///   - `samples` 为交错 f32 样本：`[L0, R0, L1, R1, ...]`（立体声）或 `[M0, M1, M2, ...]`（单声道）
    ///   - 长度 = 帧数 × 声道数，**不保证固定长度**（取决于编解码器）
    /// - `Ok(None)` - 已到达文件末尾（EOF），无更多数据
    /// - `Err(_)` - 解码失败，包含错误详情
    ///
    /// # 注意事项
    ///
    /// - **变长块**：每次调用返回的样本数可能不同，需要自行累积到目标窗口
    /// - **容错处理**：某些损坏包会被跳过，通过 `format().is_partial()` 检测
    /// - **EOF 语义**：返回 `None` 后再次调用应继续返回 `None`
    fn next_chunk(&mut self) -> AudioResult<Option<Vec<f32>>>;

    /// 获取解码进度 (0.0-1.0)
    ///
    /// # 语义说明
    ///
    /// - **计算方式**：`已处理样本数 / 总样本数`
    /// - **未知总数**：某些格式（如流媒体）总样本数未知时返回 `0.0`
    /// - **动态更新**：总样本数会在解码过程中动态修正，进度可能非单调递增
    ///
    /// # 返回值
    ///
    /// - `0.0` - 刚开始或总样本数未知
    /// - `0.0..1.0` - 解码进行中
    /// - `1.0` - 解码完成（接近 EOF）
    ///
    /// # 示例
    ///
    /// ```ignore
    /// if decoder.progress() > 0.5 {
    ///     println!("已完成一半以上");
    /// }
    /// ```
    fn progress(&self) -> f32;

    /// 获取音频格式信息（动态构造，包含实时更新的元数据）
    ///
    /// # 动态特性
    ///
    /// - **样本数更新**：`sample_count` 字段会在解码过程中动态更新为实际值
    /// - **Partial 标记**：如果跳过了损坏包，`is_partial()` 返回 `true`
    /// - **不可变引用**：虽然 `&self` 调用，但内部会动态构造最新状态
    ///
    /// # 返回字段
    ///
    /// - `sample_rate` - 采样率（Hz），如 44100、48000
    /// - `channels` - 声道数（1 或 2）
    /// - `bits_per_sample` - 位深度（如 16、24）
    /// - `sample_count` - **动态值**，实际处理的样本数
    ///
    /// # 示例
    ///
    /// ```ignore
    /// let format = decoder.format();
    /// println!("{}Hz, {} 声道, {} 位", format.sample_rate, format.channels, format.bits_per_sample);
    ///
    /// if format.is_partial() {
    ///     println!("警告：文件部分损坏，跳过了 {} 个包", format.skipped_packets());
    /// }
    /// ```
    fn format(&self) -> AudioFormat;

    /// 重置到开头
    ///
    /// 将解码器状态重置到初始位置，可重新开始读取。
    ///
    /// # 副作用
    ///
    /// - 清空内部缓冲区
    /// - 重置进度为 0.0
    /// - 清空统计信息
    ///
    /// # 错误
    ///
    /// - 如果底层 I/O 不支持 seek 操作，返回错误
    fn reset(&mut self) -> AudioResult<()>;

    /// 获取块大小统计信息（可选，仅逐包模式支持）
    ///
    /// # 副作用说明
    ///
    /// - **Finalize 操作**：调用会触发内部统计的冻结/快照操作
    /// - **需要可变借用**：因此签名为 `&mut self` 而非 `&self`
    /// - **单次调用**：多次调用可能返回相同快照，具体行为由实现决定
    ///
    /// # 返回值
    ///
    /// - `Some(stats)` - 批量处理模式，包含包大小统计（最小、最大、平均、总数）
    /// - `None` - 流式处理模式或不支持统计
    ///
    /// # 使用场景
    ///
    /// - 性能分析：了解编解码器的包分布特征
    /// - 调试诊断：检查是否存在异常大小的包
    ///
    /// # 示例
    ///
    /// ```ignore
    /// if let Some(stats) = decoder.get_chunk_stats() {
    ///     println!("包统计: 最小{}字节, 最大{}字节, 平均{}字节, 总共{}个包",
    ///         stats.min_size, stats.max_size, stats.avg_size, stats.total_chunks);
    /// }
    /// ```
    fn get_chunk_stats(&mut self) -> Option<ChunkSizeStats> {
        None // 默认不支持
    }
}
