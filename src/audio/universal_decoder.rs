//! 统一音频解码器
//!
//! 提供统一的音频解码接口，支持多种格式的自动检测和解码
//! 采用插件化架构，便于扩展新格式（如DSD等特殊格式）

use crate::error::{AudioError, AudioResult};
use std::path::Path;

/// 音频格式信息
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub sample_count: u64,
}

impl AudioFormat {
    /// 创建新的音频格式
    pub fn new(sample_rate: u32, channels: u16, bits_per_sample: u16, sample_count: u64) -> Self {
        Self {
            sample_rate,
            channels,
            bits_per_sample,
            sample_count,
        }
    }

    /// 验证格式参数的有效性
    pub fn validate(&self) -> AudioResult<()> {
        if self.sample_rate == 0 {
            return Err(AudioError::FormatError("采样率不能为0".to_string()));
        }
        if self.channels == 0 {
            return Err(AudioError::FormatError("声道数不能为0".to_string()));
        }
        if ![16, 24, 32].contains(&self.bits_per_sample) {
            return Err(AudioError::FormatError(format!(
                "不支持的位深度: {}位",
                self.bits_per_sample
            )));
        }
        Ok(())
    }

    /// 获取文件大小估算（字节）
    pub fn estimated_file_size(&self) -> u64 {
        self.sample_count * self.channels as u64 * (self.bits_per_sample as u64 / 8)
    }

    /// 获取持续时长（秒）
    pub fn duration_seconds(&self) -> f64 {
        self.sample_count as f64 / self.sample_rate as f64
    }
}

/// 流式解码器trait
pub trait StreamingDecoder {
    /// 获取下一个音频块
    fn next_chunk(&mut self) -> AudioResult<Option<Vec<f32>>>;

    /// 获取解码进度 (0.0-1.0)
    fn progress(&self) -> f32;

    /// 获取音频格式信息
    fn format(&self) -> &AudioFormat;

    /// 重置到开头
    fn reset(&mut self) -> AudioResult<()>;

    /// 获取块大小统计信息（可选，仅逐包模式支持）
    fn get_chunk_stats(&mut self) -> Option<ChunkSizeStats> {
        None // 默认不支持
    }
}

/// 格式支持信息
#[derive(Debug, Clone)]
pub struct FormatSupport {
    /// 支持的文件扩展名
    pub extensions: &'static [&'static str],
}

/// 音频解码器trait
pub trait AudioDecoder: Send + Sync {
    /// 获取解码器名称
    fn name(&self) -> &'static str;

    /// 获取支持的格式信息
    fn supported_formats(&self) -> &FormatSupport;

    /// 检测是否能解码指定文件
    fn can_decode(&self, path: &Path) -> bool {
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            self.supported_formats()
                .extensions
                .contains(&ext.to_lowercase().as_str())
        } else {
            false
        }
    }

    /// 探测文件格式（快速，不解码音频数据）
    fn probe_format(&self, path: &Path) -> AudioResult<AudioFormat>;

    /// 创建流式解码器（适用于大文件）
    fn create_streaming(&self, path: &Path) -> AudioResult<Box<dyn StreamingDecoder>>;

    /// 用于类型转换的辅助方法
    fn as_any(&self) -> &dyn std::any::Any;
}

/// PCM解码器 - 处理WAV、FLAC等PCM格式
pub struct PcmDecoder;

