#include "rust_bridge.h"
#include "foobar2000.h"

// 🚀 声明统一Rust FFI函数（直接返回格式化字符串）
extern "C" {
int rust_format_dr_analysis(const float* samples, unsigned int sample_count,
                           unsigned int channels, unsigned int sample_rate,
                           unsigned int bits_per_sample,
                           char* output_buffer, unsigned int buffer_size);
}