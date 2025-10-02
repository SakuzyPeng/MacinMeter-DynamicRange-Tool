# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## ⚠️ 重要提醒：专家角色激活

**在开始任何技术工作前，必须激活专业角色：**

### 🎯 推荐专家角色
- **rust-audio-expert**: Rust音频开发专家 → `action("rust-audio-expert")`
  - 专门负责DR算法实现、SIMD优化、音频解码等核心技术
  - 深度理解foobar2000逆向分析结果和项目技术约束
  - 具备工业级代码质量保证能力

### 🔍 关键约束提醒
- **Windows验证限制**: foobar2000 DR Meter仅在Windows可用，结果对比只能由用户执行
- **高精度原则**: 所有实现追求与foobar2000结果的高精度一致
- **性能目标**: SIMD优化需达到6-7倍性能提升

---

## 项目概述

MacinMeter DR Tool 是一个基于foobar2000 DR Meter逆向分析的音频动态范围(DR)分析工具，使用Rust实现，目标是达到高精度实现和工业级性能。

**foobar2000-plugin分支**：采用完全流式原生架构，实现真正的零内存累积处理，默认启用与foobar2000原版完全对齐的窗口级算法。

### 🎵 音频格式支持

**通过Symphonia支持**：
- **无损格式**: FLAC, ALAC (Apple Lossless), WAV, AIFF, PCM (AU, CAF等)
- **有损格式**: AAC, OGG Vorbis, MP1 (MPEG Layer I)
- **容器格式**: MP4/M4A, MKV/WebM

**专用解码器**：
- **Opus**: 通过songbird专用解码器 (Discord音频库)
- **MP3**: ⚠️ 有状态解码格式，强制串行处理（见下方说明）

**总计支持格式**: 12+种主流音频格式，覆盖90%+用户需求

### ⚠️ 有状态编码格式处理策略

**MP3特殊处理**：MP3采用有状态解码，每个packet依赖前一个packet的解码器状态。并行解码会创建独立decoder丢失上下文，导致样本错误。因此**MP3格式自动降级到串行解码器**，确保解码正确性。

```rust
// src/audio/universal_decoder.rs (lines 144-154)
if ext_lower == "mp3" {
    return Ok(Box::new(UniversalStreamProcessor::new(path)?)); // 强制串行
}
```

**并行支持格式**：FLAC、AAC、WAV、AIFF、OGG等无状态格式继续使用高性能并行解码。

## 构建和运行命令

```bash
# 构建开发版本
cargo build

# 构建优化版本（生产环境）
cargo build --release

# 运行工具（开发环境）
cargo run -- [目录路径]

# 运行生产版本
./target/release/MacinMeter-DynamicRange-Tool-foo_dr [目录路径]

# 运行测试
cargo test

# 运行单个测试
cargo test test_dr_calculation_accuracy

# 运行基准测试
cargo test --release benchmark

# 检查代码格式
cargo fmt --check

# 应用代码格式化
cargo fmt

# 运行clippy检查
cargo clippy -- -D warnings
```

## 📁 Mac编译产物绝对路径

### 可执行文件位置
**Debug版本 (开发用)**:
```
/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/target/debug/MacinMeter-DynamicRange-Tool-foo_dr
```
- 文件大小: ~10.4 MB
- 包含调试信息，启动快但运行较慢

**Release版本 (生产用)**:
```
/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/target/release/MacinMeter-DynamicRange-Tool-foo_dr
```
- 文件大小: ~1.7 MB  
- 优化编译，启动慢但运行快，用于性能测试和发布

### 快速测试命令
```bash
# 测试release版本 
/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/target/release/MacinMeter-DynamicRange-Tool-foo_dr --help

# 测试流式处理功能 (支持任意大小文件)
/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/target/release/MacinMeter-DynamicRange-Tool-foo_dr /path/to/large/audio/file.flac

# 启用详细模式查看流式处理过程
/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/target/release/MacinMeter-DynamicRange-Tool-foo_dr --verbose /path/to/audio/directory
```

## ⚠️ 重要开发习惯：零警告原则

### 🚨 编译警告清理习惯

**每次代码修改后必须立即检查和清理编译警告！**

