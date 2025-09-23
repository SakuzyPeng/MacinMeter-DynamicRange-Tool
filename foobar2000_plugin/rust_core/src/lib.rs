//! MacinMeter DR Plugin - Chunk流式FFI适配层
//!
//! 🚀 100%复用主项目：零算法重复的优雅设计
//!
//! ## 设计原则
//! - **薄包装设计**：FFI层仅做类型转换和接口适配
//! - **零算法原则**：100%复用主项目ChunkStreamDecoder + process_audio_file_streaming
//! - **零内存累积**：使用ChunkFeeder流式喂数据，避免内存爆炸
//! - **零文件操作**：直接内存流处理，无权限问题
//! - **原生异步支持**：从架构层面支持非阻塞分析和进度报告

use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_uint};
use std::sync::{LazyLock, Mutex};
use std::thread;

// 🎯 引入主项目核心：100%复用算法和格式化
// 注：准备重新设计为纯黑盒调用架构，暂时移除未使用的导入
// 注：准备重新设计为纯黑盒调用架构

// ====================================================================
// 🚀 现代异步FFI架构 - Rust拥有一切
// ====================================================================

/// 📞 C++回调函数类型定义
type ProgressCallback = unsafe extern "C" fn(current: c_int, total: c_int, message: *const c_char);
type CompletionCallback = unsafe extern "C" fn(result: *const c_char, success: bool);

/// 🎯 优雅的回调句柄类型
type CallbackHandle = u32;

/// 🏗️ 优雅的回调管理器
struct CallbackManager {
    progress_callbacks: HashMap<CallbackHandle, ProgressCallback>,
    completion_callbacks: HashMap<CallbackHandle, CompletionCallback>,
    next_handle: u32,
}

impl CallbackManager {
    fn new() -> Self {
        Self {
            progress_callbacks: HashMap::new(),
            completion_callbacks: HashMap::new(),
            next_handle: 1,
        }
    }

    fn register_progress_callback(&mut self, callback: ProgressCallback) -> CallbackHandle {
        let handle = self.next_handle;
        self.next_handle += 1;
        self.progress_callbacks.insert(handle, callback);
        handle
    }

    fn register_completion_callback(&mut self, callback: CompletionCallback) -> CallbackHandle {
        let handle = self.next_handle;
        self.next_handle += 1;
        self.completion_callbacks.insert(handle, callback);
        handle
    }

    fn call_progress(&self, handle: CallbackHandle, current: i32, total: i32, message: &str) {
        if let Some(callback) = self.progress_callbacks.get(&handle) {
            let c_message = CString::new(message).unwrap_or_else(|_| CString::new("").unwrap());
            unsafe {
                callback(current, total, c_message.as_ptr());
            }
        }
    }

    fn call_completion(&mut self, handle: CallbackHandle, result: &str, success: bool) {
        if let Some(callback) = self.completion_callbacks.remove(&handle) {
            let c_result = CString::new(result).unwrap_or_else(|_| CString::new("").unwrap());
            unsafe {
                callback(c_result.as_ptr(), success);
            }
        }
    }

    fn cleanup(&mut self, progress_handle: CallbackHandle, completion_handle: CallbackHandle) {
        self.progress_callbacks.remove(&progress_handle);
        self.completion_callbacks.remove(&completion_handle);
    }
}

/// 🌟 全局回调管理器
static CALLBACK_MANAGER: LazyLock<Mutex<CallbackManager>> =
    LazyLock::new(|| Mutex::new(CallbackManager::new()));

/// 🏗️ 会话管理器
static SESSION_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static STREAMING_SESSIONS: LazyLock<Mutex<HashMap<u32, StreamingAnalysisSession>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 🌊 【重新设计】简化分析会话 - 纯黑盒调用方案
#[allow(dead_code)] // 待重构：这些字段将在黑盒调用架构中使用
struct StreamingAnalysisSession {
    session_id: u32,
    channels: u32,
    sample_rate: u32,
    bits_per_sample: u32,
    progress_handle: Option<CallbackHandle>,
    completion_handle: CallbackHandle,

    // 🎯 【流式黑盒方案】通过子进程管道流式调用本体
    // 注：严禁完整数据收集，必须保持流式特性

    // 📊 统计信息
    processed_samples: u64, // 已处理样本总数
    chunks_processed: u32,  // 已处理chunk数量
    is_finalized: bool,     // 是否已完成

    // ⏱️ 进度估算
    start_time: std::time::Instant, // 会话开始时间
}

