//! 工具模块集合
//!
//! 包含CLI、文件处理、格式化等工具模块，支持main.rs的流程控制。

pub mod cli;
pub mod constants;
pub mod formatter;
pub mod parallel_processor;
pub mod processor;
pub mod scanner;
pub mod utils;

// 重新导出主要的公共接口
pub use cli::{AppConfig, parse_args, show_completion_info, show_startup_info};
pub use formatter::{
    calculate_official_dr, compute_official_precise_dr, create_output_header,
    format_dr_results_by_channel_count, write_output,
};
pub use parallel_processor::process_batch_parallel;
pub use processor::{
    add_failed_to_batch_output, add_to_batch_output, output_results, process_audio_file_streaming,
    process_single_audio_file, process_streaming_decoder, save_individual_result,
};
pub use scanner::{
    create_batch_output_footer, create_batch_output_header, finalize_and_write_batch_output,
    generate_batch_output_path, scan_audio_files, show_batch_completion_info, show_scan_results,
};
pub use utils::{audio, path};
