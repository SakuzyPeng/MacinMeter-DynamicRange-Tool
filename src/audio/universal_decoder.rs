//! 统一音频解码器
//!
//! 真正的UniversalDecoder - 直接处理所有音频格式的解码
//! 基于Symphonia提供完整的多格式支持

use crate::error::{self, AudioError, AudioResult};
use crate::processing::SampleConverter;
use std::path::Path;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};

// 重新导出公共接口
pub use super::format::{AudioFormat, FormatSupport};
pub use super::stats::ChunkSizeStats;
pub use super::streaming::StreamingDecoder;

// Opus解码器支持
use super::opus_decoder::SongbirdOpusDecoder;

// 并行解码器状态机
use super::parallel_decoder::DecodingState;

// 内部模块
// (所有错误处理现在内联到方法中)

/// 宏：为包含ProcessorState的StreamingDecoder实现统一的format()和progress()方法
///
/// 消除UniversalStreamProcessor和ParallelUniversalStreamProcessor中的重复代码
macro_rules! impl_streaming_decoder_state_methods {
    () => {
        fn format(&self) -> AudioFormat {
            self.state.get_format()
        }

        fn progress(&self) -> f32 {
            self.state.get_progress()
        }
    };
}

/// 统一音频解码器 - 真正的Universal
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
            // 统一格式支持声明 - 基于Symphonia features + FFmpeg/Songbird 扩展（已验证）
            extensions: &[
                // 无损格式
                "wav", "flac", "aiff", "alac", "m4a", "mp4", // 有损格式
                "mp3", "mp1", "aac", "ogg", "opus",
                // 家庭影院 / 高阶格式（FFmpeg 回退）
                "ac3", "ec3", "eac3", "dts", // DSD
                "dsf", "dff", // 容器格式
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

        // 检查是否为Opus格式，使用专用探测方法
        if let Some(ext) = path.extension().and_then(|s| s.to_str())
            && ext.to_lowercase() == "opus"
        {
            // 暂时创建一个临时解码器来获取格式信息
            // 这不是最优的，但能确保格式探测的一致性
            let temp_decoder = SongbirdOpusDecoder::new(path)?;
            return Ok(temp_decoder.format());
        }

        // 其他格式优先使用Symphonia探测，失败则尝试FFmpeg兜底
        match self.probe_with_symphonia(path) {
            Ok(fmt) => Ok(fmt),
            Err(e) => {
                // 若存在FFmpeg，尝试用FFmpeg探测（兼容容器内E-AC-3/Dolby Atmos等情况）
                if super::ffmpeg_bridge::FFmpegDecoder::is_available() {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "[INFO] Symphonia probe failed, trying FFmpeg probe / Symphonia探测失败，尝试FFmpeg探测"
                    );
                    // 通过创建临时FFmpeg解码器来获取格式（内部会运行ffprobe）
                    let decoder = super::ffmpeg_bridge::FFmpegDecoder::new(path)?;
                    Ok(decoder.format())
                } else {
                    Err(e)
                }
            }
        }
    }

    /// 创建流式解码器（串行模式，BatchPacketReader优化）- 可选项版本
    ///
    /// 允许传入 DSD → PCM 的目标采样率（Hz）。
    pub fn create_streaming_with_options<P: AsRef<Path>>(
        &self,
        path: P,
        dsd_pcm_rate: Option<u32>,
        dsd_gain_db: Option<f32>,
        dsd_filter: Option<String>,
    ) -> AudioResult<Box<dyn StreamingDecoder>> {
        let path = path.as_ref();

        // 检查是否为Opus格式，使用专用解码器
        if let Some(ext) = path.extension().and_then(|s| s.to_str())
            && ext.to_lowercase() == "opus"
        {
            return Ok(Box::new(SongbirdOpusDecoder::new(path)?));
        }

        // 检查是否为Symphonia不支持的格式（需要FFmpeg回退）
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            let ext_lower = ext.to_lowercase();

            // 支持的格式列表：AC-3, E-AC-3, DTS, DSD等
            let ffmpeg_formats = ["ac3", "ec3", "eac3", "dts", "dsf", "dff"];

            if ffmpeg_formats.contains(&ext_lower.as_str()) {
                if super::ffmpeg_bridge::FFmpegDecoder::is_available() {
                    eprintln!(
                        "[INFO] Using FFmpeg decoder for {} format / 使用FFmpeg解码器处理{}格式",
                        ext_lower.to_uppercase(),
                        ext_lower.to_uppercase()
                    );
                    return Ok(Box::new(
                        super::ffmpeg_bridge::FFmpegDecoder::new_with_options(
                            path,
                            dsd_pcm_rate,
                            dsd_gain_db,
                            dsd_filter.clone(),
                        )?,
                    ));
                } else {
                    return Err(AudioError::FormatError(format!(
                        "Format '{ext_lower}' requires FFmpeg, but FFmpeg is not installed / 格式'{ext_lower}'需要FFmpeg，但FFmpeg未安装\n\
                         See installation guide above / 请参考上方安装指南"
                    )));
                }
            }

            // 特例：mp4/m4a 容器内的 E-AC-3/AC-3（含 Atmos）
            // 若 ffprobe 可用且检测到 codec=eac3/ac3，则直接切换到 FFmpeg 解码器
            if (ext_lower == "mp4" || ext_lower == "m4a")
                && super::ffmpeg_bridge::FFmpegDecoder::is_available()
            {
                let ffprobe = if cfg!(target_os = "windows") {
                    "ffprobe.exe"
                } else {
                    "ffprobe"
                };
                if let Ok(out) = std::process::Command::new(ffprobe)
                    .args([
                        "-v",
                        "error",
                        "-select_streams",
                        "a:0",
                        "-show_entries",
                        "stream=codec_name",
                        "-of",
                        "default=noprint_wrappers=1:nokey=1",
                        &path.to_string_lossy(),
                    ])
                    .output()
                    && out.status.success()
                {
                    let codec = String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
                    if codec == "eac3" || codec == "ac3" || codec == "ec-3" {
                        eprintln!(
                            "[INFO] Detected {codec} in MP4/M4A, using FFmpeg / 在MP4/M4A中检测到{codec}，切换FFmpeg"
                        );
                        return Ok(Box::new(
                            super::ffmpeg_bridge::FFmpegDecoder::new_with_options(
                                path,
                                dsd_pcm_rate,
                                dsd_gain_db,
                                dsd_filter.clone(),
                            )?,
                        ));
                    }
                }
            }
        }

        // 尝试使用Symphonia解码器
        match UniversalStreamProcessor::new(path) {
            Ok(processor) => Ok(Box::new(processor)),
            Err(e) => {
                // Symphonia失败，尝试FFmpeg兜底
                if super::ffmpeg_bridge::FFmpegDecoder::is_available() {
                    eprintln!(
                        "[INFO] Symphonia failed, trying FFmpeg fallback / Symphonia失败，尝试FFmpeg兜底"
                    );
                    Ok(Box::new(
                        super::ffmpeg_bridge::FFmpegDecoder::new_with_options(
                            path,
                            dsd_pcm_rate,
                            dsd_gain_db,
                            dsd_filter,
                        )?,
                    ))
                } else {
                    Err(e) // FFmpeg不可用，返回原始错误
                }
            }
        }
    }

    /// 创建流式解码器（串行模式，BatchPacketReader优化）- 兼容旧API
    pub fn create_streaming<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> AudioResult<Box<dyn StreamingDecoder>> {
        self.create_streaming_with_options(path, None, None, None)
    }

    /// 创建并行高性能流式解码器（实验性，攻击解码瓶颈）
    ///
    /// 基于基准测试发现解码是唯一瓶颈的关键洞察，使用有序并行解码架构。
    /// 预期获得3-5倍性能提升，处理速度从115MB/s提升到350-600MB/s。
    ///
    /// 实验性功能：在生产环境使用前请进行充分测试。
    #[allow(clippy::too_many_arguments)]
    pub fn create_streaming_parallel_with_options<P: AsRef<Path>>(
        &self,
        path: P,
        parallel_enabled: bool,
        batch_size: Option<usize>,
        thread_count: Option<usize>,
        dsd_pcm_rate: Option<u32>,
        dsd_gain_db: Option<f32>,
        dsd_filter: Option<String>,
    ) -> AudioResult<Box<dyn StreamingDecoder>> {
        let path = path.as_ref();

        // Opus格式暂不支持并行解码，回退到专用解码器
        if let Some(ext) = path.extension().and_then(|s| s.to_str())
            && ext.to_lowercase() == "opus"
        {
            return Ok(Box::new(SongbirdOpusDecoder::new(path)?));
        }

        // FFmpeg格式无法并行解码（管道限制），使用串行FFmpeg解码器
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            let ext_lower = ext.to_lowercase();
            let ffmpeg_formats = ["ac3", "ec3", "eac3", "dts", "dsf", "dff"];

            if ffmpeg_formats.contains(&ext_lower.as_str()) {
                if super::ffmpeg_bridge::FFmpegDecoder::is_available() {
                    eprintln!(
                        "[INFO] {} format uses FFmpeg (serial only) / {}格式使用FFmpeg（仅串行）",
                        ext_lower.to_uppercase(),
                        ext_lower.to_uppercase()
                    );
                    return Ok(Box::new(
                        super::ffmpeg_bridge::FFmpegDecoder::new_with_options(
                            path,
                            dsd_pcm_rate,
                            dsd_gain_db,
                            dsd_filter.clone(),
                        )?,
                    ));
                } else {
                    return Err(AudioError::FormatError(format!(
                        "Format '{ext_lower}' requires FFmpeg, but FFmpeg is not installed / 格式'{ext_lower}'需要FFmpeg，但FFmpeg未安装"
                    )));
                }
            }

            // 特例：mp4/m4a 容器内的 E-AC-3/AC-3（含 Atmos），强制串行FFmpeg
            if (ext_lower == "mp4" || ext_lower == "m4a")
                && super::ffmpeg_bridge::FFmpegDecoder::is_available()
            {
                let ffprobe = if cfg!(target_os = "windows") {
                    "ffprobe.exe"
                } else {
                    "ffprobe"
                };
                if let Ok(out) = std::process::Command::new(ffprobe)
                    .args([
                        "-v",
                        "error",
                        "-select_streams",
                        "a:0",
                        "-show_entries",
                        "stream=codec_name",
                        "-of",
                        "default=noprint_wrappers=1:nokey=1",
                        &path.to_string_lossy(),
                    ])
                    .output()
                    && out.status.success()
                {
                    let codec = String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
                    if codec == "eac3" || codec == "ac3" || codec == "ec-3" {
                        eprintln!(
                            "[INFO] {} in MP4/M4A, falling back to serial FFmpeg / MP4/M4A中检测到{}，回退到串行FFmpeg",
                            codec.to_uppercase(),
                            codec.to_uppercase()
                        );
                        return Ok(Box::new(
                            super::ffmpeg_bridge::FFmpegDecoder::new_with_options(
                                path,
                                dsd_pcm_rate,
                                dsd_gain_db,
                                dsd_filter.clone(),
                            )?,
                        ));
                    }
                }
            }
        }

        // 有状态编码格式必须使用串行解码
        // MP3/AAC/OGG每个包依赖前一个包的解码器状态，并行解码会导致样本错误
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            let ext_lower = ext.to_lowercase();
            if ext_lower == "mp3" || ext_lower == "aac" || ext_lower == "m4a" || ext_lower == "ogg"
            {
                #[cfg(debug_assertions)]
                eprintln!(
                    "[WARNING] {} Format detected - using serial decoder (stateful encoding requires decoder context) / 格式检测到，使用串行解码器（有状态编码需要保持解码器上下文）",
                    ext_lower.to_uppercase()
                );

                return Ok(Box::new(UniversalStreamProcessor::new(path)?));
            }
        }

        // 创建并行流式处理器（支持FLAC、WAV、AAC等格式）
        use crate::tools::constants::decoder_performance::*;

        let parallel_processor = ParallelUniversalStreamProcessor::new(path)?.with_parallel_config(
            parallel_enabled,
            batch_size.unwrap_or(PARALLEL_DECODE_BATCH_SIZE),
            thread_count.unwrap_or(PARALLEL_DECODE_THREADS),
        );

        Ok(Box::new(parallel_processor))
    }

    /// 兼容旧API：保留原有签名，内部委托到 with_options 版本
    pub fn create_streaming_parallel<P: AsRef<Path>>(
        &self,
        path: P,
        parallel_enabled: bool,
        batch_size: Option<usize>,
        thread_count: Option<usize>,
    ) -> AudioResult<Box<dyn StreamingDecoder>> {
        self.create_streaming_parallel_with_options(
            path,
            parallel_enabled,
            batch_size,
            thread_count,
            None,
            None,
            None,
        )
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
            .map_err(|e| error::format_error("Failed to probe format / 格式探测失败", e))?;

        let format_reader = probed.format;

        let track = format_reader
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or_else(|| {
                AudioError::FormatError(format!("未找到音频轨道: 文件 {}", path.display()))
            })?;

        let codec_params = &track.codec_params;
        let sample_rate = codec_params.sample_rate.unwrap_or(44100);
        let channels = self.detect_channel_count(codec_params)?;
        let bits_per_sample = self.detect_bit_depth(codec_params);

        // 获取样本数，支持多种方式
        let sample_count = self.detect_sample_count(codec_params);

        // 获取真实的编解码器类型
        let mut format = AudioFormat::with_codec(
            sample_rate,
            channels,
            bits_per_sample,
            sample_count,
            codec_params.codec,
        );
        // 如果容器提供了布局或通道掩码信息，记录用于后续 LFE 识别
        if codec_params.channel_layout.is_some() {
            format.mark_has_channel_layout();
        }
        if let Some(ch_mask) = codec_params.channels {
            use symphonia::core::audio::Channels as Ch;
            let raw = ch_mask.bits();
            let mut lfe_indices = Vec::new();

            let push_index = |indices: &mut Vec<usize>, flag: Ch, raw_bits: u64, mask: Ch| {
                if ch_mask.contains(flag) {
                    let bit = mask.bits() as u64;
                    if bit > 0u64 {
                        let lower = raw_bits & (bit - 1u64);
                        indices.push(lower.count_ones() as usize);
                    }
                }
            };

            push_index(&mut lfe_indices, Ch::LFE1, raw as u64, Ch::LFE1);
            // 支持第二路 LFE（若存在）
            push_index(&mut lfe_indices, Ch::LFE2, raw as u64, Ch::LFE2);

            if !lfe_indices.is_empty() {
                format.set_lfe_indices(lfe_indices);
            } else {
                // 至少有通道掩码，仍标记存在布局线索
                format.mark_has_channel_layout();
            }
        }

        // 若仍未获得 LFE 信息，做两种兼容型回退：
        // 1) WAV: 直接解析 WAVEFORMATEXTENSIBLE 的 dwChannelMask（避免依赖外部工具）
        // 2) FLAC: 按 FLAC 规范的通道分配，在 5.1/7.1 中 LFE 位于 index=3（0-based）
        if format.lfe_indices.is_empty() && format.channels as usize >= 3 {
            // 回退 1：WAV 容器解析
            if path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("wav"))
                .unwrap_or(false)
                && let Ok(Some(mask)) = parse_wav_channel_mask(path)
            {
                // WAVEFORMATEXTENSIBLE 规定交错顺序按掩码从低位到高位排列
                const SPEAKER_LOW_FREQUENCY: u32 = 0x0008;
                if (mask & SPEAKER_LOW_FREQUENCY) != 0 {
                    let bit = SPEAKER_LOW_FREQUENCY as u64;
                    let raw_bits = mask as u64;
                    let lower = raw_bits & (bit - 1);
                    let idx = lower.count_ones() as usize;
                    format.set_lfe_indices(vec![idx]);
                }
            }

            // 回退 2：FLAC 规范的固定分配（仅当仍无 LFE 索引时）
            if format.lfe_indices.is_empty()
                && codec_params.codec == symphonia::core::codecs::CODEC_TYPE_FLAC
            {
                let ch = format.channels as usize;
                if ch == 6 || ch == 8 {
                    // FLAC 5.1/7.1 的标准分配：L, R, C, LFE, ... => LFE 在 index 3
                    format.set_lfe_indices(vec![3]);
                }
            }
        }
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
    ///
    /// 多声道处理策略：
    /// - 3+声道文件：此处默认返回2（立体声），但DR计算器（上层）会验证并拒绝处理
    /// - 这样设计确保格式探测阶段不会失败，由专业的处理层负责声道数验证
    /// - 仅支持1-2声道是DR计算的技术约束，非格式探测的限制
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
                _ => {
                    // 其他布局（如5.1、7.1）默认为立体声
                    // 上层处理会检测实际声道数并拒绝 >2 声道的文件
                    2
                }
            };
            return Ok(channel_count);
        }

        // 如果都失败，使用默认值（通常音频文件是立体声）
        // 实际声道数会在解码阶段被准确检测
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

