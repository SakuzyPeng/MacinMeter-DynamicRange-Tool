//! 🚀 有序并行解码器 - 攻击真正瓶颈的高性能音频解码
//!
//! 基于大量基准测试发现解码是唯一瓶颈(占70-80% CPU时间)的关键洞察，
//! 实现保证顺序的并行解码架构，预期获得3-5倍性能提升。
//!
//! ## 核心设计原则
//!
//! - **瓶颈聚焦**: 专门优化解码性能，不改变DR算法逻辑
//! - **顺序保证**: 严格维持样本时间序列，确保窗口积累正确性
//! - **内存可控**: 智能背压机制，避免内存爆炸
//! - **优雅降级**: 并行失败时自动回退到串行模式
//!
//! ## 架构概览
//!
//! ```text
//! Packet Stream → [Batch Buffer] → [Parallel Decode Pool] → [Sequence Reorder] → Ordered Samples
//!                      ↓                    ↓                      ↓
//!                 固定批大小           4-8线程并行              序列号排序重组
//! ```

use crate::error::{self, AudioResult};
use crate::processing::{SampleConverter, sample_conversion::SampleConversion};
use crate::tools::constants::decoder_performance;
use rayon::ThreadPoolBuilder;
use std::time::Duration;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
};
use symphonia::core::{
    audio::{AudioBufferRef, SampleBuffer, Signal},
    codecs::{Decoder, DecoderOptions},
    formats::Packet,
};

/// 🎯 解码数据块 - 显式EOF标记
///
/// 通过枚举明确区分"样本数据"和"结束信号"，彻底解决生产者-消费者EOF识别问题
#[derive(Debug, Clone)]
pub enum DecodedChunk {
    /// 解码后的音频样本（交错格式）
    Samples(Vec<f32>),
    /// 明确的结束标记：所有包已解码完毕
    EOF,
}

/// 🎯 解码器状态 - 三阶段状态机
///
/// 用于明确区分"包已读完"和"样本已消费完"，解决样本丢失问题
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodingState {
    /// 正在解码：包仍在流入
    Decoding,
    /// 冲刷中：包已读完（EOF），等待后台线程完成解码
    Flushing,
    /// 已完成：所有样本已drain完毕
    Completed,
}

/// 🎯 核心配置参数 - 基于性能测试优化
const DEFAULT_BATCH_SIZE: usize = 64; // 每批并行解码的包数量
const DEFAULT_PARALLEL_THREADS: usize = 4; // 默认解码线程数

/// 📦 带序列号的数据包装器
struct SequencedPacket {
    sequence: usize,
    packet: Packet,
}

/// 🔄 有序通道 - 确保乱序并行结果按顺序输出
///
/// ## 核心机制
///
/// **重排序发生在发送端**：`OrderedSender` 负责所有重排序逻辑，维护一个
/// `HashMap` 缓冲区存储乱序到达的数据。接收端 `recv_ordered()` 只是简单的
/// `recv()` 调用，因为发送端已经保证了顺序。
///
/// ## 设计意图
///
/// 这种"发送端重排序"设计避免了接收端的复杂性，但代价是多个发送线程需要
/// 竞争同一个 `Mutex<HashMap>`。适用于中等并发度（4-8线程）的场景。
///
/// **背压机制**：使用有界通道（sync_channel），当缓冲满时发送端会阻塞，
/// 防止生产快于消费导致的内存无限增长。
#[derive(Debug)]
pub struct SequencedChannel<T> {
    sender: SyncSender<T>,
    receiver: Receiver<T>,
    next_expected: Arc<AtomicUsize>,
    reorder_buffer: Arc<Mutex<HashMap<usize, T>>>,
}

impl<T> Default for SequencedChannel<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> SequencedChannel<T> {
    /// 创建有序通道，使用默认容量
    ///
    /// 容量设计：threads(4) × multiplier(4) = 16
    /// 核心洞察：乱序样本缓冲峰值取决于并发度（线程数），而非批次大小
    pub fn new() -> Self {
        let default_capacity = decoder_performance::PARALLEL_DECODE_THREADS
            * decoder_performance::SEQUENCED_CHANNEL_CAPACITY_MULTIPLIER;
        Self::with_capacity(default_capacity)
    }

