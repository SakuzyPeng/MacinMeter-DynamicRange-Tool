//! 智能内存管理策略
//!
//! 根据文件大小和系统资源动态选择最优的处理策略，
//! 确保既不浪费性能也不造成内存问题。

use crate::error::{AudioError, AudioResult};
use std::fs;
use std::path::Path;

/// 音频文件处理策略
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessingStrategy {
    /// 全内存加载模式 - 适用于小文件
    /// 优势：最佳性能，无IO开销
    /// 限制：文件大小 < 200MB
    FullMemory,

    /// 流式块处理模式 - 适用于大文件  
    /// 优势：恒定内存使用，支持任意大小文件
    /// 特点：按3秒块流式处理，内存使用 < 50MB
    StreamingBlocks,

    /// 混合模式 - 根据可用内存动态选择
    /// 智能在全内存和流式之间切换
    Adaptive,
}

/// 内存使用估算
#[derive(Debug, Clone)]
pub struct MemoryEstimate {
    /// 原始音频数据大小 (字节)
    pub raw_audio_bytes: u64,

    /// f32样本数组大小 (字节)
    pub samples_memory: u64,

    /// 处理过程峰值内存 (字节)
    pub peak_memory: u64,

    /// 推荐处理策略
    pub recommended_strategy: ProcessingStrategy,
}

/// 智能内存策略选择器
pub struct MemoryStrategySelector {
    /// 系统可用内存 (字节)
    available_memory: u64,

    /// 安全内存使用限制 (默认50%可用内存)
    memory_limit: u64,
}

impl MemoryStrategySelector {
    /// 创建策略选择器
    pub fn new() -> Self {
        let available_memory = Self::get_available_memory();
        let memory_limit = available_memory / 2; // 使用50%可用内存作为安全限制

        Self {
            available_memory,
            memory_limit,
        }
    }

    /// 分析文件并推荐处理策略
    pub fn analyze_file<P: AsRef<Path>>(&self, path: P) -> AudioResult<MemoryEstimate> {
        let path = path.as_ref();
        let file_size = fs::metadata(path).map_err(AudioError::IoError)?.len();

        // 音频文件通常的内存放大系数分析：
        // - 解码放大：1.0x (已是未压缩PCM) 到 10x (高压缩比格式)
        // - f32转换：通常2x (16bit->32bit) 到 1x (32bit->32bit)
        // - 处理缓冲：1.5x (临时缓冲区)
        // 保守估算：文件大小 × 15倍
        let estimated_raw_audio = file_size * 15;

        // f32样本数组：通常是原始音频的1-2倍
        let estimated_samples = estimated_raw_audio;

        // 峰值内存：样本 + 块缓冲 + 其他开销
        let estimated_peak = estimated_samples + (50 * 1024 * 1024); // +50MB开销

        let recommended_strategy = self.select_strategy(estimated_peak, file_size);

        Ok(MemoryEstimate {
            raw_audio_bytes: estimated_raw_audio,
            samples_memory: estimated_samples,
            peak_memory: estimated_peak,
            recommended_strategy,
        })
    }

    /// 根据内存估算选择最优策略
    fn select_strategy(&self, estimated_peak: u64, file_size: u64) -> ProcessingStrategy {
        // 策略1: 小文件直接全内存加载
        if file_size < 200 * 1024 * 1024 && estimated_peak < self.memory_limit {
            return ProcessingStrategy::FullMemory;
        }

        // 策略2: 超大文件或内存不足，强制流式处理
        if estimated_peak > self.memory_limit || file_size > 2 * 1024 * 1024 * 1024 {
            return ProcessingStrategy::StreamingBlocks;
        }

        // 策略3: 中等大小文件，使用自适应模式
        ProcessingStrategy::Adaptive
    }

    /// 获取系统可用内存 (字节)
    fn get_available_memory() -> u64 {
        // 使用dynamic_memory模块的精确内存检测，而不是硬编码值
        use crate::utils::dynamic_memory::DynamicMemoryManager;

        // 创建动态内存管理器来获取精确的内存信息
        let manager = DynamicMemoryManager::new();

        // 获取当前配置并返回可用内存
        if let Ok(config) = manager.refresh_memory_status() {
            config.current_memory_bytes
        } else {
            // 回退到保守估算（仅在无法获取精确信息时使用）
            4 * 1024 * 1024 * 1024 // 4GB保守估算
        }
    }

    /// 验证策略是否安全
    pub fn validate_strategy(&self, estimate: &MemoryEstimate) -> AudioResult<()> {
        match estimate.recommended_strategy {
            ProcessingStrategy::FullMemory => {
                // 全内存模式需要检查峰值内存是否超限
                if estimate.peak_memory > self.memory_limit {
                    return Err(AudioError::InvalidInput(format!(
                        "文件过大：全内存模式需要{:.1}GB，超过安全限制{:.1}GB",
                        estimate.peak_memory as f64 / (1024.0 * 1024.0 * 1024.0),
                        self.memory_limit as f64 / (1024.0 * 1024.0 * 1024.0)
                    )));
                }
            }
            ProcessingStrategy::StreamingBlocks => {
                // 流式模式只需50MB恒定内存，基本不会超限
                let streaming_memory = 50 * 1024 * 1024; // 50MB
                if streaming_memory > self.available_memory {
                    return Err(AudioError::InvalidInput(format!(
                        "系统内存不足：流式处理需要50MB，可用内存{:.1}GB",
                        self.available_memory as f64 / (1024.0 * 1024.0 * 1024.0)
                    )));
                }
            }
            ProcessingStrategy::Adaptive => {
                // 自适应模式会智能选择，无需特殊验证
            }
        }

        Ok(())
    }
}

impl Default for MemoryStrategySelector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_memory_strategy_selection() {
        let selector = MemoryStrategySelector::new();

        // 测试应该覆盖不同大小的策略选择
        assert!(selector.available_memory > 0);
        assert!(selector.memory_limit > 0);
        assert!(selector.memory_limit <= selector.available_memory);
    }

    #[test]
    fn test_strategy_for_small_files() {
        let selector = MemoryStrategySelector::new();

        // 创建小文件进行测试
        let temp_path = "/tmp/small_test_audio.dat";
        {
            let mut file = std::fs::File::create(temp_path).unwrap();
            file.write_all(&[0u8; 1024 * 1024]).unwrap(); // 1MB文件
        }

        let estimate = selector.analyze_file(temp_path).unwrap();
        // 1MB文件应该选择全内存模式
        assert_eq!(
            estimate.recommended_strategy,
            ProcessingStrategy::FullMemory
        );

        // 清理
        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn test_strategy_for_large_files() {
        let selector = MemoryStrategySelector::new();

        // 模拟大文件（不实际创建）
        let large_file_size = 3 * 1024 * 1024 * 1024u64; // 3GB
        let estimated_peak = large_file_size * 15; // 15x放大系数

        let strategy = selector.select_strategy(estimated_peak, large_file_size);

        // 3GB文件应该选择流式处理
        assert_eq!(strategy, ProcessingStrategy::StreamingBlocks);
    }
}
