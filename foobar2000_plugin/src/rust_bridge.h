#pragma once

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

// DR分析结果结构（与Rust端匹配）
typedef struct DrAnalysisResult {
    double official_dr_value;       // 整体官方DR值
    double precise_dr_value;        // 整体精确DR值
    double peak_db;                 // 整体Peak值
    double rms_db;                  // 整体RMS值
    unsigned int channel;           // 声道索引（兼容旧接口）
    unsigned int sample_rate;       // 采样率
    unsigned int channels;          // 总声道数
    unsigned int bits_per_sample;   // 位深度
    double duration_seconds;        // 时长（秒）
    char file_name[256];            // 文件名
    char codec[32];                 // 编解码器

    // 每声道明细数组 (最大支持8声道)
    double peak_db_per_channel[8];      // 每声道Peak值
    double rms_db_per_channel[8];       // 每声道RMS值
    double dr_db_per_channel[8];        // 每声道DR值
    double rms_top20_linear_per_channel[8]; // 每声道20%RMS线性值
    int peak_source_per_channel[8];     // 峰值来源：0=主峰,1=次峰,2=回退
    unsigned int total_samples;         // 总样本数（真实值）
} DrAnalysisResult;

// DR批量分析结果
typedef struct DrBatchResult {
    DrAnalysisResult* results;     // 结果数组
    size_t count;                  // 结果数量
    double average_dr;             // 平均DR值
    size_t processed_files;        // 处理成功的文件数
    size_t failed_files;           // 处理失败的文件数
} DrBatchResult;

// 进度回调函数类型
typedef void (*ProgressCallback)(const char* status, int current, int total);

// 初始化和清理
int rust_dr_engine_init(void);
void rust_dr_engine_cleanup(void);

// 分析解码后的音频数据（使用foobar2000解码器）
int rust_analyze_audio_data(const float* audio_data, size_t sample_count, 
                            unsigned int channels, unsigned int sample_rate, 
                            const char* file_name, DrAnalysisResult* result);

// 批量DR分析（使用foobar2000解码器）
int rust_analyze_audio_batch_data(const float** audio_data_list, const size_t* sample_counts,
                                  const unsigned int* channels_list, const unsigned int* sample_rates,
                                  const char** file_names, size_t count, 
                                  DrBatchResult* result, ProgressCallback callback);

// 释放批量分析结果内存
void rust_free_batch_result(DrBatchResult* result);

// 获取最后的错误信息
const char* rust_get_last_error(void);

// 获取引擎版本信息
const char* rust_get_engine_version(void);

// 获取支持的音频格式
const char* rust_get_supported_formats(void);

// 设置分析参数
int rust_set_analysis_params(int enable_simd, int enable_sum_doubling, int packet_chunk_mode);

// 获取当前分析参数
int rust_get_analysis_params(int* enable_simd, int* enable_sum_doubling, int* packet_chunk_mode);

// 会话式DR分析接口（用于流式处理）
void* dr_session_new(unsigned int channels, unsigned int sample_rate, int enable_sum_doubling);
int dr_session_feed_interleaved(void* session, const float* samples, unsigned int frame_count);
int dr_session_finalize(void* session, DrAnalysisResult* result);
void dr_session_free(void* session);

#ifdef __cplusplus
}
#endif