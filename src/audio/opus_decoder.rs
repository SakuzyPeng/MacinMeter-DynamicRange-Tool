//! Opus音频解码器模块
//!
//! 基于songbird库提供Opus格式的真实解码支持
//! 与现有的UniversalDecoder架构完美集成

use super::format::AudioFormat;
use super::stats::ChunkSizeStats;
use super::streaming::StreamingDecoder;
use crate::error::{self, AudioResult};
use crate::processing::sample_conversion::SampleConverter;
use songbird::input::Input;
use std::path::Path;
use symphonia_core::{codecs::CODEC_TYPE_OPUS, errors::Error as SymphError};

/// 🎵 Songbird Opus解码器
///
/// 通过songbird库提供Opus格式的真实解码功能
/// 完美适配现有StreamingDecoder接口
pub struct SongbirdOpusDecoder {
    /// 音频格式信息
    format: AudioFormat,

    /// songbird解析后的输入源
    input: Option<Input>,

    /// 解码进度跟踪
    current_position: u64,
    total_samples: u64,

    /// 缓冲区管理
    sample_buffer: Vec<f32>,
    buffer_offset: usize,

    /// 块统计信息
    chunk_stats: ChunkSizeStats,

    /// 路径信息（用于错误报告）
    file_path: std::path::PathBuf,

    /// 解码完成标志
    is_finished: bool,

    /// 🚀 样本转换器（启用SIMD优化）
    sample_converter: SampleConverter,
}

impl SongbirdOpusDecoder {
    /// 🚀 打开并解析Opus输入源（公共辅助函数，消除重复）
    ///
    /// 统一的 songbird Input 创建和解析逻辑，避免重复创建 tokio runtime。
    #[allow(clippy::unnecessary_to_owned)]
    fn open_playable_input(path: &Path) -> AudioResult<Input> {
        let input = Input::from(songbird::input::File::new(path.to_path_buf()));

        // 创建tokio运行时进行异步解析
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| error::decoding_error("创建tokio运行时失败", e))?;

