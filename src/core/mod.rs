//! 核心算法模块
//!
//! 包含DR计算的核心数据结构和算法实现。

pub mod dr_calculator;
pub mod histogram;
pub mod peak_selection;

// 重新导出公共接口
pub use dr_calculator::{DrCalculator, DrResult};
pub use peak_selection::{PeakSelectionStrategy, PeakSelector};
// SimpleHistogramAnalyzer和SimpleStats已删除，不再导出
// ChannelData已移动到processing层，不再从此导出
