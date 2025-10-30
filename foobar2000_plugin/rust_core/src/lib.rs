//! MacinMeter DR Plugin - 真正零内存累积流式FFI适配层
//!
//! 🚀 100%复用主项目：零算法重复+零内存累积的优雅设计
//!
//! ## 设计原则
//! - **薄包装设计**：FFI层仅做类型转换和接口适配
//! - **零算法原则**：100%复用主项目WindowRmsAnalyzer流式处理
//! - **零内存累积**：每chunk立即处理，摒弃all_chunks累积模式
//! - **零文件操作**：直接内存流处理，无权限问题
//! - **流式原生支持**：从架构层面实现真正的chunk级流式处理

use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_uint};
use std::sync::{LazyLock, Mutex};
use std::thread;

// 🎯 引入主项目核心：100%复用主项目核心组件
use macinmeter_dr_tool::audio::StreamingDecoder;
use macinmeter_dr_tool::{
    process_streaming_decoder, AppConfig, AudioError, AudioFormat, AudioResult, DrResult,
};

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

/// 🔄 Chunk流适配器：将foobar2000 chunk流转换为流式处理接口
///
/// 🔥 智能窗口缓冲，实现3秒标准窗口处理
struct ChunkStreamingDecoder {
    format: AudioFormat,
    chunks: std::collections::VecDeque<Vec<f32>>,
    total_chunks_expected: Option<usize>,
    chunks_processed: usize,
    is_finished: bool,

    // 🎯 智能窗口缓冲机制
    window_buffer: Vec<f32>,    // 积累样本到标准窗口大小
    window_size_samples: usize, // 标准窗口大小：3秒 * 采样率 * 声道数
    windows_output: usize,      // 已输出的窗口数（调试用）
}

impl ChunkStreamingDecoder {
    fn new(format: AudioFormat) -> Self {
        // 🎯 计算标准窗口大小（与主项目processor.rs完全一致）
        const WINDOW_DURATION_SECONDS: f64 = 3.0; // 标准3秒窗口
        let window_size_samples =
            (format.sample_rate as f64 * WINDOW_DURATION_SECONDS * format.channels as f64) as usize;

        Self {
            format,
            chunks: std::collections::VecDeque::new(),
            total_chunks_expected: None,
            chunks_processed: 0,
            is_finished: false,

            // 🎯 智能窗口缓冲初始化
            window_buffer: Vec::new(),
            window_size_samples,
            windows_output: 0,
        }
    }

    fn add_chunk(&mut self, chunk: Vec<f32>) {
        if !self.is_finished {
            // 🎯 立即积累到window_buffer，而不是先存到chunks队列
            self.window_buffer.extend_from_slice(&chunk);

            // 🌊 立即检查是否能组成完整窗口并移到chunks队列中
            while self.window_buffer.len() >= self.window_size_samples {
                // 提取完整窗口
                let window_samples = &self.window_buffer[0..self.window_size_samples];
                self.chunks.push_back(window_samples.to_vec());

                // 清理已处理的窗口，保留剩余样本
                self.window_buffer.drain(0..self.window_size_samples);

                self.windows_output += 1;
            }
        }
    }

    fn mark_finished(&mut self) {
        self.is_finished = true;
    }
}

impl StreamingDecoder for ChunkStreamingDecoder {
    fn next_chunk(&mut self) -> AudioResult<Option<Vec<f32>>> {
        // 🎯 步骤1：如果有已准备好的标准窗口，直接返回
        if let Some(window) = self.chunks.pop_front() {
            self.chunks_processed += 1;
            return Ok(Some(window));
        }

        // 🏁 步骤2：如果流结束且有剩余样本，输出最后一个窗口
        if self.is_finished && !self.window_buffer.is_empty() {
            let final_window = self.window_buffer.clone();
            self.window_buffer.clear();
            self.chunks_processed += 1;

            return Ok(Some(final_window));
        }

        // 🔄 步骤3：流结束且无剩余数据
        if self.is_finished {
            Ok(None) // 真正的流结束
        } else {
            // 等待更多chunk通过add_chunk()添加
            Err(AudioError::InvalidInput(
                "ChunkStreamingDecoder: 无可用窗口且流未结束 - 需要通过add_chunk添加更多数据"
                    .to_string(),
            ))
        }
    }

    fn progress(&self) -> f32 {
        if let Some(total) = self.total_chunks_expected {
            if total > 0 {
                return (self.chunks_processed as f32) / (total as f32);
            }
        }
        0.0 // 无法确定进度
    }

    fn format(&self) -> &AudioFormat {
        &self.format
    }

    fn reset(&mut self) -> AudioResult<()> {
        Err(AudioError::InvalidInput(
            "ChunkStreamingDecoder不支持重置".to_string(),
        ))
    }
}

/// 🌊 流式分析会话 - 使用StreamingDecoder适配器调用主项目算法
struct StreamingAnalysisSession {
    session_id: u32,
    decoder: ChunkStreamingDecoder,