        rt.block_on(async {
            input
                .make_playable_async(
                    &songbird::input::codecs::CODEC_REGISTRY,
                    &songbird::input::codecs::PROBE,
                )
                .await
        })
        .map_err(|e| error::decoding_error("解析opus文件失败", e))
    }

    /// 创建新的Opus解码器
    pub fn new<P: AsRef<Path>>(path: P) -> AudioResult<Self> {
        let path = path.as_ref().to_path_buf();

        // 使用songbird初步探测格式
        let format = Self::probe_opus_format(&path)?;

        Ok(Self {
            format: format.clone(),
            input: None,
            current_position: 0,
            total_samples: format.sample_count,
            sample_buffer: Vec::new(),
            buffer_offset: 0,
            chunk_stats: ChunkSizeStats::new(),
            file_path: path,
            is_finished: false,
            sample_converter: SampleConverter::new(),
        })
    }

    /// 探测Opus文件格式信息
    ///
    /// 🎯 使用songbird真实解析opus文件元数据
    fn probe_opus_format(path: &Path) -> AudioResult<AudioFormat> {
        // 🚀 使用公共函数创建并解析输入
        let parsed_input = Self::open_playable_input(path)?;

        // 获取真实的格式信息
        if let Some(parsed) = parsed_input.parsed() {
            let track = parsed
                .format
                .default_track()
                .ok_or_else(|| error::decoding_error("未找到默认音轨", ""))?;

            let codec_params = &track.codec_params;

            // 验证这确实是Opus编解码器
            if codec_params.codec != CODEC_TYPE_OPUS {
                return Err(error::decoding_error(
                    "编解码器类型不匹配",
                    format!("预期Opus，但找到: {:?}", codec_params.codec),
                ));
            }

            let sample_rate = codec_params.sample_rate.unwrap_or(48000); // Opus默认48kHz
            let channels = codec_params.channels.map(|ch| ch.count()).unwrap_or(2) as u16; // 默认立体声

            // 📝 位深语义说明：
            // - bits_per_sample = 16 表示 Opus 源格式的典型位深（元数据用途）
            // - 实际解码输出为 f32 格式（通过 SampleConverter 转换）
            // - 此字段用于格式信息展示，不影响实际样本处理
            let bits_per_sample = 16;

            // 🎯 智能样本数计算：优先使用精确元数据
            let total_samples = if let Some(n_frames) = codec_params.n_frames {
                Self::calculate_samples_from_frames(n_frames, sample_rate, channels)
            } else {
                Self::estimate_samples_from_file_size(path, sample_rate)?
            };

            // 🎯 使用真实的Opus编解码器类型
            let format = AudioFormat::with_codec(
                sample_rate,
                channels,
                bits_per_sample,
                total_samples,
                CODEC_TYPE_OPUS,
            );
            format.validate()?;
            Ok(format)
        } else {
            Err(error::decoding_error(
                "解析音频文件失败",
                "输入源无解析数据",
            ))
        }
    }

    /// 计算每声道样本数
    ///
    /// 🎯 经调试验证：songbird/symphonia对Opus也返回每声道帧数，与其他格式一致
    fn calculate_samples_from_frames(n_frames: u64, _sample_rate: u32, _channels: u16) -> u64 {
        // 🎯 修正错误假设：Opus的n_frames已经是每声道帧数，无需特殊处理
        // 之前的除法操作是错误的
        n_frames
    }

    /// 智能文件大小估算样本数
    ///
    /// 🎯 动态分析文件特征，避免硬编码比特率
    fn estimate_samples_from_file_size(path: &Path, sample_rate: u32) -> AudioResult<u64> {
        let file_size = std::fs::metadata(path)
            .map_err(crate::error::AudioError::IoError)?
            .len();

        // 🎯 智能比特率估算：基于文件大小范围
        let estimated_bitrate = if file_size < 1_000_000 {
            // 小文件：可能是低码率或短时长
            128_000
        } else if file_size < 10_000_000 {
            // 中等文件：标准质量
            256_000
        } else {
            // 大文件：高质量
            320_000
        };

        let estimated_duration_seconds = (file_size * 8) as f64 / estimated_bitrate as f64;
        let estimated_samples = (estimated_duration_seconds * sample_rate as f64) as u64;

        // 合理性检查：避免极端值
        if estimated_samples < 1000 || estimated_samples > sample_rate as u64 * 86400 {
            // 如果估算不合理，使用保守估算（1分钟）
            Ok(sample_rate as u64 * 60)
        } else {
            Ok(estimated_samples)
        }
    }

    /// 初始化songbird输入源
    fn initialize_songbird(&mut self) -> AudioResult<()> {
        if self.input.is_some() {
            return Ok(());
        }

        // 🚀 使用公共函数创建并解析输入
        let parsed_input = Self::open_playable_input(&self.file_path)?;

        // 验证输入已正确解析
        match &parsed_input {
            Input::Live(live_input, _) => {
                if live_input.is_playable() {
                    self.input = Some(parsed_input);
                    Ok(())
                } else {
                    Err(error::decoding_error("输入未被正确解析", ""))
                }
            }
            _ => Err(error::decoding_error("输入未处于Live状态", "")),
        }
    }

    /// 从songbird读取下一块真实音频数据
    fn read_next_chunk(&mut self) -> AudioResult<Option<Vec<f32>>> {
        if self.is_finished {
            return Ok(None);
        }

        if self.input.is_none() {
            self.initialize_songbird()?;
        }

        let input = self
            .input
            .as_mut()
            .ok_or_else(|| error::decoding_error("未初始化的解析输入", ""))?;

        // 获取parsed数据的可变引用
        let parsed = match input {
            Input::Live(live_input, _) => live_input
                .parsed_mut()
                .ok_or_else(|| error::decoding_error("输入未被解析", ""))?,
            _ => return Err(error::decoding_error("输入不是Live状态", "")),
        };

        let target_samples = 4096; // 目标样本数 (per channel)

        // 🚀 性能优化：预分配容量避免realloc
        let capacity = target_samples * self.format.channels as usize;
        let mut output_samples = Vec::with_capacity(capacity);

        // 🚀 零成本优化：复用临时向量，避免每次解码包都分配
        let mut temp_samples = Vec::with_capacity(2048); // 典型包大小缓冲

        // 解码循环：读取包并解码直到获得足够样本
        while output_samples.len() / (self.format.channels as usize) < target_samples {
            // 读取下一个包
            let packet = match parsed.format.next_packet() {
                Ok(packet) => packet,
                Err(SymphError::IoError(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    // 文件结束
                    self.is_finished = true;
                    break;
                }
                Err(e) => return Err(error::decoding_error("读取包失败", e)),
            };

            // 只处理我们目标音轨的包
            if packet.track_id() != parsed.track_id {
                continue;
            }

            // 解码包
            match parsed.decoder.decode(&packet) {
                Ok(audio_buf) => {
                    // 🚀 使用统一转换器（启用SIMD优化，复用processing层）
                    temp_samples.clear(); // 复用缓冲，避免重复分配
                    self.sample_converter
                        .convert_buffer_to_interleaved(&audio_buf, &mut temp_samples)?;
                    output_samples.extend_from_slice(&temp_samples);
                }
                Err(SymphError::DecodeError(_)) => {
                    // 跳过解码错误的包，继续处理
                    continue;
                }
                Err(e) => return Err(error::decoding_error("解码失败", e)),
            }
        }

        if output_samples.is_empty() {
            self.is_finished = true;
            return Ok(None);
        }

        // 更新进度：output_samples是交错格式，需要除以声道数得到每声道帧数
        let frames_decoded = output_samples.len() as u64 / (self.format.channels as u64);
        self.current_position += frames_decoded;

        // 📊 记录chunk统计（维度：interleaved样本总数）
        // - add_chunk 接收交错格式的样本总数（frames × channels）
        // - 用于分析解码块大小分布和性能特征
        // - 如需帧数统计，应传入 frames_decoded
        self.chunk_stats.add_chunk(output_samples.len());

        Ok(Some(output_samples))
    }
}