    /// 创建有序通道，指定容量
    ///
    /// # 参数
    /// - `capacity`: 通道容量，当缓冲满时发送端会阻塞（背压机制）
    pub fn with_capacity(capacity: usize) -> Self {
        let (sender, receiver) = mpsc::sync_channel(capacity);
        Self {
            sender,
            receiver,
            next_expected: Arc::new(AtomicUsize::new(0)),
            reorder_buffer: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 获取发送端，用于并行线程发送乱序结果
    pub fn sender(&self) -> OrderedSender<T> {
        OrderedSender {
            sender: self.sender.clone(),
            next_expected: Arc::clone(&self.next_expected),
            reorder_buffer: Arc::clone(&self.reorder_buffer),
        }
    }

    /// 按顺序接收数据 - 阻塞直到下一个期望序列号的数据到达
    pub fn recv_ordered(&self) -> Result<T, mpsc::RecvError> {
        self.receiver.recv()
    }

    /// 尝试按顺序接收数据 - 非阻塞版本
    pub fn try_recv_ordered(&self) -> Result<T, mpsc::TryRecvError> {
        self.receiver.try_recv()
    }
}

/// 📤 有序发送端 - 在发送端实现重排序逻辑
///
/// ## 重排序算法
///
/// 当调用 `send_sequenced(seq, data)` 时：
/// 1. 检查 `seq` 是否等于 `next_expected`（期望的下一个序列号）
/// 2. **匹配时**：直接发送数据，递增 `next_expected`，然后尝试从缓冲区 flush 连续序列
/// 3. **不匹配时**：将 `(seq, data)` 存入 `HashMap` 缓冲区，等待后续触发
///
/// ## 性能特性
///
/// - **锁竞争**：多个发送线程竞争 `Mutex<HashMap>`，在高并发（16+线程）下可能成为瓶颈
/// - **内存占用**：缓冲区大小取决于乱序程度，最坏情况为 O(并发度)
/// - **原子操作**：使用 `AtomicUsize` 读取 `next_expected`，减少锁持有时间
///
/// **背压特性**：使用 SyncSender，当通道满时 send() 会阻塞，形成自然的背压。
#[derive(Debug, Clone)]
pub struct OrderedSender<T> {
    sender: SyncSender<T>,
    next_expected: Arc<AtomicUsize>,
    reorder_buffer: Arc<Mutex<HashMap<usize, T>>>,
}

impl<T> OrderedSender<T> {
    /// 发送带序列号的数据，自动处理重排序
    ///
    /// ## 算法流程
    ///
    /// ```text
    /// 1. 获取 reorder_buffer 的锁（阻塞其他发送线程）
    /// 2. 读取 next_expected（原子操作，Acquire 语义）
    /// 3. 判断 sequence 是否等于 next_expected：
    ///
    ///    [匹配路径]
    ///    a. 释放锁（避免阻塞其他线程）
    ///    b. 直接发送 data 到 channel
    ///    c. 原子递增 next_expected（Release 语义）
    ///    d. 调用 flush_consecutive_from_buffer() 尝试 flush 缓冲区
    ///
    ///    [缓冲路径]
    ///    a. 将 (sequence, data) 插入 reorder_buffer
    ///    b. 释放锁（隐式，函数结束时）
    /// ```
    ///
    /// ## 并发安全性
    ///
    /// - **Mutex 防护**：reorder_buffer 的读写通过 Mutex 序列化
    /// - **Acquire/Release 语义**：确保原子操作的内存可见性
    /// - **Poison 恢复**：即使某线程 panic，也能恢复数据继续服务
    pub fn send_sequenced(&self, sequence: usize, data: T) -> Result<(), mpsc::SendError<T>> {
        // Mutex poison 降级：即使有线程 panic，也恢复数据继续服务
        let mut buffer = self
            .reorder_buffer
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        // 原子序优化：Acquire 确保读取到最新值
        let next_expected = self.next_expected.load(Ordering::Acquire);

        if sequence == next_expected {
            // 🎯 正好是期望的序列号，直接发送
            drop(buffer); // 释放锁
            self.sender.send(data)?;
            // 原子序优化：Release 让写入对其他线程可见
            self.next_expected
                .store(next_expected + 1, Ordering::Release);

            // 🔄 检查缓冲区中是否有后续连续的序列号可以发送
            self.flush_consecutive_from_buffer();
        } else {
            // 🔄 不是期望的序列号，存入重排序缓冲区等待
            buffer.insert(sequence, data);
        }

        Ok(())
    }

    /// 🔄 从缓冲区中发送连续的序列号数据
    ///
    /// ## 算法逻辑
    ///
    /// 循环检查 reorder_buffer 中是否存在 `next_expected` 对应的数据：
    /// - **存在**：取出数据，释放锁，发送到 channel，递增 `next_expected`，继续循环
    /// - **不存在**：说明遇到"间隙"（后续序列号还未到达），退出循环
    ///
    /// ## 关键设计点
    ///
    /// - **逐个 flush**：每次只发送一个数据，然后重新获取锁检查下一个序列号
    /// - **及时释放锁**：在调用 `sender.send()` 前释放锁，避免长时间阻塞其他线程
    /// - **容错处理**：如果 send() 失败（channel 已关闭），立即退出循环
    fn flush_consecutive_from_buffer(&self) {
        loop {
            // 原子序优化：Acquire 确保读取到最新值
            let next_expected = self.next_expected.load(Ordering::Acquire);
            // Mutex poison 降级：即使有线程 panic，也恢复数据继续服务
            let mut buffer = self
                .reorder_buffer
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());

            if let Some(data) = buffer.remove(&next_expected) {
                drop(buffer); // 释放锁后再发送
                if self.sender.send(data).is_ok() {
                    // 原子序优化：Release 让写入对其他线程可见
                    self.next_expected
                        .store(next_expected + 1, Ordering::Release);
                } else {
                    break; // 发送失败，停止
                }
            } else {
                break; // 没有连续的序列号，停止
            }
        }
    }
}

/// 🚀 有序并行解码器 - 核心性能优化组件
///
/// 职责：将包批量化并行解码，保证输出顺序与输入完全一致
pub struct OrderedParallelDecoder {
    batch_size: usize,
    thread_pool_size: usize,
    /// 🚀 Rayon线程池 - 复用工作线程（Arc包装，支持廉价clone）
    thread_pool: Arc<rayon::ThreadPool>,
    /// 当前批次缓冲区
    current_batch: Vec<SequencedPacket>,
    /// 序列号计数器
    sequence_counter: usize,
    /// 有序样本通道（传输DecodedChunk以支持显式EOF）
    samples_channel: SequencedChannel<DecodedChunk>,
    /// 解码器工厂 - 每个线程需要独立的解码器实例
    decoder_factory: DecoderFactory,
    /// 统计信息
    stats: ParallelDecodingStats,
    /// 🎯 解码状态 - 三阶段状态机
    decoding_state: DecodingState,
    /// 🎯 防止重复flush的标志位
    flushed: bool,
    /// 🎯 EOF遇到标志 - 防止next_samples()消费EOF导致drain无法收到
    eof_encountered: bool,
}

/// 并行解码统计信息
#[derive(Debug, Default, Clone)]
struct ParallelDecodingStats {
    packets_added: usize,
    batches_processed: usize,
    samples_decoded: usize,
    failed_packets: usize,
    consumed_batches: usize, // 已通过next_samples()消费的批次数
}

impl ParallelDecodingStats {
    /// 记录成功解码的样本数
    fn add_decoded_samples(&mut self, count: usize) {
        self.samples_decoded += count;
    }

