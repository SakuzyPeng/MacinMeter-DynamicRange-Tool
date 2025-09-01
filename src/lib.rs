//! MacinMeter Dynamic Range (DR) Analysis Tool  
//!
//! 基于 Measuring_DR_ENv3.md 标准实现的高精度音频动态范围计算工具。
//! 以 dr14_t.meter 项目作为参考实现，提供专业级DR测量算法。
//!
//! ## 核心特性
//! - 符合 Measuring_DR_ENv3.md 规范的DR计算算法
//! - 高精度RMS计算：RMS = sqrt(2 * Σ(smp²)/n)  
//! - 第二大Peak值选择（Pk_2nd）机制
//! - 10000-bin直方图统计和20%采样
//! - 3秒窗口RMS分析和上位20%统计
//! - SIMD向量化优化和多线程处理

pub mod audio;
pub mod core;
pub mod error;
pub mod processing;
pub mod utils;

// 重新导出核心类型
pub use audio::{AudioFormat, MultiDecoder, WavDecoder};
pub use core::dr_calculator::DrResult;
pub use core::{ChannelData, DrCalculator};
pub use error::{AudioError, AudioResult};
pub use processing::{BatchProcessor, SimdChannelData, SimdProcessor};
pub use utils::safety::SafeRunner;
