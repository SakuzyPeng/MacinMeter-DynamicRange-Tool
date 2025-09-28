//! 统一音频解码器
//!
//! 真正的UniversalDecoder - 直接处理所有音频格式的解码
//! 基于Symphonia提供完整的多格式支持

use crate::error::{AudioError, AudioResult};
use std::path::Path;

// 重新导出公共接口
pub use super::format::{AudioFormat, FormatSupport};
pub use super::stats::ChunkSizeStats;
pub use super::streaming::StreamingDecoder;

// Opus解码器支持
use super::opus_decoder::SongbirdOpusDecoder;

// 内部模块
// (所有错误处理现在内联到方法中)

/// 🌟 统一音频解码器 - 真正的Universal
///
/// 直接基于Symphonia处理所有音频格式，无需中间层抽象
pub struct UniversalDecoder;

impl Default for UniversalDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl UniversalDecoder {
    /// 创建新的统一解码器
    pub fn new() -> Self {
        Self
    }

    /// 获取支持的格式信息
    pub fn supported_formats(&self) -> &FormatSupport {
        static SUPPORT: FormatSupport = FormatSupport {
            // 🚀 统一格式支持声明 - 基于Symphonia features + Songbird扩展（已验证）
            extensions: &[
                // 无损格式 (✅ 已验证)
                "wav", "flac", "aiff", "m4a", // 有损格式 (✅ 已验证)
                "mp3", "mp1", "aac", "ogg", "opus", // 容器格式 (✅ 新增)
                "mkv", "webm",
            ],
        };
        &SUPPORT
    }

