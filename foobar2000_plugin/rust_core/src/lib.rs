use std::ffi::CString;
use std::os::raw::{c_char, c_double, c_int, c_uint};
use std::ptr;

// 引入主工程的真实DR计算核心
use macinmeter_dr_core::{AudioResult, DrCalculator, DrResult};

// 扩展的FFI结构体定义 - 包含每声道明细
#[repr(C)]
pub struct DrAnalysisResult {
    pub official_dr_value: c_double, // 整体官方DR值
    pub precise_dr_value: c_double,  // 整体精确DR值
    pub peak_db: c_double,           // 整体Peak值
    pub rms_db: c_double,            // 整体RMS值
    pub channel: c_uint,             // 声道索引（兼容旧接口）
    pub sample_rate: c_uint,         // 采样率
    pub channels: c_uint,            // 总声道数
    pub bits_per_sample: c_uint,     // 位深度
    pub duration_seconds: c_double,  // 时长（秒）
    pub file_name: [c_char; 256],    // 文件名
    pub codec: [c_char; 32],         // 编解码器

    // 每声道明细数组 (最大支持8声道)
    pub peak_db_per_channel: [c_double; 8], // 每声道Peak值
    pub rms_db_per_channel: [c_double; 8],  // 每声道RMS值
    pub dr_db_per_channel: [c_double; 8],   // 每声道DR值
    pub rms_top20_linear_per_channel: [c_double; 8], // 每声道20%RMS线性值
    pub peak_source_per_channel: [c_int; 8], // 峰值来源：0=主峰,1=次峰,2=回退
    pub total_samples: c_uint,              // 总样本数（真实值）
}

// 会话式DR分析器
pub struct DrSession {
    calculator: DrCalculator,
    channels: usize,
    sample_rate: u32,
    total_samples: usize,
    sample_buffer: Vec<f32>, // 累积交错音频数据
    enable_sum_doubling: bool,
}

impl DrSession {
    pub fn new(channels: usize, sample_rate: u32, enable_sum_doubling: bool) -> AudioResult<Self> {
        // 使用逐包直通模式：块大小设为很小的值，确保每个foobar2000包都作为独立块处理
        // 使用很小的块持续时间以实现流式处理（0.1秒 = 100ms）
        let calculator = DrCalculator::new(channels, enable_sum_doubling, sample_rate, 0.1)?;

        Ok(Self {
            calculator,
            channels,
            sample_rate,
            total_samples: 0,
            sample_buffer: Vec::new(), // 保留但不使用，为了避免FFI兼容性问题
            enable_sum_doubling,
        })
    }

    pub fn feed_interleaved(&mut self, samples: &[f32]) -> AudioResult<()> {
        // 立即处理此包数据，而不是累积
        // 使用process_decoder_chunk保持foobar2000的原生解码包边界
        self.calculator.process_decoder_chunk(samples, self.channels)?;
        self.total_samples += samples.len() / self.channels;
        Ok(())
    }

    pub fn finalize(&mut self) -> AudioResult<Vec<DrResult>> {
        if self.total_samples == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "No audio data to analyze",
            )
            .into());
        }

        // 获取累积的DR计算结果
        // 由于我们已经通过process_chunk逐包处理了所有数据
        // 现在只需要获取最终结果
        let results = self.calculator.finalize()?;
        Ok(results)
    }
}

fn copy_str_to_c_array(rust_str: &str, c_array: &mut [c_char]) {
    let c_string = CString::new(rust_str).unwrap_or_else(|_| CString::new("").unwrap());
    let bytes = c_string.as_bytes_with_nul();
    let copy_len = std::cmp::min(bytes.len(), c_array.len());

    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr() as *const c_char,
            c_array.as_mut_ptr(),
            copy_len,
        );
    }

    if copy_len > 0 {
        c_array[copy_len - 1] = 0;
    }
}