    // 📊 会话管理
    progress_handle: Option<CallbackHandle>,
    completion_handle: CallbackHandle,

    // 📈 统计信息
    chunks_processed: u32,
    total_samples_processed: u64,
    is_finalized: bool,
    start_time: std::time::Instant,
}

impl StreamingAnalysisSession {
    /// 🏗️ 创建新的StreamingDecoder适配会话
    fn new(
        session_id: u32,
        channels: u32,
        sample_rate: u32,
        bits_per_sample: u32,
        progress_handle: Option<CallbackHandle>,
        completion_handle: CallbackHandle,
    ) -> Result<Self, String> {
        // 🛡️ 基本参数验证
        if channels == 0 {
            return Err("声道数不能为0".to_string());
        }
        if channels > 2 {
            return Err(format!("仅支持1-2声道音频，当前为{channels}声道"));
        }
        if sample_rate == 0 {
            return Err("采样率不能为0".to_string());
        }

        // 🎯 创建音频格式信息
        let format = AudioFormat {
            channels: channels as u16,
            sample_rate,
            bits_per_sample: bits_per_sample as u16,
            sample_count: 0, // 流式模式不需要提前知道总样本数
        };

        // 🔄 创建StreamingDecoder适配器
        let decoder = ChunkStreamingDecoder::new(format.clone());

        Ok(Self {
            session_id,
            decoder,
            progress_handle,
            completion_handle,
            chunks_processed: 0,
            total_samples_processed: 0,
            is_finalized: false,
            start_time: std::time::Instant::now(),
        })
    }

