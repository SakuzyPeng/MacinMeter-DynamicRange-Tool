//! 多格式音频解码器
//!
//! 基于symphonia库实现FLAC、MP3、AAC等多种音频格式的解码支持。

use crate::error::{AudioError, AudioResult};
use std::path::Path;
use symphonia::core::audio::{AudioBuffer, Signal};
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use super::AudioFormat;

/// 多格式音频解码器
///
/// 支持FLAC、MP3、AAC、OGG等格式，自动转换为f32处理格式。
pub struct MultiDecoder {
    /// 音频格式信息
    format: Option<AudioFormat>,

    /// 原始样本数据（交错格式）
    samples: Vec<f32>,
}

impl MultiDecoder {
    /// 创建新的多格式解码器
    pub fn new() -> Self {
        Self {
            format: None,
            samples: Vec::new(),
        }
    }

    /// 从文件路径加载音频文件
    ///
    /// # 参数
    ///
    /// * `path` - 音频文件路径
    ///
    /// # 返回值
    ///
    /// 返回音频格式信息
    ///
    /// # 错误
    ///
    /// * `AudioError::IoError` - 文件读取失败
    /// * `AudioError::FormatError` - 不支持的音频格式
    /// * `AudioError::DecodingError` - 解码过程失败
    ///
    /// # 示例
    ///
    /// ```rust
    /// use macinmeter_dr_tool::audio::MultiDecoder;
    ///
    /// let mut decoder = MultiDecoder::new();
    /// // let format = decoder.load_file("test.flac").unwrap();
    /// // println!("采样率: {}, 声道: {}", format.sample_rate, format.channels);
    /// ```
    pub fn load_file<P: AsRef<Path>>(&mut self, path: P) -> AudioResult<AudioFormat> {
        let path = path.as_ref();

        // 验证文件存在
        if !path.exists() {
            return Err(AudioError::IoError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("文件不存在: {}", path.display()),
            )));
        }

        // 验证支持的文件格式
        self.validate_file_format(path)?;

        // 打开文件
        let src = std::fs::File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(src), Default::default());

        // 创建格式提示
        let mut hint = Hint::new();
        if let Some(ext) = path.extension() {
            let ext_str = ext.to_string_lossy();
            hint.with_extension(&ext_str);
        }

        // 使用默认选项
        let format_opts: FormatOptions = Default::default();
        let metadata_opts: MetadataOptions = Default::default();
        let decoder_opts: DecoderOptions = Default::default();

        // 探测音频格式
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &format_opts, &metadata_opts)
            .map_err(|e| AudioError::FormatError(format!("无法识别音频格式: {e}")))?;

        let mut format_reader = probed.format;

        // 查找第一个音频流
        let track = format_reader
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| AudioError::FormatError("未找到音频流".to_string()))?;

        let track_id = track.id;
        let codec_params = &track.codec_params;

        // 获取音频格式信息
        let sample_rate = codec_params
            .sample_rate
            .ok_or_else(|| AudioError::FormatError("未找到采样率信息".to_string()))?;

        let channels = codec_params
            .channels
            .ok_or_else(|| AudioError::FormatError("未找到声道信息".to_string()))?
            .count() as u16;

        // 获取真实位深度信息
        let bits_per_sample = codec_params.bits_per_sample.unwrap_or(
            codec_params.bits_per_coded_sample.unwrap_or(32), // 默认32位浮点
        ) as u16;

        // 创建解码器
        let mut decoder = symphonia::default::get_codecs()
            .make(codec_params, &decoder_opts)
            .map_err(|e| AudioError::DecodingError(format!("创建解码器失败: {e}")))?;

        // 解码音频数据
        let mut samples = Vec::new();
        let mut total_frames = 0u64;

        loop {
            match format_reader.next_packet() {
                Ok(packet) => {
                    if packet.track_id() != track_id {
                        continue;
                    }

                    match decoder.decode(&packet) {
                        Ok(audio_buf) => {
                            total_frames += audio_buf.frames() as u64;
                            self.convert_audio_buffer(&audio_buf, &mut samples)?;
                        }
                        Err(symphonia::core::errors::Error::DecodeError(err)) => {
                            eprintln!("解码错误: {err}");
                            continue; // 跳过错误的包
                        }
                        Err(err) => {
                            return Err(AudioError::DecodingError(format!("解码失败: {err}")));
                        }
                    }
                }
                Err(symphonia::core::errors::Error::IoError(err)) => match err.kind() {
                    std::io::ErrorKind::UnexpectedEof => break,
                    _ => return Err(AudioError::IoError(err)),
                },
                Err(err) => {
                    return Err(AudioError::DecodingError(format!("读取包失败: {err}")));
                }
            }
        }

        // 创建格式信息
        let format = AudioFormat::new(
            sample_rate,
            channels,
            bits_per_sample, // 使用真实位深度信息
            total_frames,
        );

        // 验证格式
        format.validate()?;

        // 存储数据
        self.format = Some(format.clone());
        self.samples = samples;

        Ok(format)
    }

    /// 获取样本数据
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// 获取指定声道的样本数据
    ///
    /// # 参数
    ///
    /// * `channel` - 声道索引（从0开始）
    ///
    /// # 返回值
    ///
    /// 返回该声道的样本数据
    pub fn channel_samples(&self, channel: usize) -> AudioResult<Vec<f32>> {
        let format = self
            .format
            .as_ref()
            .ok_or_else(|| AudioError::InvalidInput("未加载音频文件".to_string()))?;

        if channel >= format.channels as usize {
            return Err(AudioError::InvalidInput(format!(
                "声道索引{channel}超出范围（总声道数：{}）",
                format.channels
            )));
        }

        let channel_samples: Vec<f32> = self
            .samples
            .iter()
            .skip(channel)
            .step_by(format.channels as usize)
            .copied()
            .collect();

        Ok(channel_samples)
    }

    /// 清除已加载的数据
    pub fn clear(&mut self) {
        self.format = None;
        self.samples.clear();
    }

    /// 验证文件格式是否支持
    fn validate_file_format(&self, path: &Path) -> AudioResult<()> {
        if let Some(ext) = path.extension() {
            let ext_str = ext.to_string_lossy().to_lowercase();
            match ext_str.as_str() {
                "flac" | "mp3" | "m4a" | "aac" | "ogg" | "wav" => Ok(()),
                _ => Err(AudioError::FormatError(format!(
                    "不支持的文件格式: .{ext_str}"
                ))),
            }
        } else {
            Err(AudioError::FormatError("文件缺少扩展名".to_string()))
        }
    }

    /// 转换symphonia音频缓冲区到f32样本
    fn convert_audio_buffer(
        &self,
        audio_buf: &symphonia::core::audio::AudioBufferRef,
        output: &mut Vec<f32>,
    ) -> AudioResult<()> {
        use symphonia::core::audio::AudioBufferRef;

        match audio_buf {
            AudioBufferRef::U8(buf) => {
                self.convert_planar_to_interleaved(buf, output, |sample: u8| {
                    (sample as i32 - 128) as f32 / 128.0
                });
            }
            AudioBufferRef::U16(buf) => {
                self.convert_planar_to_interleaved(buf, output, |sample: u16| {
                    (sample as i32 - 32768) as f32 / 32768.0
                });
            }
            AudioBufferRef::U24(buf) => {
                self.convert_planar_to_interleaved(buf, output, |sample| {
                    ((sample.inner() as i32) - 8388608) as f32 / 8388608.0
                });
            }
            AudioBufferRef::U32(buf) => {
                self.convert_planar_to_interleaved(buf, output, |sample: u32| {
                    (sample as i64 - 2147483648) as f32 / 2147483648.0
                });
            }
            AudioBufferRef::S8(buf) => {
                self.convert_planar_to_interleaved(buf, output, |sample: i8| sample as f32 / 128.0);
            }
            AudioBufferRef::S16(buf) => {
                self.convert_planar_to_interleaved(buf, output, |sample: i16| {
                    sample as f32 / 32768.0
                });
            }
            AudioBufferRef::S24(buf) => {
                self.convert_planar_to_interleaved(buf, output, |sample| {
                    sample.inner() as f32 / 8388608.0
                });
            }
            AudioBufferRef::S32(buf) => {
                self.convert_planar_to_interleaved(buf, output, |sample: i32| {
                    sample as f32 / 2147483648.0
                });
            }
            AudioBufferRef::F32(buf) => {
                self.convert_planar_to_interleaved(buf, output, |sample: f32| sample);
            }
            AudioBufferRef::F64(buf) => {
                self.convert_planar_to_interleaved(buf, output, |sample: f64| sample as f32);
            }
        }

        Ok(())
    }

    /// 将平面格式转换为交错格式
    fn convert_planar_to_interleaved<T, F>(
        &self,
        buf: &AudioBuffer<T>,
        output: &mut Vec<f32>,
        convert: F,
    ) where
        T: symphonia::core::sample::Sample + Copy,
        F: Fn(T) -> f32,
    {
        let channels = buf.spec().channels.count();
        let frames = buf.frames();

        for frame_idx in 0..frames {
            for ch_idx in 0..channels {
                let channel_buf = buf.chan(ch_idx);
                let sample = channel_buf[frame_idx];
                output.push(convert(sample));
            }
        }
    }
}

