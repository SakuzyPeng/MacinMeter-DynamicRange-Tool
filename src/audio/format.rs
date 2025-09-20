//! 音频格式信息模块
//!
//! 定义音频格式相关的数据结构和格式支持信息

use crate::error::{AudioError, AudioResult};

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

/// 格式支持信息
#[derive(Debug, Clone)]
pub struct FormatSupport {
    /// 支持的文件扩展名
    pub extensions: &'static [&'static str],
}
