#pragma once

#include "foobar2000.h"
#include "../audio/audio_accessor.h"
#include "../bridge/rust_bridge.h"
#include <functional>
#include <string>
#include <vector>

/**
 * DR分析业务控制器
 *
 * 🎯 核心职责：
 * - 业务流程编排和协调
 * - 统一的错误处理和异常管理
 * - 结果聚合和格式化
 * - 进度报告和用户反馈
 *
 * 🏗️ 架构定位：
 * UI层 → 控制器层 → 服务层(AudioAccessor) → FFI层(rust_bridge)
 */
class DrAnalysisController {
public:
    /**
     * 🚀 统一的分析结果结构（革命性简化）
     */
    struct AnalysisResult {
        std::vector<std::string> formatted_reports;  // 🚀 格式化的DR报告字符串列表
        std::vector<AudioData> audio_data;           // 音频数据列表（用于UI显示）
        bool success = false;                        // 整体操作是否成功
        std::string error_message;                   // 错误信息（如果有）
        size_t processed_count = 0;                  // 成功处理的文件数
        size_t failed_count = 0;                     // 失败的文件数
        double total_duration = 0.0;                 // 总处理时长（秒）

        // 便利方法
        bool hasResults() const { return !formatted_reports.empty(); }
        bool hasErrors() const { return !error_message.empty() || failed_count > 0; }
        size_t totalCount() const { return processed_count + failed_count; }
    };

    /**
     * 进度回调函数类型
     * 参数：(状态消息, 当前进度, 总数)
     */
    using ProgressCallback = std::function<void(const std::string&, int, int)>;

    /**
     * 🚀 异步分析完成回调函数类型
     * 参数：(分析结果)
     */
    using AsyncCallback = std::function<void(const AnalysisResult&)>;

public:
    DrAnalysisController() = default;
    ~DrAnalysisController() = default;

    // 禁止复制和赋值（控制器应该是无状态的）
    DrAnalysisController(const DrAnalysisController&) = delete;
    DrAnalysisController& operator=(const DrAnalysisController&) = delete;

    /**
     * 🎯 核心分析接口：批量分析音频文件
     *
     * @param handles foobar2000音频文件句柄列表
     * @return 统一的分析结果，包含成功/失败信息
     */
    AnalysisResult analyzeTracks(const pfc::list_base_const_t<metadb_handle_ptr>& handles);

    /**
     * 🎯 单文件分析接口
     *
     * @param handle 单个foobar2000音频文件句柄
     * @return 统一的分析结果
     */
    AnalysisResult analyzeTrack(const metadb_handle_ptr& handle);

    /**
     * 设置进度回调函数（可选）
     *
     * @param callback 进度回调函数，用于UI进度显示
     */
    void setProgressCallback(ProgressCallback callback);

    /**
     * 🚀 异步分析接口：批量分析音频文件（非阻塞）
     *
     * 架构职责：
     * - 控制器层管理异步执行和线程生命周期
     * - UI层只需调用此接口并提供回调函数
     * - 回调函数在后台线程执行，UI层需要处理线程安全
     *
     * @param handles foobar2000音频文件句柄列表
     * @param callback 异步完成回调函数
     * @param progress_callback 进度回调函数（可选）
     */
    void analyzeTracksAsync(
        const pfc::list_base_const_t<metadb_handle_ptr>& handles,
        AsyncCallback callback,
        ProgressCallback progress_callback = nullptr
    );

private:
    // 依赖的服务
    AudioAccessor audio_accessor_;

    // 配置
    ProgressCallback progress_callback_;

    // 辅助方法
    void reportProgress(const std::string& message, int current, int total);
    void handleAnalysisError(const std::exception& e, AnalysisResult& result, const std::string& context);
    void logAnalysisStart(size_t track_count);
    void logAnalysisComplete(const AnalysisResult& result);
};