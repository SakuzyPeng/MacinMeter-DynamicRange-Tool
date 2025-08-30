//! MacinMeter Dynamic Range (DR) Analysis Tool
//!
//! 基于foobar2000 DR Meter逆向分析的高精度音频动态范围计算工具。
//! 实现24字节ChannelData结构、Sum Doubling补偿机制和双Peak回退系统。

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
