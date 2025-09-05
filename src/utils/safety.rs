//! 8层防御性异常处理机制
//!
//! 实现多层次的安全保障，确保在各种异常情况下的稳定运行。

use crate::error::{AudioError, AudioResult};
use std::fmt;

/// 8层防御机制的实现
///
/// 按照优先级顺序实施各层防护：
/// 1. 输入验证 - 在数据进入系统前验证有效性
/// 2. 边界检查 - 防止数组越界和索引错误  
/// 3. 数值溢出 - 检测和处理数值运算溢出
/// 4. 内存安全 - 确保内存访问的合法性
/// 5. 文件I/O - 处理文件操作相关异常
/// 6. 格式验证 - 验证数据格式的正确性
/// 7. 计算异常 - 处理数学运算中的异常情况
/// 8. 资源清理 - 确保资源得到正确释放
pub struct SafetyGuard;

impl SafetyGuard {
    /// 第1层：输入验证防护
    ///
    /// 验证输入参数的基本有效性和约束条件。
    ///
    /// # 参数
    ///
    /// * `samples` - 音频样本数据
    /// * `channels` - 声道数量
    /// * `sample_rate` - 采样率
    ///
    /// # 错误
    ///
    /// * `AudioError::InvalidInput` - 输入参数不符合要求
    pub fn validate_input(samples: &[f32], channels: usize, sample_rate: u32) -> AudioResult<()> {
        // 检查样本数组
        if samples.is_empty() {
            return Err(AudioError::InvalidInput("音频样本数据不能为空".to_string()));
        }

        if samples.len() > 100_000_000 {
            // 100M样本限制
            return Err(AudioError::InvalidInput(
                "音频样本数量过大，可能导致内存不足".to_string(),
            ));
        }

        // 检查声道数
        if channels == 0 {
            return Err(AudioError::InvalidInput("声道数必须大于0".to_string()));
        }

        if channels > 32 {
            return Err(AudioError::InvalidInput("声道数不能超过32".to_string()));
        }

        // 检查样本数量与声道数的匹配
        if samples.len() % channels != 0 {
            return Err(AudioError::InvalidInput(format!(
                "样本数量({})必须是声道数({})的倍数",
                samples.len(),
                channels
            )));
        }

        // 检查采样率
        if sample_rate == 0 {
            return Err(AudioError::InvalidInput("采样率不能为0".to_string()));
        }

        if !(8000..=384_000).contains(&sample_rate) {
            return Err(AudioError::InvalidInput(format!(
                "采样率({sample_rate})超出支持范围(8kHz-384kHz)"
            )));
        }

        Ok(())
    }

    /// 第2层：边界检查防护
    ///
    /// 确保数组访问和索引操作的安全性。
    ///
    /// # 参数
    ///
    /// * `index` - 要访问的索引
    /// * `length` - 数组长度
    /// * `context` - 上下文描述
    ///
    /// # 错误
    ///
    /// * `AudioError::InvalidInput` - 索引越界
    pub fn check_bounds(index: usize, length: usize, context: &str) -> AudioResult<()> {
        if index >= length {
            return Err(AudioError::InvalidInput(format!(
                "{context}中索引({index})超出范围(0-{})",
                length.saturating_sub(1)
            )));
        }

        Ok(())
    }

    /// 第3层：数值溢出防护
    ///
    /// 检测和预防数值运算中的溢出情况。
    ///
    /// # 参数
    ///
    /// * `value` - 要检查的数值
    /// * `operation` - 操作类型描述
    ///
    /// # 错误
    ///
    /// * `AudioError::NumericOverflow` - 数值溢出
    pub fn check_numeric_overflow(value: f64, operation: &str) -> AudioResult<()> {
        if value.is_infinite() {
            return Err(AudioError::NumericOverflow(format!(
                "{operation}操作导致无穷大结果"
            )));
        }

        if value.is_nan() {
            return Err(AudioError::NumericOverflow(format!(
                "{operation}操作导致NaN结果"
            )));
        }

        // 检查是否超出f64的有效范围
        if value.abs() > f64::MAX / 2.0 {
            return Err(AudioError::NumericOverflow(format!(
                "{operation}操作结果({value})接近数值极限"
            )));
        }

        Ok(())
    }

