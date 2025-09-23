#pragma once
#include "foobar2000.h"
#include <vector>

// 🎯 简化的音频数据结构（纯解码输出）
struct AudioData {
    std::vector<float> samples;     // 解码后的音频样本（浮点格式）
    uint32_t sample_rate;           // 采样率
    uint32_t channels;              // 声道数
    size_t sample_count;            // 总样本数
    double duration;                // 时长（秒）
};

/**
 * 🎯 音频文件访问器类 - 专职音频解码服务
 *
 * 单一职责：使用foobar2000解码器将音频文件解码为标准化样本数据
 * 不负责：元数据处理、批量操作、诊断统计、DR分析
 */
class AudioAccessor {
  public:
    // 🎯 核心解码接口：解码单个音频文件
    AudioData decode_audio_data(const metadb_handle_ptr& handle);

  private:
    // 核心解码实现
    void decode_audio_samples(const metadb_handle_ptr& handle, AudioData& audio);
};