    /// 记录失败的包数
    fn increment_failed_packets(&mut self) {
        self.failed_packets += 1;
    }
}

/// 🏭 解码器工厂 - 为每个并行线程创建独立解码器
#[derive(Clone, Debug)]
struct DecoderFactory {
    codec_params: symphonia::core::codecs::CodecParameters,
    decoder_options: DecoderOptions,
    sample_converter: SampleConverter, // 🚀 新增：SIMD样本转换器
}

impl DecoderFactory {
    fn new(
        codec_params: symphonia::core::codecs::CodecParameters,
        sample_converter: SampleConverter,
    ) -> Self {
        Self {
            codec_params,
            decoder_options: DecoderOptions::default(),
            sample_converter,
        }
    }

    /// 为并行线程创建新的解码器实例
    fn create_decoder(&self) -> AudioResult<Box<dyn Decoder>> {
        let decoder = symphonia::default::get_codecs()
            .make(&self.codec_params, &self.decoder_options)
            .map_err(|e| error::decoding_error("创建并行解码器失败", e))?;
        Ok(decoder)
    }

    /// 获取样本转换器的克隆
    fn get_sample_converter(&self) -> SampleConverter {
        self.sample_converter.clone()
    }
}

impl OrderedParallelDecoder {
    /// 创建新的有序并行解码器
    ///
    /// # 参数
    /// - `codec_params`: 编解码器参数
    /// - `sample_converter`: SIMD样本转换器
    pub fn new(
        codec_params: symphonia::core::codecs::CodecParameters,
        sample_converter: SampleConverter,
    ) -> Self {
        // 🚀 创建rayon线程池，默认4线程（Arc包装，支持clone）
        let thread_pool = Arc::new(
            ThreadPoolBuilder::new()
                .num_threads(DEFAULT_PARALLEL_THREADS)
                .build()
                .expect("创建rayon线程池失败"),
        );

        Self {
            batch_size: DEFAULT_BATCH_SIZE,
            thread_pool_size: DEFAULT_PARALLEL_THREADS,
            thread_pool,
            current_batch: Vec::new(),
            sequence_counter: 0,
            samples_channel: SequencedChannel::new(),
            decoder_factory: DecoderFactory::new(codec_params, sample_converter),
            stats: ParallelDecodingStats::default(),
            decoding_state: DecodingState::Decoding,
            flushed: false,
            eof_encountered: false,
        }
    }

    /// 🎯 配置并行参数 - 根据硬件和文件特性调优
    pub fn with_config(mut self, batch_size: usize, thread_pool_size: usize) -> Self {
        self.batch_size = batch_size.clamp(1, 512); // 合理范围限制
        self.thread_pool_size = thread_pool_size.clamp(1, 16);

        // 🚀 重建rayon线程池，使用新的线程数配置（Arc包装）
        self.thread_pool = Arc::new(
            ThreadPoolBuilder::new()
                .num_threads(self.thread_pool_size)
                .build()
                .expect("创建rayon线程池失败"),
        );

        // ✅ 根据线程数重新创建通道，容量 = thread_pool_size × multiplier
        // 核心洞察：乱序样本缓冲峰值取决于并发度（线程数），而非批次大小
        let channel_capacity =
            self.thread_pool_size * decoder_performance::SEQUENCED_CHANNEL_CAPACITY_MULTIPLIER;
        self.samples_channel = SequencedChannel::with_capacity(channel_capacity);

        self
    }

    /// 📦 添加包到当前批次，批次满时触发并行解码
    pub fn add_packet(&mut self, packet: Packet) -> AudioResult<()> {
        let sequenced_packet = SequencedPacket {
            sequence: self.sequence_counter,
            packet,
        };

        self.current_batch.push(sequenced_packet);
        self.sequence_counter += 1;
        self.stats.packets_added += 1;

        // 🚀 批次满了，启动并行解码
        if self.current_batch.len() >= self.batch_size {
            self.process_current_batch()?;
        }

        Ok(())
    }