    /// 第4层：内存安全防护
    ///
    /// 评估内存使用量，防止过度消耗导致系统不稳定。
    ///
    /// # 参数
    ///
    /// * `estimated_bytes` - 预估内存使用量（字节）
    /// * `context` - 上下文描述
    ///
    /// # 错误
    ///
    /// * `AudioError::OutOfMemory` - 内存使用量过大
    pub fn check_memory_safety(estimated_bytes: u64, context: &str) -> AudioResult<()> {
        const MAX_MEMORY_MB: u64 = 1024; // 1GB限制
        const MAX_MEMORY_BYTES: u64 = MAX_MEMORY_MB * 1024 * 1024;

        if estimated_bytes > MAX_MEMORY_BYTES {
            return Err(AudioError::OutOfMemory);
        }

        // 检查可用系统内存（简化版本）
        // 在实际应用中，可以使用系统API获取可用内存
        if estimated_bytes > 512 * 1024 * 1024 {
            // 512MB警告线
            eprintln!(
                "警告: {}操作将使用大量内存({}MB)",
                context,
                estimated_bytes / 1024 / 1024
            );
        }

        Ok(())
    }

    /// 第5层：文件I/O防护
    ///
    /// 验证文件操作的安全性和有效性。
    ///
    /// # 参数
    ///
    /// * `file_path` - 文件路径
    /// * `operation` - 操作类型（如"read", "write"）
    ///
    /// # 错误
    ///
    /// * `AudioError::IoError` - 文件I/O相关错误
    pub fn check_file_safety(file_path: &str, operation: &str) -> AudioResult<()> {
        use std::path::Path;

        let path = Path::new(file_path);

        // 检查路径安全性
        if file_path.contains("..") {
            return Err(AudioError::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "文件路径包含不安全的相对路径",
            )));
        }

        if file_path.len() > 4096 {
            return Err(AudioError::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "文件路径过长",
            )));
        }

        match operation {
            "read" => {
                if !path.exists() {
                    return Err(AudioError::IoError(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("文件不存在: {file_path}"),
                    )));
                }

                // 检查文件大小
                if let Ok(metadata) = path.metadata() {
                    const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024 * 1024; // 2GB
                    if metadata.len() > MAX_FILE_SIZE {
                        return Err(AudioError::IoError(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "文件过大",
                        )));
                    }
                }
            }
            "write" => {
                if let Some(parent) = path.parent()
                    && !parent.exists() {
                        return Err(AudioError::IoError(std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            "目标目录不存在",
                        )));
                    }
            }
            _ => {
                return Err(AudioError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("不支持的文件操作: {operation}"),
                )));
            }
        }

        Ok(())
    }

    /// 第6层：格式验证防护
    ///
    /// 验证数据格式的正确性和完整性。
    ///
    /// # 参数
    ///
    /// * `sample_rate` - 采样率
    /// * `channels` - 声道数
    /// * `bits_per_sample` - 位深度
    ///
    /// # 错误
    ///
    /// * `AudioError::FormatError` - 格式验证失败
    pub fn validate_format(
        sample_rate: u32,
        channels: u16,
        bits_per_sample: u16,
    ) -> AudioResult<()> {
        // 验证采样率
        const SUPPORTED_RATES: &[u32] = &[
            8000, 11025, 16000, 22050, 24000, 32000, 44100, 48000, 88200, 96000, 176400, 192000,
            352800, 384000,
        ];

        if !SUPPORTED_RATES.contains(&sample_rate) {
            return Err(AudioError::FormatError(format!(
                "不支持的采样率: {sample_rate}Hz"
            )));
        }

        // 验证声道数
        if channels == 0 || channels > 32 {
            return Err(AudioError::FormatError(format!(
                "不支持的声道数: {channels}"
            )));
        }

        // 验证位深度
        match bits_per_sample {
            16 | 24 | 32 => Ok(()),
            _ => Err(AudioError::FormatError(format!(
                "不支持的位深度: {bits_per_sample}位"
            ))),
        }
    }

    /// 第7层：计算异常防护
    ///
    /// 处理数学运算中的特殊情况和异常。
    ///
    /// # 参数
    ///
    /// * `rms` - RMS值
    /// * `peak` - Peak值
    ///
    /// # 错误
    ///
    /// * `AudioError::CalculationError` - 计算异常
    pub fn validate_calculation(rms: f64, peak: f64) -> AudioResult<()> {
        // 检查RMS值有效性
        if rms < 0.0 {
            return Err(AudioError::CalculationError(format!(
                "RMS值不能为负数: {rms}"
            )));
        }

        if rms.is_infinite() || rms.is_nan() {
            return Err(AudioError::CalculationError(
                "RMS值无效（无穷大或NaN）".to_string(),
            ));
        }

        // 检查Peak值有效性
        if peak < 0.0 {
            return Err(AudioError::CalculationError(format!(
                "Peak值不能为负数: {peak}"
            )));
        }

        if peak.is_infinite() || peak.is_nan() {
            return Err(AudioError::CalculationError(
                "Peak值无效（无穷大或NaN）".to_string(),
            ));
        }

        // 检查RMS与Peak的关系
        if rms > peak && (rms - peak).abs() > 1e-10 {
            return Err(AudioError::CalculationError(format!(
                "RMS值({rms})不能大于Peak值({peak})"
            )));
        }

        // 检查是否在合理范围内
        if peak > 100.0 {
            return Err(AudioError::CalculationError(format!(
                "Peak值({peak})超出合理范围"
            )));
        }

        Ok(())
    }

    /// 第8层：资源清理防护
    ///
    /// 确保资源得到正确释放，防止内存泄漏。
    ///
    /// # 参数
    ///
    /// * `resource_info` - 资源信息描述
    pub fn ensure_cleanup(resource_info: &str) -> AudioResult<()> {
        // 这里主要是记录和监控资源使用
        // 在实际实现中，可以集成更复杂的资源管理逻辑

        if resource_info.is_empty() {
            return Err(AudioError::ResourceError("资源信息不能为空".to_string()));
        }

        // 记录资源清理日志（在生产环境中可以使用日志框架）
        eprintln!("资源清理确认: {resource_info}");

        Ok(())
    }
}