impl StreamingAnalysisSession {
    /// 🏗️ 创建新的流式分析会话
    fn new(
        session_id: u32,
        channels: u32,
        sample_rate: u32,
        bits_per_sample: u32,
        progress_handle: Option<CallbackHandle>,
        completion_handle: CallbackHandle,
    ) -> Result<Self, String> {
        // 🛡️ 详细参数验证和调试信息
        if channels == 0 {
            return Err("声道数不能为0".to_string());
        }
        if channels > 2 {
            return Err(format!("仅支持1-2声道音频，当前为{channels}声道"));
        }
        if sample_rate == 0 {
            return Err("采样率不能为0".to_string());
        }
        if sample_rate > 384000 {
            return Err(format!("采样率过高: {sample_rate}Hz，最大支持384kHz"));
        }

        // 🎯 【待重构】纯黑盒调用架构 - 暂时移除未实现的ChunkStreamDecoder
        // TODO: 重新设计为直接调用主项目DR算法的黑盒接口

        Ok(Self {
            session_id,
            channels,
            sample_rate,
            bits_per_sample,
            progress_handle,
            completion_handle,
            processed_samples: 0,
            chunks_processed: 0,
            is_finalized: false,
            start_time: std::time::Instant::now(),
        })
    }

    /// 🌊 处理音频数据块（零内存累积）
    fn process_chunk(&mut self, samples: &[f32]) -> Result<(), String> {
        if self.is_finalized {
            return Err("会话已完成，无法继续处理数据".to_string());
        }

        // 🎯 【待重构】直接调用主项目DR算法（零算法重复）
        // TODO: 实现纯黑盒调用，直接使用DrCalculator::calculate_dr_from_samples

        // 📊 更新统计信息
        self.processed_samples += samples.len() as u64;
        self.chunks_processed += 1;

        // 📈 计算并报告进度（基于处理时间和chunk数量）
        self.update_progress();

        Ok(())
    }

    /// 🏁 完成流式分析并异步返回结果
    fn finalize(mut self) {
        if self.is_finalized {
            return;
        }

        self.is_finalized = true;

        // 🚀 在独立线程中完成分析，避免阻塞调用线程
        thread::spawn(move || {
            let result = self.complete_analysis();

            // 📞 通过回调返回结果
            if let Ok(mut manager) = CALLBACK_MANAGER.lock() {
                manager.call_completion(self.completion_handle, &result.0, result.1);

                // 🧹 清理回调句柄
                if let Some(progress_handle) = self.progress_handle {
                    manager.cleanup(progress_handle, self.completion_handle);
                }
            }

            // 🧹 从全局会话管理器中移除
            if let Ok(mut sessions) = STREAMING_SESSIONS.lock() {
                sessions.remove(&self.session_id);
            }
        });
    }

    /// 🔬 完成DR分析计算（100%复用主项目streaming API）
    fn complete_analysis(&mut self) -> (String, bool) {
        self.report_progress(80, 100, "标记数据流结束...");

        // 🏁 标记数据流结束，让ChunkStreamDecoder知道没有更多数据
        // TODO: 调用DrCalculator完成最终DR计算

        self.report_progress(85, 100, "调用主项目流式处理...");

        // 🎯 100%复用主项目的main.rs流程逻辑
        match self.process_with_main_project_streaming() {
            Ok(formatted_result) => {
                self.report_progress(100, 100, "分析完成");
                (formatted_result, true)
            }
            Err(e) => {
                let error_msg = format!("DR分析失败: {e}");
                self.report_progress(100, 100, &error_msg);
                (error_msg, false)
            }
        }
    }

    /// 🎯 【待重新设计】使用黑盒调用本体处理逻辑
    fn process_with_main_project_streaming(&mut self) -> Result<String, String> {
        // TODO: 重新设计为纯黑盒调用：写临时文件 → 调用本体 → 返回结果
        Err("待重新实现为黑盒调用架构".to_string())
    }

    /// 📊 更新进度报告
    fn update_progress(&self) {
        if self.progress_handle.is_some() {
            let elapsed = self.start_time.elapsed().as_secs_f32();

            // 🌊 基于处理时间和chunk数量估算进度（0-85%）
            // 剩余15%留给最终的DR计算
            let estimated_progress = if self.chunks_processed < 10 {
                // 早期阶段：基于时间的保守估算
                (elapsed / 10.0 * 85.0).min(20.0)
            } else {
                // 稳定阶段：基于chunk处理速度
                let chunks_per_second = self.chunks_processed as f32 / elapsed.max(1.0);
                let estimated_total_chunks = chunks_per_second * 10.0; // 估算10秒完成
                let progress =
                    (self.chunks_processed as f32 / estimated_total_chunks * 85.0).min(85.0);
                progress.max(20.0) // 确保不低于早期进度
            };

            let message = format!(
                "处理中... ({} chunks, {elapsed:.1}s)",
                self.chunks_processed
            );
            self.report_progress(estimated_progress as i32, 100, &message);
        }
    }