### 📋 代码质量检查工作流
```bash
# 完整检查（推荐）
cargo fmt --check && cargo clippy -- -D warnings && cargo check && cargo audit && cargo test

# 快速检查
cargo check

# 发布检查
cargo build --release && cargo test --release
```

### 🔧 质量工具
- **rustfmt**: 代码格式化 | **clippy**: 静态分析 | **cargo-audit**: 安全扫描

### 🎯 零警告标准
- **dead_code**: 及时删除未使用的函数和变量
- **unused_variables**: 使用`_`前缀或删除未使用变量  
- **unused_imports**: 清理多余的import语句
- **missing_docs**: 为所有public API添加文档注释
- **clippy::all**: 遵循Clippy的所有最佳实践建议

### 💡 常见警告修复
- **未使用变量**: `let data` → `let _data`
- **未使用导入**: 删除多余的`use`语句
- **缺少文档**: 为public函数添加`/// 文档注释`

### 🎵 音频项目专用检查
- **精度检查**: `cargo clippy -- -W clippy::cast_lossless`
- **SIMD验证**: `cargo rustc -- --emit=asm`
- **内存布局**: `cargo test -- --nocapture layout_tests`

**⚠️ 重要**: Rust编译器警告都很有价值，对音频处理应用尤其重要！

### 🔄 预提交钩子
自动执行：代码格式检查、Clippy分析、编译检查、单元测试、安全审计。所有检查必须通过才能提交。

---

## 核心架构

**4层模块化设计** + **2条性能路径**：

