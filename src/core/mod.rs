//! 核心算法模块
//!
//! 包含DR计算的核心数据结构和算法实现。

pub mod channel_data;
pub mod dr_calculator;
pub mod histogram;

// 重新导出公共接口
pub use channel_data::{ChannelData, PeakQuality, PeakQualityFlags};
pub use dr_calculator::{DrCalculator, DrResult, SumDoublingIssues, SumDoublingQuality};
pub use histogram::{SimpleHistogramAnalyzer, SimpleStats};