    /// 📊 报告进度（线程安全）
    fn report_progress(&self, current: i32, total: i32, message: &str) {
        if let Some(handle) = self.progress_handle {
            if let Ok(manager) = CALLBACK_MANAGER.lock() {
                manager.call_progress(handle, current, total, message);
            }
        }
    }
}

// ====================================================================
// 🌟 优雅的回调注册接口
// ====================================================================

/// 📝 注册进度回调函数
///
/// @param callback 进度回调函数指针
/// @return 回调句柄（用于后续调用）
///
/// # Safety
///
/// 此函数是unsafe的，因为它接受一个C函数指针作为回调。
/// 调用者必须确保：
/// - callback是一个有效的函数指针
/// - callback函数在整个分析过程中保持有效
/// - 不会从多个线程同时调用此函数
#[no_mangle]
pub unsafe extern "C" fn rust_register_progress_callback(
    callback: ProgressCallback,
) -> CallbackHandle {
    if let Ok(mut manager) = CALLBACK_MANAGER.lock() {
        manager.register_progress_callback(callback)
    } else {
        0 // 失败返回0（无效句柄）
    }
}

/// 📝 注册完成回调函数
///
/// @param callback 完成回调函数指针
/// @return 回调句柄（用于后续调用）
///
/// # Safety
///
/// 此函数是unsafe的，因为它接受一个C函数指针作为回调。
/// 调用者必须确保：
/// - callback是一个有效的函数指针
/// - callback函数在整个分析过程中保持有效
/// - 不会从多个线程同时调用此函数
#[no_mangle]
pub unsafe extern "C" fn rust_register_completion_callback(
    callback: CompletionCallback,
) -> CallbackHandle {
    if let Ok(mut manager) = CALLBACK_MANAGER.lock() {
        manager.register_completion_callback(callback)
    } else {
        0 // 失败返回0（无效句柄）
    }
}

// ====================================================================
// 🌊 流式分块处理FFI接口 - 零内存占用的终极解决方案
// ====================================================================

/// 🚀 【流式分析】初始化流式DR分析会话
///
/// # Safety
///
/// 此函数是unsafe的，因为它处理C FFI边界的原始参数。
/// 调用者必须确保：
/// - channels、sample_rate、bits_per_sample参数在有效范围内
/// - progress_handle和completion_handle要么为0（无效），要么是之前注册的有效句柄
/// - 不会从多个线程同时调用此函数
#[no_mangle]
pub unsafe extern "C" fn rust_streaming_analysis_init(
    channels: c_uint,
    sample_rate: c_uint,
    bits_per_sample: c_uint,
    progress_handle: CallbackHandle, // 0表示无进度回调
    completion_handle: CallbackHandle,
) -> c_int {
    // 🔍 调试日志：记录输入参数
    eprintln!("🔍 [DEBUG] rust_streaming_analysis_init called:");
    eprintln!("   channels: {channels}");
    eprintln!("   sample_rate: {sample_rate}");
    eprintln!("   bits_per_sample: {bits_per_sample}");
    eprintln!("   progress_handle: {progress_handle}");
    eprintln!("   completion_handle: {completion_handle}");

    // 🛡️ FFI边界安全检查
    if channels == 0 || sample_rate == 0 {
        eprintln!("❌ [ERROR] 基础参数检查失败: {channels}, sample_rate={sample_rate}");
        return -1;
    }

    // 🔥 提前检查声道限制
    if channels > 2 {
        eprintln!("❌ [ERROR] 声道数超限: {channels} > 2");
        return -5;
    }

    // 🎯 验证回调句柄有效性
    {
        let manager = match CALLBACK_MANAGER.lock() {
            Ok(m) => m,
            Err(e) => {
                eprintln!("❌ [ERROR] CALLBACK_MANAGER锁获取失败: {e:?}");
                return -2;
            }
        };

        if completion_handle == 0 {
            eprintln!("❌ [ERROR] completion_handle为0，无效");
            return -2;
        }

        if !manager
            .completion_callbacks
            .contains_key(&completion_handle)
        {
            eprintln!("❌ [ERROR] completion_handle {completion_handle} 未在管理器中注册");
            eprintln!(
                "   当前注册的completion_callbacks: {:?}",
                manager.completion_callbacks.keys().collect::<Vec<_>>()
            );
            return -2;
        }

        if progress_handle != 0 && !manager.progress_callbacks.contains_key(&progress_handle) {
            eprintln!("❌ [ERROR] progress_handle {progress_handle} 未在管理器中注册");
            eprintln!(
                "   当前注册的progress_callbacks: {:?}",
                manager.progress_callbacks.keys().collect::<Vec<_>>()
            );
            return -2;
        }

        eprintln!("✅ [DEBUG] 回调句柄验证通过");
    }

    // 🆔 生成唯一会话ID
    let raw_session_id = SESSION_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let session_id = if raw_session_id == 0 {
        1 // 避免会话ID为0
    } else if raw_session_id > i32::MAX as u32 {
        SESSION_COUNTER.store(2, std::sync::atomic::Ordering::SeqCst);
        1
    } else {
        raw_session_id
    };

    eprintln!("🆔 [DEBUG] 生成会话ID: {session_id}");

    // 🏗️ 创建流式分析会话
    let session = match StreamingAnalysisSession::new(
        session_id,
        channels,
        sample_rate,
        bits_per_sample,
        if progress_handle == 0 {
            None
        } else {
            Some(progress_handle)
        },
        completion_handle,
    ) {
        Ok(s) => {
            eprintln!("✅ [DEBUG] StreamingAnalysisSession创建成功");
            s
        }
        Err(e) => {
            eprintln!("❌ [ERROR] StreamingAnalysisSession创建失败: {e}");
            return -1;
        }
    };

    // 📝 注册会话到全局管理器
    if let Ok(mut sessions) = STREAMING_SESSIONS.lock() {
        sessions.insert(session_id, session);
        eprintln!("✅ [DEBUG] 会话注册成功，返回session_id: {session_id}");
        session_id as c_int
    } else {
        eprintln!("❌ [ERROR] STREAMING_SESSIONS锁获取失败");
        -1
    }
}

