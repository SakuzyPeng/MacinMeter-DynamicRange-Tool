//! WAV文件解码器
//!
//! 基于hound库实现高效的WAV文件读取和解码。

use crate::error::{AudioError, AudioResult};
use std::path::Path;

/// 音频格式信息
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFormat {
    /// 采样率 (Hz)
    pub sample_rate: u32,

    /// 声道数
    pub channels: u16,

    /// 位深度
    pub bits_per_sample: u16,

    /// 总样本数（单声道）
    pub sample_count: u64,

    /// 音频时长（秒）
    pub duration_seconds: f64,
}

impl AudioFormat {
    /// 创建新的音频格式信息
    pub fn new(sample_rate: u32, channels: u16, bits_per_sample: u16, sample_count: u64) -> Self {
        let duration_seconds = if sample_rate > 0 {
            sample_count as f64 / sample_rate as f64
        } else {
            0.0
        };

        Self {
            sample_rate,
            channels,
            bits_per_sample,
            sample_count,
            duration_seconds,
        }
    }

    /// 验证音频格式是否受支持
    pub fn validate(&self) -> AudioResult<()> {
        if self.channels == 0 {
            return Err(AudioError::FormatError("声道数不能为0".to_string()));
        }

        if self.channels > 32 {
            return Err(AudioError::FormatError("声道数不能超过32".to_string()));
        }

        if self.sample_rate == 0 {
            return Err(AudioError::FormatError("采样率不能为0".to_string()));
        }

        if self.sample_rate > 384_000 {
            return Err(AudioError::FormatError(format!(
                "采样率({})超出支持范围(最大384kHz)",
                self.sample_rate
            )));
        }

        match self.bits_per_sample {
            16 | 24 | 32 => Ok(()),
            _ => Err(AudioError::FormatError(format!(
                "不支持的位深度: {}位",
                self.bits_per_sample
            ))),
        }
    }

    /// 计算预估的内存使用量（字节）
    pub fn estimated_memory_usage(&self) -> u64 {
        // f32样本 * 声道数 * 4字节
        self.sample_count * self.channels as u64 * 4
    }
}

/// WAV文件解码器
///
/// 支持16/24/32位PCM格式，自动转换为f32处理格式。
pub struct WavDecoder {
    /// 音频格式信息
    format: Option<AudioFormat>,

    /// 原始样本数据（交错格式）
    samples: Vec<f32>,
}

impl WavDecoder {
    /// 创建新的WAV解码器
    pub fn new() -> Self {
        Self {
            format: None,
            samples: Vec::new(),
        }
    }

    /// 从文件路径加载WAV文件
    ///
    /// # 参数
    ///
    /// * `path` - WAV文件路径
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
    /// use macinmeter_dr_tool::audio::WavDecoder;
    ///
    /// let mut decoder = WavDecoder::new();
    /// // let format = decoder.load_file("test.wav").unwrap();
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

        // 验证文件扩展名
        if let Some(ext) = path.extension() {
            let ext_str = ext.to_string_lossy().to_lowercase();
            if ext_str != "wav" {
                return Err(AudioError::FormatError(format!(
                    "不支持的文件格式: .{ext_str}"
                )));
            }
        } else {
            return Err(AudioError::FormatError("文件缺少扩展名".to_string()));
        }

        // 打开WAV文件
        let mut reader = hound::WavReader::open(path)?;
        let spec = reader.spec();

        // 创建格式信息
        let format = AudioFormat::new(
            spec.sample_rate,
            spec.channels,
            spec.bits_per_sample,
            reader.len() as u64,
        );

        // 验证格式支持
        format.validate()?;

        // 检查内存使用量（防止OOM）
        let memory_mb = format.estimated_memory_usage() / 1024 / 1024;
        if memory_mb > 1024 {
            return Err(AudioError::OutOfMemory);
        }

        // 解码样本数据
        self.samples = self.decode_samples(&mut reader, &format)?;
        self.format = Some(format.clone());

