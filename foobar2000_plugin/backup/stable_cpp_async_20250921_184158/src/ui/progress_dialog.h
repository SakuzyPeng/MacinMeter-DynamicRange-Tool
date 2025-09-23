#pragma once
#include "foobar2000.h"
#include <string>
#include <functional>
#include <thread>
#include <atomic>

/**
 * 🛡️ 极简稳定异步分析器
 *
 * 设计原则：
 * - 稳定第一：避免崩溃是最高优先级
 * - 简单可靠：使用最基础的std::thread
 * - 线程安全：小心处理UI操作
 * - 立即响应：用户点击后立即返回
 */
class StableAsyncAnalyzer {
public:
    /**
     * 🎯 启动稳定的异步DR分析
     *
     * @param tracks 音频文件列表
     * @param on_complete 完成回调（在后台线程调用，需要线程安全）
     */
    static void startAsync(
        const metadb_handle_list& tracks,
        std::function<void(const std::string&, bool)> on_complete
    );

private:
    // 禁止实例化
    StableAsyncAnalyzer() = delete;

    // 静态工作线程函数
    static void workerThread(
        metadb_handle_list tracks_copy,
        std::function<void(const std::string&, bool)> on_complete
    );
};