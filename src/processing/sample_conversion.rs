//! 音频样本格式转换引擎
//!
//! 提供高性能的音频格式转换，支持多种样本格式到f32的SIMD优化转换。
//! 基于与ChannelSeparator相同的架构设计，复用SimdProcessor基础设施。

use super::simd_core::SimdProcessor;
use crate::error::{self, AudioResult};

#[cfg(debug_assertions)]
macro_rules! debug_conversion {
    ($($arg:tt)*) => {
        eprintln!("[CONVERSION_DEBUG] {}", format_args!($($arg)*));
    };
}

#[cfg(not(debug_assertions))]
macro_rules! debug_conversion {
    ($($arg:tt)*) => {};
}

/// 音频样本格式枚举
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SampleFormat {
    /// 16位有符号整数 [-32768, 32767]
    S16,
    /// 24位有符号整数 [-8388608, 8388607]
    S24,
    /// 32位有符号整数 [-2147483648, 2147483647]
    S32,
    /// 32位浮点数 [-1.0, 1.0]
    F32,
    /// 64位浮点数 [-1.0, 1.0]
    F64,
    /// 8位无符号整数 [0, 255]
    U8,
    /// 16位无符号整数 [0, 65535]
    U16,
    /// 24位无符号整数 [0, 16777215]
    U24,
    /// 32位无符号整数 [0, 4294967295]
    U32,
    /// 8位有符号整数 [-128, 127]
    S8,
}

impl SampleFormat {
    /// 获取样本格式的位深度
    pub fn bit_depth(&self) -> u8 {
        match self {
            SampleFormat::S8 | SampleFormat::U8 => 8,
            SampleFormat::S16 | SampleFormat::U16 => 16,
            SampleFormat::S24 | SampleFormat::U24 => 24,
            SampleFormat::S32 | SampleFormat::U32 | SampleFormat::F32 => 32,
            SampleFormat::F64 => 64,
        }
    }

    /// 是否为浮点格式
    pub fn is_float(&self) -> bool {
        matches!(self, SampleFormat::F32 | SampleFormat::F64)
    }

    /// 是否为有符号格式
    pub fn is_signed(&self) -> bool {
        matches!(
            self,
            SampleFormat::S8
                | SampleFormat::S16
                | SampleFormat::S24
                | SampleFormat::S32
                | SampleFormat::F32
                | SampleFormat::F64
        )
    }
}

/// 样本转换结果统计
#[derive(Debug, Clone)]
pub struct ConversionStats {
    /// 输入样本数量
    pub input_samples: usize,
    /// 输出样本数量
    pub output_samples: usize,
    /// 是否使用了SIMD优化
    pub used_simd: bool,
    /// SIMD处理的样本数量
    pub simd_samples: usize,
    /// 标量处理的样本数量
    pub scalar_samples: usize,
    /// 转换耗时(纳秒)
    pub duration_ns: u64,
}

impl ConversionStats {
    /// 创建新的统计信息
    pub fn new(input_samples: usize) -> Self {
        Self {
            input_samples,
            output_samples: 0,
            used_simd: false,
            simd_samples: 0,
            scalar_samples: 0,
            duration_ns: 0,
        }
    }

    /// 计算SIMD效率百分比
    pub fn simd_efficiency(&self) -> f32 {
        if self.input_samples == 0 {
            0.0
        } else {
            (self.simd_samples as f32) / (self.input_samples as f32) * 100.0
        }
    }

    /// 计算转换速度(样本/秒)
    pub fn samples_per_second(&self) -> f64 {
        if self.duration_ns == 0 {
            0.0
        } else {
            (self.input_samples as f64) / (self.duration_ns as f64 / 1_000_000_000.0)
        }
    }
}

/// 样本转换trait - 定义所有格式转换的通用接口
pub trait SampleConversion {
    /// 转换i16数组到f32数组
    fn convert_i16_to_f32(
        &self,
        input: &[i16],
        output: &mut Vec<f32>,
    ) -> AudioResult<ConversionStats>;

    /// 转换i24数组到f32数组
    fn convert_i24_to_f32(
        &self,
        input: &[symphonia::core::sample::i24],
        output: &mut Vec<f32>,
    ) -> AudioResult<ConversionStats>;

    /// 转换i32数组到f32数组
    fn convert_i32_to_f32(
        &self,
        input: &[i32],
        output: &mut Vec<f32>,
    ) -> AudioResult<ConversionStats>;

    /// 转换f64数组到f32数组
    fn convert_f64_to_f32(
        &self,
        input: &[f64],
        output: &mut Vec<f32>,
    ) -> AudioResult<ConversionStats>;

    /// 转换u8数组到f32数组
    fn convert_u8_to_f32(
        &self,
        input: &[u8],
        output: &mut Vec<f32>,
    ) -> AudioResult<ConversionStats>;

    /// 转换u16数组到f32数组
    fn convert_u16_to_f32(
        &self,
        input: &[u16],
        output: &mut Vec<f32>,
    ) -> AudioResult<ConversionStats>;

    /// 转换u24数组到f32数组
    fn convert_u24_to_f32(
        &self,
        input: &[symphonia::core::sample::u24],
        output: &mut Vec<f32>,
    ) -> AudioResult<ConversionStats>;

    /// 转换u32数组到f32数组
    fn convert_u32_to_f32(
        &self,
        input: &[u32],
        output: &mut Vec<f32>,
    ) -> AudioResult<ConversionStats>;

    /// 转换i8数组到f32数组
    fn convert_i8_to_f32(
        &self,
        input: &[i8],
        output: &mut Vec<f32>,
    ) -> AudioResult<ConversionStats>;

    /// 获取支持的SIMD能力
    fn simd_capabilities(&self) -> &super::simd_core::SimdCapabilities;
}

/// 高性能样本转换引擎
///
/// 提供音频样本格式到f32的SIMD优化转换，支持：
/// - 多种输入格式：i8/u8/i16/u16/i24/u24/i32/u32/f64
/// - 跨平台SIMD优化：SSE2/AVX2(x86_64), NEON(ARM64)
/// - 自动fallback到高效标量实现
/// - 详细的性能统计和监控
#[derive(Clone, Debug)]
pub struct SampleConverter {
    /// SIMD处理器实例，复用现有基础设施
    simd_processor: SimdProcessor,

    /// 转换统计信息收集
    enable_stats: bool,
}

impl SampleConverter {
    /// 创建新的样本转换器
    ///
    /// # 示例
    ///
    /// ```ignore
    /// use macinmeter_dr_tool::processing::SampleConverter;
    ///
    /// let converter = SampleConverter::new();
    /// println!("SIMD支持: {}", converter.has_simd_support());
    /// ```
    pub fn new() -> Self {
        Self {
            simd_processor: SimdProcessor::new(),
            enable_stats: false,
        }
    }

    /// 创建启用详细统计的转换器
    pub fn new_with_stats() -> Self {
        Self {
            simd_processor: SimdProcessor::new(),
            enable_stats: true,
        }
    }

    /// 检查是否支持SIMD加速
    pub fn has_simd_support(&self) -> bool {
        self.simd_processor.capabilities().has_basic_simd()
    }

    /// 获取SIMD处理器能力
    pub fn simd_capabilities(&self) -> &super::simd_core::SimdCapabilities {
        self.simd_processor.capabilities()
    }

    /// 启用或禁用统计信息收集
    pub fn set_stats_enabled(&mut self, enabled: bool) {
        self.enable_stats = enabled;
    }

    /// 转换单个S16声道并写入interleaved数组（零拷贝优化）
    ///
    /// **优化#9**：直接在SIMD内核中按stride写入interleaved缓冲，
    /// 消除中间Vec分配和二次遍历，预期性能提升20-30%。
    ///
    /// # 参数
    /// - `input_channel`: 输入声道的i16样本数组
    /// - `output_interleaved`: 输出的interleaved f32数组
    /// - `channel_index`: 当前声道索引(0或1)
    /// - `channel_count`: 总声道数(1或2)
    #[inline(always)]
    pub fn convert_i16_channel_to_interleaved(
        &self,
        input_channel: &[i16],
        output_interleaved: &mut [f32],
        channel_index: usize,
        channel_count: usize,
    ) -> AudioResult<()> {
        // 零拷贝优化：直接调用stride写入的SIMD实现
        self.convert_i16_to_f32_interleaved_simd(
            input_channel,
            output_interleaved,
            channel_count,
            channel_index,
        )
    }

