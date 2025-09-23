#include "progress_worker.h"
#include "../audio/audio_accessor.h"
#include <chrono>
#include <stdexcept>
#include <thread>

// 🔧 纯C回调函数前置声明（兼容Rust FFI）
extern "C" {
void c_progress_callback(int current, int total, const char* message);
void c_completion_callback(const char* result, bool success);
}

// 静态成员定义
MacinMeterProgressWorker* MacinMeterProgressWorker::s_current_worker = nullptr;

MacinMeterProgressWorker::MacinMeterProgressWorker(const metadb_handle_ptr& handle)
    : m_handle(handle), m_progress_handle(0), m_completion_handle(0), m_status_ptr(nullptr),
      m_analysis_completed(false), m_analysis_success(false), m_task_id(0), m_should_abort(false),
      m_current_progress(0.0f) {}

void MacinMeterProgressWorker::startAnalysis(const metadb_handle_ptr& handle) {
    // 🚀 使用官方threaded_process API启动带进度条的异步分析
    auto worker = fb2k::service_new<MacinMeterProgressWorker>(handle);

    const uint32_t flags = threaded_process::flag_show_progress | threaded_process::flag_show_item |
                           threaded_process::flag_show_abort;

    threaded_process::get()->run_modeless(worker, flags, core_api::get_main_window(),
                                          "MacinMeter Dynamic Range Analysis");
}

void MacinMeterProgressWorker::on_init(ctx_t p_wnd) {
    // 🎯 设置当前活跃的工作器实例（用于静态回调）
    s_current_worker = this;

    // 🔗 注册Rust回调（使用纯C函数确保FFI兼容性）
    m_progress_handle = rust_register_progress_callback(&c_progress_callback);
    m_completion_handle = rust_register_completion_callback(&c_completion_callback);

    // 🛡️ 验证回调注册成功
    if (m_progress_handle == 0) {
        throw std::runtime_error("进度回调注册失败");
    }
    if (m_completion_handle == 0) {
        throw std::runtime_error("完成回调注册失败");
    }
}