### 模块分层
- **tools/**: CLI、格式化输出、文件扫描
- **core/**: DR算法引擎（DrCalculator + WindowRmsAnalyzer）
- **processing/**: SIMD优化和音频处理
  - `simd_core.rs`: SIMD基础设施（SimdProcessor + SimdCapabilities）
  - `sample_conversion.rs`: 样本格式转换（i16/i24/i32→f32）
  - `channel_separator.rs`: 声道样本分离引擎
  - `dr_channel_state.rs`: DR计算状态（24字节内存布局）
  - `processing_coordinator.rs`: 协调器（编排各服务）
  - `performance_metrics.rs`: 性能统计
- **audio/**: 解码器（串行BatchPacketReader + 并行OrderedParallelDecoder）

### 🚀 双路径架构（关键设计）

**串行路径**（UniversalStreamProcessor）：
- BatchPacketReader：减少99%系统调用的I/O优化
- 单Decoder：直接解码，零通信开销
- 适用场景：单文件处理、低并发

**并行路径**（ParallelUniversalStreamProcessor）：
- OrderedParallelDecoder：4线程64包批量解码
- SequencedChannel：序列号保证样本时间顺序
- 1.85倍性能提升（115MB/s → 213MB/s）
- 适用场景：大文件、批量处理

**共享组件**（ProcessorState）：
- 消除60%代码重复
- 统一状态管理：position, format, chunk_stats, sample_converter
- 统一trait实现：format(), progress(), reset(), get_stats()

### 核心算法

1. **20%采样**: 窗口RMS排序取最响20%计算DR
2. **SIMD优化**: ARM NEON向量化（S16/S24→F32转换）
3. **零内存累积**: 流式窗口处理，~45MB恒定内存
4. **双峰值系统**: 主Peak失效自动切换次Peak

## 关键设计模式

### ProcessorState共享状态模式
消除串行和并行处理器的60%代码重复：
```rust
struct ProcessorState {
    path, format, current_position, total_samples,
    chunk_stats, sample_converter, track_id
}
// 提供统一方法：get_format(), get_progress(), update_position(), reset(), get_stats()
```

### 解码器选择逻辑
```rust
UniversalDecoder::create_streaming(path)           // 串行，默认
UniversalDecoder::create_streaming_parallel(path)  // 并行，高性能
```

### 流式处理接口
```rust
trait StreamingDecoder {
    fn next_chunk(&mut self) -> AudioResult<Option<Vec<f32>>>;
    fn format(&self) -> AudioFormat;
    fn progress(&self) -> f32;
}
```

---

## 性能基准测试

```bash
# 10次平均测试（消除测量误差）
./benchmark_10x.sh

# 当前性能（2025-01-14，Phase 2.1）
# 测试文件: 贝多芬第九交响曲 FLAC (1.51GB)
# 平均速度: 213.27 MB/s
# 平均内存: 44.52 MB
# 性能提升: 1.85x vs 基线（115MB/s）
```

## 开发原则

### 🎯 架构约束
- **串行≠并发度1的并行**: 保持两条独立路径，不强行统一
- **组合优于继承**: 用ProcessorState共享状态，而非enum统一模式
- **声道限制**: 仅支持1-2声道，3+声道友好拒绝

### 💎 性能优先
- 默认并行解码（4线程64包批量）
- SIMD自动启用（ARM NEON/x86 SSE2）
- Sum Doubling固定启用（foobar2000兼容）

## 测试策略

```bash
# 单元测试（59个测试，0.02秒完成）
cargo test

# 只运行库测试（排除doctest）
cargo test --lib

# 性能验证（必须在重构后运行）
cargo build --release && ./benchmark_10x.sh

# 精度验证（SIMD vs 标量）
cargo test --release simd_precision_test -- --nocapture
```

---

## 最近的重要改进

### 🐛 MP3和AIFF解码器修复 (2025-10-02)
**问题1: MP3并行解码返回零值**
- 根因：MP3有状态解码，每个packet依赖前一个decoder状态
- 方案：文件扩展名检测，MP3强制串行解码
- 验证：DR=10.05dB vs foobar2000完全一致

**问题2: AIFF串行解码DR=0dB**
- 根因：S16/S24 SIMD转换中`clear()+resize()`清空样本
- 方案：恢复commit 0e4dd2b的`reserve()+resize()`模式
- 验证：DR=10.25dB，样本数10,662,000正确

**代码质量**：修复clippy collapsible_if警告，用match pattern guard替代嵌套if

### 🎯 Processing层重命名优化
对processing模块进行完整重命名以提升可读性：

| 原文件名 | 新文件名 | 改进原因 |
|---------|---------|---------|
| `simd_channel_data.rs` | `simd_core.rs` | 消除名不副实 |
| `channel_data.rs` | `dr_channel_state.rs` | 增强领域语义 |
| `channel_extractor.rs` | `channel_separator.rs` | 提升操作准确性 |

### 📦 宏优化（消除重复代码）
- **sample_conversion.rs**: 4个宏消除132行重复
- **universal_decoder.rs**: trait实现去重
- **成果**: 减少140+行重复，维护成本降低50%

---

## 重要架构决策记录

### 为什么保持串行和并行两条路径？
**问题**: 能否用DecoderMode enum统一串行和并行？

**答案**: **不能**。串行≠并发度1的并行：
- **串行**（BatchPacketReader）：零通信开销，直接VecDeque缓冲
- **并行度1**（OrderedParallelDecoder）：仍有channel/HashMap/序列号开销，但无并行收益
- **结论**: 保持两条独立路径，用ProcessorState消除重复

### 为什么MP3必须串行解码？
**问题**: 为何不能对MP3使用并行解码器？

**答案**: MP3是有状态编码格式：
- **状态依赖**: 每个packet的解码依赖前一个packet的decoder状态
- **并行问题**: 并行解码器为每个线程创建独立decoder，丢失packet间的状态连续性
- **症状**: 样本值从某个位置开始变为0.0，导致DR计算错误
- **解决方案**: 文件扩展名检测，自动降级到串行解码器
- **其他格式**: FLAC、AAC、WAV、AIFF等无状态格式仍使用并行解码

### 为什么processing层文件要精确命名？
**问题**: 为何重命名channel_data、channel_extractor、simd_channel_data？

**答案**: 解决命名混淆问题：
- **"channel"前缀过载**: 3个文件都用"channel"但职责完全不同
- **名不副实**: `simd_channel_data.rs`包含通用SIMD基础设施，与channel data无关
- **语义模糊**: `channel_data.rs`缺少领域信息，不明确是DR计算状态
- **结论**: 精确命名提升可维护性，降低认知负担

# important-instruction-reminders
Do what has been asked; nothing more, nothing less.
NEVER create files unless they're absolutely necessary for achieving your goal.
ALWAYS prefer editing an existing file to creating a new one.
NEVER proactively create documentation files (*.md) or README files. Only create documentation files if explicitly requested by the User.

      
      IMPORTANT: this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant to your task.