#pragma once
#include "../bridge/rust_bridge.h"
#include "foobar2000.h"
#include <atomic>

//! MacinMeter DR分析进度工作器 - 使用foobar2000官方threaded_process API
class MacinMeterProgressWorker : public threaded_process_callback {
  public:
    MacinMeterProgressWorker(const metadb_handle_ptr& handle);

    // threaded_process_callback接口实现
    void on_init(ctx_t p_wnd) override;
    void run(threaded_process_status& p_status, abort_callback& p_abort) override;
    void on_done(ctx_t p_wnd, bool p_was_aborted) override;

    // 静态工厂方法 - 启动异步DR分析
    static void startAnalysis(const metadb_handle_ptr& handle);

    // 🔧 Public静态方法用于C回调（FFI兼容性）
    static void handle_progress_callback(int current, int total, const char* message);
    static void handle_completion_callback(const char* result, bool success);

  private:
    metadb_handle_ptr m_handle;
    CallbackHandle m_progress_handle;
    CallbackHandle m_completion_handle;
    threaded_process_status* m_status_ptr;
    bool m_analysis_completed;
    bool m_analysis_success;
    pfc::string8 m_result_text;
    int m_task_id;                         // 🎯 保存任务ID用于取消
    std::atomic<bool> m_should_abort;      // 🎯 取消标志
    std::atomic<float> m_current_progress; // 🎯 当前进度(来自Rust的实时进度)

    // 静态回调函数（用于Rust桥接）
    static void progress_callback(int current, int total, const char* message);
    static void completion_callback(const char* result, bool success);

    // 当前活跃的工作器实例（用于静态回调）
    static MacinMeterProgressWorker* s_current_worker;
};