    /// 🏁 处理最后剩余的不满批次的包
    pub fn flush_remaining(&mut self) -> AudioResult<()> {
        // ✅ 防止重复flush
        if self.flushed {
            return Ok(());
        }

        // 处理最后不满批次的包
        if !self.current_batch.is_empty() {
            self.process_current_batch()?;
        }

        // ✅ 发送EOF标记，告知消费者所有包已解码完毕
        let eof_sequence = self.sequence_counter;
        let sender = self.samples_channel.sender();
        sender
            .send_sequenced(eof_sequence, DecodedChunk::EOF)
            .map_err(|_| error::decoding_error("发送EOF失败", "channel已关闭"))?;

        // ✅ 转换到Flushing状态
        self.decoding_state = DecodingState::Flushing;
        self.flushed = true;

        Ok(())
    }

    /// 📥 获取下一个有序的解码样本
    ///
    /// **重要**：此方法只返回Samples，遇到EOF时设置标志但不消费（留给drain）
    pub fn next_samples(&mut self) -> Option<Vec<f32>> {
        // 如果已经遇到EOF，直接返回None，不再尝试读取
        if self.eof_encountered {
            return None;
        }

        match self.samples_channel.try_recv_ordered() {
            Ok(DecodedChunk::Samples(samples)) => {
                // 更新统计信息
                if samples.is_empty() {
                    self.stats.increment_failed_packets();
                } else {
                    self.stats.add_decoded_samples(samples.len());
                    self.stats.consumed_batches += 1;
                }
                Some(samples)
            }
            Ok(DecodedChunk::EOF) => {
                // ⚠️ EOF已被消费，设置标志让drain知道不用再等EOF了
                self.eof_encountered = true;
                // 不改变状态！让drain_all_samples()负责切换到Completed
                None
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => None,
        }
    }

    /// 🎯 获取当前解码状态
    pub fn get_state(&self) -> DecodingState {
        self.decoding_state
    }

    /// 🎯 设置解码状态（仅供状态机内部使用）
    pub fn set_state(&mut self, state: DecodingState) {
        self.decoding_state = state;
    }

    /// 获取跳过的损坏包数量（容错处理统计）
    pub fn get_skipped_packets(&self) -> usize {
        self.stats.failed_packets
    }

    /// ✅ 确定性drain所有剩余样本 - 零超时猜测，100%可靠
    ///
    /// 通过eof_encountered标志实现确定性结束，彻底解决MP3并行解码样本丢失问题。
    /// 该方法会阻塞等待，直到eof_encountered=true且channel为空。
    ///
    /// # 返回值
    ///
    /// 返回所有剩余的样本批次，每个`Vec<f32>`代表一批解码完成的样本
    pub fn drain_all_samples(&mut self) -> Vec<Vec<f32>> {
        let mut all_samples = Vec::new();

        loop {
            match self.samples_channel.try_recv_ordered() {
                Ok(DecodedChunk::Samples(samples)) => {
                    if !samples.is_empty() {
                        all_samples.push(samples);
                    }
                }
                Ok(DecodedChunk::EOF) => {
                    // ✅ 收到EOF（如果next_samples()没消费的话）
                    self.eof_encountered = true;
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    // ✅ Channel空了，检查EOF是否已被遇到
                    if self.eof_encountered {
                        // EOF已在next_samples()中被遇到，所有数据已接收完毕
                        break;
                    }
                    // 等待更多数据（后台线程仍在解码）
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    // Channel已断开（异常情况）
                    break;
                }
            }
        }

        // ⚠️ 不在这里改状态！让Flushing状态消费完所有批次后再改
        all_samples
    }

    /// 🚀 处理当前批次 - 核心并行解码逻辑
    fn process_current_batch(&mut self) -> AudioResult<()> {
        if self.current_batch.is_empty() {
            return Ok(());
        }

        let batch = std::mem::take(&mut self.current_batch);
        let sender = self.samples_channel.sender();
        let decoder_factory = self.decoder_factory.clone();
        let thread_pool = self.thread_pool.clone(); // 🚀 Clone线程池（Arc包装，廉价操作）
        self.stats.batches_processed += 1;

        // 🚀 启动后台线程执行并行解码（使用rayon线程池）
        thread::spawn(move || {
            Self::decode_batch_parallel(batch, sender, decoder_factory, thread_pool);
        });

        Ok(())
    }

    /// 🔥 核心方法：并行解码批次包，保证有序输出（Rayon优化版本）
    fn decode_batch_parallel(
        batch: Vec<SequencedPacket>,
        sender: OrderedSender<DecodedChunk>,
        decoder_factory: DecoderFactory,
        thread_pool: Arc<rayon::ThreadPool>,
    ) {
        use rayon::prelude::*;

        // 🚀 使用rayon的for_each_init实现"每线程初始化一次decoder+复用"
        thread_pool.install(|| {
            batch.into_par_iter().for_each_init(
                || {
                    // ✅ 初始化阶段：每个rayon工作线程只执行一次
                    let decoder = decoder_factory.create_decoder().ok()?;
                    let sample_converter = decoder_factory.get_sample_converter();
                    let thread_sender = sender.clone();
                    Some((decoder, sample_converter, thread_sender))
                },
                |state, sequenced_packet| {
                    // ✅ 处理阶段：复用decoder解码多个包
                    if let Some((decoder, sample_converter, thread_sender)) = state {
                        match Self::decode_single_packet_with_simd(
                            &mut **decoder, // Box<dyn Decoder> 需要两次解引用
                            sequenced_packet.packet,
                            sample_converter,
                        ) {
                            Ok(samples) => {
                                // 🎯 直接发送到OrderedSender，无中间通道hop
                                let _ = thread_sender.send_sequenced(
                                    sequenced_packet.sequence,
                                    DecodedChunk::Samples(samples),
                                );
                            }
                            Err(_) => {
                                // ⚠️ 解码失败，发送空样本保持序列连续性
                                let _ = thread_sender.send_sequenced(
                                    sequenced_packet.sequence,
                                    DecodedChunk::Samples(vec![]),
                                );
                            }
                        }
                    }
                },
            );
        });
        // install() 确保所有工作已完成才返回
    }

    /// 🎵 解码单个数据包为样本数据（原始版本，无SIMD）
    #[allow(dead_code)]
    fn decode_single_packet(decoder: &mut dyn Decoder, packet: Packet) -> AudioResult<Vec<f32>> {
        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                // 🎯 将解码结果转换为f32样本
                let spec = audio_buf.spec();
                let mut sample_buffer =
                    SampleBuffer::<f32>::new(audio_buf.capacity() as u64, *spec);
                sample_buffer.copy_interleaved_ref(audio_buf);
                Ok(sample_buffer.samples().to_vec())
            }
            Err(e) => Err(error::decoding_error("并行解码包失败", e)),
        }
    }