    /// 转换单个S24声道并写入interleaved数组（零拷贝优化）
    ///
    /// **优化#9**：直接在SIMD内核中按stride写入interleaved缓冲，
    /// 消除中间Vec分配和二次遍历，预期性能提升20-30%。
    ///
    /// # 参数
    /// - `input_channel`: 输入声道的i24样本数组
    /// - `output_interleaved`: 输出的interleaved f32数组
    /// - `channel_index`: 当前声道索引(0或1)
    /// - `channel_count`: 总声道数(1或2)
    #[inline(always)]
    pub fn convert_i24_channel_to_interleaved(
        &self,
        input_channel: &[symphonia::core::sample::i24],
        output_interleaved: &mut [f32],
        channel_index: usize,
        channel_count: usize,
    ) -> AudioResult<()> {
        // 零拷贝优化：直接调用stride写入的SIMD实现
        self.convert_i24_to_f32_interleaved_simd(
            input_channel,
            output_interleaved,
            channel_count,
            channel_index,
        )
    }

    /// 统一的样本缓冲区转换函数（优化#11）
    ///
    /// 将Symphonia的AudioBufferRef转换为交错f32格式，消除audio层的重复代码。
    ///
    /// # 参数
    /// - `audio_buf`: Symphonia的音频缓冲区引用
    /// - `samples`: 输出的interleaved f32数组
    ///
    /// # 契约与保证
    /// - **输出尺寸**: 函数内部会自动调用 `resize(channel_count * frame_count, 0.0)`
    ///   调整 `samples` 到正确大小，调用者无需预先分配或清空
    /// - **内存安全**: 对于 S16/S24 格式，使用 SIMD 的 interleaved 转换直接写入
    ///   已 resize 的缓冲区，不会越界访问
    /// - **格式覆盖**: 支持所有 Symphonia 的 AudioBufferRef 格式（F32, S16, S24, S32,
    ///   F64, U8, U16, U24, U32, S8）
    ///
    /// # 特性
    /// - S16/S24: 使用SIMD优化的零拷贝interleaved转换
    /// - 其他格式: 使用统一的resize + chunks_mut模式
    /// - 自动处理所有Symphonia支持的格式
    pub fn convert_buffer_to_interleaved(
        &self,
        audio_buf: &symphonia::core::audio::AudioBufferRef,
        samples: &mut Vec<f32>,
    ) -> AudioResult<()> {
        use symphonia::core::audio::{AudioBufferRef, Signal};

        let (channel_count, frame_count) = match audio_buf {
            AudioBufferRef::F32(buf) => crate::extract_buffer_info!(buf),
            AudioBufferRef::S16(buf) => crate::extract_buffer_info!(buf),
            AudioBufferRef::S24(buf) => crate::extract_buffer_info!(buf),
            AudioBufferRef::S32(buf) => crate::extract_buffer_info!(buf),
            AudioBufferRef::F64(buf) => crate::extract_buffer_info!(buf),
            AudioBufferRef::U8(buf) => crate::extract_buffer_info!(buf),
            AudioBufferRef::U16(buf) => crate::extract_buffer_info!(buf),
            AudioBufferRef::U24(buf) => crate::extract_buffer_info!(buf),
            AudioBufferRef::U32(buf) => crate::extract_buffer_info!(buf),
            AudioBufferRef::S8(buf) => crate::extract_buffer_info!(buf),
        };

        // 统一预分配模式：所有格式都使用 resize
        let total_samples = channel_count * frame_count;
        samples.resize(total_samples, 0.0);

        // 针对不同格式使用SIMD优化
        match audio_buf {
            AudioBufferRef::F32(buf) => {
                crate::convert_samples!(buf, |s| s, samples, channel_count)
            }
            // S16 SIMD优化 (统一助手函数)
            AudioBufferRef::S16(buf) => {
                for ch in 0..channel_count {
                    let channel_data = buf.chan(ch);
                    self.convert_i16_channel_to_interleaved(
                        channel_data,
                        samples,
                        ch,
                        channel_count,
                    )?;
                }
            }
            // S24 SIMD优化 (统一助手函数，主要性能提升点)
            AudioBufferRef::S24(buf) => {
                for ch in 0..channel_count {
                    let channel_data = buf.chan(ch);
                    self.convert_i24_channel_to_interleaved(
                        channel_data,
                        samples,
                        ch,
                        channel_count,
                    )?;
                }
            }
            // 其他格式使用标准转换（统一为 resize + chunks_mut 模式）
            AudioBufferRef::S32(buf) => {
                crate::convert_samples!(
                    buf,
                    |s| (s as f64 / 2147483648.0) as f32,
                    samples,
                    channel_count
                )
            }
            AudioBufferRef::F64(buf) => {
                crate::convert_samples!(buf, |s| s as f32, samples, channel_count)
            }
            AudioBufferRef::U8(buf) => {
                crate::convert_samples!(
                    buf,
                    |s| ((s as f32) - 128.0) / 128.0,
                    samples,
                    channel_count
                )
            }
            AudioBufferRef::U16(buf) => {
                crate::convert_samples!(
                    buf,
                    |s| ((s as f32) - 32768.0) / 32768.0,
                    samples,
                    channel_count
                )
            }
            AudioBufferRef::U24(buf) => {
                crate::convert_samples!(
                    buf,
                    |s: symphonia::core::sample::u24| {
                        ((s.inner() as f32) - 8388608.0) / 8388608.0
                    },
                    samples,
                    channel_count
                )
            }
            AudioBufferRef::U32(buf) => {
                crate::convert_samples!(
                    buf,
                    |s| (((s as f64) - 2147483648.0) / 2147483648.0) as f32,
                    samples,
                    channel_count
                )
            }
            AudioBufferRef::S8(buf) => {
                crate::convert_samples!(buf, |s| (s as f32) / 128.0, samples, channel_count)
            }
        }

        Ok(())
    }

    /// 智能格式转换 - 自动选择最优实现
    ///
    /// 根据输入格式和硬件能力，自动选择SIMD优化或标量实现
    pub fn convert_to_f32<T>(
        &self,
        input: &[T],
        format: SampleFormat,
        output: &mut Vec<f32>,
    ) -> AudioResult<ConversionStats>
    where
        T: Copy + Send + Sync,
    {
        if self.enable_stats {
            debug_conversion!(
                "智能转换: 格式={:?}, 样本数={}, SIMD支持={}",
                format,
                input.len(),
                self.has_simd_support()
            );
        }

        let start_time = if self.enable_stats {
            Some(std::time::Instant::now())
        } else {
            None
        };

        let mut stats = ConversionStats::new(input.len());

        // 根据格式派发到对应的转换函数
        let result = match format {
            SampleFormat::S16 => {
                // SAFETY: 将泛型类型T的切片重新解释为i16切片。
                // 前置条件：调用者必须确保T实际为i16类型（通过format参数保证）。
                // 两种类型大小相同（2字节），对齐要求相同，内存布局兼容。
                // 切片长度和生命周期保持不变，无越界风险。
                let i16_input = unsafe {
                    std::slice::from_raw_parts(input.as_ptr() as *const i16, input.len())
                };
                self.convert_i16_to_f32(i16_input, output)
            }
            SampleFormat::S24 => {
                // SAFETY: 将泛型类型T的切片重新解释为i24切片。
                // 前置条件：调用者必须确保T实际为i24类型（通过format参数保证）。
                // i24内部表示为i32（4字节），类型大小和对齐要求必须匹配。
                // 切片长度和生命周期保持不变，无越界风险。
                let i24_input = unsafe {
                    std::slice::from_raw_parts(
                        input.as_ptr() as *const symphonia::core::sample::i24,
                        input.len(),
                    )
                };
                self.convert_i24_to_f32(i24_input, output)
            }
            SampleFormat::S32 => {
                // SAFETY: 将泛型类型T的切片重新解释为i32切片。
                // 前置条件：调用者必须确保T实际为i32类型（通过format参数保证）。
                // 两种类型大小相同（4字节），对齐要求相同，内存布局兼容。
                // 切片长度和生命周期保持不变，无越界风险。
                let i32_input = unsafe {
                    std::slice::from_raw_parts(input.as_ptr() as *const i32, input.len())
                };
                self.convert_i32_to_f32(i32_input, output)
            }
            SampleFormat::F32 => {
                // SAFETY: 将泛型类型T的切片重新解释为f32切片。
                // 前置条件：调用者必须确保T实际为f32类型（通过format参数保证）。
                // 两种类型大小相同（4字节），对齐要求相同，内存布局兼容。
                // 切片长度和生命周期保持不变，无越界风险。
                let f32_input = unsafe {
                    std::slice::from_raw_parts(input.as_ptr() as *const f32, input.len())
                };
                output.extend_from_slice(f32_input);
                stats.output_samples = input.len();
                stats.scalar_samples = input.len();
                Ok(stats)
            }
            _ => {
                // TODO: 其他格式的实现
                return Err(error::format_error("格式暂未实现", format!("{format:?}")));
            }
        };

        // 记录耗时
        let mut final_result = result;
        if let (Some(start), Ok(final_stats)) = (start_time, &mut final_result) {
            final_stats.duration_ns = start.elapsed().as_nanos() as u64;
        }

        debug_conversion!(
            "转换完成: 输入={}, 输出={}, SIMD效率={:.1}%",
            input.len(),
            output.len(),
            if let Ok(ref stats) = final_result {
                stats.simd_efficiency()
            } else {
                0.0
            }
        );

        final_result
    }
}

