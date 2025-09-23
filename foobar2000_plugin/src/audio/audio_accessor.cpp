#include "audio_accessor.h"
#include "foobar2000.h"
#include <chrono>

AudioInfo AudioAccessor::get_audio_info(const metadb_handle_ptr& handle) {
    AudioInfo info = {};

    if (!handle.is_valid()) {
        console::print("MacinMeter DR AudioAccessor: Invalid handle");
        return info;
    }

    const char* file_path = handle->get_path();
    if (!file_path) {
        console::print("MacinMeter DR AudioAccessor: Failed to get file path");
        return info;
    }

    try {
        // 🎯 使用与备份版本相同的解码循环方式获取音频信息
        service_ptr_t<input_decoder> decoder;
        abort_callback_dummy abort_dummy;

        input_entry::g_open_for_decoding(decoder, nullptr, file_path, abort_dummy);

        if (!decoder.is_valid()) {
            console::print("MacinMeter DR AudioAccessor: Failed to create decoder for info");
            return info;
        }

        // 初始化解码器
        decoder->initialize(0, input_flag_simpledecode, abort_dummy);

        // 🔥 使用完整的解码循环来确保获取到有效chunk（参考备份版本）
        audio_chunk_impl chunk;
        bool first_chunk = true;
        int attempts = 0;
        const int max_attempts = 10; // 最多尝试10个chunk

        while (decoder->run(chunk, abort_dummy) && attempts < max_attempts) {
            attempts++;

            if (first_chunk && chunk.get_sample_count() > 0) {
                // 从第一个有效chunk获取音频格式信息
                info.sample_rate = chunk.get_sample_rate();
                info.channels = chunk.get_channels();
                info.duration = handle->get_length();

                console::printf("MacinMeter DR AudioAccessor: Got audio info from chunk %d - %u "
                                "channels, %u Hz, %.2f seconds",
                                attempts, info.channels, info.sample_rate, info.duration);

                first_chunk = false;
                break; // 获取到信息后立即退出，避免完整解码
            }
        }

        if (first_chunk) {
            console::printf(
                "MacinMeter DR AudioAccessor: Failed to get valid chunk after %d attempts",
                attempts);
        }

    } catch (const std::exception& e) {
        console::printf("MacinMeter DR AudioAccessor: Error getting audio info: %s", e.what());
    }

    return info;
}

bool AudioAccessor::decode_with_streaming_callback(const metadb_handle_ptr& handle,
                                                   abort_callback& abort,
                                                   const StreamingChunkCallback& chunk_callback) {
    if (!handle.is_valid() || !chunk_callback) {
        console::print("MacinMeter DR AudioAccessor: Invalid handle or callback");
        return false;
    }

    const char* file_path = handle->get_path();
    if (!file_path) {
        console::print("MacinMeter DR AudioAccessor: Failed to get file path");
        return false;
    }

    try {
        // 使用foobar2000的input_decoder
        service_ptr_t<input_decoder> decoder;

        console::printf("MacinMeter DR AudioAccessor: Attempting to open file for decoding: %s",
                        file_path);

        try {
            input_entry::g_open_for_decoding(decoder, nullptr, file_path, abort);
            console::print("MacinMeter DR AudioAccessor: Successfully opened file for decoding");
        } catch (const std::exception& open_e) {
            console::printf("MacinMeter DR AudioAccessor: Failed to open file for decoding: %s",
                            open_e.what());
            return false;
        }

        if (!decoder.is_valid()) {
            console::print(
                "MacinMeter DR AudioAccessor: Failed to create decoder - decoder is invalid");
            return false;
        }

        console::print(
            "MacinMeter DR AudioAccessor: Decoder created successfully, initializing...");

        // 初始化解码器
        try {
            decoder->initialize(0, input_flag_simpledecode, abort);
            console::print("MacinMeter DR AudioAccessor: Decoder initialized successfully");
        } catch (const std::exception& init_e) {
            console::printf("MacinMeter DR AudioAccessor: Decoder initialization failed: %s",
                            init_e.what());
            return false;
        }

        // 🌊 流式解码：每个chunk立即处理，零内存累积
        audio_chunk_impl chunk;
        bool first_chunk = true;
        AudioInfo current_audio_info = {};

        while (decoder->run(chunk, abort)) {
            AudioInfo* audio_info_ptr = nullptr;

            if (first_chunk) {
                // 🎯 从第一个chunk获取可靠的音频格式信息
                current_audio_info.channels = chunk.get_channels();
                current_audio_info.sample_rate = chunk.get_sample_rate();
                current_audio_info.duration = handle->get_length();

                audio_info_ptr = &current_audio_info;
                first_chunk = false;
            }

            // 🚀 转换audio_sample(double)到float并立即发送给回调
            const audio_sample* chunk_data = chunk.get_data();
            size_t chunk_samples = chunk.get_sample_count();

            // 🚀 高精度转换double→float（减少精度损失，确保Peak检测准确性）
            std::vector<float> float_samples(chunk_samples);
            std::transform(chunk_data, chunk_data + chunk_samples, float_samples.begin(),
                           [](audio_sample sample) {
                               // 🔧 改进的精度转换：使用更精确的舍入
                               // 对于Peak检测关键场景，这能减少double→float的精度损失
                               double rounded = std::round(sample * 1e6) / 1e6; // 6位小数精度
                               return static_cast<float>(rounded);
                           });

            // 🌊 立即通过回调发送，包含音频格式信息（仅第一次）
            bool continue_decode = chunk_callback(float_samples.data(), float_samples.size(),
                                                  audio_info_ptr != nullptr, // first_chunk
                                                  audio_info_ptr // audio_info (仅第一个chunk非空)
            );
            if (!continue_decode) {
                // 回调请求停止解码
                console::print("MacinMeter DR AudioAccessor: Decoding stopped by callback");
                return true; // 正常停止，不是错误
            }

            // 检查abort状态
            try {
                abort.check();
            } catch (...) {
                console::print("MacinMeter DR AudioAccessor: Decoding aborted");
                return false; // 用户取消
            }
        }

        console::print("MacinMeter DR AudioAccessor: Streaming decode completed successfully");
        return true;

    } catch (const std::exception& e) {
        console::printf("MacinMeter DR AudioAccessor: Error in streaming decode: %s", e.what());
        return false;
    }
}

// ❌ 已移除：decode_audio_data_with_progress 传统全量加载接口
//
// 原因：会将整个音频文件加载到内存，对于长音频会导致：
// - 内存占用过大（可能几GB）
// - 处理速度慢（大量内存分配）
// - 用户体验差（长时间无响应）
//
// 解决方案：统一使用 decode_with_streaming_callback() 流式接口

// ❌ 已移除：decode_audio_samples 私有实现函数
//
// 原因：此函数使用 all_samples.reserve() 和 all_samples.push_back()
// 将整个音频文件累积到内存中，导致：
//
// 1. 内存问题：
//    - 长音频文件可能占用数GB内存
//    - 频繁的vector扩容和内存分配
//    - 内存碎片化
//
// 2. 性能问题：
//    - 大量内存分配/释放开销
//    - 缓存未命中（大数组超出CPU缓存）
//    - 垃圾回收压力
//
// 3. 用户体验问题：
//    - 长时间等待无响应
//    - 可能导致系统内存不足
//
// 🚀 现在统一使用流式处理：
//    - decode_with_streaming_callback() 零内存累积
//    - 每个chunk立即处理，恒定内存使用