impl AudioDecoder for PcmDecoder {
    fn name(&self) -> &'static str {
        "PCM Decoder"
    }

    fn supported_formats(&self) -> &FormatSupport {
        static SUPPORT: FormatSupport = FormatSupport {
            extensions: &["wav", "flac", "alac", "aiff", "au", "caf"],
        };
        &SUPPORT
    }

    fn probe_format(&self, path: &Path) -> AudioResult<AudioFormat> {
        // 使用symphonia探测格式
        self.probe_with_symphonia(path)
    }

    fn create_streaming(&self, path: &Path) -> AudioResult<Box<dyn StreamingDecoder>> {
        // 创建PCM流式解码器（默认非逐包模式）
        Ok(Box::new(PcmStreamingDecoder::new(path)?))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl PcmDecoder {
    /// 创建流式解码器用于批处理计算（统一模式）
    ///
    /// 此方法使用统一的流式收集+批处理计算模式，移除了packet_chunk_mode选项。
    /// 解码器将以合适的块大小收集样本，然后由WindowRmsAnalyzer进行批处理计算。
    pub fn create_streaming_for_batch_processing(
        &self,
        path: &Path,
    ) -> AudioResult<Box<dyn StreamingDecoder>> {
        // 固定使用优化的流式收集模式（原packet_chunk_mode=true的行为）
        Ok(Box::new(PcmStreamingDecoder::new_for_batch_processing(
            path,
        )?))
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
    fn convert_buffer_to_interleaved(
        audio_buf: &symphonia::core::audio::AudioBufferRef,
        samples: &mut Vec<f32>,
    ) -> AudioResult<()> {
        use symphonia::core::audio::{AudioBufferRef, Signal};

        let channel_count = match audio_buf {
            AudioBufferRef::F32(buf) => buf.spec().channels.count(),
            AudioBufferRef::S16(buf) => buf.spec().channels.count(),
            AudioBufferRef::S24(buf) => buf.spec().channels.count(),
            AudioBufferRef::S32(buf) => buf.spec().channels.count(),
            AudioBufferRef::F64(buf) => buf.spec().channels.count(),
            AudioBufferRef::U8(buf) => buf.spec().channels.count(),
            AudioBufferRef::U16(buf) => buf.spec().channels.count(),
            AudioBufferRef::U24(buf) => buf.spec().channels.count(),
            AudioBufferRef::U32(buf) => buf.spec().channels.count(),
            AudioBufferRef::S8(buf) => buf.spec().channels.count(),
        };

        let frame_count = match audio_buf {
            AudioBufferRef::F32(buf) => buf.frames(),
            AudioBufferRef::S16(buf) => buf.frames(),
            AudioBufferRef::S24(buf) => buf.frames(),
            AudioBufferRef::S32(buf) => buf.frames(),
            AudioBufferRef::F64(buf) => buf.frames(),
            AudioBufferRef::U8(buf) => buf.frames(),
            AudioBufferRef::U16(buf) => buf.frames(),
            AudioBufferRef::U24(buf) => buf.frames(),
            AudioBufferRef::U32(buf) => buf.frames(),
            AudioBufferRef::S8(buf) => buf.frames(),
        };

        samples.reserve(channel_count * frame_count);

        // 转换为交错格式
        for frame in 0..frame_count {
            for ch in 0..channel_count {
                let sample_f32 = match audio_buf {
                    AudioBufferRef::F32(buf) => buf.chan(ch)[frame],
                    AudioBufferRef::S16(buf) => (buf.chan(ch)[frame] as f32) / 32768.0,
                    AudioBufferRef::S24(buf) => (buf.chan(ch)[frame].inner() as f32) / 8388608.0,
                    AudioBufferRef::S32(buf) => (buf.chan(ch)[frame] as f64 / 2147483648.0) as f32,
                    AudioBufferRef::F64(buf) => buf.chan(ch)[frame] as f32,
                    AudioBufferRef::U8(buf) => ((buf.chan(ch)[frame] as f32) - 128.0) / 128.0,
                    AudioBufferRef::U16(buf) => ((buf.chan(ch)[frame] as f32) - 32768.0) / 32768.0,
                    AudioBufferRef::U24(buf) => {
                        ((buf.chan(ch)[frame].inner() as f32) - 8388608.0) / 8388608.0
                    }
                    AudioBufferRef::U32(buf) => {
                        (((buf.chan(ch)[frame] as f64) - 2147483648.0) / 2147483648.0) as f32
                    }
                    AudioBufferRef::S8(buf) => (buf.chan(ch)[frame] as f32) / 128.0,
                };

                samples.push(sample_f32);
            }
        }

        Ok(())
    }
}

/// 块大小统计信息
#[derive(Debug, Clone)]
pub struct ChunkSizeStats {
    pub total_chunks: usize,
    pub min_size: usize,
    pub max_size: usize,
    pub mean_size: f64,
    sizes_sum: usize,
}

impl ChunkSizeStats {
    fn new() -> Self {
        Self {
            total_chunks: 0,
            min_size: usize::MAX,
            max_size: 0,
            mean_size: 0.0,
            sizes_sum: 0,
        }
    }

    fn add_chunk(&mut self, size: usize) {
        self.total_chunks += 1;
        self.sizes_sum += size;
        self.min_size = self.min_size.min(size);
        self.max_size = self.max_size.max(size);
    }

    fn finalize(&mut self) {
        if self.total_chunks > 0 {
            self.mean_size = self.sizes_sum as f64 / self.total_chunks as f64;
        }
        // 修复边界情况
        if self.min_size == usize::MAX {
            self.min_size = 0;
        }
    }
}

/// PCM流式解码器
pub struct PcmStreamingDecoder {
    path: std::path::PathBuf,
    format: AudioFormat,
    chunk_size: usize,
    current_position: u64,
    total_samples: u64,

    // 🔥 新增：逐包直通模式开关
    packet_chunk_mode: bool,
    chunk_stats: ChunkSizeStats,

    // symphonia组件
    format_reader: Option<Box<dyn symphonia::core::formats::FormatReader>>,
    decoder: Option<Box<dyn symphonia::core::codecs::Decoder>>,
    track_id: Option<u32>,
}

impl PcmStreamingDecoder {
    /// 创建用于批处理计算的流式解码器（推荐方法）
    ///
    /// 此方法固定使用最优的流式收集模式，适配WindowRmsAnalyzer批处理计算。
    /// 移除了packet_chunk_mode选项，简化架构。
    pub fn new_for_batch_processing<P: AsRef<Path>>(path: P) -> AudioResult<Self> {
        // 固定使用packet_chunk_mode=true的优化模式
        Self::new_with_packet_mode(path, true)
    }

    pub fn new<P: AsRef<Path>>(path: P) -> AudioResult<Self> {
        Self::new_with_packet_mode(path, false)
    }

    pub fn new_with_packet_mode<P: AsRef<Path>>(
        path: P,
        packet_chunk_mode: bool,
    ) -> AudioResult<Self> {
        let path = path.as_ref().to_path_buf();
        let pcm_decoder = PcmDecoder;
        let format = pcm_decoder.probe_format(&path)?;

        // 根据格式优化块大小（在逐包模式下不使用，但保留兼容性）
        let chunk_size = Self::optimize_chunk_size(&format);

        Ok(Self {
            path,
            format: format.clone(),
            chunk_size,
            current_position: 0,
            total_samples: format.sample_count,
            packet_chunk_mode,
            chunk_stats: ChunkSizeStats::new(),
            format_reader: None,
            decoder: None,
            track_id: None,
        })
    }

    /// 获取块大小统计信息（仅在逐包模式下有效）
    pub fn get_chunk_stats(&mut self) -> ChunkSizeStats {
        self.chunk_stats.finalize();
        self.chunk_stats.clone()
    }

    fn optimize_chunk_size(format: &AudioFormat) -> usize {
        // 根据采样率和声道数优化块大小
        let base_size = match format.sample_rate {
            ..=48000 => 8192,
            48001..=96000 => 16384,
            96001..=192000 => 32768,
            _ => 65536,
        };

        // 多声道需要更大的缓冲区
        let channel_multiplier = match format.channels {
            1..=2 => 1,
            3..=8 => 2,
            9..=16 => 3,
            _ => 4,
        };

        base_size * channel_multiplier
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

        Ok(())
    }
}

impl StreamingDecoder for PcmStreamingDecoder {
    fn next_chunk(&mut self) -> AudioResult<Option<Vec<f32>>> {
        use symphonia::core::errors::Error as SymphoniaError;

        // 按需初始化
        if self.format_reader.is_none() {
            self.initialize_symphonia()?;
        }

        let format_reader = self.format_reader.as_mut().unwrap();
        let decoder = self.decoder.as_mut().unwrap();
        let track_id = self.track_id.unwrap();

        if self.packet_chunk_mode {
            // 🔥 逐包直通模式：每次decode一个packet就立即返回
            loop {
                let packet = match format_reader.next_packet() {
                    Ok(packet) => packet,
                    Err(SymphoniaError::ResetRequired) => {
                        decoder.reset();
                        continue;
                    }
                    Err(SymphoniaError::IoError(ref e))
                        if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                    {
                        return Ok(None); // 文件结束
                    }
                    Err(e) => return Err(AudioError::FormatError(format!("读取包失败: {e}"))),
                };

                if packet.track_id() != track_id {
                    continue;
                }

                match decoder.decode(&packet) {
                    Ok(audio_buf) => {
                        let mut packet_samples = Vec::new();
                        PcmDecoder::convert_buffer_to_interleaved(&audio_buf, &mut packet_samples)?;

                        if !packet_samples.is_empty() {
                            // 🔥 记录块大小统计（每声道样本数）
                            let samples_per_channel =
                                packet_samples.len() / self.format.channels as usize;
                            self.chunk_stats.add_chunk(samples_per_channel);

                            // 更新位置
                            self.current_position += samples_per_channel as u64;
                            return Ok(Some(packet_samples));
                        }
                    }
                    Err(SymphoniaError::IoError(ref e))
                        if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                    {
                        return Ok(None);
                    }
                    Err(SymphoniaError::DecodeError(_)) => continue,
                    Err(e) => return Err(AudioError::FormatError(format!("解码失败: {e}"))),
                }
            }
        } else {
            // 🔄 传统模式：累加到块大小阈值
            let mut chunk_samples = Vec::new();

            // 读取直到达到块大小或文件结尾
            while chunk_samples.len() < self.chunk_size {
                let packet = match format_reader.next_packet() {
                    Ok(packet) => packet,
                    Err(SymphoniaError::ResetRequired) => {
                        decoder.reset();
                        continue;
                    }
                    Err(SymphoniaError::IoError(ref e))
                        if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                    {
                        break;
                    }
                    Err(e) => return Err(AudioError::FormatError(format!("读取包失败: {e}"))),
                };

                if packet.track_id() != track_id {
                    continue;
                }

                match decoder.decode(&packet) {
                    Ok(audio_buf) => {
                        PcmDecoder::convert_buffer_to_interleaved(&audio_buf, &mut chunk_samples)?;
                    }
                    Err(SymphoniaError::IoError(ref e))
                        if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                    {
                        break;
                    }
                    Err(SymphoniaError::DecodeError(_)) => continue,
                    Err(e) => return Err(AudioError::FormatError(format!("解码失败: {e}"))),
                }
            }

            if chunk_samples.is_empty() {
                Ok(None)
            } else {
                // 更新位置：基于帧数而不是interleaved samples数
                let frames = chunk_samples.len() as u64 / self.format.channels as u64;
                self.current_position += frames;
                Ok(Some(chunk_samples))
            }
        }
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
        if self.packet_chunk_mode {
            self.chunk_stats.finalize();
            Some(self.chunk_stats.clone())
        } else {
            None
        }
    }
}

/// 统一解码器管理器
pub struct UniversalDecoder {
    decoders: Vec<Box<dyn AudioDecoder>>,
}

impl Default for UniversalDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl UniversalDecoder {
    /// 创建新的统一解码器
    pub fn new() -> Self {
        let decoders: Vec<Box<dyn AudioDecoder>> = vec![
            // 注册PCM解码器
            Box::new(PcmDecoder),
        ];

        Self { decoders }
    }

    /// 添加自定义解码器
    pub fn add_decoder(&mut self, decoder: Box<dyn AudioDecoder>) {
        self.decoders.push(decoder);
    }

    /// 获取能处理指定文件的解码器
    pub fn get_decoder(&self, path: &Path) -> AudioResult<&dyn AudioDecoder> {
        for decoder in &self.decoders {
            if decoder.can_decode(path) {
                return Ok(decoder.as_ref());
            }
        }

        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        Err(AudioError::FormatError(format!("不支持的文件格式: .{ext}")))
    }

    /// 探测文件格式
    pub fn probe_format<P: AsRef<Path>>(&self, path: P) -> AudioResult<AudioFormat> {
        let decoder = self.get_decoder(path.as_ref())?;
        decoder.probe_format(path.as_ref())
    }

    /// 创建流式解码器
    pub fn create_streaming<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> AudioResult<Box<dyn StreamingDecoder>> {
        let decoder = self.get_decoder(path.as_ref())?;
        decoder.create_streaming(path.as_ref())
    }

    /// 创建用于批处理计算的流式解码器（推荐方法）
    ///
    /// 此方法使用统一的流式收集+批处理计算模式，简化了架构设计。
    pub fn create_streaming_for_batch_processing<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> AudioResult<Box<dyn StreamingDecoder>> {
        let decoder = self.get_decoder(path.as_ref())?;
        if let Some(pcm_decoder) = decoder.as_any().downcast_ref::<PcmDecoder>() {
            pcm_decoder.create_streaming_for_batch_processing(path.as_ref())
        } else {
            // 其他解码器使用默认流式模式
            decoder.create_streaming(path.as_ref())
        }
    }

    /// 获取支持的格式列表
    pub fn supported_formats(&self) -> Vec<(&'static str, &FormatSupport)> {
        self.decoders
            .iter()
            .map(|d| (d.name(), d.supported_formats()))
            .collect()
    }
}
