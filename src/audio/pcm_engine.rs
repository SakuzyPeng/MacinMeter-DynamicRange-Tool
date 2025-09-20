//! PCM处理引擎模块
//!
//! 提供PCM格式音频的解码和流式处理核心业务逻辑
//! 注意：此模块仅供universal_decoder协调器内部使用

use super::error_handling::handle_symphonia_error;
use super::format::{AudioFormat, FormatSupport};
use super::stats::ChunkSizeStats;
use super::streaming::StreamingDecoder;
use crate::error::{AudioError, AudioResult};
use std::path::Path;

/// PCM处理引擎 - 处理WAV、FLAC等PCM格式
///
/// 此结构仅供协调器内部使用，外部不应直接访问
pub(super) struct PcmEngine;

impl PcmEngine {
    pub(super) fn new() -> Self {
        Self
    }

    pub(super) fn name(&self) -> &'static str {
        "PCM Engine"
    }

    pub(super) fn supported_formats(&self) -> &FormatSupport {
        static SUPPORT: FormatSupport = FormatSupport {
            extensions: &["wav", "flac", "alac", "aiff", "au", "caf"],
        };
        &SUPPORT
    }

    pub(super) fn probe_format(&self, path: &Path) -> AudioResult<AudioFormat> {
        // 使用symphonia探测格式
        self.probe_with_symphonia(path)
    }

    pub(super) fn create_streaming(&self, path: &Path) -> AudioResult<Box<dyn StreamingDecoder>> {
        // 创建PCM流式解码器（固定启用逐包模式）
        Ok(Box::new(PcmStreamProcessor::new(path)?))
    }

    /// 🚀 创建高性能流式解码器（推荐方法）
    ///
    /// 固定启用逐包模式优化，遵循"无条件高性能原则"。
    /// 适配foobar2000-plugin分支的高性能要求和WindowRmsAnalyzer批处理计算。
    pub(super) fn create_streaming_optimized(
        &self,
        path: &Path,
    ) -> AudioResult<Box<dyn StreamingDecoder>> {
        // 🚀 使用统一的高性能构造函数
        Ok(Box::new(PcmStreamProcessor::new(path)?))
    }

    /// 使用symphonia探测格式
    fn probe_with_symphonia(&self, path: &Path) -> AudioResult<AudioFormat> {
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;
        use symphonia::core::probe::Hint;

        let file = std::fs::File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(extension) = path.extension() {
            hint.with_extension(&extension.to_string_lossy());
        }

        let meta_opts = MetadataOptions::default();
        let fmt_opts = FormatOptions::default();

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &fmt_opts, &meta_opts)
            .map_err(|e| AudioError::FormatError(format!("格式探测失败: {e}")))?;

        let format_reader = probed.format;

        let track = format_reader
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or_else(|| AudioError::FormatError("未找到音频轨道".to_string()))?;

        let codec_params = &track.codec_params;
        let sample_rate = codec_params.sample_rate.unwrap_or(44100);
        let channels = codec_params
            .channels
            .map(|ch| ch.count())
            .ok_or_else(|| AudioError::FormatError("无法获取声道数信息".to_string()))?
            as u16;
        let bits_per_sample = self.detect_bit_depth(codec_params);

        // 估算样本数（可能不准确）
        let sample_count = codec_params.n_frames.unwrap_or(0);

        let format = AudioFormat::new(sample_rate, channels, bits_per_sample, sample_count);
        format.validate()?;

        Ok(format)
    }

    /// 检测位深度
    fn detect_bit_depth(&self, codec_params: &symphonia::core::codecs::CodecParameters) -> u16 {
        if let Some(bits) = codec_params.bits_per_sample {
            bits as u16
        } else {
            // 根据编解码器类型推断
            match codec_params.codec {
                symphonia::core::codecs::CODEC_TYPE_PCM_S16LE
                | symphonia::core::codecs::CODEC_TYPE_PCM_S16BE => 16,
                symphonia::core::codecs::CODEC_TYPE_PCM_S24LE
                | symphonia::core::codecs::CODEC_TYPE_PCM_S24BE => 24,
                symphonia::core::codecs::CODEC_TYPE_PCM_S32LE
                | symphonia::core::codecs::CODEC_TYPE_PCM_S32BE
                | symphonia::core::codecs::CODEC_TYPE_PCM_F32LE
                | symphonia::core::codecs::CODEC_TYPE_PCM_F32BE => 32,
                _ => 16, // 默认16位
            }
        }
    }

    /// 转换symphonia缓冲区为交错格式
    pub(super) fn convert_buffer_to_interleaved(
        audio_buf: &symphonia::core::audio::AudioBufferRef,
        samples: &mut Vec<f32>,
    ) -> AudioResult<()> {
        use symphonia::core::audio::{AudioBufferRef, Signal};

        // 🔥 使用宏消除重复的缓冲区信息提取
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

        // 🔥 使用宏简化样本转换逻辑
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

        // 转换为交错格式 - 每种格式使用专门的转换器
        match audio_buf {
            AudioBufferRef::F32(buf) => convert_samples!(buf, |s| s),
            AudioBufferRef::S16(buf) => convert_samples!(buf, |s| (s as f32) / 32768.0),
            AudioBufferRef::S24(buf) => {
                convert_samples!(buf, |s: symphonia::core::sample::i24| (s.inner() as f32)
                    / 8388608.0)
            }
            AudioBufferRef::S32(buf) => convert_samples!(buf, |s| (s as f64 / 2147483648.0) as f32),
            AudioBufferRef::F64(buf) => convert_samples!(buf, |s| s as f32),
            AudioBufferRef::U8(buf) => convert_samples!(buf, |s| ((s as f32) - 128.0) / 128.0),
            AudioBufferRef::U16(buf) => convert_samples!(buf, |s| ((s as f32) - 32768.0) / 32768.0),
            AudioBufferRef::U24(buf) => {
                convert_samples!(buf, |s: symphonia::core::sample::u24| ((s.inner() as f32)
                    - 8388608.0)
                    / 8388608.0)
            }
            AudioBufferRef::U32(buf) => {
                convert_samples!(buf, |s| (((s as f64) - 2147483648.0) / 2147483648.0) as f32)
            }
            AudioBufferRef::S8(buf) => convert_samples!(buf, |s| (s as f32) / 128.0),
        }

        Ok(())
    }
}

