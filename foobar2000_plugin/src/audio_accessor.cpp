#include "audio_accessor.h"
#include "foobar2000.h"
#include "rust_bridge.h"
#include <filesystem>

std::vector<AudioData>
AudioAccessor::decode_audio_data_list(const pfc::list_base_const_t<metadb_handle_ptr>& handles) {
    std::vector<AudioData> audio_data_list;
    audio_data_list.reserve(handles.get_count());

    for (t_size i = 0; i < handles.get_count(); ++i) {
        try {
            AudioData audio = decode_audio_data(handles[i]);
            if (!audio.samples.empty()) {
                audio_data_list.push_back(std::move(audio));
            }
        } catch (const std::exception& e) {
            console::printf("MacinMeter DR: Error decoding audio data: %s", e.what());
        }
    }

    return audio_data_list;
}

AudioData AudioAccessor::decode_audio_data(const metadb_handle_ptr& handle) {
    AudioData audio;

    if (!handle.is_valid()) {
        console::print("MacinMeter DR: Invalid handle");
        return audio;
    }

    // 获取文件路径
    const char* file_path = handle->get_path();
    if (!file_path) {
        console::print("MacinMeter DR: Failed to get file path");
        return audio;
    }

    // 提取文件名
    std::filesystem::path path(file_path);
    audio.file_name = path.filename().string();

    console::printf("MacinMeter DR: Starting foobar2000 decode for: %s", audio.file_name.c_str());

    // 提取元数据信息
    extract_file_info(handle, audio);

    // 使用foobar2000解码器解码音频数据
    decode_audio_samples(handle, audio);

    return audio;
}

void AudioAccessor::extract_file_info(const metadb_handle_ptr& handle, AudioData& audio) {
    try {
        // 使用简化的方法获取基本信息
        // 使用文件名作为标题
        std::filesystem::path path(audio.file_name);
        audio.title = path.stem().string();
        audio.artist = "";
        audio.album = "";
        audio.duration = 0.0;

        console::printf("MacinMeter DR: File info - title: %s", audio.title.c_str());
    } catch (const std::exception& e) {
        console::printf("MacinMeter DR: Error extracting file info: %s", e.what());

        // 如果获取元数据失败，使用文件名作为标题
        if (audio.title.empty()) {
            std::filesystem::path path(audio.file_name);
            audio.title = path.stem().string();
        }
        audio.duration = 0.0;
    }
}

