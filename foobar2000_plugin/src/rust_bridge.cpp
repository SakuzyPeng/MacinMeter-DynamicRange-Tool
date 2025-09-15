#include "rust_bridge.h"
#include "foobar2000.h"
#include <algorithm>
#include <cstring>
#include <memory>
#include <string>

// 声明Rust FFI函数（避免与C++函数重名）
extern "C" {
void* rust_dr_session_new(unsigned int channels, unsigned int sample_rate, int enable_sum_doubling);
int rust_dr_session_feed_interleaved(void* session, const float* samples, unsigned int frame_count);
int rust_dr_session_finalize(void* session, DrAnalysisResult* result);
void rust_dr_session_free(void* session);

// 参数设置FFI函数
int rust_set_analysis_params_real(int enable_simd, int enable_sum_doubling, int packet_chunk_mode);
int rust_get_analysis_params_real(int* enable_simd, int* enable_sum_doubling,
                                  int* packet_chunk_mode);

// 错误处理FFI函数
const char* rust_get_last_error_real(void);
}

// 静态错误信息存储
static std::string g_last_error;

// 内部辅助函数
namespace {
void set_last_error(const std::string& error) {
    g_last_error = error;
    console::printf("MacinMeter DR Rust Bridge Error: %s", error.c_str());
}

void clear_last_error() {
    g_last_error.clear();
}
} // namespace

// 初始化Rust DR引擎
int rust_dr_engine_init(void) {
    try {
        clear_last_error();

        console::print("MacinMeter DR: Initializing Rust DR calculation engine...");

        // 设置默认分析参数
        // - 启用SIMD优化
        // - 启用Sum Doubling (与foobar2000兼容)
        // - 启用逐包直通模式
        int result = rust_set_analysis_params(1, 1, 1);

        if (result == 0) {
            console::print("MacinMeter DR: Rust engine initialized successfully");
            console::printf("MacinMeter DR: Engine version: %s", rust_get_engine_version());
            console::printf("MacinMeter DR: Supported formats: %s", rust_get_supported_formats());
            return 0;
        } else {
            set_last_error("Failed to set default analysis parameters");
            return -1;
        }
    } catch (const std::exception& e) {
        set_last_error(std::string("Initialization exception: ") + e.what());
        return -1;
    } catch (...) {
        set_last_error("Unknown initialization error");
        return -1;
    }
}

// 清理Rust DR引擎
void rust_dr_engine_cleanup(void) {
    try {
        console::print("MacinMeter DR: Cleaning up Rust engine resources...");
        // Rust端的清理会在DLL卸载时自动进行
        clear_last_error();
        console::print("MacinMeter DR: Cleanup completed");
    } catch (...) {
        // 清理过程中的错误不应该传播
        console::print("MacinMeter DR: Error during cleanup (ignored)");
    }
}

// 分析解码后的音频数据（使用foobar2000解码器）
int rust_analyze_audio_data(const float* audio_data, size_t sample_count, unsigned int channels,
                            unsigned int sample_rate, const char* file_name,
                            DrAnalysisResult* result) {
    if (!audio_data || !result || !file_name || sample_count == 0 || channels == 0) {
        set_last_error(
            "Invalid parameters: audio_data, result, file_name is null or invalid counts");
        return -1;
    }

    try {
        clear_last_error();

        // 初始化结果结构
        memset(result, 0, sizeof(DrAnalysisResult));

        console::printf("MacinMeter DR: Analyzing audio data: %s (%zu samples, %uch, %uHz)",
                        file_name, sample_count, channels, sample_rate);

        // 使用会话式FFI接口进行真实DR计算
        void* session = dr_session_new(channels, sample_rate, 1); // 启用sum_doubling
        if (!session) {
            set_last_error("Failed to create DR analysis session");
            return -1;
        }

        try {
            // 转换为交错格式（foobar2000提供的已经是交错格式）
            unsigned int frame_count = sample_count / channels;
            int feed_result = dr_session_feed_interleaved(session, audio_data, frame_count);

            if (feed_result != 0) {
                dr_session_free(session);
                set_last_error("Failed to feed audio data to DR session");
                return -1;
            }

            // 完成分析并获取结果
            int finalize_result = dr_session_finalize(session, result);
            dr_session_free(session);

            if (finalize_result != 0) {
                set_last_error("Failed to finalize DR analysis");
                return -1;
            }

            // 安全复制文件名
            size_t name_len = std::min(strlen(file_name), sizeof(result->file_name) - 1);
            std::strncpy(result->file_name, file_name, name_len);
            result->file_name[name_len] = '\0';

            // 设置编解码器信息为foobar2000解码
            std::strncpy(result->codec, "foobar2000", sizeof(result->codec) - 1);
            result->codec[sizeof(result->codec) - 1] = '\0';

            console::printf("MacinMeter DR: Real DR calculation completed - DR%.0f (precise: %.2f)",
                            result->official_dr_value, result->precise_dr_value);
        } catch (...) {
            dr_session_free(session);
            throw;
        }

        return 0; // 成功
    } catch (const std::exception& e) {
        set_last_error(std::string("Audio data analysis exception: ") + e.what());
        return -1;
    } catch (...) {
        set_last_error("Unknown audio data analysis error");
        return -1;
    }
}