// 转换DrResult向量为C结构体
fn convert_dr_results_to_c(
    dr_results: &[DrResult],
    c_result: *mut DrAnalysisResult,
    channels: usize,
    sample_rate: u32,
    total_frames: usize,
) {
    unsafe {
        let result = &mut *c_result;

        // 初始化结构体
        *result = std::mem::zeroed();

        // 基本信息
        result.channels = channels as c_uint;
        result.sample_rate = sample_rate;
        result.total_samples = total_frames as c_uint;
        result.bits_per_sample = 32; // 硬编码为32位（foobar2000内部格式）
        // 正确计算duration：total_frames已经是帧数，直接除以采样率
        result.duration_seconds = (total_frames as f64) / (sample_rate as f64);

        copy_str_to_c_array("foobar2000", &mut result.codec);
        copy_str_to_c_array("stream", &mut result.file_name);

        if !dr_results.is_empty() {
            // 计算整体值（取所有声道平均）
            let total_dr: f64 = dr_results.iter().map(|r| r.dr_value).sum();
            let total_peak: f64 = dr_results.iter().map(|r| 20.0 * r.peak.log10()).sum();
            let total_rms: f64 = dr_results.iter().map(|r| 20.0 * r.rms.log10()).sum();

            result.official_dr_value = total_dr / dr_results.len() as f64;
            result.precise_dr_value = result.official_dr_value;
            result.peak_db = total_peak / dr_results.len() as f64;
            result.rms_db = total_rms / dr_results.len() as f64;

            // 填充每声道明细
            for (i, dr_result) in dr_results.iter().enumerate().take(8) {
                result.peak_db_per_channel[i] = 20.0 * dr_result.peak.log10();
                result.rms_db_per_channel[i] = 20.0 * dr_result.rms.log10();
                result.dr_db_per_channel[i] = dr_result.dr_value;
                result.rms_top20_linear_per_channel[i] = dr_result.rms;
                result.peak_source_per_channel[i] = 0; // 默认主峰
            }
        }
    }
}

// 会话式FFI导出函数
#[no_mangle]
pub extern "C" fn dr_session_new(
    channels: c_uint,
    sample_rate: c_uint,
    enable_sum_doubling: c_int,
) -> *mut DrSession {
    match DrSession::new(channels as usize, sample_rate, enable_sum_doubling != 0) {
        Ok(session) => Box::into_raw(Box::new(session)),
        Err(_) => ptr::null_mut(),
    }
}

/// 向DR会话添加音频样本数据
///
/// # Safety
/// 调用者必须确保：
/// - `session` 是通过 `dr_session_new` 创建的有效指针
/// - `samples` 指向至少 `frame_count * channels` 个有效的 f32 样本
/// - 在会话被释放前，指针仍然有效
#[no_mangle]
pub unsafe extern "C" fn dr_session_feed_interleaved(
    session: *mut DrSession,
    samples: *const f32,
    frame_count: c_uint,
) -> c_int {
    if session.is_null() || samples.is_null() || frame_count == 0 {
        return -1;
    }

    let session = &mut *session;
    let channels = session.channels;
    let sample_count = (frame_count as usize) * channels;

    let sample_slice = std::slice::from_raw_parts(samples, sample_count);

    match session.feed_interleaved(sample_slice) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

/// 完成DR分析并获取结果
///
/// # Safety
/// 调用者必须确保：
/// - `session` 是通过 `dr_session_new` 创建的有效指针
/// - `result` 指向一个有效的 `DrAnalysisResult` 结构体
/// - 在会话被释放前，指针仍然有效
#[no_mangle]
pub unsafe extern "C" fn dr_session_finalize(
    session: *mut DrSession,
    result: *mut DrAnalysisResult,
) -> c_int {
    if session.is_null() || result.is_null() {
        return -1;
    }

    let session = &mut *session;

    match session.finalize() {
        Ok(dr_results) => {
            // 转换DrResult向量为C结构体
            convert_dr_results_to_c(
                &dr_results,
                result,
                session.channels,
                session.sample_rate,
                session.total_samples,
            );
            0
        }
        Err(_) => -1,
    }
}

/// 释放DR会话资源
///
/// # Safety
/// 调用者必须确保：
/// - `session` 是通过 `dr_session_new` 创建的有效指针，或者是 null
/// - 释放后不再使用该指针
/// - 每个会话只能释放一次
#[no_mangle]
pub unsafe extern "C" fn dr_session_free(session: *mut DrSession) {
    if !session.is_null() {
        drop(Box::from_raw(session));
    }
}
