#include "progress_worker.h"
#include "../audio/audio_accessor.h"
#include <thread>
#include <chrono>
#include <stdexcept>

// 静态成员定义
MacinMeterProgressWorker* MacinMeterProgressWorker::s_current_worker = nullptr;

MacinMeterProgressWorker::MacinMeterProgressWorker(const metadb_handle_ptr& handle)
    : m_handle(handle)
    , m_progress_handle(0)
    , m_completion_handle(0)
    , m_status_ptr(nullptr)
    , m_analysis_completed(false)
    , m_analysis_success(false)
{
}

void MacinMeterProgressWorker::startAnalysis(const metadb_handle_ptr& handle) {
    // 🚀 使用官方threaded_process API启动带进度条的异步分析
    auto worker = fb2k::service_new<MacinMeterProgressWorker>(handle);

    const uint32_t flags = threaded_process::flag_show_progress |
                          threaded_process::flag_show_item |
                          threaded_process::flag_show_abort;

    threaded_process::get()->run_modeless(
        worker,
        flags,
        core_api::get_main_window(),
        "MacinMeter Dynamic Range Analysis"
    );
}

void MacinMeterProgressWorker::on_init(ctx_t p_wnd) {
    // 🎯 设置当前活跃的工作器实例（用于静态回调）
    s_current_worker = this;

    // 🔗 注册Rust回调
    m_progress_handle = rust_register_progress_callback(&MacinMeterProgressWorker::progress_callback);
    m_completion_handle = rust_register_completion_callback(&MacinMeterProgressWorker::completion_callback);
}

void MacinMeterProgressWorker::run(threaded_process_status& p_status, abort_callback& p_abort) {
    m_status_ptr = &p_status;

    try {
        // 🎵 步骤1：显示当前处理的文件
        pfc::string8 file_path = m_handle->get_path();
        p_status.set_item_path(file_path);
        p_status.set_progress_float(0.0);

        // 🎵 步骤2：使用AudioAccessor解码音频
        AudioAccessor audio_accessor;
        auto audio_data = audio_accessor.decode_audio_data(m_handle);

        // 🚀 步骤3：调用Rust进行DR分析
        int task_id = rust_analyze_async_elegant(
            audio_data.samples.data(),
            static_cast<unsigned int>(audio_data.samples.size()),
            audio_data.channels,
            audio_data.sample_rate,
            32,  // bits_per_sample
            m_progress_handle,
            m_completion_handle
        );

        if (task_id <= 0) {
            throw std::runtime_error("Rust分析启动失败");
        }

        // 🔄 步骤4：等待分析完成（通过回调更新进度）
        while (!m_analysis_completed) {
            p_abort.check();  // 检查用户取消
            std::this_thread::sleep_for(std::chrono::milliseconds(50));   // 50ms轮询间隔
        }

        // 🎯 步骤5：分析完成
        p_status.set_progress_float(1.0);

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

// 静态回调函数实现
void MacinMeterProgressWorker::progress_callback(int current, int total, const char* message) {
    if (s_current_worker && s_current_worker->m_status_ptr) {
        // 🎯 更新进度条（在工作线程中，threaded_process保证线程安全）
        if (total > 0) {
            s_current_worker->m_status_ptr->set_progress(current, total);
        }

        // 🎯 更新状态消息
        if (message && strlen(message) > 0) {
            pfc::string8 status_text = pfc::string8("Processing: ") + message;
            s_current_worker->m_status_ptr->set_item(status_text);
        }
    }
}

void MacinMeterProgressWorker::completion_callback(const char* result, bool success) {
    if (s_current_worker) {
        s_current_worker->m_analysis_completed = true;
        s_current_worker->m_analysis_success = success;
        s_current_worker->m_result_text = result ? result : (success ? "分析完成" : "分析失败");
    }
}