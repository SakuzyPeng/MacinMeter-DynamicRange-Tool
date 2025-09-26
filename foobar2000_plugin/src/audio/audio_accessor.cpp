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

                // 🔧 使用foobar2000 SDK标准API获取音频格式信息
                info.bits_per_sample = 24; // FLAC 24-bit默认值
                try {
                    file_info_impl file_info;
                    handle->get_info(file_info);

                    // 🎯 使用SDK标准API获取bitrate
                    t_int64 bitrate = file_info.info_get_bitrate(); // 标准API
                    if (bitrate > 0) {
                        console::printf("MacinMeter DR AudioAccessor: 获取bitrate: %d kbps",
                                        (int)bitrate);
                    }

                    // 🎯 尝试获取bits_per_sample（使用多种常见键名）
                    const char* bps_str = nullptr;
                    const char* bps_keys[] = {"bitspersample", "bits_per_sample", "BITSPERSAMPLE",
                                              "BPS"};
                    for (const char* key : bps_keys) {
                        bps_str = file_info.info_get(key);
                        if (bps_str && strlen(bps_str) > 0) {
                            int parsed_bps = std::atoi(bps_str);
                            if (parsed_bps > 0) {
                                info.bits_per_sample = (uint32_t)parsed_bps;
                                console::printf("MacinMeter DR AudioAccessor: 获取%s: %u bits", key,
                                                info.bits_per_sample);
                                break;
                            }
                        }
                    }

                    // 🔍 调试：输出所有可用的info键
                    console::printf("MacinMeter DR AudioAccessor: 可用info键数量: %u",
                                    (unsigned int)file_info.info_get_count());
                    for (t_size i = 0; i < file_info.info_get_count() && i < 10; i++) {
                        const char* name = file_info.info_enum_name(i);
                        const char* value = file_info.info_enum_value(i);
                        console::printf("MacinMeter DR AudioAccessor: info[%u]: %s = %s",
                                        (unsigned int)i, name ? name : "null",
                                        value ? value : "null");
                    }

                } catch (const std::exception& e) {
                    console::printf("MacinMeter DR AudioAccessor: 获取音频格式信息失败: %s",
                                    e.what());
                }

                console::printf("MacinMeter DR AudioAccessor: Audio info - %u channels, %u Hz, %u "
                                "bits, %u seconds",
                                info.channels, info.sample_rate, info.bits_per_sample,
                                (unsigned int)info.duration);

                // 🔍 额外的元数据检查
                try {
                    file_info_impl file_info;
                    handle->get_info(file_info);
                    console::printf("MacinMeter DR AudioAccessor: 文件元数据时长 = %u秒 (%u分钟)",
                                    (unsigned int)info.duration,
                                    (unsigned int)(info.duration / 60.0));
                } catch (...) {
                    console::print("MacinMeter DR AudioAccessor: 无法获取文件元数据");
                }

                console::printf("MacinMeter DR AudioAccessor: Got audio info from chunk %d - %u "
                                "channels, %u Hz, %u bits, %u seconds",
                                attempts, info.channels, info.sample_rate, info.bits_per_sample,
                                (unsigned int)info.duration);

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
            // 🎯 尝试不同的初始化参数
            // 1. 从文件开头开始，使用标准解码模式
            decoder->initialize(0, input_flag_no_looping, abort);
            console::print("MacinMeter DR AudioAccessor: Decoder initialized with no_looping flag");

            // 🔍 获取解码器信息
            bool can_seek = decoder->can_seek();
            double length = handle->get_length(); // 从handle获取而不是decoder
            console::printf(
                "MacinMeter DR AudioAccessor: 解码器信息 - can_seek: %s, 文件时长: %u秒 (%u分钟)",
                can_seek ? "true" : "false", (unsigned int)length, (unsigned int)(length / 60.0));

        } catch (const std::exception& init_e) {
            console::printf("MacinMeter DR AudioAccessor: Decoder initialization failed: %s",
                            init_e.what());
            return false;
        }

        // 🌊 流式解码：每个chunk立即处理，零内存累积
        audio_chunk_impl chunk;
        bool first_chunk = true;
        AudioInfo current_audio_info = {};
        size_t total_chunks_decoded = 0;
        size_t total_samples_decoded = 0;

        console::print("MacinMeter DR AudioAccessor: 开始流式解码循环...");

        while (decoder->run(chunk, abort)) {
            total_chunks_decoded++;
            size_t chunk_sample_count = chunk.get_sample_count();
            total_samples_decoded += chunk_sample_count;
            AudioInfo* audio_info_ptr = nullptr;

            // 🔍 只在前几个chunk输出详细信息
            if (total_chunks_decoded <= 3) {
                console::printf("MacinMeter DR AudioAccessor: Chunk #%u - %u samples",
                                (unsigned int)total_chunks_decoded,
                                (unsigned int)chunk_sample_count);
            }

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

            // 🚀 原始精度转换double→float（与主项目完全一致）
            std::vector<float> float_samples(chunk_samples);
            std::transform(chunk_data, chunk_data + chunk_samples, float_samples.begin(),
                           [](audio_sample sample) {
                               // 🎯 直接转换，保持与主项目相同的原始精度
                               // 移除人为的精度舍入，避免与主项目产生结果差异
                               return static_cast<float>(sample);
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

            // 🔍 每10000个chunk报告一次进度（减少输出）
            if (total_chunks_decoded % 10000 == 0 && current_audio_info.sample_rate > 0) {
                // 🔧 修复：total_samples_decoded是interleaved总样本数，不应除以声道数
                double current_duration =
                    total_samples_decoded / (current_audio_info.sample_rate * 1.0);
                console::printf("MacinMeter DR AudioAccessor: 解码进度 - %u chunks, %u分钟",
                                (unsigned int)total_chunks_decoded,
                                (unsigned int)(current_duration / 60.0));
            }
        }

        console::printf("MacinMeter DR AudioAccessor: 解码循环结束 - decoder->run() 返回 false");
        // 🔧 修复：total_samples_decoded是每声道样本数，不是总interleaved样本数
        console::printf(
            "MacinMeter DR AudioAccessor: 🔍 预期样本数: %u (93分钟完整), 实际解码: %u (%.1f%%)",
            (unsigned int)(93 * 60 * 96000), (unsigned int)total_samples_decoded,
            total_samples_decoded * 100.0 / (93 * 60 * 96000));

        // 📊 解码完成统计
        if (current_audio_info.sample_rate > 0) {
            // 🔧 修复：total_samples_decoded是每声道样本数，直接除以采样率
            double final_duration = total_samples_decoded / (current_audio_info.sample_rate * 1.0);
            console::printf(
                "MacinMeter DR AudioAccessor: 解码完成 - 总共%u chunks, %u samples, %u秒 (%u分钟)",
                (unsigned int)total_chunks_decoded, (unsigned int)total_samples_decoded,
                (unsigned int)final_duration, (unsigned int)(final_duration / 60.0));
        } else {
            console::printf(
                "MacinMeter DR AudioAccessor: 解码完成 - 总共%u chunks, %u samples, 未知时长",
                (unsigned int)total_chunks_decoded, (unsigned int)total_samples_decoded);
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