// 批量分析音频数据（使用foobar2000解码器）
int rust_analyze_audio_batch_data(const float** audio_data_list, const size_t* sample_counts,
                                  const unsigned int* channels_list,
                                  const unsigned int* sample_rates, const char** file_names,
                                  size_t count, DrBatchResult* result, ProgressCallback callback) {
    if (!audio_data_list || !sample_counts || !channels_list || !sample_rates || !file_names ||
        !result || count == 0) {
        set_last_error("Invalid batch analysis parameters");
        return -1;
    }

    try {
        clear_last_error();

        console::printf(
            "MacinMeter DR: Starting batch analysis of %zu audio streams using foobar2000 decoder",
            count);

        // 初始化批量结果
        memset(result, 0, sizeof(DrBatchResult));
        result->results = new DrAnalysisResult[count];
        result->count = 0;
        result->processed_files = 0;
        result->failed_files = 0;

        double total_dr = 0.0;

        // 分析每个音频数据
        for (size_t i = 0; i < count; ++i) {
            if (callback) {
                callback("Analyzing audio data...", (int)i, (int)count);
            }

            DrAnalysisResult file_result;
            int status =
                rust_analyze_audio_data(audio_data_list[i], sample_counts[i], channels_list[i],
                                        sample_rates[i], file_names[i], &file_result);

            if (status == 0) {
                result->results[result->count] = file_result;
                result->count++;
                result->processed_files++;
                total_dr += file_result.official_dr_value;
            } else {
                result->failed_files++;
                console::printf("MacinMeter DR: Failed to analyze audio data for: %s",
                                file_names[i]);
            }
        }

        // 计算平均DR值
        if (result->processed_files > 0) {
            result->average_dr = total_dr / result->processed_files;
        }

        if (callback) {
            callback("Batch analysis completed", (int)count, (int)count);
        }

        console::printf("MacinMeter DR: Batch analysis using foobar2000 decoder completed - %zu "
                        "processed, %zu failed",
                        result->processed_files, result->failed_files);

        return 0;
    } catch (const std::exception& e) {
        set_last_error(std::string("Batch audio data analysis exception: ") + e.what());
        return -1;
    } catch (...) {
        set_last_error("Unknown batch audio data analysis error");
        return -1;
    }
}

// 释放批量结果内存
void rust_free_batch_result(DrBatchResult* result) {
    if (result && result->results) {
        delete[] result->results;
        result->results = nullptr;
        result->count = 0;
    }
}

// 获取最后的错误信息
const char* rust_get_last_error(void) {
    return g_last_error.c_str();
}

// 获取引擎版本信息
const char* rust_get_engine_version(void) {
    return "MacinMeter DR Engine v1.0.0 (foobar2000-plugin)";
}

// 获取支持的音频格式
const char* rust_get_supported_formats(void) {
    return "FLAC, MP3, WAV, AAC, M4A, OGG, WMA, APE, WV";
}

// 设置分析参数
int rust_set_analysis_params(int enable_simd, int enable_sum_doubling, int packet_chunk_mode) {
    try {
        console::printf(
            "MacinMeter DR: Setting analysis parameters - SIMD:%s, SumDoubling:%s, PacketChunk:%s",
            enable_simd ? "ON" : "OFF", enable_sum_doubling ? "ON" : "OFF",
            packet_chunk_mode ? "ON" : "OFF");

        // 调用真正的Rust FFI实现
        int result =
            rust_set_analysis_params_real(enable_simd, enable_sum_doubling, packet_chunk_mode);

        if (result == 0) {
            console::print("MacinMeter DR: Analysis parameters set successfully via Rust FFI");
        } else {
            set_last_error("Rust FFI failed to set analysis parameters");
        }

        return result;
    } catch (...) {
        set_last_error("Failed to set analysis parameters");
        return -1;
    }
}