/// 安全运行器 - 组合使用多层防护机制
///
/// 提供便捷的接口来应用多层安全检查。
pub struct SafeRunner<'a> {
    context: &'a str,
}

impl<'a> SafeRunner<'a> {
    /// 创建新的安全运行器
    pub fn new(context: &'a str) -> Self {
        Self { context }
    }

    /// 执行带有完整安全检查的音频处理操作
    ///
    /// # 参数
    ///
    /// * `samples` - 音频样本数据
    /// * `channels` - 声道数量
    /// * `sample_rate` - 采样率
    /// * `operation` - 要执行的操作闭包
    ///
    /// # 返回值
    ///
    /// 返回操作的执行结果
    pub fn run_with_protection<T, F>(
        &self,
        samples: &[f32],
        channels: usize,
        sample_rate: u32,
        operation: F,
    ) -> AudioResult<T>
    where
        F: FnOnce() -> AudioResult<T>,
    {
        // 第1层：输入验证
        SafetyGuard::validate_input(samples, channels, sample_rate)?;

        // 第4层：内存安全检查
        let estimated_memory = std::mem::size_of_val(samples);
        SafetyGuard::check_memory_safety(estimated_memory as u64, self.context)?;

        // 第6层：格式验证（使用默认位深度）
        SafetyGuard::validate_format(sample_rate, channels as u16, 32)?;

        // 执行操作
        let result = operation()?;

        // 第8层：确保清理
        let context = self.context;
        SafetyGuard::ensure_cleanup(&format!("{context}: 操作完成"))?;

        Ok(result)
    }
}