/// 解析 WAV (RIFF/WAVE) 的 WAVEFORMATEXTENSIBLE，提取 dwChannelMask。
/// 返回 Ok(Some(mask)) 表示成功解析；Ok(None) 表示不是 extensible 或未找到；Err 表示 I/O 错误。
fn parse_wav_channel_mask(path: &Path) -> std::io::Result<Option<u32>> {
    let mut f = File::open(path)?;

    // 读取 RIFF 头 (12 字节)
    let mut header = [0u8; 12];
    if f.read_exact(&mut header).is_err() {
        return Ok(None);
    }
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Ok(None);
    }

    // 遍历 chunk，直到找到 "fmt "
    loop {
        let mut chunk_hdr = [0u8; 8];
        if f.read_exact(&mut chunk_hdr).is_err() {
            return Ok(None);
        }
        let chunk_id = &chunk_hdr[0..4];
        let chunk_size =
            u32::from_le_bytes([chunk_hdr[4], chunk_hdr[5], chunk_hdr[6], chunk_hdr[7]]);

        if chunk_id == b"fmt " {
            // 读取 fmt chunk 内容
            let mut buf = vec![0u8; chunk_size as usize];
            f.read_exact(&mut buf)?;

            // WAVEFORMATEX 基本长度 16；WAVEFORMATEXTENSIBLE: wFormatTag=0xFFFE 且 cbSize>=22
            if buf.len() < 18 {
                return Ok(None);
            }
            let w_format_tag = u16::from_le_bytes([buf[0], buf[1]]);
            // nChannels(2) @2, nSamplesPerSec(4) @4, nAvgBytesPerSec(4) @8, nBlockAlign(2) @12,
            // wBitsPerSample(2) @14, cbSize(2) @16
            let cb_size = u16::from_le_bytes([buf[16], buf[17]]);
            if w_format_tag != 0xFFFE || cb_size < 22 {
                return Ok(None);
            }
            // 有效位深(2) @18, dwChannelMask(4) @20, SubFormat(16) @24
            if buf.len() < 20 + 2 + 4 {
                return Ok(None);
            }
            let mask_off = 20;
            let mask = u32::from_le_bytes([
                buf[mask_off],
                buf[mask_off + 1],
                buf[mask_off + 2],
                buf[mask_off + 3],
            ]);
            return Ok(Some(mask));
        } else {
            // 跳过该 chunk（对齐到偶数字节）
            let skip = chunk_size as u64 + (chunk_size as u64 % 2);
            f.seek(SeekFrom::Current(skip as i64))?;
        }
    }
}

