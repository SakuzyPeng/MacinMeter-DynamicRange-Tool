//! 音频格式信息模块
//!
//! 定义音频格式相关的数据结构和格式支持信息

use crate::error::{self, AudioResult};
use symphonia::core::codecs::CodecType;

/// 支持的最大声道数（架构约束）
///
/// 当前仅支持单声道(1)和立体声(2)，3+声道友好拒绝。
/// 这是基于 foobar2000 DR Meter 规范的设计约束。
pub const MAX_CHANNELS: u16 = 2;

/// 音频格式信息
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
    /// 源音频的位深度（来自解码器/容器元数据）
    ///
    /// 注意：这表示源格式的位深，而非工具内部处理的位深。
    /// 工具内部统一使用 f32 (32位浮点) 进行处理。
    /// 常见值：8(8位PCM), 16(16位PCM), 24(24位PCM), 32(32位整数或浮点), 64(64位浮点)
    pub bits_per_sample: u16,
    pub sample_count: u64,
    /// 真实的编解码器类型（从解码器获取，比文件扩展名更准确）
    pub codec_type: Option<CodecType>,
    /// 是否为部分分析（解码过程中跳过了损坏的音频包）
    is_partial: bool,
    /// 跳过的损坏包数量（累积统计）
    skipped_packets: usize,
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
        let mut format = Self::new(sample_rate, channels, bits_per_sample, sample_count);
        format.codec_type = Some(codec_type);
        format
    }

    /// 累加跳过的损坏包数量（支持多次调用累加统计）
    pub fn add_skipped_packets(&mut self, count: usize) {
        if count > 0 {
            self.is_partial = true;
            self.skipped_packets = self.skipped_packets.saturating_add(count);
        }
    }

    /// 标记为部分分析并记录跳过的包数量
    ///
    /// 注意：此方法会覆盖现有的skipped_packets值。
    /// 如需累加统计，请使用 [`add_skipped_packets`](Self::add_skipped_packets)
    pub fn mark_as_partial(&mut self, skipped_packets: usize) {
        self.is_partial = true;
        self.skipped_packets = skipped_packets;
    }

    /// 获取是否为部分分析结果
    pub fn is_partial(&self) -> bool {
        self.is_partial
    }

    /// 获取跳过的损坏包数量
    pub fn skipped_packets(&self) -> usize {
        self.skipped_packets
    }

    /// 验证格式参数的有效性
    pub fn validate(&self) -> AudioResult<()> {
        if self.sample_rate == 0 {
            return Err(error::format_error("采样率不能为0", ""));
        }
        if self.channels == 0 {
            return Err(error::format_error("声道数不能为0", ""));
        }
        // 放宽位深校验：支持常见的PCM位深和浮点格式
        // 8位: 8位PCM, 16位: 16位PCM, 24位: 24位PCM
        // 32位: 32位整数或32位浮点, 64位: 64位浮点
        if ![8, 16, 24, 32, 64].contains(&self.bits_per_sample) {
            return Err(error::format_error(
                "不支持的位深度",
                format!("{}位（仅支持 8/16/24/32/64）", self.bits_per_sample),
            ));
        }
        // 声道数限制检查（架构约束：仅支持1-2声道）
        if self.channels > MAX_CHANNELS {
            return Err(error::format_error(
                "不支持的声道数",
                format!("{}声道（仅支持单声道或立体声，即1-2声道）", self.channels),
            ));
        }
        Ok(())
    }

    /// 获取未压缩PCM格式的文件大小估算（字节）
    ///
    /// 注意：此方法按PCM未压缩尺寸估算，不等同于容器文件的实际大小。
    /// 对于压缩格式（FLAC、Opus、AAC等），实际文件会远小于此估算值。
    /// 使用饱和乘法防止整数溢出。
    pub fn estimated_pcm_size_bytes(&self) -> u64 {
        self.sample_count
            .saturating_mul(self.channels as u64)
            .saturating_mul(self.bits_per_sample as u64 / 8)
    }

    /// **已弃用**: 请使用 [`estimated_pcm_size_bytes`](Self::estimated_pcm_size_bytes)
    ///
    /// 此方法保留用于向后兼容，将在未来版本移除。
    #[deprecated(since = "0.1.0", note = "请使用 estimated_pcm_size_bytes() 以明确语义")]
    pub fn estimated_file_size(&self) -> u64 {
        self.estimated_pcm_size_bytes()
    }

    /// 获取持续时长（秒）
    pub fn duration_seconds(&self) -> f64 {
        self.sample_count as f64 / self.sample_rate as f64
    }

    /// 更新样本数（用于动态格式更新）
    pub fn update_sample_count(&mut self, sample_count: u64) {
        self.sample_count = sample_count;
    }

    /// 获取每样本的字节数
    ///
    /// 辅助方法，减少到处的 `as` 转换和硬编码计算
    pub fn bytes_per_sample(&self) -> usize {
        (self.bits_per_sample / 8) as usize
    }

    /// 获取声道数（usize类型）
    ///
    /// 辅助方法，用于数组索引和循环边界，避免重复的类型转换
    pub fn channels_usize(&self) -> usize {
        self.channels as usize
    }
}

/// 格式支持信息
#[derive(Debug, Clone)]
pub struct FormatSupport {
    /// 支持的文件扩展名
    pub extensions: &'static [&'static str],
}