impl Default for MultiDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_decoder_creation() {
        let decoder = MultiDecoder::new();
        assert!(decoder.format.is_none());
        assert!(decoder.samples.is_empty());
    }

    #[test]
    fn test_multi_decoder_clear() {
        let mut decoder = MultiDecoder::new();
        decoder.samples = vec![0.1, 0.2, 0.3];
        decoder.format = Some(AudioFormat::new(44100, 2, 16, 100));

        decoder.clear();

        assert!(decoder.format.is_none());
        assert!(decoder.samples.is_empty());
    }

    #[test]
    fn test_channel_samples_not_loaded() {
        let decoder = MultiDecoder::new();
        let result = decoder.channel_samples(0);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_file_format() {
        let decoder = MultiDecoder::new();

        // 支持的格式
        assert!(decoder.validate_file_format(Path::new("test.flac")).is_ok());
        assert!(decoder.validate_file_format(Path::new("test.mp3")).is_ok());
        assert!(decoder.validate_file_format(Path::new("test.m4a")).is_ok());

        // 不支持的格式
        assert!(decoder.validate_file_format(Path::new("test.txt")).is_err());
        assert!(decoder.validate_file_format(Path::new("test")).is_err());
    }

    #[test]
    fn test_load_nonexistent_file() {
        let mut decoder = MultiDecoder::new();
        let result = decoder.load_file("nonexistent.flac");
        assert!(result.is_err());
    }
}