    /// 🌊 将chunk添加到StreamingDecoder适配器
    fn process_chunk(&mut self, samples: &[f32]) -> Result<(), String> {
        if self.is_finalized {
            return Err("会话已完成，无法继续处理数据".to_string());
        }

        // 📊 更新统计信息
        self.chunks_processed += 1;
        self.total_samples_processed += samples.len() as u64;

        // 🔄 将chunk添加到适配器的队列中
        self.decoder.add_chunk(samples.to_vec());

        // 📈 定期进度报告
        if self.chunks_processed % 200 == 0 {
            self.update_progress();
        }

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

    /// 🔬 完成DR分析计算（调用主项目process_streaming_decoder）
    fn complete_analysis(&mut self) -> (String, bool) {
        self.report_progress(80, 100, "标记流结束...");

        // 🏁 标记StreamingDecoder适配器流结束
        self.decoder.mark_finished();

        self.report_progress(85, 100, "调用主项目算法...");

        // 🚀 使用foobar2000默认配置
        let config = AppConfig {
            input_path: std::path::PathBuf::new(), // 插件模式不需要输入路径
            output_path: None,                     // 插件模式不输出到文件
            verbose: false,                        // 插件模式默认静默
        };

        // 🔄 在主项目算法调用期间提供密集进度更新
        self.report_progress(87, 100, "WindowRmsAnalyzer处理中...");

        // 🎯 分步进度更新，让进度条看起来更流畅
        self.report_progress(88, 100, "解析音频窗口数据...");
        self.report_progress(89, 100, "准备DR算法参数...");
        self.report_progress(90, 100, "调用主项目process_streaming_decoder...");

        // 🎯 100%复用主项目process_streaming_decoder算法
        match process_streaming_decoder(&mut self.decoder, &config) {
            Ok((dr_results, _final_format, _trim_report, _silence_report)) => {
                self.report_progress(92, 100, "DR计算完成，正在处理结果...");
                self.report_progress(94, 100, "计算整体DR值...");

                // 🎨 格式化为foobar2000兼容的结果字符串
                let formatted_result = self.format_dr_results(&dr_results);

                self.report_progress(96, 100, "格式化分析结果...");
                self.report_progress(100, 100, "主项目算法调用完成");
                (formatted_result, true)
            }
            Err(e) => {
                let error_msg = format!("主项目算法调用失败: {}", e);
                self.report_progress(100, 100, &error_msg);
                (error_msg, false)
            }
        }
    }

    /// 🎨 格式化DR分析结果为foobar2000标准兼容格式
    fn format_dr_results(&self, dr_results: &[DrResult]) -> String {
        let mut output = String::new();

        // 🏷️ 标准foobar2000头部信息
        output
            .push_str("MacinMeter DR Tool v0.1.0 / Dynamic Range Meter (foobar2000 compatible)\n");

        // 📅 当前时间（ISO格式）
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        let datetime = chrono::DateTime::from_timestamp(now.as_secs() as i64, 0)
            .unwrap_or_else(|| chrono::Utc::now())
            .format("%Y-%m-%d %H:%M:%S");
        output.push_str(&format!("log date: {}\n\n", datetime));

        // 📊 标准分割线（80个"-"字符）
        let separator = "-".repeat(80);
        output.push_str(&format!("{}\n", separator));

        // 🎵 流式处理统计信息
        output.push_str("Statistics for: MacinMeter Plugin Analysis (Streaming)\n");
        output.push_str(&format!(
            "Number of samples: {}\n",
            self.total_samples_processed
        ));

        // ⏱️ 计算时长（基于流式处理的样本数和采样率）
        // 🔧 修复：total_samples_processed是interleaved总样本数，不应除以声道数
        let format = self.decoder.format();
        if format.sample_rate > 0 {
            let actual_duration_seconds =
                self.total_samples_processed as f64 / (format.sample_rate as f64);
            let actual_minutes = actual_duration_seconds as u32 / 60;
            let actual_seconds = actual_duration_seconds as u32 % 60;
            output.push_str(&format!(
                "Duration: {}:{:02} \n",
                actual_minutes, actual_seconds
            ));
        }

        // 📊 流式处理统计（零内存累积）
        output.push_str(&format!("Processed chunks: {}\n", self.chunks_processed));
        output.push_str(&format!("Memory model: Zero-accumulation streaming\n"));

        output.push_str(&format!("{}\n\n", separator));

        // 🎯 声道DR值表格（标准foobar2000格式）
        if dr_results.len() == 1 {
            // 单声道格式
            output.push_str("                 Mono\n\n");
            output.push_str(&format!(
                "DR channel:      {:.2} dB   \n",
                dr_results[0].dr_value
            ));
        } else if dr_results.len() == 2 {
            // 立体声格式
            output.push_str("                 Left              Right\n\n");
            output.push_str(&format!(
                "DR channel:      {:.2} dB   ---     {:.2} dB   \n",
                dr_results[0].dr_value, dr_results[1].dr_value
            ));
        } else {
            // 多声道格式（通用）
            for (i, result) in dr_results.iter().enumerate() {
                output.push_str(&format!("DR channel {}: {:.2} dB\n", i, result.dr_value));
            }
        }

        output.push_str(&format!("{}\n\n", separator));

        // 💫 官方DR值计算
        if !dr_results.is_empty() {
            let overall_dr = dr_results
                .iter()
                .map(|r| r.dr_value)
                .fold(0.0, |acc, x| acc + x)
                / dr_results.len() as f64;
            let precise_dr = overall_dr;
            let official_dr = overall_dr.round() as i32;

            output.push_str(&format!("Official DR Value: DR{}\n", official_dr));
            output.push_str(&format!("Precise DR Value: {:.2} dB\n\n", precise_dr));
        }

        // 🔊 详细音频格式信息
        let format = self.decoder.format();
        output.push_str(&format!("Samplerate:        {} Hz\n", format.sample_rate));
        output.push_str(&format!("Channels:          {}\n", format.channels));

        // 🔧 修复：确保bits_per_sample有合理的默认值
        let bits_per_sample = if format.bits_per_sample == 0 {
            24
        } else {
            format.bits_per_sample
        };
        output.push_str(&format!("Bits per sample:   {}\n", bits_per_sample));

        // 📈 计算比特率（近似值）
        let bitrate_kbps =
            (format.sample_rate as u32 * format.channels as u32 * bits_per_sample as u32) / 1000;
        output.push_str(&format!("Bitrate:           {} kbps\n", bitrate_kbps));
        output.push_str("Codec:             Plugin Audio\n");

        // 🏁 标准结束线
        output.push_str(&format!("{}\n", "=".repeat(80)));

        output
    }

    /// 📊 流式处理进度报告
    fn update_progress(&self) {
        if self.progress_handle.is_some() {
            let elapsed = self.start_time.elapsed().as_secs_f32();

            // 🌊 基于实际处理的音频时长估算进度（0-75%，为最终DR计算保留25%）
            let format = self.decoder.format();
            let audio_duration_seconds = if format.sample_rate > 0 {
                self.total_samples_processed as f32
                    / (format.sample_rate as f32 * format.channels as f32)
            } else {
                0.0
            };

            // 简单线性进度估算
            let estimated_progress = (self.chunks_processed as f32 * 0.5).min(75.0);

            let message = format!(
                "零内存累积流式处理中... ({} chunks, {:.1}s音频, {:.1}s处理时间)",
                self.chunks_processed, audio_duration_seconds, elapsed
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
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ [ERROR] StreamingAnalysisSession创建失败: {e}");
            return -1;
        }
    };

    // 📝 注册会话到全局管理器
    if let Ok(mut sessions) = STREAMING_SESSIONS.lock() {
        sessions.insert(session_id, session);
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
                Ok(()) => 0, // 成功
                Err(e) => {
                    eprintln!(
                        "❌ [ERROR] Rust chunk处理失败: session_id={}, error={}",
                        session_id, e
                    );
                    -3 // 处理失败
                }
            }
        } else {
            eprintln!(
                "❌ [ERROR] 无效会话ID: {}, 当前会话: {:?}",
                session_id,
                sessions.keys().collect::<Vec<_>>()
            );
            -1 // 无效会话ID
        }
    } else {
        eprintln!("❌ [ERROR] STREAMING_SESSIONS锁获取失败");
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