impl Default for SampleConverter {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 宏：消除重复代码模式 ====================

/// 公共工具宏：提取Symphonia缓冲区信息（声道数和帧数）
///
/// 用于从AudioBuffer中提取基本元数据，消除audio层的重复代码
#[macro_export]
macro_rules! extract_buffer_info {
    ($buf:expr) => {{ ($buf.spec().channels.count(), $buf.frames()) }};
}

/// 公共工具宏：标量样本转换（统一 resize + chunks_mut 模式）
///
/// 用于标量格式的样本转换，消除audio层的重复代码。
#[macro_export]
macro_rules! convert_samples {
    ($buf:expr, $converter:expr, $samples:expr, $channel_count:expr) => {{
        for (frame_idx, chunk) in $samples.chunks_mut($channel_count).enumerate() {
            for ch in 0..$channel_count {
                chunk[ch] = $converter($buf.chan(ch)[frame_idx]);
            }
        }
    }};
}

/// 宏1: 生成标准的样本转换函数实现
///
/// 统一实现模式：统计→预留→SIMD选择→日志
macro_rules! impl_sample_conversion_method {
    (
        $method_name:ident,
        $input_type:ty,
        $simd_impl:ident,
        $scalar_impl:ident,
        $format_name:expr
    ) => {
        fn $method_name(
            &self,
            input: &[$input_type],
            output: &mut Vec<f32>,
        ) -> AudioResult<ConversionStats> {
            let mut stats = ConversionStats::new(input.len());

            // 确保输出容量足够
            output.reserve(input.len());
            let start_len = output.len();

            if self.enable_stats {
                debug_conversion!("{}→f32转换: {} 个样本", $format_name, input.len());
            }

            if self.has_simd_support() && input.len() >= 8 {
                // 使用SIMD优化路径
                stats.used_simd = true;
                self.$simd_impl(input, output, &mut stats)?;
            } else {
                // 使用标量路径
                self.$scalar_impl(input, output, &mut stats);
            }

            stats.output_samples = output.len() - start_len;

            if self.enable_stats {
                debug_conversion!(
                    "{}→f32完成: SIMD={}, 效率={:.1}%",
                    $format_name,
                    stats.used_simd,
                    stats.simd_efficiency()
                );
            }

            Ok(stats)
        }
    };
}

/// 宏2: 生成平台自适应的SIMD派发函数
///
/// 根据目标平台选择SSE2/NEON实现，或回退到标量
macro_rules! impl_simd_dispatch {
    (
        $method_name:ident,
        $input_type:ty,
        $sse2_method:ident,
        $neon_method:ident,
        $scalar_method:ident,
        $format_name:expr
    ) => {
        fn $method_name(
            &self,
            input: &[$input_type],
            output: &mut Vec<f32>,
            stats: &mut ConversionStats,
        ) -> AudioResult<()> {
            #[cfg(target_arch = "x86_64")]
            {
                self.$sse2_method(input, output, stats)
            }

            #[cfg(target_arch = "aarch64")]
            {
                self.$neon_method(input, output, stats)
            }

            #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
            {
                eprintln!(
                    "[PERFORMANCE_WARNING] 架构{}不支持SIMD，回退到标量{}→f32转换，性能将显著下降",
                    std::env::consts::ARCH,
                    $format_name
                );
                self.$scalar_method(input, output, stats);
                Ok(())
            }
        }
    };
}

/// 宏3: 生成SSE2包装函数（x86_64平台）
///
/// 检测SSE2支持并调用unsafe实现
macro_rules! impl_sse2_wrapper {
    (
        $method_name:ident,
        $input_type:ty,
        $unsafe_method:ident,
        $scalar_method:ident,
        $format_name:expr
    ) => {
        #[cfg(target_arch = "x86_64")]
        fn $method_name(
            &self,
            input: &[$input_type],
            output: &mut Vec<f32>,
            stats: &mut ConversionStats,
        ) -> AudioResult<()> {
            if self.enable_stats {
                debug_conversion!("使用SSE2优化{}→f32转换", $format_name);
            }

            if !self.simd_processor.capabilities().has_basic_simd() {
                eprintln!(
                    "[PERFORMANCE_WARNING] SSE2不可用，回退到标量{}→f32转换，性能将显著下降",
                    $format_name
                );
                self.$scalar_method(input, output, stats);
                return Ok(());
            }

            // SAFETY: convert_{}_sse2_unsafe需要SSE2支持，已通过capabilities检查验证。
            // input/output生命周期有效，函数内部会正确处理数组边界。
            unsafe { self.$unsafe_method(input, output, stats) }
            Ok(())
        }
    };
}

/// 宏4: 生成NEON包装函数（ARM64平台）
///
/// 检测NEON支持并调用unsafe实现
macro_rules! impl_neon_wrapper {
    (
        $method_name:ident,
        $input_type:ty,
        $unsafe_method:ident,
        $scalar_method:ident,
        $format_name:expr
    ) => {
        #[cfg(target_arch = "aarch64")]
        fn $method_name(
            &self,
            input: &[$input_type],
            output: &mut Vec<f32>,
            stats: &mut ConversionStats,
        ) -> AudioResult<()> {
            if self.enable_stats {
                debug_conversion!(
                    "Using NEON optimization for {0}->f32 conversion / 使用NEON优化{0}->f32转换",
                    $format_name
                );
            }

            if !self.simd_processor.capabilities().has_basic_simd() {
                eprintln!(
                    "[PERFORMANCE_WARNING] NEON不可用，回退到标量{}→f32转换，性能将显著下降",
                    $format_name
                );
                self.$scalar_method(input, output, stats);
                return Ok(());
            }

            // SAFETY: convert_{}_neon_unsafe需要NEON支持，已通过capabilities检查验证。
            // input/output生命周期有效，函数内部会正确处理数组边界。
            unsafe { self.$unsafe_method(input, output, stats) }
            Ok(())
        }
    };
}

// 为SampleConverter实现SampleConversion trait
impl SampleConversion for SampleConverter {
    // 使用宏生成i16→f32转换实现
    impl_sample_conversion_method!(
        convert_i16_to_f32,
        i16,
        convert_i16_to_f32_simd_impl,
        convert_i16_to_f32_scalar,
        "i16"
    );

    // 使用宏生成i24→f32转换实现
    impl_sample_conversion_method!(
        convert_i24_to_f32,
        symphonia::core::sample::i24,
        convert_i24_to_f32_simd_impl,
        convert_i24_to_f32_scalar,
        "i24"
    );

    // 使用宏生成i32→f32转换实现
    impl_sample_conversion_method!(
        convert_i32_to_f32,
        i32,
        convert_i32_to_f32_simd_impl,
        convert_i32_to_f32_scalar,
        "i32"
    );

    fn convert_f64_to_f32(
        &self,
        _input: &[f64],
        _output: &mut Vec<f32>,
    ) -> AudioResult<ConversionStats> {
        // TODO: 实现f64转换
        Err(crate::error::AudioError::FormatError(
            "f64转换暂未实现".to_string(),
        ))
    }

    fn convert_u8_to_f32(
        &self,
        _input: &[u8],
        _output: &mut Vec<f32>,
    ) -> AudioResult<ConversionStats> {
        // TODO: 实现u8转换
        Err(crate::error::AudioError::FormatError(
            "u8转换暂未实现".to_string(),
        ))
    }

    fn convert_u16_to_f32(
        &self,
        _input: &[u16],
        _output: &mut Vec<f32>,
    ) -> AudioResult<ConversionStats> {
        // TODO: 实现u16转换
        Err(crate::error::AudioError::FormatError(
            "u16转换暂未实现".to_string(),
        ))
    }

    fn convert_u24_to_f32(
        &self,
        _input: &[symphonia::core::sample::u24],
        _output: &mut Vec<f32>,
    ) -> AudioResult<ConversionStats> {
        // TODO: 实现u24转换
        Err(crate::error::AudioError::FormatError(
            "u24转换暂未实现".to_string(),
        ))
    }

    fn convert_u32_to_f32(
        &self,
        _input: &[u32],
        _output: &mut Vec<f32>,
    ) -> AudioResult<ConversionStats> {
        // TODO: 实现u32转换
        Err(crate::error::AudioError::FormatError(
            "u32转换暂未实现".to_string(),
        ))
    }

    fn convert_i8_to_f32(
        &self,
        _input: &[i8],
        _output: &mut Vec<f32>,
    ) -> AudioResult<ConversionStats> {
        // TODO: 实现i8转换
        Err(crate::error::AudioError::FormatError(
            "i8转换暂未实现".to_string(),
        ))
    }

