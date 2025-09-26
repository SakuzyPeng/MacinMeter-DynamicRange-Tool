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
      // 🕐 初始化计时器和阶段信息
      m_current_stage("准备中..."),
      // 🎭 初始化双进度条滑块动画
      m_slider_center(0.2f), m_animation_direction(true) {}

void MacinMeterProgressWorker::startAnalysis(const metadb_handle_ptr& handle) {
    // 🚀 使用官方threaded_process API启动带进度条的异步分析
    auto worker = fb2k::service_new<MacinMeterProgressWorker>(handle);

    // 🎯 只显示文本和取消按钮，不显示原生进度条
    const uint32_t flags = threaded_process::flag_show_item | threaded_process::flag_show_abort;

    threaded_process::get()->run_modeless(worker, flags, core_api::get_main_window(),
                                          "MacinMeter Dynamic Range Analysis");
}

void MacinMeterProgressWorker::on_init(ctx_t p_wnd) {
    // 🎯 设置当前活跃的工作器实例（用于静态回调）
    s_current_worker = this;

    // 🕐 记录开始时间
    m_start_time = std::chrono::steady_clock::now();
    m_last_animation_update = m_start_time;
    m_current_stage = "初始化分析...";

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
        // 🎵 显示当前处理的文件
        pfc::string8 file_path = m_handle->get_path();
        p_status.set_item_path(file_path);

        // 🎭 设置初始阶段
        m_current_stage = "准备解码音频文件...";
        updateAnimationAndDisplay();

        // 🚀 音频解码和分析
        AudioAccessor audio_accessor;
        const size_t BATCH_SIZE = 256 * 1024 / sizeof(float);
        std::vector<float> batch_buffer;
        batch_buffer.reserve(BATCH_SIZE);

        bool rust_initialized = false;

        auto chunk_callback = [this, &batch_buffer, BATCH_SIZE, &rust_initialized](
                                  const float* samples, size_t sample_count, bool first_chunk,
                                  const AudioInfo* audio_info) -> bool {
            if (m_should_abort) {
                return false;
            }

            // 🎯 初始化Rust分析引擎
            if (first_chunk && audio_info && !rust_initialized) {
                m_current_stage = "初始化DR分析引擎...";
                updateAnimationAndDisplay();

                // 基础验证
                if (audio_info->channels > 2) {
                    char error_msg[256];
                    snprintf(error_msg, sizeof(error_msg),
                             "仅支持单声道和立体声文件(1-2声道)，当前文件为%u声道。",
                             audio_info->channels);
                    throw std::runtime_error(error_msg);
                }

                if (audio_info->channels == 0 || audio_info->sample_rate == 0) {
                    char error_msg[256];
                    snprintf(error_msg, sizeof(error_msg), "音频格式信息无效: %u声道, %uHz采样率",
                             audio_info->channels, audio_info->sample_rate);
                    throw std::runtime_error(error_msg);
                }

                // 🔍 打印详细的音频信息用于调试
                console::printf(
                    "MacinMeter DR: 准备初始化Rust分析 - %u声道, %uHz, %u位深度, 时长%.2f秒",
                    audio_info->channels, audio_info->sample_rate, audio_info->bits_per_sample,
                    audio_info->duration);

                // 初始化Rust流式分析
                m_task_id = rust_streaming_analysis_init(
                    audio_info->channels, audio_info->sample_rate, audio_info->bits_per_sample,
                    m_progress_handle, m_completion_handle);

                if (m_task_id <= 0) {
                    char error_msg[512];
                    snprintf(error_msg, sizeof(error_msg),
                             "Rust流式分析初始化失败: 错误码 %d\n音频信息: %u声道, %uHz采样率",
                             m_task_id, audio_info->channels, audio_info->sample_rate);
                    throw std::runtime_error(error_msg);
                }

                rust_initialized = true;
                m_current_stage = "流式分析音频数据中...";
                updateAnimationAndDisplay();
            }

            // 🎭 定期更新动画
            updateAnimationAndDisplay();

            // 🚀 批量发送数据到Rust
            if (rust_initialized) {
                batch_buffer.insert(batch_buffer.end(), samples, samples + sample_count);

                if (batch_buffer.size() >= BATCH_SIZE) {
                    int result = rust_streaming_analysis_send_chunk(
                        m_task_id, batch_buffer.data(),
                        static_cast<unsigned int>(batch_buffer.size()));

                    if (result != 0) {
                        console::printf("MacinMeter DR: Chunk send failed with error %d (batch "
                                        "size: %u, task_id: %d)",
                                        result, (unsigned int)batch_buffer.size(), m_task_id);
                        console::printf("MacinMeter DR: 这将导致解码提前终止！");
                        m_should_abort = true;
                        return false;
                    }

                    batch_buffer.clear();
                }
            }

            return true;
        };

        // 开始流式解码
        m_current_stage = "正在解码音频文件...";
        updateAnimationAndDisplay();

        bool decode_success =
            audio_accessor.decode_with_streaming_callback(m_handle, p_abort, chunk_callback);

        if (!decode_success || m_should_abort) {
            if (m_task_id > 0) {
                rust_streaming_analysis_cancel(m_task_id);
            }
            throw std::runtime_error(!decode_success ? "音频解码失败" : "用户取消了分析");
        }

        // 🏁 处理最后剩余的批量数据
        if (rust_initialized && !batch_buffer.empty() && !m_should_abort) {
            int result = rust_streaming_analysis_send_chunk(
                m_task_id, batch_buffer.data(), static_cast<unsigned int>(batch_buffer.size()));

            if (result != 0) {
                rust_streaming_analysis_cancel(m_task_id);
                throw std::runtime_error("发送最后批量数据失败");
            }
        }

        // 🏁 完成分析
        if (rust_initialized) {
            m_current_stage = "正在计算DR值...";
            updateAnimationAndDisplay();

            int finalize_result = rust_streaming_analysis_finalize(m_task_id);
            if (finalize_result != 0) {
                throw std::runtime_error("完成DR分析失败");
            }
        } else {
            throw std::runtime_error("未收到有效的音频数据，无法进行DR分析");
        }

        // 🔄 等待分析完成
        if (rust_initialized) {
            auto start_wait_time = std::chrono::steady_clock::now();

            // 🎯 根据音频文件长度动态计算超时时间
            // 基础超时300秒(5分钟) + 音频时长 + 额外缓冲时间(音频时长的50%)
            double base_timeout = 300.0;                    // 5分钟基础超时
            double audio_duration = m_handle->get_length(); // 音频时长（秒）
            double buffer_time = audio_duration * 0.5;      // 50%缓冲时间
            double total_timeout = base_timeout + audio_duration + buffer_time;

            // 最小10分钟，最大2小时
            total_timeout = std::max(600.0, std::min(7200.0, total_timeout));

            const auto timeout = std::chrono::seconds((long long)total_timeout);

            console::printf(
                "MacinMeter DR: 设置分析超时时间为%.0f秒 (音频%.1f秒 + 基础%.0f秒 + 缓冲%.1f秒)",
                total_timeout, audio_duration, base_timeout, buffer_time);

            while (!m_analysis_completed) {
                try {
                    p_abort.check();
                } catch (...) {
                    m_should_abort = true;
                    if (m_task_id > 0) {
                        rust_streaming_analysis_cancel(m_task_id);
                    }
                    throw;
                }

                auto elapsed = std::chrono::steady_clock::now() - start_wait_time;
                if (elapsed > timeout) {
                    m_should_abort = true;
                    if (m_task_id > 0) {
                        rust_streaming_analysis_cancel(m_task_id);
                    }
                    throw std::runtime_error("分析超时（120秒）");
                }

                // 🎭 等待期间也更新动画
                m_current_stage = "等待DR计算完成...";
                updateAnimationAndDisplay();

                std::this_thread::sleep_for(std::chrono::milliseconds(100));
            }

            // 🎉 分析完成
            if (!m_should_abort) {
                m_current_stage = "DR分析完成！";
                p_status.set_progress_float(1.0f);
                updateAnimationAndDisplay();
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
            // 🕐 计算总耗时并添加到结果中
            auto total_time = std::chrono::steady_clock::now() - m_start_time;
            auto total_seconds =
                std::chrono::duration_cast<std::chrono::seconds>(total_time).count();
            auto total_minutes = total_seconds / 60;
            auto remaining_seconds = total_seconds % 60;

            pfc::string8 result_with_timing = m_result_text;
            result_with_timing << "\n\n";
            result_with_timing << "================================================================"
                                  "================\n";
            result_with_timing << "分析耗时: ";

            if (total_minutes > 0) {
                result_with_timing << total_minutes << "分" << remaining_seconds << "秒";
            } else {
                result_with_timing << total_seconds << "秒";
            }

            result_with_timing << "\n";
            result_with_timing << "================================================================"
                                  "================";

            popup_message::g_show(result_with_timing, "MacinMeter DR Analysis Result");
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
        // 🎭 Rust进度回调已被新的阶段显示系统取代
        // 保留此函数以维持FFI兼容性，但不再执行任何操作
        // 所有进度显示由updateAnimationAndDisplay()统一管理
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

// 🎭 动画和显示更新实现
void MacinMeterProgressWorker::updateAnimationAndDisplay() {
    if (!m_status_ptr)
        return;

    auto now = std::chrono::steady_clock::now();

    // 🕐 计算已消耗时间
    auto elapsed = now - m_start_time;
    auto elapsed_seconds = std::chrono::duration_cast<std::chrono::seconds>(elapsed).count();
    auto elapsed_minutes = elapsed_seconds / 60;
    auto remaining_seconds = elapsed_seconds % 60;

    // 🎨 固定字符进度条参数
    const int TRACK_LENGTH = 21;     // 固定轨道长度
    const int SLIDER_LENGTH = 2;     // 固定滑块长度
    const float MOVE_SPEED = 0.012f; // 调整移动速度匹配新的更新频率（16ms vs 40ms）

    // 🎨 创建高帧率Unicode字符进度条动画（每16ms更新，60fps流畅度）
    auto animation_elapsed = now - m_last_animation_update;
    if (animation_elapsed >= std::chrono::milliseconds(16)) {
        m_last_animation_update = now;

        // 更新滑块中心位置
        if (m_animation_direction) {
            m_slider_center += MOVE_SPEED;
            if (m_slider_center >= 1.0f - (float)SLIDER_LENGTH / TRACK_LENGTH * 0.5f) {
                m_slider_center = 1.0f - (float)SLIDER_LENGTH / TRACK_LENGTH * 0.5f;
                m_animation_direction = false;
            }
        } else {
            m_slider_center -= MOVE_SPEED;
            if (m_slider_center <= (float)SLIDER_LENGTH / TRACK_LENGTH * 0.5f) {
                m_slider_center = (float)SLIDER_LENGTH / TRACK_LENGTH * 0.5f;
                m_animation_direction = true;
            }
        }
    }

    // 🎨 生成字符进度条
    pfc::string8 progress_bar;

    // 计算滑块在字符数组中的位置
    int slider_start =
        (int)((m_slider_center - (float)SLIDER_LENGTH / TRACK_LENGTH * 0.5f) * TRACK_LENGTH);
    int slider_end = slider_start + SLIDER_LENGTH;

    // 边界检查
    slider_start = std::max(0, std::min(slider_start, TRACK_LENGTH - SLIDER_LENGTH));
    slider_end = slider_start + SLIDER_LENGTH;

    // 构建优化字符进度条（双线轨道 + 居中滑块）
    progress_bar << "["; // 左边界
    for (int i = 0; i < TRACK_LENGTH; i++) {
        if (i >= slider_start && i < slider_end) {
            progress_bar << "■"; // 居中方块滑块
        } else {
            progress_bar << "═"; // 双线轨道
        }
    }
    progress_bar << "]"; // 右边界

    // 🎯 组合显示信息：固定格式防止长度变化
    pfc::string8 display_text;

    // 固定阶段文本
    display_text << "处理中... " << progress_bar << " ";

    // 固定格式计时器（确保长度一致）
    if (elapsed_minutes > 0) {
        display_text << elapsed_minutes << ":" << (remaining_seconds < 10 ? "0" : "")
                     << remaining_seconds;
    } else {
        if (elapsed_seconds < 10) {
            display_text << " " << elapsed_seconds << "s"; // 添加空格保持对齐
        } else {
            display_text << elapsed_seconds << "s";
        }
    }

    m_status_ptr->set_item(display_text);
}
