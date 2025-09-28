//! 流式处理接口模块
//!
//! 定义流式解码器trait和相关接口
//! 注意：此模块仅供universal_decoder协调器内部使用

use super::format::AudioFormat;
use super::stats::ChunkSizeStats;
use crate::error::AudioResult;

/// 流式解码器trait
///
/// 此trait通过协调器对外提供服务，内部实现由协调器管理
pub trait StreamingDecoder {
    /// 获取下一个音频块
    fn next_chunk(&mut self) -> AudioResult<Option<Vec<f32>>>;

    /// 获取解码进度 (0.0-1.0)
    fn progress(&self) -> f32;

    /// 获取音频格式信息（动态构造，包含实时样本数）
    fn format(&self) -> AudioFormat;

    /// 重置到开头
    fn reset(&mut self) -> AudioResult<()>;

    /// 获取块大小统计信息（可选，仅逐包模式支持）
    fn get_chunk_stats(&mut self) -> Option<ChunkSizeStats> {
        None // 默认不支持
    }
}
