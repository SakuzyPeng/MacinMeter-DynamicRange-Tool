//! 音频格式信息模块
//!
//! 定义音频格式相关的数据结构和格式支持信息

use crate::error::{self, AudioResult};
use symphonia::core::codecs::CodecType;

/// 音频格式信息
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub sample_count: u64,
    /// 真实的编解码器类型（从解码器获取，比文件扩展名更准确）
    pub codec_type: Option<CodecType>,
    /// 是否为部分分析（解码过程中跳过了损坏的音频包）
    pub is_partial: bool,
    /// 跳过的损坏包数量
    pub skipped_packets: usize,
}

impl AudioFormat {
    /// 创建新的音频格式
    pub fn new(sample_rate: u32, channels: u16, bits_per_sample: u16, sample_count: u64) -> Self {
        Self {
            sample_rate,
            channels,
            bits_per_sample,
            sample_count,
            codec_type: None,
            is_partial: false,
            skipped_packets: 0,
        }
    }

    /// 创建包含编解码器信息的音频格式
    pub fn with_codec(
        sample_rate: u32,
        channels: u16,
        bits_per_sample: u16,
        sample_count: u64,
        codec_type: CodecType,
    ) -> Self {
        Self {
            sample_rate,
            channels,
            bits_per_sample,
            sample_count,
            codec_type: Some(codec_type),
            is_partial: false,
            skipped_packets: 0,
        }
    }

    /// 标记为部分分析并记录跳过的包数量
    pub fn mark_as_partial(&mut self, skipped_packets: usize) {
        self.is_partial = true;
        self.skipped_packets = skipped_packets;
    }

    /// 验证格式参数的有效性
    pub fn validate(&self) -> AudioResult<()> {
        if self.sample_rate == 0 {
            return Err(error::format_error("采样率不能为0", ""));
        }
        if self.channels == 0 {
            return Err(error::format_error("声道数不能为0", ""));
        }
        if ![16, 24, 32].contains(&self.bits_per_sample) {
            return Err(error::format_error(
                "不支持的位深度",
                format!("{}位", self.bits_per_sample),
            ));
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

    /// 更新样本数（用于动态格式更新）
    pub fn update_sample_count(&mut self, sample_count: u64) {
        self.sample_count = sample_count;
    }
}

/// 格式支持信息
#[derive(Debug, Clone)]
pub struct FormatSupport {
    /// 支持的文件扩展名
    pub extensions: &'static [&'static str],
}
