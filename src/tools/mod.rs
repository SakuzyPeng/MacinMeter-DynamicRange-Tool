//! Tools 模块 - 稳定公开 API 层（Public Facade）
//!
//! 本模块作为 tools 层的统一入口，仅导出稳定的公开 API。
//! 内部实现细节（如格式化辅助函数、内部工具函数）保留在各子模块命名空间中。
//!
//! # API 稳定性说明
//!
//! ## 稳定 API（保证向后兼容）
//! - 配置和流程控制：`AppConfig`, `parse_args`, `show_*`
//! - 核心处理函数：`process_single_audio_file`, `output_results`, `process_streaming_decoder`
//! - DR 计算：`calculate_official_dr`, `compute_official_precise_dr` (测试/插件使用)
//! - 批处理入口：`process_batch_parallel`
//! - 文件扫描：`scan_audio_files`, `show_scan_results`
//! - 统计类型：`BatchStatsSnapshot`, `SerialBatchStats`, `ParallelBatchStats`
//! - 工具函数模块：`audio` (dB转换), `path` (路径处理) - 测试使用
//!
//! ## 内部 API（仅供子模块使用）
//! - 格式化细节：`formatter::create_output_header`, `formatter::format_dr_results_by_channel_count`
//! - 工具函数：`utils::extract_filename_lossy`, `utils::effective_parallel_degree`
//!
//! 使用内部 API 时，请通过完整模块路径访问（如 `tools::utils::...`）。

// ========== 子模块声明 ==========
pub mod batch_state;
pub mod cli;
pub mod constants;
pub mod formatter;
pub mod parallel_processor;
pub mod processor;
pub mod scanner;
pub mod utils;

// ========== 稳定公开 API 导出 ==========

// --- 配置和流程控制 ---
pub use cli::{AppConfig, parse_args, show_completion_info, show_startup_info};

// --- 核心处理函数 ---
pub use processor::{
    BatchExclusionStats, add_failed_to_batch_output, add_to_batch_output, output_results,
    process_single_audio_file, process_streaming_decoder, save_individual_result,
};

// --- DR 计算（测试和插件使用）---
pub use formatter::{calculate_official_dr, compute_official_precise_dr};

// --- 批处理 ---
pub use parallel_processor::process_batch_parallel;
pub use scanner::{
    create_batch_output_footer, create_batch_output_header, finalize_and_write_batch_output,
    generate_batch_output_path,
};

// --- 文件扫描 ---
pub use scanner::{scan_audio_files, show_scan_results};

// --- 统计管理 ---
pub use batch_state::{BatchStatsSnapshot, ParallelBatchStats, SerialBatchStats};

// --- 工具函数（测试使用）---
pub use utils::{audio, path};