/// 🌊 【流式分析】发送音频数据块
///
/// # Safety
///
/// 此函数是unsafe的，因为它解引用原始指针。
/// 调用者必须确保：
/// - session_id是之前通过rust_streaming_analysis_init返回的有效ID
/// - samples指针指向有效的f32数组，包含至少sample_count个元素
/// - sample_count准确反映samples数组的大小
/// - samples指针在函数调用期间保持有效
#[no_mangle]
pub unsafe extern "C" fn rust_streaming_analysis_send_chunk(
    session_id: c_int,
    samples: *const f32,
    sample_count: c_uint,
) -> c_int {
    // 🛡️ FFI边界安全检查
    if samples.is_null() || sample_count == 0 || session_id <= 0 {
        return -2;
    }

    // 📊 安全转换样本数据
    let samples_slice = std::slice::from_raw_parts(samples, sample_count as usize);

    // 🔍 查找并处理会话
    if let Ok(mut sessions) = STREAMING_SESSIONS.lock() {
        if let Some(session) = sessions.get_mut(&(session_id as u32)) {
            match session.process_chunk(samples_slice) {
                Ok(()) => 0,  // 成功
                Err(_) => -3, // 处理失败
            }
        } else {
            -1 // 无效会话ID
        }
    } else {
        -1 // 锁获取失败
    }
}

/// 🏁 【流式分析】完成流式分析并获取结果
///
/// # Safety
///
/// 此函数是unsafe的，因为它处理C FFI边界的原始参数。
/// 调用者必须确保：
/// - session_id是之前通过rust_streaming_analysis_init返回的有效ID
/// - 该session_id没有被之前的finalize或cancel调用消费过
/// - 不会从多个线程同时调用此函数
#[no_mangle]
pub unsafe extern "C" fn rust_streaming_analysis_finalize(session_id: c_int) -> c_int {
    if session_id <= 0 {
        return -1;
    }

    // 🔍 移除并完成会话
    if let Ok(mut sessions) = STREAMING_SESSIONS.lock() {
        if let Some(session) = sessions.remove(&(session_id as u32)) {
            // 🚀 异步完成分析
            session.finalize();
            0 // 成功启动完成处理
        } else {
            -1 // 无效会话ID
        }
    } else {
        -2 // 锁获取失败
    }
}

/// 🛑 【流式分析】取消流式分析会话
///
/// # Safety
///
/// 此函数是unsafe的，因为它处理C FFI边界的原始参数。
/// 调用者必须确保：
/// - session_id是之前通过rust_streaming_analysis_init返回的有效ID
/// - 该session_id没有被之前的finalize或cancel调用消费过
/// - 不会从多个线程同时调用此函数
#[no_mangle]
pub unsafe extern "C" fn rust_streaming_analysis_cancel(session_id: c_int) -> c_int {
    if session_id <= 0 {
        return -1;
    }

    // 🛑 移除会话实现取消
    if let Ok(mut sessions) = STREAMING_SESSIONS.lock() {
        if sessions.remove(&(session_id as u32)).is_some() {
            0 // 成功取消
        } else {
            -1 // 会话不存在
        }
    } else {
        -1 // 锁获取失败
    }
}