void AudioAccessor::decode_audio_samples(const metadb_handle_ptr& handle, AudioData& audio) {
    try {
        console::printf("MacinMeter DR: Opening foobar2000 decoder for: %s",
                        audio.file_name.c_str());

        // 使用foobar2000的input_decoder
        service_ptr_t<input_decoder> decoder;
        abort_callback_dummy abort;

        const char* file_path = handle->get_path();
        input_entry::g_open_for_decoding(decoder, nullptr, file_path, abort);

        if (!decoder.is_valid()) {
            throw std::runtime_error("Failed to create decoder");
        }

        // 初始化解码器（第一个子歌曲，简单解码模式）
        decoder->initialize(0, input_flag_simpledecode, abort);

        console::printf(
            "MacinMeter DR: Decoder initialized, starting packet-by-packet DR analysis...");

        // 解码音频数据并实时进行DR计算（逐包直通模式）
        audio_chunk_impl chunk;
        bool first_chunk = true;
        size_t total_samples = 0;
        void* dr_session = nullptr;

        // 包大小统计
        ChunkStats chunk_stats;

        while (decoder->run(chunk, abort)) {
            if (first_chunk) {
                // 从第一个chunk获取音频格式信息
                audio.sample_rate = chunk.get_sample_rate();
                audio.channels = chunk.get_channels();

                console::printf("MacinMeter DR: Audio format - %uHz, %uch", audio.sample_rate,
                                audio.channels);

                // 创建DR分析会话（启用Sum Doubling）
                dr_session = dr_session_new(audio.channels, audio.sample_rate, 1);
                if (!dr_session) {
                    throw std::runtime_error("Failed to create DR analysis session");
                }

                console::printf("MacinMeter DR: DR analysis session created, beginning "
                                "packet-by-packet processing...");
                first_chunk = false;
            }

            // 转换audio_sample(double)到float并直接喂给DR计算引擎
            const audio_sample* chunk_data = chunk.get_data(); // audio_sample = double
            size_t chunk_samples = chunk.get_sample_count();

            // 创建float缓冲区（交错格式）
            std::vector<float> float_buffer;
            float_buffer.reserve(chunk_samples);

            for (size_t j = 0; j < chunk_samples; ++j) {
                float_buffer.push_back(static_cast<float>(chunk_data[j]));
            }

            // 统计包大小
            chunk_stats.chunk_sizes.push_back(chunk_samples);

            // 立即将此包数据喂给DR计算引擎（逐包直通）
            unsigned int frame_count = chunk_samples / audio.channels;
            int feed_result =
                dr_session_feed_interleaved(dr_session, float_buffer.data(), frame_count);

            if (feed_result != 0) {
                dr_session_free(dr_session);
                throw std::runtime_error("Failed to feed chunk data to DR analysis engine");
            }

            total_samples += chunk_samples;

            // 周期性日志输出
            if (total_samples % 100000 == 0) {
                console::printf("MacinMeter DR: Processed %zu samples in packet-by-packet mode...",
                                total_samples);
            }
        }

        // 计算并输出包大小统计信息
        if (!chunk_stats.chunk_sizes.empty()) {
            calculate_chunk_stats(chunk_stats);
            print_chunk_stats(chunk_stats, audio.file_name);
        }

        // 完成分析并获取DR结果
        if (dr_session) {
            DrAnalysisResult dr_result;
            int finalize_result = dr_session_finalize(dr_session, &dr_result);
            dr_session_free(dr_session);

            if (finalize_result != 0) {
                throw std::runtime_error("Failed to finalize DR analysis");
            }

            // 从foobar2000获取真实的音频信息并修正DR结果
            try {
                file_info_impl info;
                handle->get_info(info);

                // 获取真实的bits per sample
                const char* bps_str = info.meta_get("BITSPERSAMPLE", 0);
                if (!bps_str) {
                    bps_str = info.info_get("bitspersample");
                }
                if (bps_str) {
                    dr_result.bits_per_sample = (unsigned int)std::atoi(bps_str);
                }

                // 计算正确的duration（total_samples已经是帧数，不需要再除以声道数）
                if (audio.sample_rate > 0 && dr_result.total_samples > 0) {
                    dr_result.duration_seconds =
                        (double)dr_result.total_samples / audio.sample_rate;
                }

                console::printf(
                    "MacinMeter DR: Audio info corrected - %u Hz, %u ch, %u bits, %.2f s",
                    audio.sample_rate, audio.channels, dr_result.bits_per_sample,
                    dr_result.duration_seconds);
            } catch (const std::exception& e) {
                console::printf("MacinMeter DR: Warning - could not get file info: %s", e.what());
            }

            // 将DR结果转换为AudioData格式（兼容现有接口）
            audio.sample_count = dr_result.total_samples;
            if (audio.sample_rate > 0 && audio.channels > 0) {
                // total_samples现在是交错样本总数，需要除以声道数得到帧数
                unsigned int frames = audio.sample_count / audio.channels;
                audio.duration = (double)frames / audio.sample_rate;
            }

            // 存储DR计算结果（不存储样本数据）
            audio.samples.clear(); // 逐包模式不保存所有样本

            console::printf("MacinMeter DR: Packet-by-packet analysis completed - DR%.0f (precise: "
                            "%.2f), %u samples, %.2fs",
                            dr_result.official_dr_value, dr_result.precise_dr_value,
                            dr_result.total_samples, audio.duration);
        } else {
            throw std::runtime_error("DR analysis session was not created");
        }

    } catch (const std::exception& e) {
        console::printf("MacinMeter DR: Error in packet-by-packet audio processing: %s", e.what());

        // 确保在错误情况下清理数据
        audio.samples.clear();
        audio.sample_count = 0;
        audio.sample_rate = 0;
        audio.channels = 0;
        audio.duration = 0.0;
    }
}

std::string AudioAccessor::get_safe_string(const file_info& info, const char* field) {
    const char* value = info.meta_get(field, 0);
    return value ? std::string(value) : std::string();
}

