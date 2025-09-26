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
    int m_task_id;                    // 🎯 保存任务ID用于取消
    std::atomic<bool> m_should_abort; // 🎯 取消标志

    // 🕐 计时器和阶段信息
    std::chrono::steady_clock::time_point m_start_time; // 开始时间
    pfc::string8 m_current_stage;                       // 当前阶段描述

    // 🎭 双进度条滑块动画
    float m_slider_center;                                         // 滑块中心位置 (0.0-1.0)
    bool m_animation_direction;                                    // 移动方向 (true=右, false=左)
    std::chrono::steady_clock::time_point m_last_animation_update; // 上次动画更新时间

    // 🎭 动画和显示更新
    void updateAnimationAndDisplay();

    // 当前活跃的工作器实例（用于静态回调）
    static MacinMeterProgressWorker* s_current_worker;
};