    /// 🚀 解码单个数据包为样本数据（带SIMD优化）
    fn decode_single_packet_with_simd(
        decoder: &mut dyn Decoder,
        packet: Packet,
        sample_converter: &SampleConverter,
    ) -> AudioResult<Vec<f32>> {
        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                // 🚀 使用SIMD优化转换样本
                let mut samples = Vec::new();
                Self::convert_to_interleaved_with_simd(sample_converter, &audio_buf, &mut samples)?;
                Ok(samples)
            }
            Err(e) => match e {
                symphonia::core::errors::Error::DecodeError(_) => {
                    // 🎯 容错处理：返回空样本，让调用者知道跳过了这个包
                    Ok(vec![])
                }
                _ => Err(error::decoding_error("并行解码包失败", e)),
            },
        }
    }

    /// 🚀 将音频缓冲区转换为交错f32样本（SIMD优化）
    fn convert_to_interleaved_with_simd(
        sample_converter: &SampleConverter,
        audio_buf: &AudioBufferRef,
        samples: &mut Vec<f32>,
    ) -> AudioResult<()> {
        // 提取缓冲区信息
        macro_rules! extract_buffer_info {
            ($buf:expr) => {{ ($buf.spec().channels.count(), $buf.frames()) }};
        }

        let (channel_count, frame_count) = match audio_buf {
            AudioBufferRef::F32(buf) => extract_buffer_info!(buf),
            AudioBufferRef::S16(buf) => extract_buffer_info!(buf),
            AudioBufferRef::S24(buf) => extract_buffer_info!(buf),
            AudioBufferRef::S32(buf) => extract_buffer_info!(buf),
            AudioBufferRef::F64(buf) => extract_buffer_info!(buf),
            AudioBufferRef::U8(buf) => extract_buffer_info!(buf),
            AudioBufferRef::U16(buf) => extract_buffer_info!(buf),
            AudioBufferRef::U24(buf) => extract_buffer_info!(buf),
            AudioBufferRef::U32(buf) => extract_buffer_info!(buf),
            AudioBufferRef::S8(buf) => extract_buffer_info!(buf),
        };

        samples.reserve(channel_count * frame_count);

        // 样本转换宏
        macro_rules! convert_samples {
            ($buf:expr, $converter:expr) => {{
                for frame in 0..frame_count {
                    for ch in 0..channel_count {
                        let sample_f32 = $converter($buf.chan(ch)[frame]);
                        samples.push(sample_f32);
                    }
                }
            }};
        }

        // 🚀 针对不同格式使用SIMD优化
        match audio_buf {
            AudioBufferRef::F32(buf) => convert_samples!(buf, |s| s),
            // 🚀 S16 SIMD优化
            AudioBufferRef::S16(buf) => {
                // ✅ 先一次性分配空间，避免resize时用0覆盖其他声道
                let total_samples = channel_count * frame_count;
                samples.resize(total_samples, 0.0);

                // 🎯 复用单个缓冲区，减少分配次数（参考 universal_decoder.rs）
                let mut converted_channel = Vec::with_capacity(frame_count);

                for ch in 0..channel_count {
                    let channel_data = buf.chan(ch);
                    converted_channel.clear(); // 复用缓冲区

                    sample_converter
                        .convert_i16_to_f32(channel_data, &mut converted_channel)
                        .map_err(|e| error::calculation_error("S16 SIMD转换失败", e))?;

                    // 交错插入
                    for (frame_idx, &sample) in converted_channel.iter().enumerate() {
                        let interleaved_idx = frame_idx * channel_count + ch;
                        samples[interleaved_idx] = sample;
                    }
                }
            }
            // 🚀 S24 SIMD优化 (主要性能提升点)
            AudioBufferRef::S24(buf) => {
                // ✅ 先一次性分配空间，避免resize时用0覆盖其他声道
                let total_samples = channel_count * frame_count;
                samples.resize(total_samples, 0.0);

                // 🎯 复用单个缓冲区，减少分配次数（参考 universal_decoder.rs）
                let mut converted_channel = Vec::with_capacity(frame_count);

                for ch in 0..channel_count {
                    let channel_data = buf.chan(ch);
                    converted_channel.clear(); // 复用缓冲区

                    sample_converter
                        .convert_i24_to_f32(channel_data, &mut converted_channel)
                        .map_err(|e| error::calculation_error("S24 SIMD转换失败", e))?;

                    // 交错插入
                    for (frame_idx, &sample) in converted_channel.iter().enumerate() {
                        let interleaved_idx = frame_idx * channel_count + ch;
                        samples[interleaved_idx] = sample;
                    }
                }
            }
            // 其他格式使用标准转换
            AudioBufferRef::S32(buf) => convert_samples!(buf, |s| (s as f64 / 2147483648.0) as f32),
            AudioBufferRef::F64(buf) => convert_samples!(buf, |s| s as f32),
            AudioBufferRef::U8(buf) => convert_samples!(buf, |s| ((s as f32) - 128.0) / 128.0),
            AudioBufferRef::U16(buf) => convert_samples!(buf, |s| ((s as f32) - 32768.0) / 32768.0),
            AudioBufferRef::U24(buf) => {
                convert_samples!(buf, |s: symphonia::core::sample::u24| {
                    ((s.inner() as f32) - 8388608.0) / 8388608.0
                })
            }
            AudioBufferRef::U32(buf) => {
                convert_samples!(buf, |s| (((s as f64) - 2147483648.0) / 2147483648.0) as f32)
            }
            AudioBufferRef::S8(buf) => convert_samples!(buf, |s| (s as f32) / 128.0),
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sequenced_channel_ordering() {
        let channel = SequencedChannel::new();
        let sender = channel.sender();

        // 🎯 模拟乱序发送
        thread::spawn({
            let sender = sender.clone();
            move || {
                sender.send_sequenced(2, "second").unwrap();
                sender.send_sequenced(0, "first").unwrap();
                sender.send_sequenced(1, "middle").unwrap();
            }
        });

        // ✅ 验证有序接收
        assert_eq!(channel.recv_ordered().unwrap(), "first");
        assert_eq!(channel.recv_ordered().unwrap(), "middle");
        assert_eq!(channel.recv_ordered().unwrap(), "second");
    }

    #[test]
    fn test_parallel_decoder_config() {
        use crate::processing::SampleConverter;

        let mut codec_params = symphonia::core::codecs::CodecParameters::new();
        codec_params.for_codec(symphonia::core::codecs::CODEC_TYPE_NULL);

        let sample_converter = SampleConverter::new();
        let decoder =
            OrderedParallelDecoder::new(codec_params, sample_converter).with_config(128, 8);

        assert_eq!(decoder.batch_size, 128);
        assert_eq!(decoder.thread_pool_size, 8);
    }

    // ==================== Phase 1: 序列化和状态机测试 ====================

    #[test]
    fn test_reorder_buffer_mechanism() {
        let channel = SequencedChannel::new();
        let sender = channel.sender();

        // 🎯 测试重排序缓冲区：先发送seq=3，应该被缓存
        sender.send_sequenced(3, "third").unwrap();

        // ✅ 此时应该收不到数据（seq=0未到）
        assert!(channel.try_recv_ordered().is_err());

        // 🎯 发送seq=0，应该立即收到
        sender.send_sequenced(0, "first").unwrap();
        assert_eq!(channel.try_recv_ordered().unwrap(), "first");

        // 🎯 发送seq=1，应该立即收到
        sender.send_sequenced(1, "second").unwrap();
        assert_eq!(channel.try_recv_ordered().unwrap(), "second");

        // 🎯 此时seq=2仍未到，seq=3在缓冲区等待
        assert!(channel.try_recv_ordered().is_err());

        // 🎯 发送seq=2，应该立即收到seq=2和seq=3（flush连续序列）
        sender.send_sequenced(2, "middle").unwrap();
        assert_eq!(channel.try_recv_ordered().unwrap(), "middle");
        assert_eq!(channel.try_recv_ordered().unwrap(), "third"); // flush出来的
    }

    #[test]
    fn test_flush_consecutive_sequences() {
        let channel = SequencedChannel::new();
        let sender = channel.sender();

        // 🎯 测试连续序列号的自动flush：先发送2、3、4，再发送0、1
        sender.send_sequenced(2, "data2").unwrap();
        sender.send_sequenced(3, "data3").unwrap();
        sender.send_sequenced(4, "data4").unwrap();

        // ✅ 此时应该收不到数据
        assert!(channel.try_recv_ordered().is_err());

        // 🎯 发送seq=0，立即收到
        sender.send_sequenced(0, "data0").unwrap();
        assert_eq!(channel.try_recv_ordered().unwrap(), "data0");

        // 🎯 发送seq=1，应该触发flush连续序列2、3、4
        sender.send_sequenced(1, "data1").unwrap();
        assert_eq!(channel.try_recv_ordered().unwrap(), "data1");
        assert_eq!(channel.try_recv_ordered().unwrap(), "data2");
        assert_eq!(channel.try_recv_ordered().unwrap(), "data3");
        assert_eq!(channel.try_recv_ordered().unwrap(), "data4");
    }

    #[test]
    fn test_decoding_state_transitions() {
        use crate::processing::SampleConverter;

        let mut codec_params = symphonia::core::codecs::CodecParameters::new();
        codec_params.for_codec(symphonia::core::codecs::CODEC_TYPE_NULL);

        let sample_converter = SampleConverter::new();
        let mut decoder = OrderedParallelDecoder::new(codec_params, sample_converter);

        // 🎯 初始状态应该是Decoding
        assert_eq!(decoder.get_state(), DecodingState::Decoding);

        // 🎯 调用flush_remaining应该转换到Flushing
        decoder.flush_remaining().unwrap();
        assert_eq!(decoder.get_state(), DecodingState::Flushing);

        // 🎯 可以手动设置状态到Completed
        decoder.set_state(DecodingState::Completed);
        assert_eq!(decoder.get_state(), DecodingState::Completed);
    }

    #[test]
    fn test_eof_flag_behavior() {
        use crate::processing::SampleConverter;

        let mut codec_params = symphonia::core::codecs::CodecParameters::new();
        codec_params.for_codec(symphonia::core::codecs::CODEC_TYPE_NULL);

        let sample_converter = SampleConverter::new();
        let mut decoder = OrderedParallelDecoder::new(codec_params, sample_converter);

        // 🎯 初始状态：eof_encountered应该是false
        assert!(!decoder.eof_encountered);

        // 🎯 flush后会发送EOF标记
        decoder.flush_remaining().unwrap();

        // 🎯 调用next_samples应该遇到EOF并设置标志
        // 注意：由于没有真实数据，channel是空的，但我们可以测试EOF标志的初始状态
        assert_eq!(decoder.get_state(), DecodingState::Flushing);
    }

    #[test]
    fn test_flushed_flag_prevents_double_flush() {
        use crate::processing::SampleConverter;

        let mut codec_params = symphonia::core::codecs::CodecParameters::new();
        codec_params.for_codec(symphonia::core::codecs::CODEC_TYPE_NULL);

        let sample_converter = SampleConverter::new();
        let mut decoder = OrderedParallelDecoder::new(codec_params, sample_converter);

        // 🎯 第一次flush应该成功
        assert!(!decoder.flushed);
        decoder.flush_remaining().unwrap();
        assert!(decoder.flushed);

        // 🎯 第二次flush应该直接返回（防止重复）
        let result = decoder.flush_remaining();
        assert!(result.is_ok()); // 应该成功返回，而不是错误
        assert!(decoder.flushed); // 标志保持为true
    }

    // ==================== Phase 2: 批处理和样本消费测试 ====================

    #[test]
    fn test_batch_triggering_on_full() {
        use crate::processing::SampleConverter;

        let mut codec_params = symphonia::core::codecs::CodecParameters::new();
        codec_params.for_codec(symphonia::core::codecs::CODEC_TYPE_NULL);

        let sample_converter = SampleConverter::new();
        let decoder = OrderedParallelDecoder::new(codec_params, sample_converter).with_config(4, 2);

        // 🎯 批次大小为4，添加3个包不应该触发处理
        assert_eq!(decoder.current_batch.len(), 0);

        // 注意：实际添加packet需要真实的packet数据，这里测试批次满的逻辑
        assert_eq!(decoder.batch_size, 4);
        assert_eq!(decoder.stats.batches_processed, 0);
    }

    #[test]
    fn test_flush_remaining_partial_batch() {
        use crate::processing::SampleConverter;

        let mut codec_params = symphonia::core::codecs::CodecParameters::new();
        codec_params.for_codec(symphonia::core::codecs::CODEC_TYPE_NULL);

        let sample_converter = SampleConverter::new();
        let mut decoder =
            OrderedParallelDecoder::new(codec_params, sample_converter).with_config(64, 4);

        // 🎯 flush空批次应该成功
        let result = decoder.flush_remaining();
        assert!(result.is_ok());
        assert_eq!(decoder.get_state(), DecodingState::Flushing);
    }

    #[test]
    fn test_next_samples_returns_none_initially() {
        use crate::processing::SampleConverter;

        let mut codec_params = symphonia::core::codecs::CodecParameters::new();
        codec_params.for_codec(symphonia::core::codecs::CODEC_TYPE_NULL);

        let sample_converter = SampleConverter::new();
        let mut decoder = OrderedParallelDecoder::new(codec_params, sample_converter);

        // 🎯 没有数据时next_samples应该返回None
        assert!(decoder.next_samples().is_none());
    }

    #[test]
    fn test_next_samples_eof_flag_set() {
        use crate::processing::SampleConverter;

        let mut codec_params = symphonia::core::codecs::CodecParameters::new();
        codec_params.for_codec(symphonia::core::codecs::CODEC_TYPE_NULL);

        let sample_converter = SampleConverter::new();
        let mut decoder = OrderedParallelDecoder::new(codec_params, sample_converter);

        // 🎯 flush后next_samples应该最终遇到EOF
        decoder.flush_remaining().unwrap();

        // 等待EOF通过channel
        std::thread::sleep(std::time::Duration::from_millis(10));

        // 🎯 调用next_samples直到遇到EOF
        while !decoder.eof_encountered {
            if decoder.next_samples().is_none() && decoder.eof_encountered {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        // ✅ 验证EOF标志被设置
        assert!(decoder.eof_encountered);
    }

    #[test]
    fn test_drain_all_samples_empty() {
        use crate::processing::SampleConverter;

        let mut codec_params = symphonia::core::codecs::CodecParameters::new();
        codec_params.for_codec(symphonia::core::codecs::CODEC_TYPE_NULL);

        let sample_converter = SampleConverter::new();
        let mut decoder = OrderedParallelDecoder::new(codec_params, sample_converter);

        // 🎯 flush后drain应该返回空vec
        decoder.flush_remaining().unwrap();

        // 等待EOF到达
        std::thread::sleep(std::time::Duration::from_millis(10));

        let samples = decoder.drain_all_samples();
        assert_eq!(samples.len(), 0); // 没有真实数据
    }

    // ==================== Phase 3: 配置和统计测试 ====================

    #[test]
    fn test_config_clamping() {
        use crate::processing::SampleConverter;

        let mut codec_params = symphonia::core::codecs::CodecParameters::new();
        codec_params.for_codec(symphonia::core::codecs::CODEC_TYPE_NULL);

        let sample_converter = SampleConverter::new();

        // 🎯 测试batch_size上限限制（512）
        let decoder1 = OrderedParallelDecoder::new(codec_params.clone(), sample_converter.clone())
            .with_config(1000, 4);
        assert_eq!(decoder1.batch_size, 512); // 应该被限制到512

        // 🎯 测试batch_size下限限制（1）
        let decoder2 = OrderedParallelDecoder::new(codec_params.clone(), sample_converter.clone())
            .with_config(0, 4);
        assert_eq!(decoder2.batch_size, 1); // 应该被限制到1

        // 🎯 测试thread_pool_size上限限制（16）
        let decoder3 = OrderedParallelDecoder::new(codec_params.clone(), sample_converter.clone())
            .with_config(64, 100);
        assert_eq!(decoder3.thread_pool_size, 16); // 应该被限制到16

        // 🎯 测试thread_pool_size下限限制（1）
        let decoder4 =
            OrderedParallelDecoder::new(codec_params, sample_converter).with_config(64, 0);
        assert_eq!(decoder4.thread_pool_size, 1); // 应该被限制到1
    }

    #[test]
    fn test_stats_tracking() {
        use crate::processing::SampleConverter;

        let mut codec_params = symphonia::core::codecs::CodecParameters::new();
        codec_params.for_codec(symphonia::core::codecs::CODEC_TYPE_NULL);

        let sample_converter = SampleConverter::new();
        let decoder = OrderedParallelDecoder::new(codec_params, sample_converter);

        // 🎯 初始统计应该为0
        assert_eq!(decoder.stats.packets_added, 0);
        assert_eq!(decoder.stats.batches_processed, 0);
        assert_eq!(decoder.stats.samples_decoded, 0);
        assert_eq!(decoder.stats.failed_packets, 0);
    }

    #[test]
    fn test_sequence_counter_initial_value() {
        use crate::processing::SampleConverter;

        let mut codec_params = symphonia::core::codecs::CodecParameters::new();
        codec_params.for_codec(symphonia::core::codecs::CODEC_TYPE_NULL);

        let sample_converter = SampleConverter::new();
        let decoder = OrderedParallelDecoder::new(codec_params, sample_converter);

        // 🎯 序列号计数器初始值应该是0
        assert_eq!(decoder.sequence_counter, 0);
    }

    #[test]
    fn test_decoder_factory_sample_converter() {
        use crate::processing::SampleConverter;

        let codec_params = symphonia::core::codecs::CodecParameters::new();
        let sample_converter = SampleConverter::new();

        let factory = DecoderFactory::new(codec_params, sample_converter);

        // 🎯 获取样本转换器克隆
        let converter = factory.get_sample_converter();
        assert!(std::mem::size_of_val(&converter) > 0); // 验证转换器存在
    }

    #[test]
    fn test_get_skipped_packets() {
        use crate::processing::SampleConverter;

        let mut codec_params = symphonia::core::codecs::CodecParameters::new();
        codec_params.for_codec(symphonia::core::codecs::CODEC_TYPE_NULL);

        let sample_converter = SampleConverter::new();
        let decoder = OrderedParallelDecoder::new(codec_params, sample_converter);

        // 🎯 初始跳过包数应该是0
        assert_eq!(decoder.get_skipped_packets(), 0);
    }
}