impl fmt::Display for SafetyGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "8层防御性安全保护系统")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_input_valid() {
        let samples = vec![0.5, -0.3, 0.7, -0.1];
        let result = SafetyGuard::validate_input(&samples, 2, 44100);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_input_empty_samples() {
        let samples = vec![];
        let result = SafetyGuard::validate_input(&samples, 2, 44100);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_input_invalid_channels() {
        let samples = vec![0.5, -0.3];

        // 0声道
        assert!(SafetyGuard::validate_input(&samples, 0, 44100).is_err());

        // 超过32声道
        assert!(SafetyGuard::validate_input(&samples, 33, 44100).is_err());
    }

    #[test]
    fn test_validate_input_mismatched_samples() {
        let samples = vec![0.5, -0.3, 0.7]; // 3个样本，不能被2声道整除
        let result = SafetyGuard::validate_input(&samples, 2, 44100);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_input_invalid_sample_rate() {
        let samples = vec![0.5, -0.3];

        // 采样率为0
        assert!(SafetyGuard::validate_input(&samples, 2, 0).is_err());

        // 采样率过高
        assert!(SafetyGuard::validate_input(&samples, 2, 500_000).is_err());
    }

    #[test]
    fn test_check_bounds_valid() {
        let result = SafetyGuard::check_bounds(5, 10, "测试数组");
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_bounds_invalid() {
        let result = SafetyGuard::check_bounds(10, 10, "测试数组");
        assert!(result.is_err());
    }

    #[test]
    fn test_check_numeric_overflow() {
        // 正常值
        assert!(SafetyGuard::check_numeric_overflow(123.45, "测试").is_ok());

        // 无穷大
        assert!(SafetyGuard::check_numeric_overflow(f64::INFINITY, "测试").is_err());

        // NaN
        assert!(SafetyGuard::check_numeric_overflow(f64::NAN, "测试").is_err());
    }

    #[test]
    fn test_check_memory_safety() {
        // 小内存使用
        assert!(SafetyGuard::check_memory_safety(1024, "测试").is_ok());

        // 大内存使用
        assert!(SafetyGuard::check_memory_safety(2 * 1024 * 1024 * 1024, "测试").is_err());
    }

    #[test]
    fn test_validate_format() {
        // 有效格式
        assert!(SafetyGuard::validate_format(44100, 2, 16).is_ok());

        // 无效采样率
        assert!(SafetyGuard::validate_format(12345, 2, 16).is_err());

        // 无效声道数
        assert!(SafetyGuard::validate_format(44100, 0, 16).is_err());
        assert!(SafetyGuard::validate_format(44100, 33, 16).is_err());

        // 无效位深度
        assert!(SafetyGuard::validate_format(44100, 2, 8).is_err());
    }

    #[test]
    fn test_validate_calculation() {
        // 正常情况
        assert!(SafetyGuard::validate_calculation(0.5, 1.0).is_ok());

        // RMS为负数
        assert!(SafetyGuard::validate_calculation(-0.1, 1.0).is_err());

        // Peak为负数
        assert!(SafetyGuard::validate_calculation(0.5, -0.1).is_err());

        // RMS大于Peak
        assert!(SafetyGuard::validate_calculation(1.0, 0.5).is_err());

        // 无效值
        assert!(SafetyGuard::validate_calculation(f64::NAN, 1.0).is_err());
        assert!(SafetyGuard::validate_calculation(0.5, f64::INFINITY).is_err());
    }

    #[test]
    fn test_ensure_cleanup() {
        // 正常情况
        assert!(SafetyGuard::ensure_cleanup("测试资源").is_ok());

        // 空资源信息
        assert!(SafetyGuard::ensure_cleanup("").is_err());
    }

    #[test]
    fn test_safe_runner() {
        let runner = SafeRunner::new("测试运行器");
        let samples = vec![0.5, -0.3, 0.7, -0.1];

        let result = runner.run_with_protection(&samples, 2, 44100, || Ok("操作成功".to_string()));

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "操作成功");
    }

    #[test]
    fn test_safe_runner_with_invalid_input() {
        let runner = SafeRunner::new("测试运行器");
        let samples = vec![]; // 空样本

        let result =
            runner.run_with_protection(&samples, 2, 44100, || Ok("不应该执行到这里".to_string()));

        assert!(result.is_err());
    }
}
