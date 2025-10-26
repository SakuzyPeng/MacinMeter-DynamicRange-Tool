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
- **性能达成**: 当前已达到 **+230% 性能提升**（vs 早期基线）

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
- **dead_code**: 及时删除未使用的函数、结构体、常量和变量
- **unused_variables**: 尽量不要通过`_`隐藏未使用变量；优先优先确认该变量是否可以安全删除。
- **unused_imports**: 清理多余的import语句
- **missing_docs**: 为所有 public API（函数、结构体、枚举、模块等）添加文档注释
- **clippy::all**: 遵循Clippy的所有最佳实践建议

### 💡 常见警告修复
- **未使用变量**: 优先检查该变量是否多余，能删则删；若必须保留（例如临时占位或接口要求），再使用 _ 前缀
- **未使用导入**: 删除多余的`use`语句，若 IDE 自动导入，请定期运行 cargo fix --allow-dirty --allow-staged 清理
- **缺少文档**: 为public函数添加`/// 文档注释`，简要说明功能、参数与返回值

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
- 性能提升：累积 3.3倍 (115MB/s → 705MB/s，2025-10-25 数据)
- 适用场景：大文件、批量处理、复杂音频流

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

# 当前性能基线（2025-10-25，O(n)峰值优化后）
# 测试文件: 贝多芬第九交响曲 FLAC (1.51GB)
#
# 处理速度:
#   平均值: 705.38 MB/s
#   中位数: 750.97 MB/s (最可信指标)
#   标准差: 74.60 MB/s (CV: 10.58%)
#
# 运行时间:
#   平均值: 2.909s
#   中位数: 2.698s
#
# 内存峰值:
#   平均值: 44.02 MB
#   中位数: 44.215 MB
#   标准差: 2.70 MB
#
# 性能与前序对标:
#   vs 2025-10-24 基线: 无回归 (+0.47% 平均值, -0.09% 中位数)
#   vs 2025-01-14: +230.8% (213.27 → 705.38 MB/s)
#
# 最新优化 (2025-10-25):
#   ✅ O(n)单遍扫描峰值选择 (替代 O(n log n) 排序)
#   ✅ 长曲目性能改善: 12-20倍 (峰值查询)
#   ✅ 168 个单元测试全通过
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
# 单元测试（168个测试，1.5秒完成）
cargo test --lib

# 完整测试（包括集成测试，但跳过慢速性能测试）
cargo test

# 运行被忽略的慢速性能测试（仅在Release模式下运行）
cargo test --release -- --ignored

# 性能基准测试（10次取平均值，消除系统噪声）
# 生成性能报告：平均/中位/标准差/CV%
./benchmark_10x.sh

# 对标特定基线版本
./benchmark_10x.sh --exe /path/to/baseline/binary

# 串行模式性能测试
./benchmark_10x.sh --serial
```

### 📊 性能基准脚本说明

**benchmark_10x.sh**: 10次重复测试取统计数据
- **功能**: 运行10次基准测试，计算平均值、中位数、标准差
- **输出指标**: 运行时间、处理速度、内存峰值、CPU使用率
- **用途**: 消除系统缓存/调度的随机性，获得可靠的性能数据
- **选项**:
  - `--exe PATH`: 指定可执行文件路径（默认：target/release版本）
  - `--serial`: 使用串行解码而非并行解码
  - `--help`: 显示帮助信息

**推荐工作流**:
```bash
# 1. 性能优化后，重新构建Release版本
cargo build --release

# 2. 运行10x基准测试（生成当前版本性能数据）
./benchmark_10x.sh

# 3. 对标历史版本
./benchmark_10x.sh --exe /path/to/old-version

# 4. 观察指标：看中位数而非平均值（更稳定）
# 性能差异 < 5% 属于正常噪声范围
```

### ⚠️ 慢速测试说明

**已标记为 `#[ignore]` 的慢速测试**：

这些测试在Debug模式下运行极慢，已标记为忽略以避免CI超时。包括：
- SIMD性能测试（大数据集）
- 内存完整性验证
- 并发解码器高负载测试

**使用建议**：
- 常规开发：运行 `cargo test`（自动跳过慢速测试）
- 性能验证：运行 `cargo test --release -- --ignored`（Release模式执行慢速测试）

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