    fn simd_capabilities(&self) -> &super::simd_core::SimdCapabilities {
        self.simd_processor.capabilities()
    }
}

// 实现细节 - 不同平台的SIMD实现
impl SampleConverter {
    /// 标量i16→f32转换实现
    fn convert_i16_to_f32_scalar(
        &self,
        input: &[i16],
        output: &mut Vec<f32>,
        stats: &mut ConversionStats,
    ) {
        debug_conversion!("使用标量i16→f32转换");

        const SCALE: f32 = 1.0 / 32768.0;

        for &sample in input {
            output.push((sample as f32) * SCALE);
        }

        stats.scalar_samples = input.len();
    }

    /// 标量i24→f32转换实现
    fn convert_i24_to_f32_scalar(
        &self,
        input: &[symphonia::core::sample::i24],
        output: &mut Vec<f32>,
        stats: &mut ConversionStats,
    ) {
        debug_conversion!("使用标量i24→f32转换");

        const SCALE: f64 = 1.0 / 8388608.0; // 2^23 = 8388608

        for &sample in input {
            let i32_val = sample.inner(); // 获取i24的内部i32值
            let normalized = (i32_val as f64) * SCALE;
            output.push(normalized as f32);
        }

        stats.scalar_samples = input.len();
    }

    // 使用宏生成i24的SIMD派发函数
    impl_simd_dispatch!(
        convert_i24_to_f32_simd_impl,
        symphonia::core::sample::i24,
        convert_i24_to_f32_sse2,
        convert_i24_to_f32_neon,
        convert_i24_to_f32_scalar,
        "i24"
    );

    // 使用宏生成i24的SSE2包装函数
    impl_sse2_wrapper!(
        convert_i24_to_f32_sse2,
        symphonia::core::sample::i24,
        convert_i24_to_f32_sse2_unsafe,
        convert_i24_to_f32_scalar,
        "i24"
    );