/// PCM流式处理器
///
/// 此结构仅供协调器内部使用，外部不应直接访问
pub(super) struct PcmStreamProcessor {
    path: std::path::PathBuf,
    format: AudioFormat,
    current_position: u64,
    total_samples: u64,

    // 🚀 逐包统计信息（固定启用高性能模式）
    chunk_stats: ChunkSizeStats,

    // symphonia组件
    format_reader: Option<Box<dyn symphonia::core::formats::FormatReader>>,
    decoder: Option<Box<dyn symphonia::core::codecs::Decoder>>,
    track_id: Option<u32>,
}

impl PcmStreamProcessor {
    /// 🚀 创建高性能流式处理器
    ///
    /// 固定启用逐包模式，遵循"无条件高性能原则"。
    /// foobar2000-plugin分支专用，提供最优的流式处理性能。
    pub(super) fn new<P: AsRef<Path>>(path: P) -> AudioResult<Self> {
        let path = path.as_ref().to_path_buf();
        let pcm_engine = PcmEngine::new();
        let format = pcm_engine.probe_format(&path)?;

        Ok(Self {
            path,
            format: format.clone(),
            current_position: 0,
            total_samples: format.sample_count,
            chunk_stats: ChunkSizeStats::new(),
            format_reader: None,
            decoder: None,
            track_id: None,
        })
    }

    fn initialize_symphonia(&mut self) -> AudioResult<()> {
        use symphonia::core::codecs::DecoderOptions;
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;
        use symphonia::core::probe::Hint;

        let file = std::fs::File::open(&self.path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(extension) = self.path.extension() {
            hint.with_extension(&extension.to_string_lossy());
        }

        let meta_opts = MetadataOptions::default();
        let fmt_opts = FormatOptions::default();

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &fmt_opts, &meta_opts)
            .map_err(|e| AudioError::FormatError(format!("格式探测失败: {e}")))?;

        let format_reader = probed.format;

        let track = format_reader
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or_else(|| AudioError::FormatError("未找到音频轨道".to_string()))?;

        let track_id = track.id;
        let codec_params = &track.codec_params;

        let dec_opts = DecoderOptions::default();
        let decoder = symphonia::default::get_codecs()
            .make(codec_params, &dec_opts)
            .map_err(|e| AudioError::FormatError(format!("创建解码器失败: {e}")))?;

        self.format_reader = Some(format_reader);
        self.decoder = Some(decoder);
        self.track_id = Some(track_id);

        // 🔍 调试模式：输出音频格式信息
        #[cfg(debug_assertions)]
        {
            let ext = self
                .path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");
            eprintln!("\n🎵 开始解码音频文件:");
            eprintln!("   文件: {}", self.path.display());
            eprintln!("   格式: {}", ext.to_uppercase());
            eprintln!("   采样率: {} Hz", self.format.sample_rate);
            eprintln!("   声道数: {}", self.format.channels);
            eprintln!("   位深度: {} bit", self.format.bits_per_sample);
            eprintln!("   总样本: {} 样本/声道", self.format.sample_count);
            eprintln!("   时长: {:.2} 秒\n", self.format.duration_seconds());
        }

        Ok(())
    }

