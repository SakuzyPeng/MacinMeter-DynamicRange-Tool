//! 工具模块
//!
//! 提供安全性保障和辅助功能。

pub mod dynamic_memory;
pub mod memory_analysis;
pub mod memory_strategy;
pub mod safety;

// 重新导出公共接口
pub use dynamic_memory::{
    DynamicMemoryConfig, DynamicMemoryManager, MemoryTier, get_adaptive_memory_for_format,
    get_memory_status_report, should_use_emergency_mode,
};
pub use memory_strategy::{MemoryEstimate, MemoryStrategySelector, ProcessingStrategy};
pub use safety::SafetyGuard;
