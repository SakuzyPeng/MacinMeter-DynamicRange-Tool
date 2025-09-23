#include "dr_analysis_controller.h"
#include "foobar2000.h"
#include "../bridge/rust_bridge.h"
#include <chrono>
#include <filesystem>
#include <memory>
#include <thread>
#include <mutex>

DrAnalysisController::AnalysisResult
DrAnalysisController::analyzeTracks(const pfc::list_base_const_t<metadb_handle_ptr>& handles) {
    AnalysisResult result;

    // 🛡️ 输入验证
    if (handles.get_count() == 0) {
        result.error_message = "No audio files provided for analysis";
        console::print("MacinMeter DR Controller: No files to analyze");
        return result;
    }

    const size_t total_count = handles.get_count();
    logAnalysisStart(total_count);

    auto start_time = std::chrono::steady_clock::now();

    // 🎯 批量分析：逐个处理每个文件
    for (t_size i = 0; i < total_count; ++i) {
        const metadb_handle_ptr& handle = handles[i];

        // 🎯 控制器层管理文件名提取（不依赖底层）
        std::string current_file_name = "";
        try {
            const char* file_path = handle->get_path();
            if (file_path) {
                std::filesystem::path path(file_path);
                current_file_name = path.filename().string();
            }
        } catch (...) {
            current_file_name = "file_" + std::to_string(i + 1);
        }

        // 报告进度
        reportProgress(("Analyzing: " + current_file_name).c_str(), static_cast<int>(i), static_cast<int>(total_count));

        try {
            // 🎯 步骤1：委托AudioAccessor进行音频解码
            AudioData audio_data = audio_accessor_.decode_audio_data(handle);

            if (!audio_data.samples.empty()) {
                // 🚀 步骤2：使用统一FFI接口获取格式化DR报告
                const size_t BUFFER_SIZE = 8192; // 8KB缓冲区用于DR报告
                char formatted_output[BUFFER_SIZE];
                memset(formatted_output, 0, BUFFER_SIZE);

                // 获取bits per sample（用于DR分析）
                unsigned int bits_per_sample = 32; // 默认foobar2000内部浮点精度
                try {
                    file_info_impl info;
                    handle->get_info(info);
                    const char* bps_str = info.meta_get("BITSPERSAMPLE", 0);
                    if (!bps_str) {
                        bps_str = info.info_get("bitspersample");
                    }
                    if (bps_str) {
                        bits_per_sample = (unsigned int)std::atoi(bps_str);
                    }
                } catch (const std::exception& e) {
                    console::printf("MacinMeter DR Controller: Warning - could not get bitspersample: %s", e.what());
                }

                // 🚀 调用统一Rust FFI接口（直接获取格式化字符串）
                int analysis_result = rust_format_dr_analysis(
                    audio_data.samples.data(),
                    audio_data.samples.size(),
                    audio_data.channels,
                    audio_data.sample_rate,
                    bits_per_sample,
                    formatted_output,
                    BUFFER_SIZE
                );

                if (analysis_result == 0 && strlen(formatted_output) > 0) {
                    // 🎯 控制器层管理文件名（不依赖底层服务）
                    std::string file_name = "";
                    try {
                        const char* file_path = handle->get_path();
                        if (file_path) {
                            std::filesystem::path path(file_path);
                            file_name = path.filename().string();
                        }
                    } catch (...) {
                        file_name = "file_" + std::to_string(i + 1);
                    }

                    // 🚀 存储格式化的DR报告字符串（革命性简化）
                    result.formatted_reports.push_back(std::string(formatted_output));
                    result.audio_data.push_back(audio_data);
                    result.processed_count++;

                    console::printf("MacinMeter DR Controller: Successfully analyzed %s - DR report generated",
                                  file_name.c_str());
                } else {
                    result.failed_count++;
                    std::string error_msg = "DR analysis failed with code " + std::to_string(analysis_result);
                    if (analysis_result == -5) {
                        error_msg = "声道数超出限制 (rust_core仅支持1-2声道)";
                    }
                    console::printf("MacinMeter DR Controller: %s", error_msg.c_str());
                }
            } else {
                result.failed_count++;
                console::printf("MacinMeter DR Controller: No audio data decoded for file %zu", i + 1);
            }

        } catch (const std::exception& e) {
            result.failed_count++;
            handleAnalysisError(e, result, "file " + std::to_string(i + 1));
        }
    }

    // 📊 完成分析，计算总耗时
    auto end_time = std::chrono::steady_clock::now();
    auto duration = std::chrono::duration_cast<std::chrono::milliseconds>(end_time - start_time);
    result.total_duration = duration.count() / 1000.0; // 转换为秒

    // ✅ 确定整体成功状态
    result.success = (result.processed_count > 0);

    // 最终进度报告
    reportProgress("Analysis completed", static_cast<int>(total_count), static_cast<int>(total_count));

    logAnalysisComplete(result);

    return result;
}