void MacinMeterProgressWorker::run(threaded_process_status& p_status, abort_callback& p_abort) {
    m_status_ptr = &p_status;

    try {
        // 🎵 步骤1：显示当前处理的文件
        pfc::string8 file_path = m_handle->get_path();
        p_status.set_item_path(file_path);
        p_status.set_progress_float(0.0);
        p_status.set_item("正在初始化分析...");

        // 🎯 进度条分配：解码1%，DR分析98%，完成1%
        const float DECODE_PROGRESS_START = 0.0f;    // 解码开始：0%
        const float DECODE_PROGRESS_END = 0.01f;     // 解码结束：1%
        const float ANALYSIS_PROGRESS_START = 0.01f; // DR分析开始：1%
        const float ANALYSIS_PROGRESS_END = 0.99f;   // DR分析结束：99%
        const float FINAL_PROGRESS_END = 1.0f;       // 最终完成：100%

        p_status.set_item("正在启动真正的流式分析...");

        // 🚀 步骤2：真正的流式架构 - 在第一个chunk时动态获取音频信息并初始化Rust
        AudioAccessor audio_accessor;

        // 🚀 批量缓存优化 - 减少FFI调用100-250倍，显著提升性能
        const size_t BATCH_SIZE = 256 * 1024 / sizeof(float); // 256KB批量缓存（平衡性能和内存）
        std::vector<float> batch_buffer;
        batch_buffer.reserve(BATCH_SIZE);

        // 🚀 流式分析状态
        bool rust_initialized = false;
        size_t total_samples_processed = 0;
        size_t estimated_total_samples = 0;

        auto chunk_callback = [this, &batch_buffer, BATCH_SIZE, &rust_initialized, &p_status,
                               &total_samples_processed, &estimated_total_samples,
                               DECODE_PROGRESS_START, DECODE_PROGRESS_END, ANALYSIS_PROGRESS_START,
                               ANALYSIS_PROGRESS_END](const float* samples, size_t sample_count,
                                                      bool first_chunk,
                                                      const AudioInfo* audio_info) -> bool {
            if (m_should_abort) {
                return false; // 请求停止解码
            }

            // 🎯 第一个chunk：使用可靠的音频格式信息初始化Rust
            if (first_chunk && audio_info && !rust_initialized) {
                // 🛡️ 基础验证
                if (audio_info->channels > 2) {
                    char error_msg[256];
                    snprintf(error_msg, sizeof(error_msg),
                             "仅支持单声道和立体声文件 "
                             "(1-2声道)，当前文件为%u声道。多声道支持正在开发中。",
                             audio_info->channels);
                    throw std::runtime_error(error_msg);
                }

                if (audio_info->channels == 0 || audio_info->sample_rate == 0) {
                    char error_msg[256];
                    snprintf(error_msg, sizeof(error_msg), "音频格式信息无效: %u声道, %uHz采样率",
                             audio_info->channels, audio_info->sample_rate);
                    throw std::runtime_error(error_msg);
                }

                // 🎯 估算总样本数用于进度计算
                estimated_total_samples = static_cast<size_t>(
                    audio_info->duration * audio_info->sample_rate * audio_info->channels);

                p_status.set_item("正在初始化Rust分析引擎...");

                // 🚀 初始化Rust流式分析会话
                m_task_id = rust_streaming_analysis_init(audio_info->channels,    // 可靠的声道数
                                                         audio_info->sample_rate, // 可靠的采样率
                                                         32, // bits_per_sample (固定使用32位浮点)
                                                         m_progress_handle,  // 进度回调
                                                         m_completion_handle // 完成回调
                );

                // 🛡️ 初始化失败检查
                if (m_task_id <= 0) {
                    char error_msg[512];
                    snprintf(error_msg, sizeof(error_msg),
                             "Rust流式分析初始化失败: 错误码 %d\n"
                             "音频信息: %u声道, %uHz采样率",
                             m_task_id, audio_info->channels, audio_info->sample_rate);
                    throw std::runtime_error(error_msg);
                }

                rust_initialized = true;
                p_status.set_item("正在流式分析音频数据...");
            }

            // 🎯 关键修复：一旦Rust初始化，立即切换到DR分析进度
            if (!rust_initialized) {
                // 解码阶段：仅在Rust未初始化时更新解码进度
                total_samples_processed += sample_count;
                if (estimated_total_samples > 0) {
                    float decode_progress =
                        static_cast<float>(total_samples_processed) / estimated_total_samples;
                    decode_progress = std::min(decode_progress, 1.0f);
                    float mapped_progress =
                        DECODE_PROGRESS_START +
                        decode_progress * (DECODE_PROGRESS_END - DECODE_PROGRESS_START);
                    p_status.set_progress_float(mapped_progress);
                }
            } else {
                // DR分析阶段：一旦Rust初始化，立即使用DR分析进度
                float rust_progress = m_current_progress.load(); // 0.0-1.0
                float mapped_rust_progress =
                    ANALYSIS_PROGRESS_START +
                    rust_progress * (ANALYSIS_PROGRESS_END - ANALYSIS_PROGRESS_START);
                p_status.set_progress_float(mapped_rust_progress);
            }

            // 🚀 批量缓存：积累到256KB再发送，减少FFI调用开销
            if (rust_initialized) {
                // 只有在Rust已初始化时才累积数据
                batch_buffer.insert(batch_buffer.end(), samples, samples + sample_count);

                // 缓存满了时，批量发送给Rust
                if (batch_buffer.size() >= BATCH_SIZE) {
                    int result = rust_streaming_analysis_send_chunk(
                        m_task_id,                                     // 会话ID
                        batch_buffer.data(),                           // 批量样本数据
                        static_cast<unsigned int>(batch_buffer.size()) // 批量样本数量
                    );

                    if (result != 0) {
                        // 发送失败，记录错误并停止
                        console::printf(
                            "MacinMeter DR ProgressWorker: Chunk send failed with error %d",
                            result);
                        m_should_abort = true;
                        return false;
                    }

                    // 清空缓存，准备下一批
                    batch_buffer.clear();
                }
            }
            // 如果Rust未初始化，直接丢弃数据，避免内存累积

            return true; // 继续解码
        };

        // 使用AudioAccessor的流式解码接口
        bool decode_success =
            audio_accessor.decode_with_streaming_callback(m_handle, p_abort, chunk_callback);

        if (!decode_success || m_should_abort) {
            if (m_task_id > 0) {
                rust_streaming_analysis_cancel(m_task_id);
            }

            if (!decode_success) {
                throw std::runtime_error("音频解码失败");
            } else {
                throw std::runtime_error("用户取消了分析");
            }
        }

        // 🎯 解码完成，进度条到达5%，开始DR分析阶段
        p_status.set_progress_float(DECODE_PROGRESS_END);
        p_status.set_item("解码完成，开始DR分析...");

        // 🏁 处理最后剩余的批量数据（如果有）
        if (rust_initialized && !batch_buffer.empty() && !m_should_abort) {
            int result = rust_streaming_analysis_send_chunk(
                m_task_id,                                     // 会话ID
                batch_buffer.data(),                           // 剩余样本数据
                static_cast<unsigned int>(batch_buffer.size()) // 剩余样本数量
            );

            if (result != 0) {
                rust_streaming_analysis_cancel(m_task_id);
                throw std::runtime_error("发送最后批量数据失败");
            }
        }

        // 🏁 完成分析（仅在Rust已初始化时）
        if (rust_initialized) {
            p_status.set_item("正在完成DR分析...");

            int finalize_result = rust_streaming_analysis_finalize(m_task_id);
            if (finalize_result != 0) {
                throw std::runtime_error("完成DR分析失败");
            }
        } else {
            // 如果Rust从未初始化，说明没有收到有效的音频数据
            throw std::runtime_error("未收到有效的音频数据，无法进行DR分析");
        }

        // 🔄 等待分析完成（进度更新已在chunk_callback中处理）
        if (rust_initialized) {
            auto start_time = std::chrono::steady_clock::now();
            const auto timeout = std::chrono::seconds(120); // 120秒超时

            while (!m_analysis_completed) {
                try {
                    p_abort.check(); // 检查用户取消
                } catch (...) {
                    // 🛑 用户取消：立即取消Rust任务
                    m_should_abort = true;
                    if (m_task_id > 0) {
                        rust_streaming_analysis_cancel(m_task_id);
                    }
                    throw;
                }

                // ⏰ 超时检查
                auto elapsed = std::chrono::steady_clock::now() - start_time;
                if (elapsed > timeout) {
                    m_should_abort = true;
                    if (m_task_id > 0) {
                        rust_streaming_analysis_cancel(m_task_id);
                    }
                    throw std::runtime_error("分析超时（120秒）");
                }

                std::this_thread::sleep_for(std::chrono::milliseconds(100)); // 100ms足够流畅
            }

            // 🎯 分析完成，进度条到达100%
            if (!m_should_abort) {
                p_status.set_progress_float(FINAL_PROGRESS_END); // 确保到达100%
                p_status.set_item("DR分析完成！");
            }
        }

    } catch (const std::exception& e) {
        m_analysis_completed = true;
        m_analysis_success = false;
        m_result_text = pfc::string8("❌ 分析失败: ") + e.what();
    }
}