// 直接进行DR分析并返回结果列表（逐包会话模式）
std::vector<DrAnalysisResult>
AudioAccessor::analyze_dr_data_list(const pfc::list_base_const_t<metadb_handle_ptr>& handles) {
    std::vector<DrAnalysisResult> results;
    results.reserve(handles.get_count());

    for (t_size i = 0; i < handles.get_count(); ++i) {
        try {
            DrAnalysisResult result = analyze_dr_data(handles[i]);
            // 检查结果是否有效（官方DR值不为0）
            if (result.official_dr_value > 0) {
                results.push_back(result);
            }
        } catch (const std::exception& e) {
            console::printf("MacinMeter DR: Error analyzing audio data for DR: %s", e.what());
        }
    }

    return results;
}

// 直接进行DR分析并返回结果（逐包会话模式）
DrAnalysisResult AudioAccessor::analyze_dr_data(const metadb_handle_ptr& handle) {
    DrAnalysisResult result;
    memset(&result, 0, sizeof(DrAnalysisResult));

    if (!handle.is_valid()) {
        console::print("MacinMeter DR: Invalid handle for DR analysis");
        return result;
    }

    // 获取文件路径和基本信息
    const char* file_path = handle->get_path();
    if (!file_path) {
        console::print("MacinMeter DR: Failed to get file path for DR analysis");
        return result;
    }

    // 提取文件名
    std::filesystem::path path(file_path);
    std::string file_name = path.filename().string();

    console::printf("MacinMeter DR: Starting direct DR analysis for: %s", file_name.c_str());

    try {
        // 使用foobar2000的input_decoder进行逐包DR分析
        service_ptr_t<input_decoder> decoder;
        abort_callback_dummy abort;

        input_entry::g_open_for_decoding(decoder, nullptr, file_path, abort);

        if (!decoder.is_valid()) {
            throw std::runtime_error("Failed to create decoder for DR analysis");
        }

        // 初始化解码器
        decoder->initialize(0, input_flag_simpledecode, abort);

        console::printf("MacinMeter DR: Decoder initialized for direct DR analysis...");

        // 解码并进行实时DR分析
        audio_chunk_impl chunk;
        bool first_chunk = true;
        size_t total_samples = 0;
        void* dr_session = nullptr;
        uint32_t sample_rate = 0;
        uint32_t channels = 0;

        // 包大小统计
        ChunkStats chunk_stats;

        while (decoder->run(chunk, abort)) {
            if (first_chunk) {
                // 从第一个chunk获取音频格式信息
                sample_rate = chunk.get_sample_rate();
                channels = chunk.get_channels();

                console::printf("MacinMeter DR: Direct analysis - Audio format %uHz, %uch",
                                sample_rate, channels);

                // 创建DR分析会话（启用Sum Doubling）
                dr_session = dr_session_new(channels, sample_rate, 1);
                if (!dr_session) {
                    throw std::runtime_error(
                        "Failed to create DR analysis session for direct analysis");
                }

                first_chunk = false;
            }

            // 转换并喂给DR计算引擎（逐包直通）
            const audio_sample* chunk_data = chunk.get_data();
            size_t chunk_samples = chunk.get_sample_count();

            std::vector<float> float_buffer;
            float_buffer.reserve(chunk_samples);

            for (size_t j = 0; j < chunk_samples; ++j) {
                float_buffer.push_back(static_cast<float>(chunk_data[j]));
            }

            // 统计包大小
            chunk_stats.chunk_sizes.push_back(chunk_samples);

            unsigned int frame_count = chunk_samples / channels;
            int feed_result =
                dr_session_feed_interleaved(dr_session, float_buffer.data(), frame_count);

            if (feed_result != 0) {
                dr_session_free(dr_session);
                throw std::runtime_error(
                    "Failed to feed chunk data to DR analysis engine in direct mode");
            }

            total_samples += chunk_samples;
        }

        // 计算并输出包大小统计信息
        if (!chunk_stats.chunk_sizes.empty()) {
            calculate_chunk_stats(chunk_stats);
            print_chunk_stats(chunk_stats, file_name);
        }

        // 完成分析并获取DR结果
        if (dr_session) {
            int finalize_result = dr_session_finalize(dr_session, &result);
            dr_session_free(dr_session);

            if (finalize_result != 0) {
                throw std::runtime_error("Failed to finalize direct DR analysis");
            }

            // 填充文件信息
            size_t name_len = std::min(file_name.length(), sizeof(result.file_name) - 1);
            std::strncpy(result.file_name, file_name.c_str(), name_len);
            result.file_name[name_len] = '\0';

            // 从foobar2000获取真实的音频信息
            try {
                file_info_impl info;
                handle->get_info(info);

                // 获取真实的bits per sample
                const char* bps_str = info.meta_get("BITSPERSAMPLE", 0);
                if (!bps_str) {
                    bps_str = info.info_get("bitspersample");
                }
                if (bps_str) {
                    result.bits_per_sample = (unsigned int)std::atoi(bps_str);
                } else {
                    // 如果无法获取，保持Rust侧的默认值（32位浮点）
                    // result.bits_per_sample 已经由 Rust 侧设置
                }

                // 计算正确的duration（total_samples已经是帧数，不需要再除以声道数）
                if (sample_rate > 0 && result.total_samples > 0) {
                    result.duration_seconds = (double)result.total_samples / sample_rate;
                }

                console::printf("MacinMeter DR: Audio info - %u Hz, %u ch, %u bits, %.2f s",
                                sample_rate, channels, result.bits_per_sample,
                                result.duration_seconds);
            } catch (const std::exception& e) {
                console::printf("MacinMeter DR: Warning - could not get file info: %s", e.what());
            }

            console::printf(
                "MacinMeter DR: Direct DR analysis completed - DR%.0f (precise: %.2f), %u samples",
                result.official_dr_value, result.precise_dr_value, result.total_samples);
        } else {
            throw std::runtime_error("DR analysis session was not created for direct analysis");
        }

    } catch (const std::exception& e) {
        console::printf("MacinMeter DR: Error in direct DR analysis: %s", e.what());
        memset(&result, 0, sizeof(DrAnalysisResult));
    }

    return result;
}