/// 批量包预读器 - I/O性能优化核心
///
/// 通过批量预读减少系统调用次数，可减少约99%的I/O系统调用
/// 内存开销约1-2MB，换取显著的整体性能提升
struct BatchPacketReader {
    format_reader: Box<dyn symphonia::core::formats::FormatReader>,
    packet_buffer: std::collections::VecDeque<symphonia::core::formats::Packet>,

    // 性能调优参数（见 constants::decoder_performance）
    batch_size: usize,         // 每次预读包数
    prefetch_threshold: usize, // 触发预读的阈值

    // 性能统计
    total_reads: usize,   // 总预读次数
    total_packets: usize, // 总处理包数
}

impl BatchPacketReader {
    /// 创建批量包预读器，使用优化的默认参数
    fn new(format_reader: Box<dyn symphonia::core::formats::FormatReader>) -> Self {
        use crate::tools::constants::decoder_performance::*;

        Self {
            format_reader,
            packet_buffer: std::collections::VecDeque::with_capacity(BATCH_PACKET_SIZE),
            batch_size: BATCH_PACKET_SIZE,
            prefetch_threshold: PREFETCH_THRESHOLD,
            total_reads: 0,
            total_packets: 0,
        }
    }

    /// 智能预读：当缓冲区不足时批量读取包
    ///
    /// 这是性能优化的核心：将频繁的单次I/O调用合并为批量操作
    fn ensure_buffered(&mut self) -> AudioResult<()> {
        // 仅在缓冲区不足时触发预读，避免过度缓冲
        if self.packet_buffer.len() < self.prefetch_threshold {
            self.total_reads += 1;

            // 批量预读：一次读取多个包，大幅减少系统调用
            for _ in 0..self.batch_size {
                match self.format_reader.next_packet() {
                    Ok(packet) => {
                        self.packet_buffer.push_back(packet);
                        self.total_packets += 1;
                    }
                    Err(symphonia::core::errors::Error::IoError(e))
                        if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                    {
                        break; // 正常EOF，停止预读
                    }
                    Err(e) => {
                        return Err(error::format_error(
                            "Failed to prefetch packets / 预读包失败",
                            e,
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// 零系统调用的包获取：从缓冲区直接获取
    ///
    /// 替代原来的format_reader.next_packet()，消除大部分I/O等待
    fn next_packet(&mut self) -> AudioResult<Option<symphonia::core::formats::Packet>> {
        // 智能缓冲管理：确保缓冲区有足够数据
        self.ensure_buffered()?;

        // 从缓冲区获取包，无I/O阻塞
        Ok(self.packet_buffer.pop_front())
    }
}

/// 共同状态 - 消除串行和并行的重复字段
///
/// 提取60%的共同字段，避免代码重复
struct ProcessorState {
    path: std::path::PathBuf,
    format: AudioFormat,
    current_position: u64,
    total_samples: u64,
    chunk_stats: ChunkSizeStats,
    sample_converter: SampleConverter,
    track_id: Option<u32>,
    /// 跳过的损坏包总数（用于容错处理统计）
    skipped_packets: usize,
    /// 连续解码错误计数（成功时重置，用于检测严重损坏）
    consecutive_errors: usize,
}

impl ProcessorState {
    fn new(path: std::path::PathBuf, format: AudioFormat) -> Self {
        Self {
            path,
            format: format.clone(),
            current_position: 0,
            total_samples: format.sample_count,
            chunk_stats: ChunkSizeStats::new(),
            sample_converter: SampleConverter::new(),
            track_id: None,
            skipped_packets: 0,
            consecutive_errors: 0,
        }
    }

    /// 获取当前格式（动态更新样本数）
    fn get_format(&self) -> AudioFormat {
        let mut current_format = self.format.clone();
        current_format.update_sample_count(self.total_samples);
        // 如果跳过了损坏包，标记为部分分析
        if self.skipped_packets > 0 {
            current_format.mark_as_partial(self.skipped_packets);
        }
        current_format
    }

    /// 获取进度
    fn get_progress(&self) -> f32 {
        if self.total_samples == 0 {
            0.0
        } else {
            (self.current_position as f32) / (self.total_samples as f32)
        }
    }

    /// 更新位置和样本数
    fn update_position(&mut self, samples: &[f32], channels: u16) {
        let samples_per_channel = samples.len() as u64 / channels as u64;
        self.current_position += samples_per_channel;
        self.total_samples = self.current_position; // 动态更新
    }

    /// 重置状态
    fn reset(&mut self) {
        self.current_position = 0;
        self.track_id = None;
    }

    /// 获取统计信息
    fn get_stats(&mut self) -> ChunkSizeStats {
        self.chunk_stats.finalize();
        self.chunk_stats.clone()
    }
}

/// 统一流式处理器 - 串行优化版本
///
/// 使用BatchPacketReader进行I/O优化，适合单线程场景
pub struct UniversalStreamProcessor {
    state: ProcessorState,

    // 串行专用组件
    batch_packet_reader: Option<BatchPacketReader>,
    decoder: Option<Box<dyn symphonia::core::codecs::Decoder>>,
}

impl UniversalStreamProcessor {
    /// 创建统一流式处理器（串行模式）
    ///
    /// 固定启用智能缓冲流式处理，遵循"无条件高性能原则"。
    /// foobar2000-plugin分支专用，提供最优的流式处理性能。
    pub fn new<P: AsRef<Path>>(path: P) -> AudioResult<Self> {
        let path = path.as_ref().to_path_buf();
        let decoder = UniversalDecoder::new();
        let format = decoder.probe_format(&path)?;

        Ok(Self {
            state: ProcessorState::new(path, format),
            batch_packet_reader: None, // 延迟初始化
            decoder: None,
        })
    }

    fn initialize_symphonia(&mut self) -> AudioResult<()> {
        use symphonia::core::codecs::DecoderOptions;
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;
        use symphonia::core::probe::Hint;

        let file = std::fs::File::open(&self.state.path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(extension) = self.state.path.extension() {
            hint.with_extension(&extension.to_string_lossy());
        }

        let meta_opts = MetadataOptions::default();
        let fmt_opts = FormatOptions::default();

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &fmt_opts, &meta_opts)
            .map_err(|e| error::format_error("Failed to create decoder / 创建解码器失败", e))?;

        let format_reader = probed.format;

        let track = format_reader
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or_else(|| {
                AudioError::FormatError(format!(
                    "未找到音频轨道: 文件 {}",
                    self.state.path.display()
                ))
            })?;

        let track_id = track.id;
        let codec_params = &track.codec_params;

        let decoder_opts = DecoderOptions::default();
        let decoder = symphonia::default::get_codecs()
            .make(codec_params, &decoder_opts)
            .map_err(|e| error::format_error("Failed to create decoder / 创建解码器失败", e))?;

        // 创建批量包预读器：核心I/O优化
        self.batch_packet_reader = Some(BatchPacketReader::new(format_reader));
        self.decoder = Some(decoder);
        self.state.track_id = Some(track_id);

        Ok(())
    }

    /// 从解码的音频缓冲区提取样本
    fn extract_samples_from_decoded(
        sample_converter: &SampleConverter,
        decoded: &symphonia::core::audio::AudioBufferRef,
    ) -> AudioResult<Vec<f32>> {
        let mut samples = Vec::new();
        Self::convert_buffer_to_interleaved_with_simd(sample_converter, decoded, &mut samples)?;
        Ok(samples)
    }

    /// 转换symphonia缓冲区为交错格式 (SIMD优化)
    ///
    /// 优化#11：使用processing层的统一转换函数，消除重复代码
    fn convert_buffer_to_interleaved_with_simd(
        sample_converter: &SampleConverter,
        audio_buf: &symphonia::core::audio::AudioBufferRef,
        samples: &mut Vec<f32>,
    ) -> AudioResult<()> {
        // 使用processing层的统一公共函数
        sample_converter.convert_buffer_to_interleaved(audio_buf, samples)
    }
}

impl StreamingDecoder for UniversalStreamProcessor {
    // 使用宏实现通用方法（format和progress）
    impl_streaming_decoder_state_methods!();

    fn next_chunk(&mut self) -> AudioResult<Option<Vec<f32>>> {
        if self.batch_packet_reader.is_none() {
            self.initialize_symphonia()?;
        }

        let batch_reader = self
            .batch_packet_reader
            .as_mut()
            .expect("batch_packet_reader必须已初始化，initialize_symphonia()已设置");
        let decoder = self
            .decoder
            .as_mut()
            .expect("decoder必须已初始化，initialize_symphonia()已设置");
        let track_id = self
            .state
            .track_id
            .expect("track_id必须已初始化，initialize_symphonia()已设置");

        // 使用循环替代递归，避免栈溢出风险
        loop {
            // 使用批量预读器获取包：大幅减少I/O系统调用
            match batch_reader.next_packet()? {
                Some(packet) => {
                    if packet.track_id() != track_id {
                        continue; // 跳过非目标轨道的包，继续读取下一个
                    }

                    // 记录包统计信息
                    self.state.chunk_stats.add_chunk(packet.dur() as usize);

                    // 解码音频包
                    match decoder.decode(&packet) {
                        Ok(decoded) => {
                            let samples = Self::extract_samples_from_decoded(
                                &self.state.sample_converter,
                                &decoded,
                            )?;

                            // 成功解码，重置连续错误计数
                            self.state.consecutive_errors = 0;

                            // 更新位置和样本数
                            self.state
                                .update_position(&samples, self.state.format.channels);

                            return Ok(Some(samples));
                        }
                        Err(e) => match e {
                            symphonia::core::errors::Error::DecodeError(_) => {
                                // 容错处理：跳过解码错误的包，继续处理
                                self.state.skipped_packets += 1;
                                self.state.consecutive_errors += 1;

                                // 安全检查：连续错误过多表示文件严重损坏
                                const MAX_CONSECUTIVE_ERRORS: usize = 100;
                                if self.state.consecutive_errors > MAX_CONSECUTIVE_ERRORS {
                                    return Err(error::decoding_error(
                                        "Too many consecutive decode failures, file may be corrupted / 连续解码失败过多，文件可能已损坏",
                                        format!(
                                            "Consecutive failures: {failures} times, skipped {skipped} packets in total / 连续失败{failures}次，总共跳过{skipped}个包",
                                            failures = self.state.consecutive_errors,
                                            skipped = self.state.skipped_packets
                                        ),
                                    ));
                                }

                                continue; // 继续处理下一个包
                            }
                            _ => {
                                return Err(error::decoding_error(
                                    "Audio packet decoding failed / 音频包解码失败",
                                    e,
                                ));
                            }
                        },
                    }
                }
                None => {
                    // 批量预读器已到达文件末尾
                    return Ok(None);
                }
            }
        }
    }

    fn reset(&mut self) -> AudioResult<()> {
        self.batch_packet_reader = None;
        self.decoder = None;
        self.state.reset();
        Ok(())
    }

    fn get_chunk_stats(&mut self) -> Option<ChunkSizeStats> {
        Some(self.state.get_stats())
    }
}

/// 并行统一流式处理器 - 攻击解码瓶颈的高性能版本
///
/// 基于基准测试发现解码是唯一瓶颈的关键洞察，使用有序并行解码架构
/// 预期获得3-5倍性能提升，处理速度从115MB/s提升到350-600MB/s
pub struct ParallelUniversalStreamProcessor {
    state: ProcessorState,

    // 并行专用组件
    parallel_decoder: Option<super::parallel_decoder::OrderedParallelDecoder>,
    format_reader: Option<Box<dyn symphonia::core::formats::FormatReader>>,

    // 并行优化配置
    parallel_enabled: bool,   // 是否启用并行解码
    batch_size: usize,        // 批量解码包数
    thread_count: usize,      // 并行线程数
    processed_packets: usize, // 已处理包数量

    // Flushing状态样本缓存
    drained_samples: Option<std::collections::VecDeque<Vec<f32>>>, // 缓存drain_all_samples()的结果
                                                                   // 使用VecDeque以便pop_front()直接移动数据，避免额外克隆
}

impl ParallelUniversalStreamProcessor {
    /// 创建并行流式处理器
    pub fn new<P: AsRef<Path>>(path: P) -> AudioResult<Self> {
        use crate::tools::constants::decoder_performance::*;

        let path = path.as_ref().to_path_buf();
        let decoder = UniversalDecoder::new();
        let format = decoder.probe_format(&path)?;

        Ok(Self {
            state: ProcessorState::new(path, format),
            parallel_decoder: None,
            format_reader: None,
            parallel_enabled: true, // 默认启用并行解码
            batch_size: PARALLEL_DECODE_BATCH_SIZE,
            thread_count: PARALLEL_DECODE_THREADS,
            processed_packets: 0,
            drained_samples: None,
        })
    }

    /// 配置并行解码参数
    pub fn with_parallel_config(
        mut self,
        enabled: bool,
        batch_size: usize,
        thread_count: usize,
    ) -> Self {
        self.parallel_enabled = enabled;
        self.batch_size = batch_size;
        self.thread_count = thread_count;
        self
    }

    /// 初始化并行解码器
    fn initialize_parallel(&mut self) -> AudioResult<()> {
        if self.format_reader.is_some() {
            return Ok(()); // 已初始化
        }

        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;
        use symphonia::core::probe::Hint;

        let file = std::fs::File::open(&self.state.path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(extension) = self.state.path.extension() {
            hint.with_extension(&extension.to_string_lossy());
        }

        let meta_opts = MetadataOptions::default();
        let fmt_opts = FormatOptions::default();

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &fmt_opts, &meta_opts)
            .map_err(|e| {
                error::format_error("Failed to probe parallel decoder / 并行解码器探测失败", e)
            })?;

        let format_reader = probed.format;

        // 找到音频轨道
        let track = format_reader
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or_else(|| {
                AudioError::FormatError(format!(
                    "未找到音频轨道: 文件 {} (并行解码器)",
                    self.state.path.display()
                ))
            })?;

        let track_id = track.id;
        let codec_params = track.codec_params.clone();

        // 创建有序并行解码器（带SIMD优化）
        let parallel_decoder = if self.parallel_enabled {
            super::parallel_decoder::OrderedParallelDecoder::new(
                codec_params.clone(),
                self.state.sample_converter.clone(),
            )
            .with_config(self.batch_size, self.thread_count)
        } else {
            super::parallel_decoder::OrderedParallelDecoder::new(
                codec_params,
                self.state.sample_converter.clone(),
            )
            .with_config(1, 1) // 禁用并行：单包单线程（等效串行）
        };

        self.format_reader = Some(format_reader);
        self.parallel_decoder = Some(parallel_decoder);
        self.state.track_id = Some(track_id);

        Ok(())
    }

    /// 处理一批包并返回下一个可用样本
    fn process_packets_batch(&mut self, batch_size: usize) -> AudioResult<()> {
        let format_reader = self
            .format_reader
            .as_mut()
            .expect("format_reader必须已初始化，initialize_parallel_symphonia()已设置");
        let parallel_decoder = self
            .parallel_decoder
            .as_mut()
            .expect("parallel_decoder必须已初始化，initialize_parallel_symphonia()已设置");
        let target_track_id = self
            .state
            .track_id
            .expect("track_id必须已初始化，initialize_parallel_symphonia()已设置");

        // 批量读取包并提交给并行解码器
        let mut packets_added = 0;
        while packets_added < batch_size {
            match format_reader.next_packet() {
                Ok(packet) => {
                    // 只处理目标轨道的包
                    if packet.track_id() == target_track_id {
                        self.state.chunk_stats.add_chunk(packet.dur() as usize);
                        parallel_decoder.add_packet(packet)?;
                        packets_added += 1;
                        self.processed_packets += 1;
                    }
                    // 其他轨道的包跳过，不计入批次
                }
                Err(symphonia::core::errors::Error::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    // 处理文件末尾剩余的包
                    parallel_decoder.flush_remaining()?;
                    break;
                }
                Err(e) => {
                    return Err(error::format_error(
                        "Failed to read packet in parallel mode / 并行读包失败",
                        e,
                    ));
                }
            }
        }

        Ok(())
    }

    /// 同步跳过包计数（从并行解码器到ProcessorState）
    fn sync_skipped_packets(&mut self) {
        if let Some(decoder) = &self.parallel_decoder {
            self.state.skipped_packets = decoder.get_skipped_packets();
        }
    }
}

impl StreamingDecoder for ParallelUniversalStreamProcessor {
    // 使用宏实现通用方法（format和progress）
    impl_streaming_decoder_state_methods!();

    /// 并行解码的核心方法 - 三阶段状态机驱动
    fn next_chunk(&mut self) -> AudioResult<Option<Vec<f32>>> {
        // 延迟初始化：首次调用时设置并行解码器
        if self.parallel_decoder.is_none() {
            self.initialize_parallel()?;
        }

        // 使用循环替代递归，处理状态切换
        loop {
            // 获取当前状态
            let current_state = self
                .parallel_decoder
                .as_ref()
                .expect("parallel_decoder必须已初始化")
                .get_state();

            // 状态机驱动
            match current_state {
                DecodingState::Decoding => {
                    // 尝试获取已解码样本
                    match self
                        .parallel_decoder
                        .as_mut()
                        .expect("parallel_decoder必须已初始化")
                        .next_samples()
                    {
                        Some(samples) if !samples.is_empty() => {
                            self.state
                                .update_position(&samples, self.state.format.channels);
                            self.sync_skipped_packets();
                            return Ok(Some(samples));
                        }
                        _ => {}
                    }

                    // 没有样本，读取更多包
                    let batch_size = self.batch_size;
                    self.process_packets_batch(batch_size)?;

                    // 等待后台线程解码，最多等待100ms
                    const MAX_WAIT_ATTEMPTS: usize = 100;
                    const WAIT_INTERVAL_MS: u64 = 1;

                    for _attempt in 0..MAX_WAIT_ATTEMPTS {
                        match self
                            .parallel_decoder
                            .as_mut()
                            .expect("parallel_decoder必须已初始化")
                            .next_samples()
                        {
                            Some(samples) if !samples.is_empty() => {
                                self.state
                                    .update_position(&samples, self.state.format.channels);
                                self.sync_skipped_packets();
                                return Ok(Some(samples));
                            }
                            _ => {}
                        }
                        std::thread::sleep(std::time::Duration::from_millis(WAIT_INTERVAL_MS));
                    }

                    // 等待超时，检查状态是否已切换到Flushing（process_packets_batch遇到EOF）
                    let new_state = self
                        .parallel_decoder
                        .as_ref()
                        .expect("parallel_decoder必须已初始化")
                        .get_state();

                    if new_state == DecodingState::Flushing {
                        // 状态已切换，循环继续进入Flushing分支
                        continue;
                    }

                    // 仍在Decoding，暂无样本
                    return Ok(None);
                }

                DecodingState::Flushing => {
                    // EOF已到，drain所有剩余样本
                    // 首次进入Flushing时拉取全部剩余批次，并转为VecDeque方便逐批弹出
                    if self.drained_samples.is_none() {
                        let remaining = self
                            .parallel_decoder
                            .as_mut()
                            .expect("parallel_decoder必须已初始化")
                            .drain_all_samples();
                        self.drained_samples = Some(std::collections::VecDeque::from(remaining));
                    }

                    // 逐批移动样本（不clone），减少多余的内存分配/释放
                    if let Some(ref mut samples_batches) = self.drained_samples {
                        if let Some(samples) = samples_batches.pop_front() {
                            if !samples.is_empty() {
                                self.state
                                    .update_position(&samples, self.state.format.channels);
                                self.sync_skipped_packets();
                                return Ok(Some(samples));
                            }
                        } else {
                            // 所有批次已消费完，切换到Completed状态
                            self.parallel_decoder
                                .as_mut()
                                .unwrap()
                                .set_state(DecodingState::Completed);
                        }
                    }

                    // 所有样本已消费完
                    self.sync_skipped_packets();
                    return Ok(None);
                }

                DecodingState::Completed => {
                    // 真正的EOF
                    return Ok(None);
                }
            }
        }
    }

    fn reset(&mut self) -> AudioResult<()> {
        self.format_reader = None;
        self.parallel_decoder = None;
        self.state.reset();
        self.processed_packets = 0;
        self.drained_samples = None;
        Ok(())
    }

    fn get_chunk_stats(&mut self) -> Option<ChunkSizeStats> {
        Some(self.state.get_stats())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_default_constructor() {
        let decoder = UniversalDecoder;
        assert!(
            !decoder.supported_formats().extensions.is_empty(),
            "默认构造函数应创建有效的解码器"
        );
    }

    #[test]
    fn test_supported_formats() {
        let decoder = UniversalDecoder::new();
        let formats = decoder.supported_formats();

        // 验证支持主要格式
        let expected_formats = [
            "wav", "flac", "aiff", "alac", "m4a", "mp4", "mp3", "mp1", "aac", "ogg", "opus", "ac3",
            "ec3", "eac3", "dts", "dsf", "dff", "mkv", "webm",
        ];

        for format in &expected_formats {
            assert!(formats.extensions.contains(format), "应支持格式: {format}");
        }

        // 验证总数合理（至少11种格式）
        assert!(
            formats.extensions.len() >= expected_formats.len(),
            "支持的音频格式数量应不少于{}",
            expected_formats.len()
        );
    }

    #[test]
    fn test_can_decode() {
        let decoder = UniversalDecoder::new();

        // 支持的格式
        let supported_cases = vec![
            ("test.wav", true),
            ("test.flac", true),
            ("test.mp3", true),
            ("test.aac", true),
            ("test.m4a", true),
            ("test.mp4", true),
            ("test.opus", true),
            ("test.ac3", true),
            ("test.ec3", true),
            ("test.dts", true),
            ("test.dsf", true),
            ("test.dff", true),
            ("TEST.WAV", true), // 大小写不敏感
            ("path/to/test.flac", true),
        ];

        for (path_str, expected) in supported_cases {
            let path = PathBuf::from(path_str);
            assert_eq!(
                decoder.can_decode(&path),
                expected,
                "路径 {path_str} 的检测结果应为 {expected}"
            );
        }

        // 不支持的格式
        let unsupported_cases = vec![
            ("test.txt", false),
            ("test.pdf", false),
            ("test", false), // 无扩展名
            ("", false),     // 空路径
        ];

        for (path_str, expected) in unsupported_cases {
            let path = PathBuf::from(path_str);
            assert_eq!(
                decoder.can_decode(&path),
                expected,
                "路径 {path_str} 的检测结果应为 {expected}"
            );
        }
    }

    #[test]
    fn test_batch_packet_reader_creation() {
        use crate::tools::constants::decoder_performance::*;

        // 测试BatchPacketReader的创建和基本参数
        // 注意：这个测试需要实际的format_reader，所以我们通过间接方式验证
        // BatchPacketReader的存在性和配置

        // 验证默认配置值与常量定义一致
        assert_eq!(BATCH_PACKET_SIZE, 64, "批量大小应为64");
        assert_eq!(PREFETCH_THRESHOLD, 20, "预读阈值应为20");
    }

    #[test]
    fn test_processor_state_creation() {
        let path = PathBuf::from("test.wav");
        let format = AudioFormat::new(44100, 2, 16, 100000);

        let state = ProcessorState::new(path.clone(), format.clone());

        assert_eq!(state.path, path);
        assert_eq!(state.format.sample_rate, 44100);
        assert_eq!(state.format.channels, 2);
        assert_eq!(state.current_position, 0);
        assert_eq!(state.total_samples, 100000);
        assert_eq!(state.skipped_packets, 0);
    }

    #[test]
    fn test_processor_state_progress() {
        let path = PathBuf::from("test.flac");
        let format = AudioFormat::new(48000, 2, 24, 480000);
        let mut state = ProcessorState::new(path, format);

        // 初始进度应为0
        assert_eq!(state.get_progress(), 0.0);

        // 模拟处理进度
        state.current_position = 240000; // 50%
        assert!((state.get_progress() - 0.5).abs() < 0.001);

        state.current_position = 480000; // 100%
        assert!((state.get_progress() - 1.0).abs() < 0.001);

        // 边界情况：total_samples为0
        state.total_samples = 0;
        assert_eq!(state.get_progress(), 0.0);
    }

    #[test]
    fn test_processor_state_position_update() {
        let path = PathBuf::from("test.wav");
        let format = AudioFormat::new(44100, 2, 16, 0);
        let mut state = ProcessorState::new(path, format);

        // 双声道样本：1000个样本 = 500帧
        let samples = vec![0.0f32; 1000];
        state.update_position(&samples, 2);

        assert_eq!(state.current_position, 500);
        assert_eq!(state.total_samples, 500);

        // 继续更新
        state.update_position(&samples, 2);
        assert_eq!(state.current_position, 1000);
        assert_eq!(state.total_samples, 1000);

        // 单声道样本
        let mono_samples = vec![0.0f32; 100];
        state.update_position(&mono_samples, 1);
        assert_eq!(state.current_position, 1100);
    }

    #[test]
    fn test_processor_state_format_with_skipped_packets() {
        let path = PathBuf::from("test.mp3");
        let format = AudioFormat::new(44100, 2, 16, 100000);
        let mut state = ProcessorState::new(path, format);

        // 正常情况：无跳过包
        let current_format = state.get_format();
        assert_eq!(current_format.sample_count, 100000);

        // 模拟跳过包
        state.skipped_packets = 5;
        state.total_samples = 95000;

        let updated_format = state.get_format();
        assert_eq!(updated_format.sample_count, 95000);
    }

    #[test]
    fn test_processor_state_reset() {
        let path = PathBuf::from("test.aac");
        let format = AudioFormat::new(48000, 2, 16, 100000);
        let mut state = ProcessorState::new(path, format);

        // 修改状态
        state.current_position = 50000;
        state.track_id = Some(1);
        state.skipped_packets = 3;

        // 重置
        state.reset();

        assert_eq!(state.current_position, 0);
        assert_eq!(state.track_id, None);
        // 注意：reset不清零skipped_packets（需要保留错误信息）
    }

    #[test]
    fn test_parallel_config() {
        use crate::tools::constants::decoder_performance::*;

        let path = PathBuf::from("test.flac");
        let format = AudioFormat::new(44100, 2, 16, 100000);
        let processor = ParallelUniversalStreamProcessor {
            state: ProcessorState::new(path, format),
            parallel_decoder: None,
            format_reader: None,
            parallel_enabled: false,
            batch_size: PARALLEL_DECODE_BATCH_SIZE,
            thread_count: PARALLEL_DECODE_THREADS,
            processed_packets: 0,
            drained_samples: None,
        };

        // 测试配置方法
        let configured = processor.with_parallel_config(true, 128, 8);
        assert!(configured.parallel_enabled, "应启用并行解码");
        assert_eq!(configured.batch_size, 128, "batch_size应为128");
        assert_eq!(configured.thread_count, 8, "thread_count应为8");

        // 禁用并行
        let path2 = PathBuf::from("test2.flac");
        let format2 = AudioFormat::new(44100, 2, 16, 100000);
        let processor2 = ParallelUniversalStreamProcessor {
            state: ProcessorState::new(path2, format2),
            parallel_decoder: None,
            format_reader: None,
            parallel_enabled: true,
            batch_size: PARALLEL_DECODE_BATCH_SIZE,
            thread_count: PARALLEL_DECODE_THREADS,
            processed_packets: 0,
            drained_samples: None,
        };

        let configured2 = processor2.with_parallel_config(false, 64, 4);
        assert!(!configured2.parallel_enabled, "应禁用并行解码");
        assert_eq!(configured2.batch_size, 64, "batch_size应为64");
        assert_eq!(configured2.thread_count, 4, "thread_count应为4");
    }

    #[test]
    fn test_detect_bit_depth() {
        use symphonia::core::codecs::CodecParameters;
        use symphonia::core::sample::SampleFormat;

        let decoder = UniversalDecoder::new();

        // 测试显式bits_per_sample
        let mut params = CodecParameters::new();
        params.with_bits_per_sample(24);
        assert_eq!(decoder.detect_bit_depth(&params), 24);

        // 测试从sample_format推断
        let mut params2 = CodecParameters::new();
        params2.with_sample_format(SampleFormat::S16);
        assert_eq!(decoder.detect_bit_depth(&params2), 16);

        let mut params3 = CodecParameters::new();
        params3.with_sample_format(SampleFormat::S24);
        assert_eq!(decoder.detect_bit_depth(&params3), 24);

        let mut params4 = CodecParameters::new();
        params4.with_sample_format(SampleFormat::S32);
        assert_eq!(decoder.detect_bit_depth(&params4), 32);

        // 默认值
        let params_default = CodecParameters::new();
        assert_eq!(decoder.detect_bit_depth(&params_default), 16);
    }

    #[test]
    fn test_detect_channel_count() {
        use symphonia::core::audio::{Channels, Layout};
        use symphonia::core::codecs::CodecParameters;

        let decoder = UniversalDecoder::new();

        // 测试标准channels参数
        let mut params = CodecParameters::new();
        params.with_channels(Channels::FRONT_LEFT | Channels::FRONT_RIGHT);
        assert_eq!(decoder.detect_channel_count(&params).unwrap(), 2);

        // 测试channel_layout
        let mut params2 = CodecParameters::new();
        params2.with_channel_layout(Layout::Mono);
        assert_eq!(decoder.detect_channel_count(&params2).unwrap(), 1);

        let mut params3 = CodecParameters::new();
        params3.with_channel_layout(Layout::Stereo);
        assert_eq!(decoder.detect_channel_count(&params3).unwrap(), 2);

        // 默认值（立体声）
        let params_default = CodecParameters::new();
        assert_eq!(decoder.detect_channel_count(&params_default).unwrap(), 2);
    }

    #[test]
    fn test_detect_sample_count() {
        use symphonia::core::codecs::CodecParameters;
        use symphonia::core::units::TimeBase;

        let decoder = UniversalDecoder::new();

        // 测试n_frames
        let mut params = CodecParameters::new();
        params.with_n_frames(100000);
        assert_eq!(decoder.detect_sample_count(&params), 100000);

        // 测试从time_base估算
        let mut params2 = CodecParameters::new();
        params2
            .with_time_base(TimeBase::new(1, 1))
            .with_sample_rate(44100);
        let estimated = decoder.detect_sample_count(&params2);
        assert_eq!(estimated, 44100); // 1秒 * 44100Hz

        // 默认值
        let params_default = CodecParameters::new();
        assert_eq!(decoder.detect_sample_count(&params_default), 0);
    }

    #[test]
    fn test_parallel_processor_sync_skipped_packets() {
        use crate::tools::constants::decoder_performance::*;

        let path = PathBuf::from("test.flac");
        let format = AudioFormat::new(44100, 2, 16, 100000);
        let mut processor = ParallelUniversalStreamProcessor {
            state: ProcessorState::new(path, format),
            parallel_decoder: None,
            format_reader: None,
            parallel_enabled: true,
            batch_size: PARALLEL_DECODE_BATCH_SIZE,
            thread_count: PARALLEL_DECODE_THREADS,
            processed_packets: 0,
            drained_samples: None,
        };

        // 初始状态
        assert_eq!(processor.state.skipped_packets, 0);

        // 模拟跳过包（通过直接修改state）
        processor.state.skipped_packets = 3;

        // sync_skipped_packets在parallel_decoder为None时不应panic
        processor.sync_skipped_packets();
        assert_eq!(processor.state.skipped_packets, 3);
    }

    #[test]
    fn test_processor_state_stats() {
        let path = PathBuf::from("test.wav");
        let format = AudioFormat::new(44100, 2, 16, 100000);
        let mut state = ProcessorState::new(path, format);

        // 添加一些chunk统计
        state.chunk_stats.add_chunk(1024);
        state.chunk_stats.add_chunk(2048);
        state.chunk_stats.add_chunk(512);

        let stats = state.get_stats();
        assert_eq!(stats.total_chunks, 3);
        assert_eq!(stats.min_size, 512);
        assert_eq!(stats.max_size, 2048);
    }

    #[test]
    fn test_universal_stream_processor_creation() {
        // 测试UniversalStreamProcessor的基本创建（不需要真实文件）
        let path = PathBuf::from("test.flac");
        let format = AudioFormat::new(44100, 2, 16, 100000);

        let processor = UniversalStreamProcessor {
            state: ProcessorState::new(path.clone(), format.clone()),
            batch_packet_reader: None,
            decoder: None,
        };

        assert_eq!(processor.state.path, path);
        assert_eq!(processor.state.format.sample_rate, 44100);
        assert!(processor.batch_packet_reader.is_none());
        assert!(processor.decoder.is_none());
    }

    #[test]
    fn test_parallel_processor_creation() {
        use crate::tools::constants::decoder_performance::*;

        let path = PathBuf::from("test.flac");
        let format = AudioFormat::new(48000, 2, 24, 100000);

        let processor = ParallelUniversalStreamProcessor {
            state: ProcessorState::new(path.clone(), format.clone()),
            parallel_decoder: None,
            format_reader: None,
            parallel_enabled: true,
            batch_size: PARALLEL_DECODE_BATCH_SIZE,
            thread_count: PARALLEL_DECODE_THREADS,
            processed_packets: 0,
            drained_samples: None,
        };

        assert_eq!(processor.state.path, path);
        assert_eq!(processor.state.format.sample_rate, 48000);
        assert!(processor.parallel_enabled);
        assert_eq!(processor.batch_size, PARALLEL_DECODE_BATCH_SIZE);
        assert_eq!(processor.thread_count, PARALLEL_DECODE_THREADS);
        assert_eq!(processor.processed_packets, 0);
        assert!(processor.drained_samples.is_none());
    }

    #[test]
    fn test_detect_bit_depth_edge_cases() {
        use symphonia::core::codecs::CodecParameters;
        use symphonia::core::sample::SampleFormat;

        let decoder = UniversalDecoder::new();

        // 测试F32格式
        let mut params = CodecParameters::new();
        params.with_sample_format(SampleFormat::F32);
        assert_eq!(decoder.detect_bit_depth(&params), 32);

        // 测试F64格式
        let mut params2 = CodecParameters::new();
        params2.with_sample_format(SampleFormat::F64);
        assert_eq!(decoder.detect_bit_depth(&params2), 64);
    }

    #[test]
    fn test_detect_sample_count_edge_cases() {
        use symphonia::core::codecs::CodecParameters;
        use symphonia::core::units::TimeBase;

        let decoder = UniversalDecoder::new();

        // 测试time_base分母为0的情况（detect_sample_count内部检查denom > 0）
        let mut params = CodecParameters::new();
        params
            .with_time_base(TimeBase::new(2, 1))
            .with_sample_rate(44100);
        let result = decoder.detect_sample_count(&params);
        assert_eq!(result, 88200); // 2秒 * 44100Hz

        // 测试没有sample_rate的情况
        let mut params2 = CodecParameters::new();
        params2.with_time_base(TimeBase::new(1, 1));
        assert_eq!(decoder.detect_sample_count(&params2), 0);

        // 测试仅有n_frames的情况（最高优先级）
        let mut params3 = CodecParameters::new();
        params3.with_n_frames(123456);
        assert_eq!(decoder.detect_sample_count(&params3), 123456);
    }

    #[test]
    fn test_parallel_processor_with_config_chaining() {
        use crate::tools::constants::decoder_performance::*;

        let path = PathBuf::from("test.opus");
        let format = AudioFormat::new(48000, 2, 16, 200000);

        // 测试配置方法的链式调用
        let processor = ParallelUniversalStreamProcessor {
            state: ProcessorState::new(path, format),
            parallel_decoder: None,
            format_reader: None,
            parallel_enabled: false,
            batch_size: PARALLEL_DECODE_BATCH_SIZE,
            thread_count: PARALLEL_DECODE_THREADS,
            processed_packets: 0,
            drained_samples: None,
        }
        .with_parallel_config(true, 256, 16);

        assert!(processor.parallel_enabled);
        assert_eq!(processor.batch_size, 256);
        assert_eq!(processor.thread_count, 16);
        assert!(processor.parallel_decoder.is_none()); // 尚未初始化
    }

    #[test]
    fn test_processor_state_multiple_updates() {
        let path = PathBuf::from("test.aac");
        let format = AudioFormat::new(44100, 2, 16, 0);
        let mut state = ProcessorState::new(path, format);

        // 模拟多次更新
        for i in 1..=10 {
            let samples = vec![0.0f32; 100];
            state.update_position(&samples, 2);
            assert_eq!(state.current_position, (i * 50) as u64);
            assert_eq!(state.total_samples, (i * 50) as u64);
        }
    }
}
