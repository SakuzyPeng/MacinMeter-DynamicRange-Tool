#pragma once
#include "foobar2000.h"
#include <functional>
#include <vector>

// ❌ 已移除：AudioData结构体（冗余，未被使用）
//
// 原因：现在采用100%流式处理，不再需要存储音频数据：
// - 样本数据直接通过回调传递，无需存储
// - 音频信息通过AudioInfo获取
// - 结果显示直接在ProgressWorker中实现

// 🎯 基础音频信息结构（不包含样本数据）
struct AudioInfo {
    uint32_t sample_rate;
    uint32_t channels;
    uint32_t bits_per_sample; // 🔧 添加位深度信息
    double duration;
};

// 🎯 解码进度回调类型定义
typedef std::function<void(float progress, const char* message)> DecodeProgressCallback;

// 🌊 流式解码回调类型定义 - 每个解码块立即处理
// first_chunk: 是否为第一个chunk, audio_info: 音频格式信息(仅第一个chunk有效)
typedef std::function<bool(const float* samples, size_t sample_count, bool first_chunk,
                           const AudioInfo* audio_info)>
    StreamingChunkCallback;

/**
 * 🎯 音频文件访问器类 - 专职音频解码服务
 *
 * 单一职责：使用foobar2000解码器将音频文件解码为标准化样本数据
 * 不负责：元数据处理、批量操作、诊断统计、DR分析
 */
class AudioAccessor {
  public:
    // 🎯 获取音频基础信息（无解码，快速获取）
    AudioInfo get_audio_info(const metadb_handle_ptr& handle);

    // 🌊 流式解码接口：零内存占用，每个chunk立即回调处理
    bool decode_with_streaming_callback(const metadb_handle_ptr& handle, abort_callback& abort,
                                        const StreamingChunkCallback& chunk_callback);

    // ❌ 已移除：传统全量加载接口会导致长音频内存占用过大
    // 请使用 decode_with_streaming_callback() 进行零内存占用的流式解码

  private:
    // ❌ 已移除：私有的全量解码实现
    // 现在统一使用流式解码，避免内存累积
};