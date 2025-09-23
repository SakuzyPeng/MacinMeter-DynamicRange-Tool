#pragma once

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

// 🚀 统一FFI接口：直接返回格式化的DR分析报告（100%复用主项目formatter）
// 返回值: 0=成功, -1=无效参数, -2=计算失败, -3=缓冲区太小, -5=声道数超限(>2)
int rust_format_dr_analysis(const float* samples, unsigned int sample_count,
                           unsigned int channels, unsigned int sample_rate,
                           unsigned int bits_per_sample,
                           char* output_buffer, unsigned int buffer_size);

#ifdef __cplusplus
}
#endif