impl StreamingDecoder for SongbirdOpusDecoder {
    fn next_chunk(&mut self) -> AudioResult<Option<Vec<f32>>> {
        loop {
            // 如果缓冲区中还有数据，优先返回缓冲区数据
            if self.buffer_offset < self.sample_buffer.len() {
                let chunk_size = 1024.min(self.sample_buffer.len() - self.buffer_offset);
                let chunk = self.sample_buffer[self.buffer_offset..self.buffer_offset + chunk_size]
                    .to_vec();
                self.buffer_offset += chunk_size;
                return Ok(Some(chunk));
            }

            // 缓冲区用完了，读取下一块数据
            self.buffer_offset = 0;
            match self.read_next_chunk()? {
                Some(new_data) => {
                    self.sample_buffer = new_data;
                    // 🔄 迭代模式：继续循环从新数据中返回第一个chunk
                }
                None => return Ok(None),
            }
        }
    }

    fn format(&self) -> AudioFormat {
        // 🎯 动态构造包含实时样本数的格式信息
        let mut current_format = self.format.clone();
        current_format.update_sample_count(self.current_position);
        current_format
    }

    fn progress(&self) -> f32 {
        if self.total_samples == 0 {
            0.0
        } else {
            (self.current_position as f32) / (self.total_samples as f32)
        }
    }

    fn reset(&mut self) -> AudioResult<()> {
        self.input = None;
        self.current_position = 0;
        self.sample_buffer.clear();
        self.buffer_offset = 0;
        self.is_finished = false;
        Ok(())
    }

    fn get_chunk_stats(&mut self) -> Option<ChunkSizeStats> {
        self.chunk_stats.finalize();
        Some(self.chunk_stats.clone())
    }
}