DrAnalysisController::AnalysisResult
DrAnalysisController::analyzeTrack(const metadb_handle_ptr& handle) {
    AnalysisResult result;

    if (!handle.is_valid()) {
        result.error_message = "Invalid audio file handle";
        console::print("MacinMeter DR Controller: Invalid handle provided");
        return result;
    }

    console::print("MacinMeter DR Controller: Starting single file analysis");

    auto start_time = std::chrono::steady_clock::now();

    try {
        // 🎯 步骤1：委托AudioAccessor进行音频解码
        AudioData audio_data = audio_accessor_.decode_audio_data(handle);

        if (!audio_data.samples.empty()) {
            // 🚀 步骤2：使用统一FFI接口获取格式化DR报告
            const size_t BUFFER_SIZE = 8192; // 8KB缓冲区用于DR报告
            char formatted_output[BUFFER_SIZE];
            memset(formatted_output, 0, BUFFER_SIZE);

            // 获取bits per sample（用于DR分析）
            unsigned int bits_per_sample = 32; // 默认foobar2000内部浮点精度
            try {
                file_info_impl info;
                handle->get_info(info);
                const char* bps_str = info.meta_get("BITSPERSAMPLE", 0);
                if (!bps_str) {
                    bps_str = info.info_get("bitspersample");
                }
                if (bps_str) {
                    bits_per_sample = (unsigned int)std::atoi(bps_str);
                }
            } catch (const std::exception& e) {
                console::printf("MacinMeter DR Controller: Warning - could not get bitspersample: %s", e.what());
            }

            // 🚀 调用统一Rust FFI接口（直接获取格式化字符串）
            int analysis_result = rust_format_dr_analysis(
                audio_data.samples.data(),
                audio_data.samples.size(),
                audio_data.channels,
                audio_data.sample_rate,
                bits_per_sample,
                formatted_output,
                BUFFER_SIZE
            );

            if (analysis_result == 0 && strlen(formatted_output) > 0) {
                // 🎯 控制器层管理文件名（不依赖底层服务）
                std::string file_name = "";
                try {
                    const char* file_path = handle->get_path();
                    if (file_path) {
                        std::filesystem::path path(file_path);
                        file_name = path.filename().string();
                    }
                } catch (...) {
                    file_name = "audio_file";
                }

                // 🚀 存储格式化的DR报告字符串（革命性简化）
                result.formatted_reports.push_back(std::string(formatted_output));
                result.audio_data.push_back(audio_data);
                result.processed_count = 1;
                result.success = true;

                console::printf("MacinMeter DR Controller: Single file analysis completed - %s, DR report generated",
                              file_name.c_str());
            } else {
                result.failed_count = 1;
                std::string error_msg = "DR analysis failed with code " + std::to_string(analysis_result);
                if (analysis_result == -5) {
                    error_msg = "声道数超出限制 (rust_core仅支持1-2声道)";
                }
                result.error_message = error_msg;
                console::printf("MacinMeter DR Controller: Single file analysis failed - %s", error_msg.c_str());
            }
        } else {
            result.failed_count = 1;
            result.error_message = "No audio data decoded from file";
            console::print("MacinMeter DR Controller: Single file analysis failed - no audio data");
        }

    } catch (const std::exception& e) {
        result.failed_count = 1;
        handleAnalysisError(e, result, "single file analysis");
    }

    // 计算耗时
    auto end_time = std::chrono::steady_clock::now();
    auto duration = std::chrono::duration_cast<std::chrono::milliseconds>(end_time - start_time);
    result.total_duration = duration.count() / 1000.0;

    return result;
}