    /// 检测是否能解码指定文件
    pub fn can_decode(&self, path: &Path) -> bool {
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            self.supported_formats()
                .extensions
                .contains(&ext.to_lowercase().as_str())
        } else {
            false
        }
    }

    /// 探测文件格式
    pub fn probe_format<P: AsRef<Path>>(&self, path: P) -> AudioResult<AudioFormat> {
        let path = path.as_ref();

        // 🎵 检查是否为Opus格式，使用专用探测方法
        if let Some(ext) = path.extension().and_then(|s| s.to_str())
            && ext.to_lowercase() == "opus"
        {
            // 暂时创建一个临时解码器来获取格式信息
            // 这不是最优的，但能确保格式探测的一致性
            let temp_decoder = SongbirdOpusDecoder::new(path)?;
            return Ok(temp_decoder.format());
        }

        // 其他格式使用Symphonia探测
        self.probe_with_symphonia(path)
    }

    /// 创建流式解码器
    pub fn create_streaming<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> AudioResult<Box<dyn StreamingDecoder>> {
        let path = path.as_ref();

        // 🎵 检查是否为Opus格式，使用专用解码器
        if let Some(ext) = path.extension().and_then(|s| s.to_str())
            && ext.to_lowercase() == "opus"
        {
            return Ok(Box::new(SongbirdOpusDecoder::new(path)?));
        }

        // 其他格式使用通用解码器
        Ok(Box::new(UniversalStreamProcessor::new(path)?))
    }

    /// 🚀 创建高性能流式解码器（推荐方法）
    ///
    /// 固定启用智能缓冲流式处理，遵循"无条件高性能原则"。
    /// 适配foobar2000-plugin分支的高性能要求和WindowRmsAnalyzer批处理计算。
    pub fn create_streaming_optimized<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> AudioResult<Box<dyn StreamingDecoder>> {
        let path = path.as_ref();

        // 🎵 检查是否为Opus格式，使用专用解码器
        if let Some(ext) = path.extension().and_then(|s| s.to_str())
            && ext.to_lowercase() == "opus"
        {
            return Ok(Box::new(SongbirdOpusDecoder::new(path)?));
        }

        // 其他格式使用通用解码器
        Ok(Box::new(UniversalStreamProcessor::new(path)?))
    }

    /// 使用Symphonia探测格式
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
        let channels = self.detect_channel_count(codec_params)?;
        let bits_per_sample = self.detect_bit_depth(codec_params);

        // 获取样本数，支持多种方式
        let sample_count = self.detect_sample_count(codec_params);

        // 🎯 获取真实的编解码器类型
        let format = AudioFormat::with_codec(
            sample_rate,
            channels,
            bits_per_sample,
            sample_count,
            codec_params.codec,
        );
        format.validate()?;

        Ok(format)
    }

    /// 检测位深度
    fn detect_bit_depth(&self, codec_params: &symphonia::core::codecs::CodecParameters) -> u16 {
        codec_params.bits_per_sample.unwrap_or({
            if let Some(sample_format) = codec_params.sample_format {
                match sample_format {
                    symphonia::core::sample::SampleFormat::S16 => 16,
                    symphonia::core::sample::SampleFormat::S24 => 24,
                    symphonia::core::sample::SampleFormat::S32 => 32,
                    symphonia::core::sample::SampleFormat::F32 => 32,
                    symphonia::core::sample::SampleFormat::F64 => 64,
                    _ => 16, // 默认16位
                }
            } else {
                16 // 默认16位
            }
        }) as u16
    }

    /// 检测声道数，支持多种格式（包括M4A等特殊格式）
    fn detect_channel_count(
        &self,
        codec_params: &symphonia::core::codecs::CodecParameters,
    ) -> AudioResult<u16> {
        // 首先尝试标准方法
        if let Some(channels) = codec_params.channels {
            return Ok(channels.count() as u16);
        }

        // 对于M4A等格式，尝试从channel_layout获取
        if let Some(channel_layout) = codec_params.channel_layout {
            // 根据Layout枚举确定声道数
            let channel_count = match channel_layout {
                symphonia::core::audio::Layout::Mono => 1,
                symphonia::core::audio::Layout::Stereo => 2,
                _ => 2, // 其他布局默认为立体声
            };
            return Ok(channel_count);
        }

        // 如果都失败，使用默认值（通常音频文件是立体声）
        Ok(2)
    }

    /// 检测样本数，支持多种格式
    fn detect_sample_count(&self, codec_params: &symphonia::core::codecs::CodecParameters) -> u64 {
        // 首先尝试从codec参数获取
        if let Some(n_frames) = codec_params.n_frames {
            return n_frames;
        }

        // 对于AAC等格式，尝试从时长和采样率估算
        if let (Some(duration), Some(sample_rate)) =
            (codec_params.time_base, codec_params.sample_rate)
            && duration.denom > 0
        {
            let time_base_seconds = duration.numer as f64 / duration.denom as f64;
            let estimated_samples = (time_base_seconds * sample_rate as f64) as u64;
            if estimated_samples > 0 {
                return estimated_samples;
            }
        }

        // 对于无法确定样本数的格式，返回一个合理的占位值
        // 这将在实际处理时被正确的样本计数覆盖
        0
    }
}

/// 🌟 统一流式处理器 - 真正的Universal流式解码
///
/// 直接基于Symphonia处理所有音频格式的流式解码
pub struct UniversalStreamProcessor {
    path: std::path::PathBuf,
    format: AudioFormat,
    current_position: u64,
    total_samples: u64,

    // 🚀 智能缓冲统计信息（固定启用高性能模式）
    chunk_stats: ChunkSizeStats,

    // symphonia组件
    format_reader: Option<Box<dyn symphonia::core::formats::FormatReader>>,
    decoder: Option<Box<dyn symphonia::core::codecs::Decoder>>,
    track_id: Option<u32>,
}