        Ok(format)
    }

    /// 解码音频样本数据
    fn decode_samples(
        &self,
        reader: &mut hound::WavReader<std::io::BufReader<std::fs::File>>,
        format: &AudioFormat,
    ) -> AudioResult<Vec<f32>> {
        let mut samples = Vec::new();

        match format.bits_per_sample {
            16 => {
                // 16位PCM: -32768 到 32767
                // 🔧 dr14_t.meter兼容性：使用32769归一化因子 (2^15 + 1)
                // 🎯 精度修正：使用f64避免f32→f64转换的精度损失
                for sample_result in reader.samples::<i16>() {
                    let sample = sample_result?;
                    let normalized = sample as f64 / 32769.0;
                    samples.push(normalized as f32);
                }
            }
            24 => {
                // 24位PCM: -8388608 到 8388607
                // 🔧 dr14_t.meter兼容性：使用8388609归一化因子 (2^23 + 1)
                for sample_result in reader.samples::<i32>() {
                    let sample = sample_result?;
                    let normalized = sample as f32 / 8388609.0;
                    samples.push(normalized);
                }
            }
            32 => {
                // 32位PCM或浮点
                if reader.spec().sample_format == hound::SampleFormat::Float {
                    // 32位浮点
                    for sample_result in reader.samples::<f32>() {
                        let sample = sample_result?;
                        samples.push(sample);
                    }
                } else {
                    // 32位整数PCM: -2147483648 到 2147483647
                    // 🔧 dr14_t.meter兼容性：使用2147483649归一化因子 (2^31 + 1)
                    for sample_result in reader.samples::<i32>() {
                        let sample = sample_result?;
                        let normalized = sample as f32 / 2147483649.0;
                        samples.push(normalized);
                    }
                }
            }
            _ => {
                return Err(AudioError::DecodingError(format!(
                    "不支持的位深度: {}位",
                    format.bits_per_sample
                )));
            }
        }

        Ok(samples)
    }

    /// 获取音频格式信息
    pub fn format(&self) -> Option<&AudioFormat> {
        self.format.as_ref()
    }

    /// 获取交错排列的音频样本数据
    ///
    /// 返回格式为[L1, R1, L2, R2, ...]（立体声示例）
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
    /// 返回该声道的所有样本数据
    ///
    /// # 错误
    ///
    /// * `AudioError::InvalidInput` - 声道索引超出范围
    pub fn channel_samples(&self, channel: usize) -> AudioResult<Vec<f32>> {
        let format = self
            .format
            .as_ref()
            .ok_or_else(|| AudioError::InvalidInput("尚未加载任何音频文件".to_string()))?;

        if channel >= format.channels as usize {
            return Err(AudioError::InvalidInput(format!(
                "声道索引({})超出范围(0-{})",
                channel,
                format.channels - 1
            )));
        }

        let channel_count = format.channels as usize;
        let mut channel_samples = Vec::new();

        // 从交错数据中提取指定声道
        for sample_idx in (channel..self.samples.len()).step_by(channel_count) {
            channel_samples.push(self.samples[sample_idx]);
        }

        Ok(channel_samples)
    }

    /// 获取所有声道的分离样本数据
    ///
    /// # 返回值
    ///
    /// 返回Vec<Vec<f32>>，每个内层Vec包含一个声道的所有样本
    pub fn all_channel_samples(&self) -> AudioResult<Vec<Vec<f32>>> {
        let format = self
            .format
            .as_ref()
            .ok_or_else(|| AudioError::InvalidInput("尚未加载任何音频文件".to_string()))?;

        let mut all_samples = Vec::with_capacity(format.channels as usize);

        for channel in 0..format.channels as usize {
            all_samples.push(self.channel_samples(channel)?);
        }

        Ok(all_samples)
    }

    /// 检查是否已加载音频数据
    pub fn is_loaded(&self) -> bool {
        self.format.is_some() && !self.samples.is_empty()
    }

    /// 清空解码器状态
    pub fn clear(&mut self) {
        self.format = None;
        self.samples.clear();
    }
}