// 获取当前分析参数
int rust_get_analysis_params(int* enable_simd, int* enable_sum_doubling, int* packet_chunk_mode) {
    if (!enable_simd || !enable_sum_doubling || !packet_chunk_mode) {
        set_last_error("Invalid parameter pointers");
        return -1;
    }

    try {
        // 调用真正的Rust FFI实现
        int result =
            rust_get_analysis_params_real(enable_simd, enable_sum_doubling, packet_chunk_mode);

        if (result == 0) {
            console::printf("MacinMeter DR: Analysis parameters retrieved via Rust FFI - SIMD:%s, "
                            "SumDoubling:%s, PacketChunk:%s",
                            *enable_simd ? "ON" : "OFF", *enable_sum_doubling ? "ON" : "OFF",
                            *packet_chunk_mode ? "ON" : "OFF");
        } else {
            set_last_error("Rust FFI failed to get analysis parameters");
        }

        return result;
    } catch (...) {
        set_last_error("Failed to get analysis parameters");
        return -1;
    }
}

// 会话式DR分析接口实现（用于流式处理）
void* dr_session_new(unsigned int channels, unsigned int sample_rate, int enable_sum_doubling) {
    try {
        console::printf("MacinMeter DR: Creating DR analysis session - %uch, %uHz, SumDoubling:%s",
                        channels, sample_rate, enable_sum_doubling ? "ON" : "OFF");

        // 调用实际的Rust FFI创建会话
        void* session = rust_dr_session_new(channels, sample_rate, enable_sum_doubling);

        if (session) {
            console::printf("MacinMeter DR: DR session created successfully");
        } else {
            set_last_error("Rust FFI failed to create DR session");
        }
        return session;
    } catch (const std::exception& e) {
        set_last_error(std::string("Failed to create DR session: ") + e.what());
        return nullptr;
    } catch (...) {
        set_last_error("Unknown error creating DR session");
        return nullptr;
    }
}

int dr_session_feed_interleaved(void* session, const float* samples, unsigned int frame_count) {
    if (!session || !samples || frame_count == 0) {
        set_last_error("Invalid session parameters");
        return -1;
    }

    try {
        // 调用实际的Rust FFI喂数据
        int result = rust_dr_session_feed_interleaved(session, samples, frame_count);

        // 处理进度日志（每10000帧输出一次）
        static unsigned int total_frames = 0;
        total_frames += frame_count;

        if (total_frames % 10000 == 0) {
            console::printf("MacinMeter DR: Session fed %u frames (total: %u)", frame_count,
                            total_frames);
        }

        return result;
    } catch (const std::exception& e) {
        set_last_error(std::string("Failed to feed data to DR session: ") + e.what());
        return -1;
    } catch (...) {
        set_last_error("Unknown error feeding data to DR session");
        return -1;
    }
}

int dr_session_finalize(void* session, DrAnalysisResult* result) {
    if (!session || !result) {
        set_last_error("Invalid finalize parameters");
        return -1;
    }

    try {
        console::print("MacinMeter DR: Finalizing DR analysis session...");

        // 调用实际的Rust FFI完成分析
        int rust_result = rust_dr_session_finalize(session, result);

        if (rust_result == 0) {
            console::printf("MacinMeter DR: Session finalized - DR%.0f (precise: %.2f)",
                            result->official_dr_value, result->precise_dr_value);
        } else {
            // 获取详细的Rust错误信息
            const char* rust_error = rust_get_last_error_real();
            std::string detailed_error = "Rust FFI failed to finalize DR session";
            if (rust_error && strlen(rust_error) > 0) {
                detailed_error += ": ";
                detailed_error += rust_error;
            }
            set_last_error(detailed_error);
        }

        return rust_result;
    } catch (const std::exception& e) {
        set_last_error(std::string("Failed to finalize DR session: ") + e.what());
        return -1;
    } catch (...) {
        set_last_error("Unknown error finalizing DR session");
        return -1;
    }
}

void dr_session_free(void* session) {
    if (!session) {
        return;
    }

    try {
        console::print("MacinMeter DR: Freeing DR analysis session...");

        // 调用实际的Rust FFI释放会话
        rust_dr_session_free(session);

        console::print("MacinMeter DR: DR session freed");
    } catch (...) {
        // 清理过程中的错误不应该传播
        console::print("MacinMeter DR: Error during session cleanup (ignored)");
    }
}