impl UniversalStreamProcessor {
    /// 🚀 创建统一流式处理器
    ///
    /// 固定启用智能缓冲流式处理，遵循"无条件高性能原则"。
    /// foobar2000-plugin分支专用，提供最优的流式处理性能。
    pub fn new<P: AsRef<Path>>(path: P) -> AudioResult<Self> {
        let path = path.as_ref().to_path_buf();
        let decoder = UniversalDecoder::new();
        let format = decoder.probe_format(&path)?;

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
            .map_err(|e| AudioError::FormatError(format!("创建解码器失败: {e}")))?;

        let format_reader = probed.format;

        let track = format_reader
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or_else(|| AudioError::FormatError("未找到音频轨道".to_string()))?;

        let track_id = track.id;
        let codec_params = &track.codec_params;

        let decoder_opts = DecoderOptions::default();
        let decoder = symphonia::default::get_codecs()
            .make(codec_params, &decoder_opts)
            .map_err(|e| AudioError::FormatError(format!("创建解码器失败: {e}")))?;

        self.format_reader = Some(format_reader);
        self.decoder = Some(decoder);
        self.track_id = Some(track_id);

        Ok(())
    }

    /// 从解码的音频缓冲区提取样本
    fn extract_samples_from_decoded(
        decoded: &symphonia::core::audio::AudioBufferRef,
    ) -> AudioResult<Vec<f32>> {
        let mut samples = Vec::new();
        Self::convert_buffer_to_interleaved(decoded, &mut samples)?;
        Ok(samples)
    }

    /// 转换symphonia缓冲区为交错格式
    fn convert_buffer_to_interleaved(
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

impl StreamingDecoder for UniversalStreamProcessor {
    fn format(&self) -> AudioFormat {
        // 🎯 动态构造包含实时样本数的格式信息
        let mut current_format = self.format.clone();
        current_format.update_sample_count(self.total_samples);
        current_format
    }

    fn progress(&self) -> f32 {
        if self.total_samples == 0 {
            0.0
        } else {
            (self.current_position as f32) / (self.total_samples as f32)
        }
    }

    fn next_chunk(&mut self) -> AudioResult<Option<Vec<f32>>> {
        if self.format_reader.is_none() {
            self.initialize_symphonia()?;
        }

        let format_reader = self.format_reader.as_mut().unwrap();
        let decoder = self.decoder.as_mut().unwrap();
        let track_id = self.track_id.unwrap();

        // 🚀 读取下一个音频包
        match format_reader.next_packet() {
            Ok(packet) => {
                if packet.track_id() != track_id {
                    return self.next_chunk(); // 跳过非目标轨道的包
                }

                // 记录包统计信息
                self.chunk_stats.add_chunk(packet.dur() as usize);

                // 解码音频包
                match decoder.decode(&packet) {
                    Ok(decoded) => {
                        let samples = Self::extract_samples_from_decoded(&decoded)?;
                        let samples_per_channel =
                            samples.len() as u64 / self.format.channels as u64;
                        self.current_position += samples_per_channel;

                        // 🎯 动态更新总样本数（关键修复：AAC等格式的准确样本计数）
                        // 始终使用当前处理进度作为总样本数，确保准确性
                        self.total_samples = self.current_position;

                        Ok(Some(samples))
                    }
                    Err(e) => match e {
                        symphonia::core::errors::Error::DecodeError(_) => {
                            // 跳过解码错误的包，继续处理
                            self.next_chunk()
                        }
                        _ => Err(AudioError::FormatError(format!("解码错误: {e}"))),
                    },
                }
            }
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                Ok(None) // 正常结束
            }
            Err(e) => Err(AudioError::FormatError(format!("读取包错误: {e}"))),
        }
    }

    fn reset(&mut self) -> AudioResult<()> {
        self.format_reader = None;
        self.decoder = None;
        self.track_id = None;
        self.current_position = 0;
        Ok(())
    }

    fn get_chunk_stats(&mut self) -> Option<ChunkSizeStats> {
        // 智能缓冲模式固定启用，总是提供统计信息
        self.chunk_stats.finalize();
        Some(self.chunk_stats.clone())
    }
}
