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

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock 流式解码器用于契约级单测
    struct MockStreamingDecoder {
        /// 待返回的块序列（Some 表示数据块，None 表示 EOF）
        chunks: Vec<Option<Vec<f32>>>,
        /// 当前读取位置
        position: usize,
        /// 格式信息
        fmt: AudioFormat,
        /// 已处理的总样本数（用于进度计算）
        processed_samples: u64,
        /// 音频文件总样本数
        total_samples: u64,
        /// 错误配置（在第 N 次 next_chunk 调用时注入错误）
        error_at_call: Option<usize>,
        /// 当前调用次数（用于错误注入）
        call_count: usize,
    }

    impl MockStreamingDecoder {
        /// 创建一个简单的 Mock 解码器
        fn new(chunks: Vec<Option<Vec<f32>>>, fmt: AudioFormat, total_samples: u64) -> Self {
            Self {
                chunks,
                position: 0,
                fmt,
                processed_samples: 0,
                total_samples,
                error_at_call: None,
                call_count: 0,
            }
        }

        /// 配置在第 N 次调用时注入错误
        fn with_error_at(mut self, call_number: usize) -> Self {
            self.error_at_call = Some(call_number);
            self
        }

        /// 动态更新 total_samples（模拟流式处理中的总样本数修正）
        fn set_total_samples(&mut self, total_samples: u64) {
            self.total_samples = total_samples;
        }

        /// 返回当前已处理的样本数
        fn processed_samples(&self) -> u64 {
            self.processed_samples
        }
    }

    impl StreamingDecoder for MockStreamingDecoder {
        fn next_chunk(&mut self) -> AudioResult<Option<Vec<f32>>> {
            self.call_count += 1;

            // 注入错误路径
            if let Some(error_at) = self.error_at_call {
                if self.call_count == error_at {
                    return Err(crate::error::AudioError::DecodingError(
                        "Injected error for testing".to_string(),
                    ));
                }
            }

            // 标准 EOF 语义：EOF 后继续返回 None
            if self.position >= self.chunks.len() {
                return Ok(None);
            }

            let chunk = self.chunks[self.position].clone();
            self.position += 1;

            if let Some(ref samples) = chunk {
                self.processed_samples += (samples.len() / self.fmt.channels as usize) as u64;
            }

            Ok(chunk)
        }

        fn progress(&self) -> f32 {
            if self.total_samples == 0 {
                0.0
            } else {
                (self.processed_samples as f32 / self.total_samples as f32).min(1.0)
            }
        }

        fn format(&self) -> AudioFormat {
            self.fmt.clone()
        }

        fn reset(&mut self) -> AudioResult<()> {
            self.position = 0;
            self.processed_samples = 0;
            self.call_count = 0;
            Ok(())
        }
    }

    #[test]
    fn test_eof_semantics_after_none() {
        // 验证 EOF 后继续调用应返回 None
        let chunks = vec![
            Some(vec![0.1, 0.2, 0.3, 0.4]),
            Some(vec![0.5, 0.6, 0.7, 0.8]),
            None, // EOF
        ];
        let fmt = AudioFormat::new(44100, 2, 16, 4);
        let mut decoder = MockStreamingDecoder::new(chunks, fmt, 4);

        // 读取第一块
        assert!(decoder.next_chunk().unwrap().is_some());
        // 读取第二块
        assert!(decoder.next_chunk().unwrap().is_some());
        // 读取 EOF
        assert!(decoder.next_chunk().unwrap().is_none());
        // 继续调用应仍返回 None（重要的 EOF 语义）
        assert!(decoder.next_chunk().unwrap().is_none());
        assert!(decoder.next_chunk().unwrap().is_none());
    }

    #[test]
    fn test_progress_boundary_0_to_1() {
        // 验证 progress 从 0.0 → 1.0 的边界行为
        let chunks = vec![
            Some(vec![0.1, 0.2, 0.3, 0.4]), // 2 帧 × 2 声道
            Some(vec![0.5, 0.6, 0.7, 0.8]), // 2 帧 × 2 声道
        ];
        let fmt = AudioFormat::new(44100, 2, 16, 4);
        let mut decoder = MockStreamingDecoder::new(chunks, fmt, 4); // 总 4 帧

        // 初始状态：进度 = 0.0
        assert_eq!(decoder.progress(), 0.0);

        // 读取第一块后：进度 = 2/4 = 0.5
        decoder.next_chunk().unwrap();
        assert_eq!(decoder.progress(), 0.5);

        // 读取第二块后：进度 = 4/4 = 1.0
        decoder.next_chunk().unwrap();
        assert_eq!(decoder.progress(), 1.0);

        // 读取 EOF 后：进度应保持 1.0（不超过）
        decoder.next_chunk().unwrap();
        assert_eq!(decoder.progress(), 1.0);
    }

    #[test]
    fn test_progress_unknown_total_samples() {
        // 验证总样本数未知时的进度行为
        let chunks = vec![Some(vec![0.1, 0.2, 0.3, 0.4]), None];
        let fmt = AudioFormat::new(44100, 2, 16, 2);
        let mut decoder = MockStreamingDecoder::new(chunks, fmt, 0); // total_samples = 0

        // 当 total_samples = 0 时，progress 应返回 0.0
        assert_eq!(decoder.progress(), 0.0);
        decoder.next_chunk().unwrap();
        assert_eq!(decoder.progress(), 0.0);
    }

    #[test]
    fn test_format_passthrough() {
        // 验证 format() 正确透传格式信息
        let chunks = vec![Some(vec![0.1, 0.2])];
        let mut fmt = AudioFormat::new(48000, 1, 24, 1);
        fmt.mark_as_partial(5);
        let decoder = MockStreamingDecoder::new(chunks, fmt.clone(), 1);

        let returned_fmt = decoder.format();
        assert_eq!(returned_fmt.sample_rate, 48000);
        assert_eq!(returned_fmt.channels, 1);
        assert_eq!(returned_fmt.bits_per_sample, 24);
        assert_eq!(returned_fmt.sample_count, 1);
        assert!(returned_fmt.is_partial());
        assert_eq!(returned_fmt.skipped_packets(), 5);
    }

    #[test]
    fn test_error_propagation_at_specific_call() {
        // 验证错误注入与传播
        let chunks = vec![
            Some(vec![0.1, 0.2, 0.3, 0.4]),
            Some(vec![0.5, 0.6, 0.7, 0.8]),
            Some(vec![0.9, 1.0, 1.1, 1.2]),
        ];
        let fmt = AudioFormat::new(44100, 2, 16, 6);

        let mut decoder = MockStreamingDecoder::new(chunks, fmt, 6).with_error_at(2);

        // 第一次调用：成功
        assert!(decoder.next_chunk().unwrap().is_some());

        // 第二次调用：注入错误
        let result = decoder.next_chunk();
        assert!(result.is_err());

        // 验证错误消息
        if let Err(e) = result {
            assert!(e.to_string().contains("Injected error"));
        }
    }

    #[test]
    fn test_reset_semantics() {
        // 验证 reset() 重置所有状态
        let chunks = vec![
            Some(vec![0.1, 0.2, 0.3, 0.4]),
            Some(vec![0.5, 0.6, 0.7, 0.8]),
            None,
        ];
        let fmt = AudioFormat::new(44100, 2, 16, 4);
        let mut decoder = MockStreamingDecoder::new(chunks, fmt, 4);

        // 读取一些数据
        decoder.next_chunk().unwrap();
        decoder.next_chunk().unwrap();
        assert_eq!(decoder.progress(), 1.0);

        // 重置
        decoder.reset().unwrap();

        // 验证状态被重置
        assert_eq!(decoder.progress(), 0.0);
        // 可以重新读取
        assert!(decoder.next_chunk().unwrap().is_some());
        assert_eq!(decoder.progress(), 0.5);
    }

    #[test]
    fn test_variable_chunk_sizes() {
        // 验证变长块处理
        let chunks = vec![
            Some(vec![0.1, 0.2]),                     // 1 帧 × 2 声道 = 2 样本
            Some(vec![0.3, 0.4, 0.5, 0.6, 0.7, 0.8]), // 3 帧 × 2 声道 = 6 样本
            Some(vec![0.9, 1.0]),                     // 1 帧 × 2 声道 = 2 样本
            None,
        ];
        let fmt = AudioFormat::new(44100, 2, 16, 5);
        let mut decoder = MockStreamingDecoder::new(chunks, fmt, 5);

        assert_eq!(decoder.progress(), 0.0);

        let chunk1 = decoder.next_chunk().unwrap().unwrap();
        assert_eq!(chunk1.len(), 2); // 1 帧 × 2 声道
        assert!((decoder.progress() - 0.2).abs() < 0.001); // 1/5

        let chunk2 = decoder.next_chunk().unwrap().unwrap();
        assert_eq!(chunk2.len(), 6); // 3 帧 × 2 声道
        assert!((decoder.progress() - 0.8).abs() < 0.001); // 4/5

        let chunk3 = decoder.next_chunk().unwrap().unwrap();
        assert_eq!(chunk3.len(), 2); // 1 帧 × 2 声道
        assert_eq!(decoder.progress(), 1.0); // 5/5
    }

    #[test]
    fn test_multiple_resets() {
        // 验证多次 reset 的正确性
        let chunks = vec![Some(vec![0.1, 0.2, 0.3, 0.4]), None];
        let fmt = AudioFormat::new(44100, 2, 16, 2);
        let mut decoder = MockStreamingDecoder::new(chunks, fmt, 2);

        for _ in 0..3 {
            // 第一个循环：读取所有数据
            decoder.next_chunk().unwrap();
            assert_eq!(decoder.progress(), 1.0);
            decoder.next_chunk().unwrap(); // EOF

            // 重置并重新开始
            decoder.reset().unwrap();
            assert_eq!(decoder.progress(), 0.0);
        }
    }

    #[test]
    fn test_dynamic_format_update() {
        // 验证动态格式更新场景：总样本数初始未知，后续修正
        // 这对应 trait 文档中的注释："总样本数会在解码过程中动态修正"
        let chunks = vec![
            Some(vec![0.1, 0.2, 0.3, 0.4]), // 2 帧 × 2 声道
            Some(vec![0.5, 0.6, 0.7, 0.8]), // 2 帧 × 2 声道
            None,
        ];
        let fmt = AudioFormat::new(44100, 2, 16, 0); // 初始 sample_count = 0（未知）
        let mut decoder = MockStreamingDecoder::new(chunks, fmt, 0); // 初始 total_samples = 0

        // 初始状态：总样本数未知，progress 返回 0.0
        assert_eq!(decoder.progress(), 0.0);
        assert_eq!(decoder.processed_samples(), 0);

        // 读取第一块数据（2 帧 = 2 样本）
        decoder.next_chunk().unwrap();
        assert_eq!(decoder.processed_samples(), 2);
        // 由于 total_samples=0，progress 仍为 0.0
        assert_eq!(decoder.progress(), 0.0);

        // 动态更新总样本数（模拟解码器发现总长度）
        decoder.set_total_samples(4); // 修正为 4 帧 = 4 样本

        // 现在 progress 应该动态计算为 2/4 = 0.5
        assert_eq!(decoder.progress(), 0.5);

        // 读取第二块数据（2 帧 = 2 样本）
        decoder.next_chunk().unwrap();
        assert_eq!(decoder.processed_samples(), 4);
        // 现在 progress = 4/4 = 1.0
        assert_eq!(decoder.progress(), 1.0);

        // 读取 EOF
        decoder.next_chunk().unwrap();
        // progress 应保持 1.0（不超过）
        assert_eq!(decoder.progress(), 1.0);
    }
}