void DrAnalysisController::setProgressCallback(ProgressCallback callback) {
    // 🔒 线程安全：使用原子操作避免竞争条件
    std::lock_guard<std::mutex> lock(progress_mutex_);
    progress_callback_ = std::move(callback);
}

void DrAnalysisController::reportProgress(const std::string& message, int current, int total) {
    // 🔒 线程安全：保护progress_callback_的访问
    std::lock_guard<std::mutex> lock(progress_mutex_);
    if (progress_callback_) {
        progress_callback_(message, current, total);
    }
}

void DrAnalysisController::handleAnalysisError(const std::exception& e, AnalysisResult& result, const std::string& context) {
    std::string error_msg = "Error in " + context + ": " + e.what();

    // 📝 记录错误但不覆盖之前的错误信息
    if (result.error_message.empty()) {
        result.error_message = error_msg;
    } else {
        result.error_message += "; " + error_msg;
    }

    console::printf("MacinMeter DR Controller: %s", error_msg.c_str());
}


void DrAnalysisController::logAnalysisStart(size_t track_count) {
    console::printf("MacinMeter DR Controller: Starting batch analysis of %zu track(s)", track_count);

    if (track_count == 1) {
        console::print("MacinMeter DR Controller: Single track mode - optimized for individual file analysis");
    } else {
        console::print("MacinMeter DR Controller: Batch mode - processing multiple files sequentially");
    }
}

void DrAnalysisController::logAnalysisComplete(const AnalysisResult& result) {
    console::printf("MacinMeter DR Controller: Analysis completed in %.2f seconds", result.total_duration);
    console::printf("MacinMeter DR Controller: Results - %zu successful, %zu failed, %zu total",
                    result.processed_count, result.failed_count, result.totalCount());

    if (result.success) {
        console::printf("MacinMeter DR Controller: ✅ Batch analysis successful - %zu files processed",
                        result.processed_count);
    } else {
        console::printf("MacinMeter DR Controller: ❌ Batch analysis failed - no valid results obtained");
    }

    if (result.hasErrors()) {
        console::printf("MacinMeter DR Controller: ⚠️  Errors encountered: %s", result.error_message.c_str());
    }
}

// 🚀 异步分析实现 - 控制器层负责线程管理（线程安全版本）
void DrAnalysisController::analyzeTracksAsync(
    const pfc::list_base_const_t<metadb_handle_ptr>& handles,
    AsyncCallback callback,
    ProgressCallback progress_callback) {

    // 🛡️ 输入验证
    if (handles.get_count() == 0) {
        AnalysisResult result;
        result.error_message = "No audio files provided for analysis";
        if (callback) {
            callback(result);
        }
        return;
    }

    // 🎯 创建具体的数据副本用于线程传递
    metadb_handle_list handles_copy(handles);

    // 🔒 创建独立的控制器实例避免this指针生命周期问题
    // 每个异步任务使用独立的控制器，避免共享状态竞争
    auto independent_controller = std::make_shared<DrAnalysisController>();

    // 🎯 设置进度回调到独立实例
    if (progress_callback) {
        independent_controller->setProgressCallback(progress_callback);
    }

    // 🚀 使用独立控制器执行分析，避免this指针悬垂
    std::thread analysis_thread([independent_controller, handles_copy, callback]() {
        try {
            // 🎯 使用独立控制器执行分析（线程安全）
            auto analysis_result = independent_controller->analyzeTracks(handles_copy);

            // 🚀 调用UI层回调
            if (callback) {
                callback(analysis_result);
            }
        } catch (const std::exception& e) {
            // 🛡️ 异常处理
            AnalysisResult error_result;
            error_result.success = false;
            error_result.error_message = "分析过程中发生异常: " + std::string(e.what());
            if (callback) {
                callback(error_result);
            }
        }
    });

    // 🔒 线程安全：使用独立控制器后可以安全地分离线程
    analysis_thread.detach();
}