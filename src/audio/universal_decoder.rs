//! 统一音频解码器协调器
//!
//! 提供统一的音频解码接口，协调各个子模块提供完整的解码服务
//! 采用模块化架构，各子模块仅供协调器内部使用

use crate::error::{AudioError, AudioResult};
use std::path::Path;

// 重新导出公共接口
pub use super::format::{AudioFormat, FormatSupport};
pub use super::stats::ChunkSizeStats;
pub use super::streaming::StreamingDecoder;

// 内部使用的模块
use super::pcm_engine::PcmEngine;

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

/// PCM解码器协调器 - 处理WAV、FLAC等PCM格式
///
/// 作为协调器，委托给内部的PcmEngine处理具体业务逻辑
pub struct PcmDecoder {
    engine: PcmEngine,
}

impl Default for PcmDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl PcmDecoder {
    pub fn new() -> Self {
        Self {
            engine: PcmEngine::new(),
        }
    }
}

impl AudioDecoder for PcmDecoder {
    fn name(&self) -> &'static str {
        self.engine.name()
    }

    fn supported_formats(&self) -> &FormatSupport {
        self.engine.supported_formats()
    }

    fn probe_format(&self, path: &Path) -> AudioResult<AudioFormat> {
        self.engine.probe_format(path)
    }

    fn create_streaming(&self, path: &Path) -> AudioResult<Box<dyn StreamingDecoder>> {
        self.engine.create_streaming(path)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl PcmDecoder {
    /// 🚀 创建高性能流式解码器（推荐方法）
    ///
    /// 固定启用逐包模式优化，遵循"无条件高性能原则"。
    /// 适配foobar2000-plugin分支的高性能要求和WindowRmsAnalyzer批处理计算。
    pub fn create_streaming_optimized(
        &self,
        path: &Path,
    ) -> AudioResult<Box<dyn StreamingDecoder>> {
        self.engine.create_streaming_optimized(path)
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
            Box::new(PcmDecoder::new()),
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

    /// 🔥 创建高性能流式解码器（推荐方法）
    ///
    /// 自动选择最优的解码器和配置，遵循"无条件高性能原则"。
    /// 适配foobar2000-plugin分支的高性能要求。
    pub fn create_streaming_optimized<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> AudioResult<Box<dyn StreamingDecoder>> {
        let decoder = self.get_decoder(path.as_ref())?;
        if let Some(pcm_decoder) = decoder.as_any().downcast_ref::<PcmDecoder>() {
            // 🚀 PCM格式使用高性能优化模式
            pcm_decoder.create_streaming_optimized(path.as_ref())
        } else {
            // 🔄 其他格式使用标准流式模式
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
