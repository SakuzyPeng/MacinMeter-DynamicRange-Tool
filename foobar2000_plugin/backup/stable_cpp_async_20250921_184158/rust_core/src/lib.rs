//! MacinMeter DR Plugin - 统一FFI适配层
//!
//! 🚀 统一FFI适配层：为foobar2000插件提供格式化字符串接口
//!
//! ## 设计原则
//! - **零算法原则**：100%复用主项目DrCalculator
//! - **零向后兼容**：单一统一接口，拒绝冗余实现
//! - **统一格式化**：直接复用主项目formatter，确保输出100%一致
//! - **安全边界原则**：确保跨FFI边界的内存安全

use std::os::raw::{c_char, c_int, c_uint};

// 🎯 引入主项目核心：100%复用算法和格式化
use macinmeter_dr_tool::{AudioFormat, DrCalculator};
use macinmeter_dr_tool::tools::formatter;

/// 🚀 革命性简化FFI接口：直接返回格式化的DR分析报告
///
/// ## 设计理念
/// - 复用主项目formatter，零代码重复
/// - UI层直接显示，无需C++端格式化
/// - 保证插件与主程序输出完全一致
///
/// ## 安全要求
/// - `samples` 必须指向至少 `sample_count` 个有效的f32样本
/// - `output_buffer` 必须至少有 `buffer_size` 字节容量
/// - 调用者负责内存管理
#[no_mangle]
pub unsafe extern "C" fn rust_format_dr_analysis(
    samples: *const f32,
    sample_count: c_uint,
    channels: c_uint,
    sample_rate: c_uint,
    bits_per_sample: c_uint,
    output_buffer: *mut c_char,
    buffer_size: c_uint,
) -> c_int {
    // 🛡️ FFI边界安全检查
    if samples.is_null() || output_buffer.is_null() ||
       sample_count == 0 || channels == 0 || buffer_size == 0 {
        return -1; // 无效参数
    }

    // 🔥 声道数限制检查
    if channels > 2 {
        return -5; // 超出声道限制（仅支持1-2声道）
    }

    // 1️⃣ 类型转换：C指针 → Rust安全类型
    let samples_slice = std::slice::from_raw_parts(samples, sample_count as usize);

    // 2️⃣ 调用主项目核心API（零重复实现）
    let calculator = match DrCalculator::new(channels as usize) {
        Ok(calc) => calc,
        Err(_) => return -2, // DrCalculator创建失败
    };

    let dr_results = match calculator.calculate_dr_from_samples(samples_slice, channels as usize) {
        Ok(results) => results,
        Err(_) => return -3, // DR计算失败
    };

    // 3️⃣ 创建AudioFormat用于格式化
    let audio_format = AudioFormat::new(
        sample_rate,
        channels as u16,
        bits_per_sample as u16,
        sample_count as u64,
    );

    // 4️⃣ 🚀 使用主项目formatter（零代码重复！）
    let formatted_result = formatter::format_dr_results_by_channel_count(&dr_results, &audio_format);

    // 5️⃣ 安全的字符串复制到C缓冲区
    let result_bytes = formatted_result.as_bytes();
    let copy_len = std::cmp::min(result_bytes.len(), (buffer_size - 1) as usize);

    std::ptr::copy_nonoverlapping(
        result_bytes.as_ptr(),
        output_buffer as *mut u8,
        copy_len,
    );

    // 确保null终止
    *output_buffer.add(copy_len) = 0;

    0 // 成功
}


