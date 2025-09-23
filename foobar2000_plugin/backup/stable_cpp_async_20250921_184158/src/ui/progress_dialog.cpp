#include "progress_dialog.h"
#include "../core/dr_analysis_controller.h"
#include <chrono>

void StableAsyncAnalyzer::startAsync(
    const metadb_handle_list& tracks,
    std::function<void(const std::string&, bool)> on_complete) {

    // 🎯 复制数据用于线程安全传递
    metadb_handle_list tracks_copy(tracks);

    // 🚀 启动分离的工作线程（立即返回）
    std::thread worker(workerThread, std::move(tracks_copy), on_complete);
    worker.detach(); // 分离线程，避免生命周期管理复杂性
}

void StableAsyncAnalyzer::workerThread(
    metadb_handle_list tracks_copy,
    std::function<void(const std::string&, bool)> on_complete) {

    try {
        // 🎯 在后台线程执行DR分析
        DrAnalysisController controller;
        auto analysis_result = controller.analyzeTracks(tracks_copy);

        // 🚀 准备结果文本
        std::string result_text;
        bool success = false;

        if (analysis_result.success && analysis_result.hasResults()) {
            // 合并格式化报告
            for (const auto& report : analysis_result.formatted_reports) {
                result_text += report;
                if (&report != &analysis_result.formatted_reports.back()) {
                    result_text += "\n" + std::string(70, '-') + "\n";
                }
            }
            success = true;
        } else {
            result_text = "分析失败: " +
                         (analysis_result.error_message.empty() ?
                          "未能获得有效的DR分析结果" :
                          analysis_result.error_message);
            success = false;
        }

        // 🎯 调用完成回调（让调用者处理UI线程问题）
        if (on_complete) {
            on_complete(result_text, success);
        }

    } catch (const std::exception& e) {
        // 🛡️ 异常保护
        if (on_complete) {
            std::string error_msg = "分析过程中发生异常: " + std::string(e.what());
            on_complete(error_msg, false);
        }
    } catch (...) {
        // 🛡️ 捕获所有异常，避免崩溃
        if (on_complete) {
            on_complete("发生未知异常", false);
        }
    }
}