void MacinMeterProgressWorker::on_done(ctx_t p_wnd, bool p_was_aborted) {
    // 🧹 清理回调句柄
    if (m_progress_handle != 0) {
        // Rust会自动清理回调，无需手动清理
        m_progress_handle = 0;
    }
    if (m_completion_handle != 0) {
        m_completion_handle = 0;
    }

    // 🎯 显示分析结果（如果没有被取消）
    if (!p_was_aborted) {
        if (m_analysis_success) {
            popup_message::g_show(m_result_text, "MacinMeter DR Analysis Result");
        } else {
            popup_message::g_complain("MacinMeter DR", m_result_text);
        }
    }

    // 🧹 清理静态引用
    s_current_worker = nullptr;
}

// 🔧 Public静态方法实现（用于C回调）
void MacinMeterProgressWorker::handle_progress_callback(int current, int total,
                                                        const char* message) {
    if (s_current_worker) {
        // 🎯 更新原子进度值（0.0-1.0）
        if (total > 0) {
            float progress = static_cast<float>(current) / static_cast<float>(total);
            s_current_worker->m_current_progress.store(progress);
        }

        // 🎯 更新状态消息（如果在工作线程中）
        if (s_current_worker->m_status_ptr && message && strlen(message) > 0) {
            pfc::string8 status_text = pfc::string8("DR分析中: ") + message;
            s_current_worker->m_status_ptr->set_item(status_text);
        }
    }
}

void MacinMeterProgressWorker::handle_completion_callback(const char* result, bool success) {
    if (s_current_worker) {
        s_current_worker->m_analysis_completed = true;
        s_current_worker->m_analysis_success = success;
        s_current_worker->m_result_text = result ? result : (success ? "分析完成" : "分析失败");
    }
}

// 🔧 纯C回调函数实现（兼容Rust FFI）
extern "C" void c_progress_callback(int current, int total, const char* message) {
    MacinMeterProgressWorker::handle_progress_callback(current, total, message);
}

extern "C" void c_completion_callback(const char* result, bool success) {
    MacinMeterProgressWorker::handle_completion_callback(result, success);
}

// 静态回调函数实现（保留兼容性）
void MacinMeterProgressWorker::progress_callback(int current, int total, const char* message) {
    c_progress_callback(current, total, message);
}

void MacinMeterProgressWorker::completion_callback(const char* result, bool success) {
    c_completion_callback(result, success);
}