    /// SSE2 i24→f32转换的unsafe实现
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "sse2")]
    unsafe fn convert_i24_to_f32_sse2_unsafe(
        &self,
        input: &[symphonia::core::sample::i24],
        output: &mut Vec<f32>,
        stats: &mut ConversionStats,
    ) {
        use std::arch::x86_64::*;

        const SCALE: f32 = 1.0 / 8388608.0;
        let len = input.len();
        let mut i = 0;

        // 预分配输出容量，避免Vec重新分配
        output.reserve(len);

        // SSE2处理：一次处理4个i24样本（因为i24→i32需要更多空间）
        // SAFETY: SSE2向量化i24→f32转换。
        // 前置条件：i + 4 <= len确保有4个有效i24样本可访问。
        // _mm_set_epi32/cvtepi32_ps/mul_ps是纯寄存器操作，无内存风险。
        // _mm_storeu_ps写入栈上临时数组，允许未对齐访问，完全安全。
        unsafe {
            let scale_vec = _mm_set1_ps(SCALE);
            while i + 4 <= len {
                // 提取4个i24值为i32
                let i32_0 = input[i].inner();
                let i32_1 = input[i + 1].inner();
                let i32_2 = input[i + 2].inner();
                let i32_3 = input[i + 3].inner();

                // 创建i32向量
                let i32_vec = _mm_set_epi32(i32_3, i32_2, i32_1, i32_0);

                // 转换为浮点数并缩放
                let f32_vec = _mm_mul_ps(_mm_cvtepi32_ps(i32_vec), scale_vec);

                // 存储结果
                let mut temp = [0.0f32; 4];
                _mm_storeu_ps(temp.as_mut_ptr(), f32_vec);
                output.extend_from_slice(&temp);

                i += 4;
                stats.simd_samples += 4;
            }
        }

        // 处理剩余样本（标量方式）
        const SCALAR_SCALE: f64 = 1.0 / 8388608.0;
        while i < len {
            let i32_val = input[i].inner();
            let normalized = (i32_val as f64) * SCALAR_SCALE;
            output.push(normalized as f32);
            i += 1;
            stats.scalar_samples += 1;
        }
    }

    // 使用宏生成i24的NEON包装函数
    impl_neon_wrapper!(
        convert_i24_to_f32_neon,
        symphonia::core::sample::i24,
        convert_i24_to_f32_neon_unsafe,
        convert_i24_to_f32_scalar,
        "i24"
    );

    /// ARM NEON i24→f32转换的unsafe实现（优化版）
    #[cfg(target_arch = "aarch64")]
    #[target_feature(enable = "neon")]
    unsafe fn convert_i24_to_f32_neon_unsafe(
        &self,
        input: &[symphonia::core::sample::i24],
        output: &mut Vec<f32>,
        stats: &mut ConversionStats,
    ) {
        use std::arch::aarch64::*;

        const SCALE: f32 = 1.0 / 8388608.0;
        let scale_vec = vdupq_n_f32(SCALE);
        let len = input.len();
        let mut i = 0;

        // **性能优化**: 预分配输出容量，避免重复realloc
        output.reserve(len);

        // **NEON优化**: 一次处理8个i24样本（双向量并行）
        while i + 8 <= len {
            // **内存优化**: 直接构造NEON向量，避免临时数组
            // 第一组4个样本
            let i32_vec1 = vsetq_lane_s32(
                input[i].inner(),
                vsetq_lane_s32(
                    input[i + 1].inner(),
                    vsetq_lane_s32(
                        input[i + 2].inner(),
                        vsetq_lane_s32(input[i + 3].inner(), vdupq_n_s32(0), 3),
                        2,
                    ),
                    1,
                ),
                0,
            );

            // 第二组4个样本
            let i32_vec2 = vsetq_lane_s32(
                input[i + 4].inner(),
                vsetq_lane_s32(
                    input[i + 5].inner(),
                    vsetq_lane_s32(
                        input[i + 6].inner(),
                        vsetq_lane_s32(input[i + 7].inner(), vdupq_n_s32(0), 3),
                        2,
                    ),
                    1,
                ),
                0,
            );

            // **并行转换**: 同时处理两个向量
            let f32_vec1 = vmulq_f32(vcvtq_f32_s32(i32_vec1), scale_vec);
            let f32_vec2 = vmulq_f32(vcvtq_f32_s32(i32_vec2), scale_vec);

            // SAFETY: 直接写入output内存的高效存储。
            // 前置条件：已通过output.reserve(len)预分配足够容量。
            // set_len安全：新长度current_len+8不超过已分配容量。
            // vst1q_f32写入output内存：指针有效，偏移在预分配范围内。
            // 第一个vst1q写入[current_len..current_len+4]，第二个写入[current_len+4..current_len+8]。
            let current_len = output.len();
            unsafe {
                output.set_len(current_len + 8); // 安全：已预分配容量
                vst1q_f32(output.as_mut_ptr().add(current_len), f32_vec1);
                vst1q_f32(output.as_mut_ptr().add(current_len + 4), f32_vec2);
            }

            i += 8;
            stats.simd_samples += 8;
        }

        // **回退处理**: 处理剩余4个样本（单向量）
        if i + 4 <= len {
            let i32_vec = vsetq_lane_s32(
                input[i].inner(),
                vsetq_lane_s32(
                    input[i + 1].inner(),
                    vsetq_lane_s32(
                        input[i + 2].inner(),
                        vsetq_lane_s32(input[i + 3].inner(), vdupq_n_s32(0), 3),
                        2,
                    ),
                    1,
                ),
                0,
            );

            let f32_vec = vmulq_f32(vcvtq_f32_s32(i32_vec), scale_vec);

            // SAFETY: 处理剩余4样本的NEON存储。
            // 前置条件：已预分配容量，i + 4 <= len确保样本有效。
            // set_len安全：新长度current_len+4不超过预分配容量。
            // vst1q_f32写入output[current_len..current_len+4]，指针和偏移有效。
            let current_len = output.len();
            unsafe {
                output.set_len(current_len + 4);
                vst1q_f32(output.as_mut_ptr().add(current_len), f32_vec);
            }

            i += 4;
            stats.simd_samples += 4;
        }

        // 处理剩余样本（标量方式）
        const SCALAR_SCALE: f64 = 1.0 / 8388608.0;
        while i < len {
            let i32_val = input[i].inner();
            let normalized = (i32_val as f64) * SCALAR_SCALE;
            output.push(normalized as f32);
            i += 1;
            stats.scalar_samples += 1;
        }
    }

    // 使用宏生成i16的SIMD派发函数
    impl_simd_dispatch!(
        convert_i16_to_f32_simd_impl,
        i16,
        convert_i16_to_f32_sse2,
        convert_i16_to_f32_neon,
        convert_i16_to_f32_scalar,
        "i16"
    );

    // 使用宏生成i16的SSE2包装函数
    impl_sse2_wrapper!(
        convert_i16_to_f32_sse2,
        i16,
        convert_i16_to_f32_sse2_unsafe,
        convert_i16_to_f32_scalar,
        "i16"
    );

    /// SSE2 i16→f32转换的unsafe实现
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "sse2")]
    unsafe fn convert_i16_to_f32_sse2_unsafe(
        &self,
        input: &[i16],
        output: &mut Vec<f32>,
        stats: &mut ConversionStats,
    ) {
        use std::arch::x86_64::*;

        const SCALE: f32 = 1.0 / 32768.0;
        let len = input.len();
        let mut i = 0;

        // 预分配输出容量，避免Vec重新分配
        output.reserve(len);

        // SIMD处理：一次处理8个i16样本
        // SAFETY: SSE2向量化i16→f32转换。
        // 前置条件：i + 8 <= len确保有8个有效i16样本（16字节）可读取。
        // _mm_loadu_si128从未对齐内存加载，input.as_ptr().add(i)指针在边界内。
        // unpacklo/hi/cvtepi32_ps/mul_ps是纯寄存器操作，无内存访问风险。
        // 直接将结果写入output已预留的空间（使用set_len扩展长度后再写入）。
        // set_len安全性：output.reserve(len)已保证容量≥最终长度；每次追加固定8个元素且不越界。
        unsafe {
            let scale_vec = _mm_set1_ps(SCALE);
            while i + 8 <= len {
                // 加载8个i16值 (128位)
                let i16_data = _mm_loadu_si128(input.as_ptr().add(i) as *const __m128i);

                // 修复符号扩展：生成符号掩码（负数→0xFFFF，非负→0x0000）
                let sign_mask = _mm_cmplt_epi16(i16_data, _mm_setzero_si128());
                // 使用符号掩码进行符号扩展（而非零扩展）
                let i32_lo = _mm_unpacklo_epi16(i16_data, sign_mask);
                let i32_hi = _mm_unpackhi_epi16(i16_data, sign_mask);

                // 转换为浮点数并缩放
                let f32_lo = _mm_mul_ps(_mm_cvtepi32_ps(i32_lo), scale_vec);
                let f32_hi = _mm_mul_ps(_mm_cvtepi32_ps(i32_hi), scale_vec);

                // 直接写入output尾部
                let current_len = output.len();
                output.set_len(current_len + 8);
                _mm_storeu_ps(output.as_mut_ptr().add(current_len), f32_lo);
                _mm_storeu_ps(output.as_mut_ptr().add(current_len + 4), f32_hi);

                i += 8;
                stats.simd_samples += 8;
            }
        }

        // 处理剩余样本（标量方式）
        while i < len {
            output.push((input[i] as f32) * SCALE);
            i += 1;
            stats.scalar_samples += 1;
        }
    }

    // 使用宏生成i16的NEON包装函数
    impl_neon_wrapper!(
        convert_i16_to_f32_neon,
        i16,
        convert_i16_to_f32_neon_unsafe,
        convert_i16_to_f32_scalar,
        "i16"
    );

    /// ARM NEON i16→f32转换的unsafe实现
    #[cfg(target_arch = "aarch64")]
    #[target_feature(enable = "neon")]
    unsafe fn convert_i16_to_f32_neon_unsafe(
        &self,
        input: &[i16],
        output: &mut Vec<f32>,
        stats: &mut ConversionStats,
    ) {
        use std::arch::aarch64::*;

        const SCALE: f32 = 1.0 / 32768.0;
        let scale_vec = vdupq_n_f32(SCALE);
        let len = input.len();
        let mut i = 0;

        // NEON处理：一次处理8个i16样本
        while i + 8 <= len {
            // SAFETY: ARM NEON向量化i16→f32转换。
            // 前置条件：i + 8 <= len确保有8个有效i16样本（16字节）可读取。
            // vld1q_s16从内存加载8个i16到NEON向量，指针input.as_ptr().add(i)在边界内。
            // vmovl/vcvtq/vmulq是纯NEON寄存器操作，无内存访问风险。
            // vst1q_f32写入栈上临时数组，安全地将向量存储到有效内存。
            unsafe {
                // 加载8个i16值
                let i16_data = vld1q_s16(input.as_ptr().add(i));

                // 转换为两个f32向量（低4位和高4位）
                let i32_lo = vmovl_s16(vget_low_s16(i16_data));
                let i32_hi = vmovl_s16(vget_high_s16(i16_data));

                let f32_lo = vmulq_f32(vcvtq_f32_s32(i32_lo), scale_vec);
                let f32_hi = vmulq_f32(vcvtq_f32_s32(i32_hi), scale_vec);

                // 存储结果
                let mut temp_lo = [0.0f32; 4];
                let mut temp_hi = [0.0f32; 4];
                vst1q_f32(temp_lo.as_mut_ptr(), f32_lo);
                vst1q_f32(temp_hi.as_mut_ptr(), f32_hi);

                output.extend_from_slice(&temp_lo);
                output.extend_from_slice(&temp_hi);
            }

            i += 8;
            stats.simd_samples += 8;
        }

        // 处理剩余样本（标量方式）
        const SCALAR_SCALE: f32 = 1.0 / 32768.0;
        while i < len {
            output.push((input[i] as f32) * SCALAR_SCALE);
            i += 1;
            stats.scalar_samples += 1;
        }
    }

    /// 标量i32→f32转换实现
    fn convert_i32_to_f32_scalar(
        &self,
        input: &[i32],
        output: &mut Vec<f32>,
        stats: &mut ConversionStats,
    ) {
        debug_conversion!("使用标量i32→f32转换");

        const SCALE: f64 = 1.0 / 2147483648.0; // 2^31 = 2147483648

        for &sample in input {
            output.push((sample as f64 * SCALE) as f32);
        }

        stats.scalar_samples = input.len();
    }

    // 使用宏生成i32的SIMD派发函数
    impl_simd_dispatch!(
        convert_i32_to_f32_simd_impl,
        i32,
        convert_i32_to_f32_sse2,
        convert_i32_to_f32_neon,
        convert_i32_to_f32_scalar,
        "i32"
    );

    // 使用宏生成i32的SSE2包装函数
    impl_sse2_wrapper!(
        convert_i32_to_f32_sse2,
        i32,
        convert_i32_to_f32_sse2_unsafe,
        convert_i32_to_f32_scalar,
        "i32"
    );

    /// SSE2 i32→f32转换的unsafe实现
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "sse2")]
    unsafe fn convert_i32_to_f32_sse2_unsafe(
        &self,
        input: &[i32],
        output: &mut Vec<f32>,
        stats: &mut ConversionStats,
    ) {
        use std::arch::x86_64::*;

        const SCALE: f32 = 1.0 / 2147483648.0;
        let len = input.len();
        let mut i = 0;

        // 预分配输出容量，避免Vec重新分配
        output.reserve(len);

        // SSE2处理：一次处理4个i32样本
        // SAFETY: SSE2向量化i32→f32转换。
        // 前置条件：i + 4 <= len确保有4个有效i32样本（16字节）可读取。
        // _mm_loadu_si128从未对齐内存加载4个i32，指针有效且在边界内。
        // _mm_cvtepi32_ps和_mm_mul_ps是纯SSE2寄存器操作，无内存访问风险。
        // 直接将结果写入output已预留的空间（使用set_len扩展长度后再写入）。
        // set_len安全性：output.reserve(len)已保证容量≥最终长度；每次追加固定4个元素且不越界。
        unsafe {
            let scale_vec = _mm_set1_ps(SCALE);
            while i + 4 <= len {
                // 加载4个i32值
                let i32_vec = _mm_loadu_si128(input.as_ptr().add(i) as *const __m128i);

                // 转换为浮点数并缩放
                let f32_vec = _mm_mul_ps(_mm_cvtepi32_ps(i32_vec), scale_vec);

                // 直接写入output尾部
                let current_len = output.len();
                output.set_len(current_len + 4);
                _mm_storeu_ps(output.as_mut_ptr().add(current_len), f32_vec);

                i += 4;
                stats.simd_samples += 4;
            }
        }

        // 处理剩余样本（标量方式）
        const SCALAR_SCALE: f64 = 1.0 / 2147483648.0;
        while i < len {
            output.push((input[i] as f64 * SCALAR_SCALE) as f32);
            i += 1;
            stats.scalar_samples += 1;
        }
    }

    // 使用宏生成i32的NEON包装函数
    impl_neon_wrapper!(
        convert_i32_to_f32_neon,
        i32,
        convert_i32_to_f32_neon_unsafe,
        convert_i32_to_f32_scalar,
        "i32"
    );

    /// ARM NEON i32→f32转换的unsafe实现
    #[cfg(target_arch = "aarch64")]
    #[target_feature(enable = "neon")]
    unsafe fn convert_i32_to_f32_neon_unsafe(
        &self,
        input: &[i32],
        output: &mut Vec<f32>,
        stats: &mut ConversionStats,
    ) {
        use std::arch::aarch64::*;

        const SCALE: f32 = 1.0 / 2147483648.0;
        let scale_vec = vdupq_n_f32(SCALE);
        let len = input.len();
        let mut i = 0;

        // 预分配输出容量
        output.reserve(len);

        // NEON优化：一次处理8个i32样本（双向量并行）
        while i + 8 <= len {
            // SAFETY: ARM NEON向量化i32→f32转换（8样本并行）。
            // 前置条件：i + 8 <= len确保有8个有效i32样本（32字节）可读取。
            // 已通过output.reserve(len)预分配足够容量。
            // vld1q_s32从内存加载4个i32到NEON向量，两次加载共8个样本。
            // vcvtq_f32_s32和vmulq_f32是纯NEON寄存器操作。
            // set_len安全：新长度current_len+8不超过预分配容量。
            // vst1q_f32写入output内存，指针和偏移在预分配范围内。
            unsafe {
                // 加载8个i32值（两个向量）
                let i32_vec1 = vld1q_s32(input.as_ptr().add(i));
                let i32_vec2 = vld1q_s32(input.as_ptr().add(i + 4));

                // 并行转换为f32并缩放
                let f32_vec1 = vmulq_f32(vcvtq_f32_s32(i32_vec1), scale_vec);
                let f32_vec2 = vmulq_f32(vcvtq_f32_s32(i32_vec2), scale_vec);

                // 直接写入output内存
                let current_len = output.len();
                output.set_len(current_len + 8);
                vst1q_f32(output.as_mut_ptr().add(current_len), f32_vec1);
                vst1q_f32(output.as_mut_ptr().add(current_len + 4), f32_vec2);
            }

            i += 8;
            stats.simd_samples += 8;
        }

        // 处理剩余4个样本（单向量）
        if i + 4 <= len {
            // SAFETY: ARM NEON向量化i32→f32转换（4样本处理）。
            // 前置条件：i + 4 <= len确保有4个有效i32样本（16字节）可读取。
            // 已预分配容量，set_len安全：新长度current_len+4不超过预分配容量。
            // vld1q_s32/vcvtq_f32_s32/vmulq_f32是NEON寄存器操作。
            // vst1q_f32写入output内存，指针和偏移在预分配范围内。
            unsafe {
                let i32_vec = vld1q_s32(input.as_ptr().add(i));
                let f32_vec = vmulq_f32(vcvtq_f32_s32(i32_vec), scale_vec);

                let current_len = output.len();
                output.set_len(current_len + 4);
                vst1q_f32(output.as_mut_ptr().add(current_len), f32_vec);
            }

            i += 4;
            stats.simd_samples += 4;
        }

        // 处理剩余样本（标量方式）
        const SCALAR_SCALE: f64 = 1.0 / 2147483648.0;
        while i < len {
            output.push((input[i] as f64 * SCALAR_SCALE) as f32);
            i += 1;
            stats.scalar_samples += 1;
        }
    }

    // ==================== 优化#9：零拷贝 Interleaved 转换 ====================

    /// i16→f32 零拷贝interleaved转换（SIMD优化）
    ///
    /// 直接在SIMD内核中按stride写入interleaved缓冲，消除中间Vec分配。
    ///
    /// # 参数
    /// - `input`: 单声道i16样本数组
    /// - `output`: interleaved f32目标缓冲（已预分配）
    /// - `channel_count`: 总声道数（stride）
    /// - `channel_offset`: 当前声道的起始偏移
    #[inline(always)]
    fn convert_i16_to_f32_interleaved_simd(
        &self,
        input: &[i16],
        output: &mut [f32],
        channel_count: usize,
        channel_offset: usize,
    ) -> AudioResult<()> {
        if self.has_simd_support() && input.len() >= 8 {
            // 使用SIMD优化路径
            #[cfg(target_arch = "x86_64")]
            {
                // SAFETY: convert_i16_to_f32_interleaved_sse2需要SSE2支持，已通过has_simd_support()验证。
                // input/output生命周期有效，函数内部会正确处理数组边界。
                unsafe {
                    self.convert_i16_to_f32_interleaved_sse2(
                        input,
                        output,
                        channel_count,
                        channel_offset,
                    )?;
                }
            }

            #[cfg(target_arch = "aarch64")]
            {
                // SAFETY: convert_i16_to_f32_interleaved_neon需要NEON支持，已通过has_simd_support()验证。
                // input/output生命周期有效，函数内部会正确处理数组边界。
                unsafe {
                    self.convert_i16_to_f32_interleaved_neon(
                        input,
                        output,
                        channel_count,
                        channel_offset,
                    )?;
                }
            }

            #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
            {
                self.convert_i16_to_f32_interleaved_scalar(
                    input,
                    output,
                    channel_count,
                    channel_offset,
                );
            }
        } else {
            // 使用标量路径
            self.convert_i16_to_f32_interleaved_scalar(
                input,
                output,
                channel_count,
                channel_offset,
            );
        }

        Ok(())
    }

    /// i24→f32 零拷贝interleaved转换（SIMD优化）
    ///
    /// 直接在SIMD内核中按stride写入interleaved缓冲，消除中间Vec分配。
    ///
    /// # 参数
    /// - `input`: 单声道i24样本数组
    /// - `output`: interleaved f32目标缓冲（已预分配）
    /// - `channel_count`: 总声道数（stride）
    /// - `channel_offset`: 当前声道的起始偏移
    #[inline(always)]
    fn convert_i24_to_f32_interleaved_simd(
        &self,
        input: &[symphonia::core::sample::i24],
        output: &mut [f32],
        channel_count: usize,
        channel_offset: usize,
    ) -> AudioResult<()> {
        // 修复阈值：i24 SIMD每次处理4个样本，门槛应为>=4（不是8）
        if self.has_simd_support() && input.len() >= 4 {
            // 使用SIMD优化路径
            #[cfg(target_arch = "x86_64")]
            {
                // SAFETY: convert_i24_to_f32_interleaved_sse2需要SSE2支持，已通过has_simd_support()验证。
                // input/output生命周期有效，函数内部会正确处理数组边界。
                unsafe {
                    self.convert_i24_to_f32_interleaved_sse2(
                        input,
                        output,
                        channel_count,
                        channel_offset,
                    )?;
                }
            }

            #[cfg(target_arch = "aarch64")]
            {
                // SAFETY: convert_i24_to_f32_interleaved_neon需要NEON支持，已通过has_simd_support()验证。
                // input/output生命周期有效，函数内部会正确处理数组边界。
                unsafe {
                    self.convert_i24_to_f32_interleaved_neon(
                        input,
                        output,
                        channel_count,
                        channel_offset,
                    )?;
                }
            }

            #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
            {
                self.convert_i24_to_f32_interleaved_scalar(
                    input,
                    output,
                    channel_count,
                    channel_offset,
                );
            }
        } else {
            // 使用标量路径
            self.convert_i24_to_f32_interleaved_scalar(
                input,
                output,
                channel_count,
                channel_offset,
            );
        }

        Ok(())
    }

    /// 标量 i16→f32 interleaved 转换
    fn convert_i16_to_f32_interleaved_scalar(
        &self,
        input: &[i16],
        output: &mut [f32],
        channel_count: usize,
        channel_offset: usize,
    ) {
        const SCALE: f32 = 1.0 / 32768.0;

        for (frame_idx, &sample) in input.iter().enumerate() {
            let interleaved_idx = frame_idx * channel_count + channel_offset;
            output[interleaved_idx] = (sample as f32) * SCALE;
        }
    }

    /// 标量 i24→f32 interleaved 转换
    fn convert_i24_to_f32_interleaved_scalar(
        &self,
        input: &[symphonia::core::sample::i24],
        output: &mut [f32],
        channel_count: usize,
        channel_offset: usize,
    ) {
        const SCALE: f64 = 1.0 / 8388608.0;

        for (frame_idx, &sample) in input.iter().enumerate() {
            let i32_val = sample.inner();
            let normalized = (i32_val as f64) * SCALE;
            let interleaved_idx = frame_idx * channel_count + channel_offset;
            output[interleaved_idx] = normalized as f32;
        }
    }

    /// SSE2 i16→f32 interleaved 转换（优化版：指针写入消除边界检查）
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "sse2")]
    unsafe fn convert_i16_to_f32_interleaved_sse2(
        &self,
        input: &[i16],
        output: &mut [f32],
        channel_count: usize,
        channel_offset: usize,
    ) -> AudioResult<()> {
        use std::arch::x86_64::*;

        const SCALE: f32 = 1.0 / 32768.0;
        let len = input.len();
        let mut i = 0;

        // 优化#11：尺寸断言，帮助编译器消除后续边界检查
        debug_assert_eq!(output.len(), input.len() * channel_count);

        // SAFETY: SSE2向量化i16→f32 interleaved转换（优化版）。
        // 前置条件：
        // 1. i + 8 <= len确保有8个有效i16样本可读取
        // 2. debug_assert已验证output尺寸正确性
        // 3. 使用指针写入消除边界检查和重复乘法
        unsafe {
            let scale_vec = _mm_set1_ps(SCALE);
            let base_ptr = output.as_mut_ptr().add(channel_offset);
            let stride = channel_count;

            while i + 8 <= len {
                // 加载8个i16值
                let i16_data = _mm_loadu_si128(input.as_ptr().add(i) as *const __m128i);

                // 修复符号扩展：使用符号掩码而非零扩展
                let sign_mask = _mm_cmplt_epi16(i16_data, _mm_setzero_si128());
                let i32_lo = _mm_unpacklo_epi16(i16_data, sign_mask);
                let i32_hi = _mm_unpackhi_epi16(i16_data, sign_mask);
                let f32_lo = _mm_mul_ps(_mm_cvtepi32_ps(i32_lo), scale_vec);
                let f32_hi = _mm_mul_ps(_mm_cvtepi32_ps(i32_hi), scale_vec);

                // 存储到栈上临时数组
                let mut temp_lo = [0.0f32; 4];
                let mut temp_hi = [0.0f32; 4];
                _mm_storeu_ps(temp_lo.as_mut_ptr(), f32_lo);
                _mm_storeu_ps(temp_hi.as_mut_ptr(), f32_hi);

                // 优化#12：指针递增消除循环内乘法
                // SAFETY:
                // - base_ptr 已偏移到正确声道位置
                // - 指针递增替代乘法，进一步降低指令开销
                // - debug_assert 保证了 output 容量足够
                let mut p = base_ptr.add(i * stride);
                for &lo_value in &temp_lo {
                    *p = lo_value;
                    p = p.add(stride);
                }
                let mut p_hi = base_ptr.add((i + 4) * stride);
                for &hi_value in &temp_hi {
                    *p_hi = hi_value;
                    p_hi = p_hi.add(stride);
                }

                i += 8;
            }
        }

        // 处理剩余样本（标量）
        self.convert_i16_to_f32_interleaved_scalar(
            &input[i..],
            output,
            channel_count,
            channel_offset + i * channel_count,
        );

        Ok(())
    }

    /// ARM NEON i16→f32 interleaved 转换（优化版：指针写入消除边界检查）
    #[cfg(target_arch = "aarch64")]
    #[target_feature(enable = "neon")]
    unsafe fn convert_i16_to_f32_interleaved_neon(
        &self,
        input: &[i16],
        output: &mut [f32],
        channel_count: usize,
        channel_offset: usize,
    ) -> AudioResult<()> {
        use std::arch::aarch64::*;

        const SCALE: f32 = 1.0 / 32768.0;
        let scale_vec = vdupq_n_f32(SCALE);
        let len = input.len();
        let mut i = 0;

        // 优化#11：尺寸断言，帮助编译器消除后续边界检查
        debug_assert_eq!(output.len(), input.len() * channel_count);

        // SAFETY: ARM NEON向量化i16→f32 interleaved转换（优化版）。
        // 前置条件：
        // 1. i + 8 <= len确保有8个有效i16样本可读取
        // 2. debug_assert已验证output尺寸正确性
        // 3. 使用指针写入消除边界检查和重复乘法
        unsafe {
            let base_ptr = output.as_mut_ptr().add(channel_offset);
            let stride = channel_count;

            while i + 8 <= len {
                // 加载8个i16值
                let i16_data = vld1q_s16(input.as_ptr().add(i));

                // 转换为两个f32向量
                let i32_lo = vmovl_s16(vget_low_s16(i16_data));
                let i32_hi = vmovl_s16(vget_high_s16(i16_data));
                let f32_lo = vmulq_f32(vcvtq_f32_s32(i32_lo), scale_vec);
                let f32_hi = vmulq_f32(vcvtq_f32_s32(i32_hi), scale_vec);

                // 存储到栈上临时数组
                let mut temp_lo = [0.0f32; 4];
                let mut temp_hi = [0.0f32; 4];
                vst1q_f32(temp_lo.as_mut_ptr(), f32_lo);
                vst1q_f32(temp_hi.as_mut_ptr(), f32_hi);

                // 优化#12：指针递增消除循环内乘法
                // SAFETY:
                // - base_ptr 已偏移到正确声道位置
                // - 指针递增替代乘法，进一步降低指令开销
                // - debug_assert 保证了 output 容量足够
                let mut p = base_ptr.add(i * stride);
                for &lo_value in &temp_lo {
                    *p = lo_value;
                    p = p.add(stride);
                }
                let mut p_hi = base_ptr.add((i + 4) * stride);
                for &hi_value in &temp_hi {
                    *p_hi = hi_value;
                    p_hi = p_hi.add(stride);
                }

                i += 8;
            }
        }

        // 处理剩余样本（标量）
        self.convert_i16_to_f32_interleaved_scalar(
            &input[i..],
            output,
            channel_count,
            channel_offset + i * channel_count,
        );

        Ok(())
    }

    /// SSE2 i24→f32 interleaved 转换（优化版：指针写入消除边界检查）
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "sse2")]
    unsafe fn convert_i24_to_f32_interleaved_sse2(
        &self,
        input: &[symphonia::core::sample::i24],
        output: &mut [f32],
        channel_count: usize,
        channel_offset: usize,
    ) -> AudioResult<()> {
        use std::arch::x86_64::*;

        const SCALE: f32 = 1.0 / 8388608.0;
        let len = input.len();
        let mut i = 0;

        // 优化#11：尺寸断言，帮助编译器消除后续边界检查
        debug_assert_eq!(output.len(), input.len() * channel_count);

        // SAFETY: SSE2向量化i24→f32 interleaved转换（优化版）。
        // 前置条件：
        // 1. i + 4 <= len确保有4个有效i24样本可读取
        // 2. debug_assert已验证output尺寸正确性
        // 3. 使用指针写入消除边界检查和重复乘法
        unsafe {
            let scale_vec = _mm_set1_ps(SCALE);
            let base_ptr = output.as_mut_ptr().add(channel_offset);
            let stride = channel_count;

            while i + 4 <= len {
                // 提取4个i24值为i32
                let i32_0 = input[i].inner();
                let i32_1 = input[i + 1].inner();
                let i32_2 = input[i + 2].inner();
                let i32_3 = input[i + 3].inner();

                // 创建i32向量并转换
                let i32_vec = _mm_set_epi32(i32_3, i32_2, i32_1, i32_0);
                let f32_vec = _mm_mul_ps(_mm_cvtepi32_ps(i32_vec), scale_vec);

                // 存储到栈上临时数组
                let mut temp = [0.0f32; 4];
                _mm_storeu_ps(temp.as_mut_ptr(), f32_vec);

                // 优化#12：指针递增消除循环内乘法
                // SAFETY:
                // - base_ptr 已偏移到正确声道位置
                // - 指针递增替代乘法，进一步降低指令开销
                // - debug_assert 保证了 output 容量足够
                let mut p = base_ptr.add(i * stride);
                for &value in &temp {
                    *p = value;
                    p = p.add(stride);
                }

                i += 4;
            }
        }

        // 处理剩余样本（标量）
        self.convert_i24_to_f32_interleaved_scalar(
            &input[i..],
            output,
            channel_count,
            channel_offset + i * channel_count,
        );

        Ok(())
    }

    /// ARM NEON i24→f32 interleaved 转换（优化版：指针写入消除边界检查）
    #[cfg(target_arch = "aarch64")]
    #[target_feature(enable = "neon")]
    unsafe fn convert_i24_to_f32_interleaved_neon(
        &self,
        input: &[symphonia::core::sample::i24],
        output: &mut [f32],
        channel_count: usize,
        channel_offset: usize,
    ) -> AudioResult<()> {
        use std::arch::aarch64::*;

        const SCALE: f32 = 1.0 / 8388608.0;
        let scale_vec = vdupq_n_f32(SCALE);
        let len = input.len();
        let mut i = 0;

        // 优化#11：尺寸断言，帮助编译器消除后续边界检查
        debug_assert_eq!(output.len(), input.len() * channel_count);

        // SAFETY: ARM NEON向量化i24→f32 interleaved转换（优化版）。
        // 前置条件：
        // 1. i + 4 <= len确保有4个有效i24样本可读取
        // 2. debug_assert已验证output尺寸正确性
        // 3. 使用指针写入消除边界检查和重复乘法
        unsafe {
            let base_ptr = output.as_mut_ptr().add(channel_offset);
            let stride = channel_count;

            while i + 4 <= len {
                // 构造i32向量
                let i32_vec = vsetq_lane_s32(
                    input[i].inner(),
                    vsetq_lane_s32(
                        input[i + 1].inner(),
                        vsetq_lane_s32(
                            input[i + 2].inner(),
                            vsetq_lane_s32(input[i + 3].inner(), vdupq_n_s32(0), 3),
                            2,
                        ),
                        1,
                    ),
                    0,
                );

                // 转换为f32向量
                let f32_vec = vmulq_f32(vcvtq_f32_s32(i32_vec), scale_vec);

                // 存储到栈上临时数组
                let mut temp = [0.0f32; 4];
                vst1q_f32(temp.as_mut_ptr(), f32_vec);

                // 优化#12：指针递增消除循环内乘法
                // SAFETY:
                // - base_ptr 已偏移到正确声道位置
                // - 指针递增替代乘法，进一步降低指令开销
                // - debug_assert 保证了 output 容量足够
                let mut p = base_ptr.add(i * stride);
                for &value in &temp {
                    *p = value;
                    p = p.add(stride);
                }

                i += 4;
            }
        }

        // 处理剩余样本（标量）
        self.convert_i24_to_f32_interleaved_scalar(
            &input[i..],
            output,
            channel_count,
            channel_offset + i * channel_count,
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_format_properties() {
        assert_eq!(SampleFormat::S16.bit_depth(), 16);
        assert!(SampleFormat::S16.is_signed());
        assert!(!SampleFormat::S16.is_float());

        assert_eq!(SampleFormat::F32.bit_depth(), 32);
        assert!(SampleFormat::F32.is_signed());
        assert!(SampleFormat::F32.is_float());
    }

    #[test]
    fn test_sample_converter_creation() {
        let converter = SampleConverter::new();
        println!("SIMD支持: {}", converter.has_simd_support());
        println!("SIMD能力: {:?}", converter.simd_capabilities());
    }

    #[test]
    fn test_i16_to_f32_scalar_conversion() {
        let converter = SampleConverter::new();

        // 测试典型的i16值
        let input = vec![0, 16384, -16384, 32767, -32768];
        let mut output = Vec::new();

        let mut stats = ConversionStats::new(input.len());
        converter.convert_i16_to_f32_scalar(&input, &mut output, &mut stats);

        assert_eq!(output.len(), input.len());

        // 验证转换精度
        assert!((output[0] - 0.0).abs() < 1e-6); // 0
        assert!((output[1] - 0.5).abs() < 1e-6); // 16384/32768 = 0.5
        assert!((output[2] - (-0.5)).abs() < 1e-6); // -16384/32768 = -0.5
        assert!((output[3] - 0.999_969_5).abs() < 1e-6); // 32767/32768
        assert!((output[4] - (-1.0)).abs() < 1e-6); // -32768/32768 = -1.0

        assert_eq!(stats.scalar_samples, input.len());
        assert_eq!(stats.simd_samples, 0);
    }

    #[test]
    fn test_i16_to_f32_full_conversion() {
        let converter = SampleConverter::new();

        // 创建测试数据
        let input: Vec<i16> = (0..100).map(|i| (i * 327) as i16).collect();
        let mut output = Vec::new();

        let result = converter.convert_i16_to_f32(&input, &mut output);
        assert!(result.is_ok());

        let stats = result.unwrap();
        assert_eq!(stats.input_samples, 100);
        assert_eq!(stats.output_samples, 100);
        assert_eq!(output.len(), 100);

        println!(
            "转换统计: 输入={}, 输出={}, SIMD效率={:.1}%",
            stats.input_samples,
            stats.output_samples,
            stats.simd_efficiency()
        );
    }

    #[test]
    fn test_conversion_stats() {
        let mut stats = ConversionStats::new(1000);
        stats.simd_samples = 800;
        stats.scalar_samples = 200;
        stats.duration_ns = 1000;

        assert_eq!(stats.simd_efficiency(), 80.0);
        assert_eq!(stats.samples_per_second(), 1_000_000_000.0); // 1000样本/1000纳秒 = 10亿样本/秒
    }

    #[test]
    fn test_i24_to_f32_scalar_conversion() {
        let converter = SampleConverter::new();

        // 创建测试i24值 - 使用From trait
        let input = vec![
            symphonia::core::sample::i24::from(0i32),
            symphonia::core::sample::i24::from(4194304i32), // 8388608/2 = 0.5
            symphonia::core::sample::i24::from(-4194304i32), // -8388608/2 = -0.5
            symphonia::core::sample::i24::from(8388607i32), // 最大值 ≈ 1.0
            symphonia::core::sample::i24::from(-8388608i32), // 最小值 = -1.0
        ];

        let mut output = Vec::new();
        let mut stats = ConversionStats::new(input.len());
        converter.convert_i24_to_f32_scalar(&input, &mut output, &mut stats);

        assert_eq!(output.len(), input.len());

        // 验证转换精度
        assert!((output[0] - 0.0).abs() < 1e-6); // 0
        assert!((output[1] - 0.5).abs() < 1e-6); // 4194304/8388608 = 0.5
        assert!((output[2] - (-0.5)).abs() < 1e-6); // -4194304/8388608 = -0.5
        assert!((output[3] - 0.999_999_9).abs() < 1e-6); // 8388607/8388608
        assert!((output[4] - (-1.0)).abs() < 1e-6); // -8388608/8388608 = -1.0

        assert_eq!(stats.scalar_samples, input.len());
        assert_eq!(stats.simd_samples, 0);
    }

    #[test]
    fn test_i24_to_f32_full_conversion() {
        let converter = SampleConverter::new();

        // 创建测试数据 - 使用i24范围内的值
        let input: Vec<symphonia::core::sample::i24> = (0..100)
            .map(|i| symphonia::core::sample::i24::from(i * 83886)) // 缩放到i24范围
            .collect();
        let mut output = Vec::new();

        let result = converter.convert_i24_to_f32(&input, &mut output);
        assert!(result.is_ok());

        let stats = result.unwrap();
        assert_eq!(stats.input_samples, 100);
        assert_eq!(stats.output_samples, 100);
        assert_eq!(output.len(), 100);

        println!(
            "i24转换统计: 输入={}, 输出={}, SIMD效率={:.1}%",
            stats.input_samples,
            stats.output_samples,
            stats.simd_efficiency()
        );
    }

    #[test]
    fn test_sample_format_dispatch() {
        let converter = SampleConverter::new();

        // 测试S16格式
        let i16_data: Vec<i16> = vec![0, 16384, -16384, 32767, -32768];
        let mut output = Vec::new();

        // 通过convert_to_f32进行格式派发测试
        let result = converter.convert_to_f32(&i16_data, SampleFormat::S16, &mut output);
        assert!(result.is_ok());
        assert_eq!(output.len(), 5);

        // 测试F32格式（直接复制）
        let f32_data: Vec<f32> = vec![0.0, 0.5, -0.5, 1.0, -1.0];
        let mut output2 = Vec::new();
        let result2 = converter.convert_to_f32(&f32_data, SampleFormat::F32, &mut output2);
        assert!(result2.is_ok());
        assert_eq!(output2, f32_data);
    }

    #[test]
    fn test_i32_to_f32_scalar_conversion() {
        let converter = SampleConverter::new();

        // 测试典型的i32值
        let input = vec![
            0,
            1073741824,  // 2^30 = 0.5
            -1073741824, // -2^30 = -0.5
            2147483647,  // 最大值 ≈ 1.0
            -2147483648, // 最小值 = -1.0
        ];
        let mut output = Vec::new();

        let mut stats = ConversionStats::new(input.len());
        converter.convert_i32_to_f32_scalar(&input, &mut output, &mut stats);

        assert_eq!(output.len(), input.len());

        // 验证转换精度
        assert!((output[0] - 0.0).abs() < 1e-6); // 0
        assert!((output[1] - 0.5).abs() < 1e-6); // 1073741824/2147483648 = 0.5
        assert!((output[2] - (-0.5)).abs() < 1e-6); // -1073741824/2147483648 = -0.5
        assert!((output[3] - 0.999_999_999_5).abs() < 1e-6); // 2147483647/2147483648
        assert!((output[4] - (-1.0)).abs() < 1e-6); // -2147483648/2147483648 = -1.0

        assert_eq!(stats.scalar_samples, input.len());
        assert_eq!(stats.simd_samples, 0);
    }

    #[test]
    fn test_i32_to_f32_full_conversion() {
        let converter = SampleConverter::new();

        // 创建测试数据 - 使用i32范围内的值
        let input: Vec<i32> = (0..100).map(|i| i * 21474836).collect();
        let mut output = Vec::new();

        let result = converter.convert_i32_to_f32(&input, &mut output);
        assert!(result.is_ok());

        let stats = result.unwrap();
        assert_eq!(stats.input_samples, 100);
        assert_eq!(stats.output_samples, 100);
        assert_eq!(output.len(), 100);

        println!(
            "i32转换统计: 输入={}, 输出={}, SIMD效率={:.1}%",
            stats.input_samples,
            stats.output_samples,
            stats.simd_efficiency()
        );
    }
}