impl Default for WavDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // 辅助函数：创建简单的测试WAV文件
    fn create_test_wav_file(
        path: &str,
        sample_rate: u32,
        channels: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let spec = hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let mut writer = hound::WavWriter::create(path, spec)?;

        // 写入简单的测试数据
        for i in 0..1000 {
            let sample = (i as f32 * 0.001).sin();
            let amplitude = (sample * i16::MAX as f32) as i16;
            for _ in 0..channels {
                writer.write_sample(amplitude)?;
            }
        }

        writer.finalize()?;
        Ok(())
    }

    #[test]
    fn test_audio_format_new() {
        let format = AudioFormat::new(44100, 2, 16, 44100);

        assert_eq!(format.sample_rate, 44100);
        assert_eq!(format.channels, 2);
        assert_eq!(format.bits_per_sample, 16);
        assert_eq!(format.sample_count, 44100);
        assert!((format.duration_seconds - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_audio_format_validation() {
        // 有效格式
        let format = AudioFormat::new(44100, 2, 16, 1000);
        assert!(format.validate().is_ok());

        // 无效声道数
        let format = AudioFormat::new(44100, 0, 16, 1000);
        assert!(format.validate().is_err());

        let format = AudioFormat::new(44100, 33, 16, 1000);
        assert!(format.validate().is_err());

        // 无效采样率
        let format = AudioFormat::new(0, 2, 16, 1000);
        assert!(format.validate().is_err());

        let format = AudioFormat::new(500_000, 2, 16, 1000);
        assert!(format.validate().is_err());

        // 无效位深度
        let format = AudioFormat::new(44100, 2, 8, 1000);
        assert!(format.validate().is_err());
    }

    #[test]
    fn test_wav_decoder_new() {
        let decoder = WavDecoder::new();
        assert!(!decoder.is_loaded());
        assert!(decoder.format().is_none());
        assert!(decoder.samples().is_empty());
    }

    #[test]
    fn test_load_nonexistent_file() {
        let mut decoder = WavDecoder::new();
        let result = decoder.load_file("nonexistent.wav");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_file_wrong_extension() {
        // 创建临时文件
        let temp_path = "/tmp/test.txt";
        fs::write(temp_path, "not a wav file").unwrap();

        let mut decoder = WavDecoder::new();
        let result = decoder.load_file(temp_path);
        assert!(result.is_err());

        // 清理
        let _ = fs::remove_file(temp_path);
    }

    #[test]
    fn test_channel_samples_not_loaded() {
        let decoder = WavDecoder::new();
        let result = decoder.channel_samples(0);
        assert!(result.is_err());
    }

    #[test]
    fn test_clear() {
        let mut decoder = WavDecoder::new();
        // 这里我们不能真正加载文件，所以手动设置一些状态
        decoder.format = Some(AudioFormat::new(44100, 2, 16, 1000));
        decoder.samples = vec![0.5, -0.3];

        assert!(decoder.is_loaded());

        decoder.clear();
        assert!(!decoder.is_loaded());
        assert!(decoder.format().is_none());
        assert!(decoder.samples().is_empty());
    }

    // 注意：以下测试需要实际的WAV文件，在CI环境中可能不适用
    #[test]
    #[ignore] // 需要文件系统操作，标记为ignore
    fn test_load_valid_wav_file() {
        let temp_path = "/tmp/test_valid.wav";

        // 创建测试WAV文件
        if create_test_wav_file(temp_path, 44100, 2).is_ok() {
            let mut decoder = WavDecoder::new();
            let result = decoder.load_file(temp_path);

            if let Ok(format) = result {
                assert_eq!(format.sample_rate, 44100);
                assert_eq!(format.channels, 2);
                assert_eq!(format.bits_per_sample, 16);
                assert!(decoder.is_loaded());
                assert!(!decoder.samples().is_empty());
            }

            // 清理
            let _ = fs::remove_file(temp_path);
        }
    }
}
