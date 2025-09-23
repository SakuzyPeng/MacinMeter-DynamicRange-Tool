#pragma once

#include <cstdint>
#include <functional>

// ====================================================================
// 🚀 现代异步FFI桥接 - 革命性Rust线程管理架构
// ====================================================================

/**
 * 🎯 现代异步架构设计原则：
 * - **Rust拥有一切**：线程、内存、生命周期完全由Rust管理
 * - **零阻塞设计**：立即返回任务ID，不阻塞UI线程
 * - **原生进度支持**：内置线程安全的进度回调机制
 * - **完全类型安全**：编译时保证无数据竞争
 * - **斩草除根**：彻底抛弃旧的同步接口
 */

#ifdef __cplusplus
extern "C" {
#endif

// 📞 回调函数类型定义
typedef void (*ProgressCallback)(int current, int total, const char* message);
typedef void (*CompletionCallback)(const char* result, bool success);

// 🎯 优雅的回调句柄类型
typedef unsigned int CallbackHandle;

// ====================================================================
// 🌟 优雅的回调注册接口
// ====================================================================

/**
 * 📝 注册进度回调函数
 *
 * @param callback 进度回调函数指针
 * @return 回调句柄（用于后续调用，0表示失败）
 */
CallbackHandle rust_register_progress_callback(ProgressCallback callback);

/**
 * 📝 注册完成回调函数
 *
 * @param callback 完成回调函数指针
 * @return 回调句柄（用于后续调用，0表示失败）
 */
CallbackHandle rust_register_completion_callback(CompletionCallback callback);

/**
 * 🚀 【主接口】基于样本数据的异步DR分析
 *
 * ## 设计特点
 * - **音频解码在C++侧**: 使用foobar2000的AudioAccessor正确解码
 * - **样本数据传递**: 避免文件路径访问问题
 * - **后台DR分析**: Rust在独立线程中进行DR计算
 * - **回调句柄模式**: 类型安全的进度和完成回调
 *
 * @param samples 音频样本数据指针（f32数组）
 * @param sample_count 样本总数
 * @param channels 声道数（1-2声道，自动拒绝3+声道）
 * @param sample_rate 采样率
 * @param bits_per_sample 位深度
 * @param progress_handle 进度回调句柄（0表示无进度回调）
 * @param completion_handle 完成回调句柄（必须提供有效句柄）
 *
 * @return >0: 任务ID（用于取消）, -1: 无效参数, -2: 无效句柄, -5: 声道数超限
 */
int rust_analyze_async_elegant(
    const float* samples,
    unsigned int sample_count,
    unsigned int channels,
    unsigned int sample_rate,
    unsigned int bits_per_sample,
    CallbackHandle progress_handle,
    CallbackHandle completion_handle
);

/**
 * 🚀 【新一代接口】完全异步的文件分析
 *
 * ## 革命性改进
 * - **零主线程阻塞**：包括音频解码在内的所有操作都在后台线程进行
 * - **文件路径输入**：直接传递文件路径，让Rust处理音频解码
 * - **真正的异步**：主线程立即返回，绝不阻塞UI
 * - **完整进度支持**：从解码到分析的全程进度报告
 *
 * @param file_path 音频文件路径（UTF-8编码）
 * @param progress_handle 进度回调句柄（0表示无进度回调）
 * @param completion_handle 完成回调句柄（必须提供有效句柄）
 *
 * @return >0: 任务ID（用于取消）, -1: 无效参数, -2: 无效句柄, -3: 文件不存在
 */
int rust_analyze_file_async_complete(
    const char* file_path,
    CallbackHandle progress_handle,
    CallbackHandle completion_handle
);

/**
 * 🛑 取消正在进行的异步分析任务
 *
 * @param task_id rust_analyze_async返回的任务ID
 * @return 0: 成功取消, -1: 任务不存在或已完成
 */
int rust_cancel_analysis(int task_id);

#ifdef __cplusplus
}
#endif