    /// 🚀 高性能逐包处理模式（静态版本避免借用冲突）
    ///
    /// 每个解码包立即返回，最大化流式处理效率，适配foobar2000原版行为。
    fn process_packet_chunk_mode_static(
        chunk_stats: &mut ChunkSizeStats,
        current_position: &mut u64,
        format: &AudioFormat,
        format_reader: &mut Box<dyn symphonia::core::formats::FormatReader>,
        decoder: &mut Box<dyn symphonia::core::codecs::Decoder>,
        track_id: u32,
    ) -> AudioResult<Option<Vec<f32>>> {
        loop {
            // 🔧 手动处理EOF以便输出统计
            let packet = match format_reader.next_packet() {
                Ok(packet) => packet,
                Err(symphonia::core::errors::Error::ResetRequired) => {
                    decoder.reset();
                    continue;
                }
                Err(symphonia::core::errors::Error::IoError(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    // 🔍 文件结束时输出统计
                    #[cfg(debug_assertions)]
                    {
                        chunk_stats.finalize();
                    }
                    return Ok(None);
                }
                Err(e) => return Err(AudioError::FormatError(format!("symphonia错误: {e}"))),
            };

            if packet.track_id() != track_id {
                continue;
            }

            // 🔧 使用统一错误处理宏处理解码
            if let Some(audio_buf) =
                handle_symphonia_error!(decoder.decode(&packet), decoder, continue_on_reset)
            {
                let mut packet_samples = Vec::new();
                PcmEngine::convert_buffer_to_interleaved(&audio_buf, &mut packet_samples)?;

                if !packet_samples.is_empty() {
                    // 🔥 记录块大小统计（每声道样本数）
                    let samples_per_channel = packet_samples.len() / format.channels as usize;
                    chunk_stats.add_chunk(samples_per_channel);

                    // 更新位置
                    *current_position += samples_per_channel as u64;
                    return Ok(Some(packet_samples));
                }
            }
            // decode失败或返回空缓冲区，继续下一次循环
        }
    }
}

impl StreamingDecoder for PcmStreamProcessor {
    fn next_chunk(&mut self) -> AudioResult<Option<Vec<f32>>> {
        // 按需初始化
        if self.format_reader.is_none() {
            self.initialize_symphonia()?;
        }

        let format_reader = self.format_reader.as_mut().unwrap();
        let decoder = self.decoder.as_mut().unwrap();
        let track_id = self.track_id.unwrap();

        // 🎯 高性能逐包处理架构：
        // 每个解码包立即返回，最大化流式处理效率。
        // 符合foobar2000-plugin分支的"无条件高性能原则"。

        // 🚀 固定使用高性能逐包模式
        Self::process_packet_chunk_mode_static(
            &mut self.chunk_stats,
            &mut self.current_position,
            &self.format,
            format_reader,
            decoder,
            track_id,
        )
    }

    fn progress(&self) -> f32 {
        if self.total_samples > 0 {
            (self.current_position as f32) / (self.total_samples as f32)
        } else {
            0.0
        }
    }

    fn format(&self) -> &AudioFormat {
        &self.format
    }

    fn reset(&mut self) -> AudioResult<()> {
        self.format_reader = None;
        self.decoder = None;
        self.track_id = None;
        self.current_position = 0;
        Ok(())
    }

    fn get_chunk_stats(&mut self) -> Option<ChunkSizeStats> {
        // 逐包模式固定启用，总是提供统计信息
        self.chunk_stats.finalize();
        Some(self.chunk_stats.clone())
    }
}
