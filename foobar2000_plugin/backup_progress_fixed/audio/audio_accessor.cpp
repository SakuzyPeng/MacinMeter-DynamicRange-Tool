#include "audio_accessor.h"
#include "foobar2000.h"

AudioData AudioAccessor::decode_audio_data(const metadb_handle_ptr& handle) {
    AudioData audio = {};

    if (!handle.is_valid()) {
        console::print("MacinMeter DR AudioAccessor: Invalid handle");
        return audio;
    }

    const char* file_path = handle->get_path();
    if (!file_path) {
        console::print("MacinMeter DR AudioAccessor: Failed to get file path");
        return audio;
    }

    // 🎯 专职音频解码
    decode_audio_samples(handle, audio);

    return audio;
}

void AudioAccessor::decode_audio_samples(const metadb_handle_ptr& handle, AudioData& audio) {
    try {
        // 使用foobar2000的input_decoder
        service_ptr_t<input_decoder> decoder;
        abort_callback_dummy abort;

        const char* file_path = handle->get_path();
        input_entry::g_open_for_decoding(decoder, nullptr, file_path, abort);

        if (!decoder.is_valid()) {
            throw std::runtime_error("Failed to create decoder");
        }

        // 初始化解码器
        decoder->initialize(0, input_flag_simpledecode, abort);

        // 🎯 专职音频解码：收集所有样本数据
        audio_chunk_impl chunk;
        bool first_chunk = true;
        std::vector<float> all_samples;
        all_samples.reserve(1024 * 1024); // 预分配1M样本

        while (decoder->run(chunk, abort)) {
            if (first_chunk) {
                // 从第一个chunk获取音频格式信息
                audio.sample_rate = chunk.get_sample_rate();
                audio.channels = chunk.get_channels();

                // 🔥 声道数限制检查（与系统限制一致）
                if (audio.channels > 2) {
                    throw std::runtime_error("仅支持单声道和立体声文件 (1-2声道)，当前文件为" +
                                            std::to_string(audio.channels) + "声道。多声道支持正在开发中。");
                }

                first_chunk = false;
            }

            // 转换audio_sample(double)到float并累积到缓冲区
            const audio_sample* chunk_data = chunk.get_data();
            size_t chunk_samples = chunk.get_sample_count();

            // 累积样本到总缓冲区
            all_samples.reserve(all_samples.size() + chunk_samples);
            for (size_t j = 0; j < chunk_samples; ++j) {
                all_samples.push_back(static_cast<float>(chunk_data[j]));
            }
        }

        // 🎯 填充AudioData结果（纯解码输出）
        if (!all_samples.empty()) {
            audio.samples = std::move(all_samples);
            audio.sample_count = audio.samples.size();

            // 计算时长
            if (audio.sample_rate > 0 && audio.channels > 0) {
                unsigned int frames = audio.sample_count / audio.channels;
                audio.duration = (double)frames / audio.sample_rate;
            }
        } else {
            throw std::runtime_error("No audio samples collected during decoding");
        }

    } catch (const std::exception& e) {
        console::printf("MacinMeter DR AudioAccessor: Error decoding audio: %s", e.what());

        // 确保在错误情况下清理数据
        audio = {};
    }
}