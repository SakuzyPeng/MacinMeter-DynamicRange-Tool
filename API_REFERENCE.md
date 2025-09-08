# MacinMeter DR Tool API Reference (Early-Version Branch)

**基于foobar2000 DR Meter逆向分析的音频动态范围分析工具API文档**

*版本: 0.1.0 (early-version分支)*  
*最后更新: 2025-09-08*

## 📋 目录

- [概述](#概述)
- [核心计算API](#核心计算api)
- [数据处理API](#数据处理api) 
- [音频解码API](#音频解码api)
- [错误处理](#错误处理)
- [工具类API](#工具类api)
- [使用示例](#使用示例)
- [Early-Version分支变更](#early-version分支变更)

## 概述

MacinMeter DR Tool 提供了一套完整的音频动态范围分析API，专门针对foobar2000 DR Meter算法的高精度实现。核心设计围绕以下原则：

- **🎯 高精度**: 与foobar2000 DR Meter结果的精确匹配
- **⚡ 高性能**: SIMD优化和并行处理支持
- **🔧 跨平台**: 纯Rust实现，支持主要操作系统
- **🛡️ 安全**: 8层防御性异常处理机制

### 核心数据流

```
Audio File → Decoder → Interleaved Samples → BatchProcessor → DrCalculator → DrResult
                                                    ↓
                                        SimdProcessor + ChannelData + Histogram
```

### 重要概念

- **24字节ChannelData**: foobar2000兼容的内存布局
- **累加器级Sum Doubling**: 在批次结束时对RMS累加器进行2倍处理
- **10001-bin直方图**: 超高精度DR分布统计（0.0000-1.0000幅度范围）
- **逆向遍历20%采样**: 从高RMS向低RMS遍历的算法

---

## 核心计算API

### `DrCalculator`

DR计算引擎，负责协调整个动态范围计算过程。

#### 构造函数

```rust
impl DrCalculator {
    /// 创建DR计算器（固定使用foobar2000兼容模式）
    pub fn new(
        channel_count: usize, 
        sum_doubling: bool, 
        sample_rate: u32
    ) -> AudioResult<Self>
}
```

**参数说明**:
- `channel_count`: 音频声道数量
- `sum_doubling`: 是否启用累加器级Sum Doubling补偿
- `sample_rate`: 采样率（Hz）

**注意**: Early-version分支固定使用foobar2000兼容模式（20%采样算法），无需额外参数指定。

#### 核心方法

```rust
impl DrCalculator {
    /// 处理交错音频样本（主要API）
    pub fn process_interleaved_samples(
        &mut self, 
        samples: &[f32]
    ) -> AudioResult<usize>
    
    /// 处理分离的声道样本
    pub fn process_channel_samples(
        &mut self, 
        channel_samples: &[Vec<f32>]
    ) -> AudioResult<usize>
    
    /// 计算DR值（核心方法）
    pub fn calculate_dr(&self) -> AudioResult<Vec<DrResult>>
    
    /// 重置计算器状态
    pub fn reset(&mut self)
}
```

#### 状态查询

```rust
impl DrCalculator {
    /// 获取已处理样本数量
    pub fn sample_count(&self) -> usize
    
    /// 获取声道数量
    pub fn channel_count(&self) -> usize
    
    /// 检查Sum Doubling是否启用
    pub fn sum_doubling_enabled(&self) -> bool
    
    /// 检查foobar2000模式是否启用
    pub fn foobar2000_mode(&self) -> bool
    
    /// 获取采样率
    pub fn sample_rate(&self) -> u32
    
    /// 获取直方图统计信息（foobar2000模式）
    pub fn get_histogram_stats(
        &self, 
        channel_idx: usize
    ) -> Option<SimpleStats>
}
```

### `DrResult`

DR计算结果数据结构。

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct DrResult {
    /// 声道索引
    pub channel: usize,
    
    /// 计算得到的DR值
    pub dr_value: f64,
    
    /// RMS值（用于调试和验证）
    pub rms: f64,
    
    /// Peak值（用于调试和验证）
    pub peak: f64,
    
    /// 参与计算的样本数量
    pub sample_count: usize,
}

impl DrResult {
    /// 创建新的DR计算结果
    pub fn new(
        channel: usize, 
        dr_value: f64, 
        rms: f64, 
        peak: f64, 
        sample_count: usize
    ) -> Self
    
    /// 格式化DR值为整数显示（与foobar2000兼容）
    pub fn dr_value_rounded(&self) -> i32
}
```

---

## 数据处理API

### `ChannelData`

24字节内存对齐的声道数据结构，兼容foobar2000内存布局。

```rust
impl ChannelData {
    /// 创建新的声道数据结构
    pub fn new() -> Self
    
    /// 处理单个样本
    pub fn process_sample(&mut self, sample: f32)
    
    /// 计算标准RMS值
    pub fn calculate_rms(&self, sample_count: usize) -> f64
    
    /// 🆕 计算带累加器级Sum Doubling的RMS值
    pub fn calculate_rms_with_accumulator_sum_doubling(
        &self, 
        sample_count: usize, 
        apply_sum_doubling: bool
    ) -> f64
    
    /// 获取有效Peak值（双Peak回退系统）
    pub fn get_effective_peak(&self) -> f64
    
    /// 获取带验证的有效Peak值
    pub fn get_effective_peak_with_validation(&self) -> (f64, PeakQuality)
    
    /// 重置声道数据
    pub fn reset(&mut self)
}
```

### `BatchProcessor`

高性能批量处理器，支持SIMD优化和并行处理。

```rust
impl BatchProcessor {
    /// 创建批量处理器
    pub fn new(
        enable_multithreading: bool, 
        thread_pool_size: Option<usize>
    ) -> Self
    
    /// 🚨 Early-Version API: 处理交错音频批次（4个参数，固定foobar2000模式）
    pub fn process_interleaved_batch(
        &self,
        samples: &[f32],           // 交错音频样本
        channel_count: usize,      // 声道数量
        sample_rate: u32,          // 采样率
        sum_doubling: bool,        // Sum Doubling开关
    ) -> AudioResult<BatchResult>
    
    /// 获取SIMD能力信息
    pub fn simd_capabilities(&self) -> &SimdCapabilities
    
    /// 设置多线程处理
    pub fn set_multithreading(&mut self, enabled: bool)
    
    /// 检查是否应该使用SIMD
    pub fn should_use_simd(&self, sample_count: usize) -> bool
    
    /// 获取线程池大小
    pub fn thread_pool_size(&self) -> Option<usize>
}
```

### `BatchResult`

批量处理结果，包含DR值和性能统计。

```rust
#[derive(Debug, Clone)]
pub struct BatchResult {
    /// DR计算结果
    pub dr_results: Vec<DrResult>,
    
    /// 处理性能统计
    pub performance_stats: BatchPerformanceStats,
    
    /// SIMD使用情况
    pub simd_usage: SimdUsageStats,
}
```

### `BatchPerformanceStats`

性能统计信息。

```rust
#[derive(Debug, Clone)]
pub struct BatchPerformanceStats {
    /// 总处理时间（微秒）
    pub total_duration_us: u64,
    
    /// 每秒处理样本数
    pub samples_per_second: f64,
    
    /// 处理的声道数
    pub channels_processed: usize,
    
    /// 处理的样本总数
    pub total_samples: usize,
    
    /// SIMD加速比（相对于标量实现）
    pub simd_speedup: f64,
}
```

---

## 音频解码API

### `WavDecoder`

WAV格式音频解码器。

```rust
impl WavDecoder {
    /// 创建新的WAV解码器
    pub fn new() -> Self
    
    /// 加载WAV文件
    pub fn load_file<P: AsRef<Path>>(
        &mut self, 
        path: P
    ) -> AudioResult<AudioFormat>
    
    /// 获取音频格式信息
    pub fn format(&self) -> Option<&AudioFormat>
    
    /// 获取交错音频样本
    pub fn samples(&self) -> &[f32]
    
    /// 获取指定声道的样本
    pub fn channel_samples(
        &self, 
        channel: usize
    ) -> AudioResult<Vec<f32>>
    
    /// 获取所有声道的样本
    pub fn all_channel_samples(&self) -> AudioResult<Vec<Vec<f32>>>
    
    /// 检查是否已加载文件
    pub fn is_loaded(&self) -> bool
    
    /// 清除已加载的数据
    pub fn clear(&mut self)
}
```

### `MultiDecoder`

多格式音频解码器（支持FLAC、MP3、AAC、OGG等）。

```rust
impl MultiDecoder {
    /// 创建新的多格式解码器
    pub fn new() -> Self
    
    /// 加载音频文件（自动格式检测）
    pub fn load_file<P: AsRef<Path>>(
        &mut self, 
        path: P
    ) -> AudioResult<AudioFormat>
    
    /// 获取交错音频样本
    pub fn samples(&self) -> &[f32]
    
    /// 获取指定声道的样本
    pub fn channel_samples(
        &self, 
        channel: usize
    ) -> AudioResult<Vec<f32>>
    
    /// 清除已加载的数据
    pub fn clear(&mut self)
}
```

### `AudioFormat`

音频格式描述结构。

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFormat {
    /// 采样率（Hz）
    pub sample_rate: u32,
    
    /// 声道数
    pub channels: u16,
    
    /// 每样本位数
    pub bits_per_sample: u16,
    
    /// 样本总数（单声道）
    pub sample_count: u64,
}

impl AudioFormat {
    /// 创建新的音频格式描述
    pub fn new(
        sample_rate: u32, 
        channels: u16, 
        bits_per_sample: u16, 
        sample_count: u64
    ) -> Self
    
    /// 验证格式参数的有效性
    pub fn validate(&self) -> AudioResult<()>
    
    /// 估算内存使用量
    pub fn estimated_memory_usage(&self) -> u64
}
```

---

## 错误处理

### `AudioError`

统一的音频处理错误类型。

```rust
#[derive(Debug, Clone, thiserror::Error)]
pub enum AudioError {
    #[error("无效输入: {0}")]
    InvalidInput(String),
    
    #[error("文件IO错误: {0}")]
    IoError(String),
    
    #[error("解码错误: {0}")]
    DecodeError(String),
    
    #[error("计算错误: {0}")]
    CalculationError(String),
    
    #[error("内存不足: {0}")]
    OutOfMemory(String),
    
    #[error("不支持的格式: {0}")]
    UnsupportedFormat(String),
}
```

### `AudioResult<T>`

音频处理结果类型别名。

```rust
pub type AudioResult<T> = Result<T, AudioError>;
```

---

## 工具类API

### `SafeRunner`

8层防御性异常处理机制。

```rust
impl SafeRunner {
    /// 安全执行带异常处理的操作
    pub fn run_safe<F, T>(operation: F) -> AudioResult<T>
    where
        F: FnOnce() -> AudioResult<T>,
    
    /// 带自定义错误消息的安全执行
    pub fn run_with_context<F, T>(
        operation: F, 
        context: &str
    ) -> AudioResult<T>
    where
        F: FnOnce() -> AudioResult<T>,
}
```

---

## 使用示例

### 基本DR计算

```rust
use macinmeter_dr_tool::*;

// 1. 创建DR计算器
let mut calculator = DrCalculator::new_with_mode(
    2,      // 立体声
    true,   // 启用Sum Doubling
    true,   // 启用foobar2000兼容模式
    44100   // 44.1kHz
)?;

// 2. 处理音频样本
let samples = vec![0.1, -0.1, 0.2, -0.2, 0.05, -0.05]; // 交错样本
calculator.process_interleaved_samples(&samples)?;

// 3. 计算DR值
let results = calculator.calculate_dr()?;
for result in results {
    println!("声道 {}: DR{}", result.channel, result.dr_value_rounded());
}
```

### 批量处理

```rust
use macinmeter_dr_tool::*;

// 1. 创建批量处理器
let processor = BatchProcessor::new(true, Some(4)); // 启用多线程，4线程

// 2. 批量处理音频数据
let batch_result = processor.process_interleaved_batch(
    &samples,       // 音频样本
    2,             // 立体声
    44100,         // 采样率
    true,          // Sum Doubling（固定foobar2000模式）
)?;

// 3. 查看结果和性能统计
println!("处理时间: {}µs", batch_result.performance_stats.total_duration_us);
println!("SIMD加速比: {:.1}x", batch_result.performance_stats.simd_speedup);
```

### 音频文件处理

```rust
use macinmeter_dr_tool::*;
use std::path::Path;

// 1. 加载音频文件
let mut decoder = MultiDecoder::new();
let format = decoder.load_file("test.flac")?;

println!("格式: {}Hz, {}声道", format.sample_rate, format.channels);

// 2. 创建批量处理器
let processor = BatchProcessor::new(true, None);

// 3. 处理音频数据
let result = processor.process_interleaved_batch(
    decoder.samples(),
    format.channels as usize,
    format.sample_rate,
    true, // Sum Doubling（固定foobar2000模式）
)?;

// 4. 显示DR结果
for dr_result in result.dr_results {
    println!("声道 {}: DR{} (RMS: {:.6}, Peak: {:.6})", 
        dr_result.channel, 
        dr_result.dr_value_rounded(),
        dr_result.rms,
        dr_result.peak
    );
}
```

---

## Early-Version分支变更

### 🚨 重要API变更

**BatchProcessor.process_interleaved_batch 方法签名更新**:

```rust
// ❌ 旧版本（6个参数）- 已废弃
pub fn process_interleaved_batch(
    samples: &[f32], 
    channels: usize, 
    sample_rate: u32,
    sum_doubling: bool,
    foobar2000_mode: bool,
    weighted_rms: bool,  // 已移除
) -> AudioResult<BatchResult>

// ✅ 新版本（4个参数）- Early-version分支
pub fn process_interleaved_batch(
    samples: &[f32],
    channels: usize, 
    sample_rate: u32,
    sum_doubling: bool, // 固定使用foobar2000模式
) -> AudioResult<BatchResult>
```

### 移除的功能

以下功能在early-version分支中已被移除：

- ❌ `weighted_rms` 参数和相关实验性功能
- ❌ `DrCalculator.set_weighted_rms()` 等控制方法
- ❌ `DrCalculator.enable_weighted_rms()` 方法
- ❌ `DrCalculator.disable_weighted_rms()` 方法
- ❌ `DrCalculator.is_weighted_rms_enabled()` 方法
- ❌ `SimpleHistogramAnalyzer.calculate_weighted_20_percent_rms()` 方法

### 新增功能

- ✅ `ChannelData.calculate_rms_with_accumulator_sum_doubling()` - 累加器级Sum Doubling
- ✅ 多声道感知的直方图内存布局支持
- ✅ 精确的20%采样边界控制算法

### 算法改进

1. **累加器级Sum Doubling**: 
   - Sum Doubling现在在批次结束时对整个RMS累加器进行2倍处理
   - 不再在RMS值级别进行修正，确保与foobar2000的最佳匹配

2. **代码简化**:
   - 移除了60+行的weighted_rms实验性代码
   - API参数从6个减少到5个，降低使用复杂度
   - 统一文档风格，专注foobar2000兼容性

3. **质量保证**:
   - 自动化预提交钩子，确保代码质量
   - 零警告标准，所有Clippy警告必须修复
   - 完整的单元测试和文档测试覆盖

### 兼容性说明

如果您正在从其他分支迁移到early-version分支，请注意以下变更：

1. **更新方法调用**: 移除`weighted_rms`参数
2. **移除权重设置**: 删除所有`set_weighted_rms`相关调用
3. **测试更新**: 更新测试用例以匹配新的API签名

---

## 性能注意事项

- **SIMD优化**: 在x86_64架构上自动启用SSE2，其他架构回退到标量计算
- **内存对齐**: ChannelData必须8字节对齐以获得最佳性能
- **并行处理**: 使用rayon进行批量文件处理，不是单文件内并行
- **浮点精度**: 使用f64进行累加运算，f32用于样本输入

## 许可证

MIT License - 详见项目根目录的LICENSE文件。

---

*本文档反映early-version分支（commit 380ca3c）的API状态*  
*生成时间: 2025-09-08*