void AudioAccessor::calculate_chunk_stats(ChunkStats& stats) {
    if (stats.chunk_sizes.empty()) {
        return;
    }

    // 排序以计算百分位数
    std::sort(stats.chunk_sizes.begin(), stats.chunk_sizes.end());

    stats.total_chunks = stats.chunk_sizes.size();
    stats.min_size = stats.chunk_sizes.front();
    stats.max_size = stats.chunk_sizes.back();

    // 计算平均值
    size_t total = 0;
    for (size_t size : stats.chunk_sizes) {
        total += size;
    }
    stats.mean_size = static_cast<double>(total) / stats.total_chunks;

    // 计算中位数
    size_t mid = stats.total_chunks / 2;
    if (stats.total_chunks % 2 == 0) {
        stats.median_size = (stats.chunk_sizes[mid - 1] + stats.chunk_sizes[mid]) / 2;
    } else {
        stats.median_size = stats.chunk_sizes[mid];
    }

    // 计算95和99百分位数
    size_t p95_idx = static_cast<size_t>(stats.total_chunks * 0.95);
    size_t p99_idx = static_cast<size_t>(stats.total_chunks * 0.99);

    stats.p95_size = stats.chunk_sizes[std::min(p95_idx, stats.total_chunks - 1)];
    stats.p99_size = stats.chunk_sizes[std::min(p99_idx, stats.total_chunks - 1)];
}

void AudioAccessor::print_chunk_stats(const ChunkStats& stats, const std::string& file_name) {
    console::printf("MacinMeter DR: Chunk size statistics for %s:", file_name.c_str());
    console::printf("  Total chunks: %u", static_cast<unsigned int>(stats.total_chunks));
    console::printf("  Min size: %u samples", static_cast<unsigned int>(stats.min_size));
    console::printf("  Max size: %u samples", static_cast<unsigned int>(stats.max_size));
    console::printf("  Mean size: %d samples", static_cast<int>(stats.mean_size + 0.5));
    console::printf("  Median size: %u samples", static_cast<unsigned int>(stats.median_size));
    console::printf("  95th percentile: %u samples", static_cast<unsigned int>(stats.p95_size));
    console::printf("  99th percentile: %u samples", static_cast<unsigned int>(stats.p99_size));
}