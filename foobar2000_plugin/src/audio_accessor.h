#pragma once
#include "foobar2000.h"
#include "rust_bridge.h"
#include <vector>
#include <string>
#include <algorithm>

// 包大小统计结构
struct ChunkStats {
    std::vector<size_t> chunk_sizes;  // 所有chunk的样本数
    size_t min_size = SIZE_MAX;       // 最小包大小
    size_t max_size = 0;              // 最大包大小
    double mean_size = 0.0;           // 平均包大小
    size_t median_size = 0;           // 中位数包大小
    size_t p95_size = 0;              // 95百分位包大小
    size_t p99_size = 0;              // 99百分位包大小
    size_t total_chunks = 0;          // 总包数
};

// 音频数据结构（使用foobar2000解码器）
struct AudioData {
    std::vector<float> samples; // 解码后的音频样本（浮点格式）
    std::string file_name;      // 文件名（不含路径）
    std::string title;          // 歌曲标题
    std::string artist;         // 艺术家
    std::string album;          // 专辑名
    double duration;            // 时长（秒）
    uint32_t sample_rate;       // 采样率
    uint32_t channels;          // 声道数
    size_t sample_count;        // 总样本数
};

// 音频文件访问器类（使用foobar2000解码器）
class AudioAccessor {
public:
    // 从foobar2000的metadb_handle列表解码音频数据
    std::vector<AudioData> decode_audio_data_list(const pfc::list_base_const_t<metadb_handle_ptr>& handles);
    
    // 从单个metadb_handle解码音频数据
    AudioData decode_audio_data(const metadb_handle_ptr& handle);

    // 直接进行DR分析并返回结果（逐包会话模式）
    std::vector<DrAnalysisResult> analyze_dr_data_list(const pfc::list_base_const_t<metadb_handle_ptr>& handles);
    DrAnalysisResult analyze_dr_data(const metadb_handle_ptr& handle);

private:
    // 辅助方法
    void extract_file_info(const metadb_handle_ptr& handle, AudioData& audio);
    void decode_audio_samples(const metadb_handle_ptr& handle, AudioData& audio);
    std::string get_safe_string(const file_info& info, const char* field);

    // 包大小统计方法
    void calculate_chunk_stats(ChunkStats& stats);
    void print_chunk_stats(const ChunkStats& stats, const std::string& file_name);
};