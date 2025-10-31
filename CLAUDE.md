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
- **高精度原则**: 与foobar2000结果保持±0.02-0.05的精度差异，接近X.5边界时四舍五入可能相差1级
- **性能达成**: 当前已达到 **+230% 性能提升**（vs 早期基线）

---

## 项目概述

MacinMeter DR Tool 是一个基于foobar2000 DR Meter逆向分析的音频动态范围(DR)分析工具，使用Rust实现，目标是达到高精度实现和工业级性能。

**foobar2000-plugin分支**：采用完全流式原生架构，实现真正的零内存累积处理，默认启用与foobar2000原版完全对齐的窗口级算法。

### 🆕 实验性功能

**静音过滤** (`--filter-silence`):
- 在窗口RMS计算时过滤低于阈值的样本（默认 -70 dBFS）
- 防止静音段虚高动态范围评分，适用于现场录音/有静音间隔的音乐
- 用法: `--filter-silence` (使用默认 -70 dB) 或 `--filter-silence=-80` (自定义阈值)

**边缘裁切** (`--trim-edges`):
- 样本级首尾静音裁切（P0实现），保留中段艺术静音
- 三态状态机 (Leading→Passing→Trailing) + 迟滞机制防止弱音误判
- 用法: `--trim-edges` (默认 -60 dB) 配合 `--trim-min-run=100` (最小持续60ms)
- 时间/空间复杂度: O(N) / O(min_run_frames)

**DR边界预警**:
- 检测DR值接近四舍五入边界的情况（如10.45、10.52、15.48等）
- 原因：与foobar2000存在±0.02-0.05的精度差异，接近X.5边界时四舍五入后可能跨越1级
- 示例：10.47(MacinMeter)→DR10 vs 10.52(foobar2000)→DR11
- 帮助用户理解为何结果与foobar2000可能相差1个DR级别

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

### 📦 常量管理：src/tools/constants.rs

**集中管理所有重要常量，避免"默认值漂移"**：

**模块分类**：
- `dr_analysis`: DR算法核心常量（窗口时长、削波阈值、精度常量）
- `format_constraints`: 音频格式约束（最大声道数等）
- `decoder_performance`: 解码器性能参数（批大小、线程数、超时时间）
- `defaults`: 默认配置值（并行参数）
- `parallel_limits`: 并发度限制范围
- `buffers`: 内存优化常量（预分配倍数、硬上限）
- `app_info`: 应用程序文案（分支信息、版本、输出标识）

**使用原则**：
- **新增常量必须加入constants.rs**，不得在代码中硬编码魔法数字
- 每个常量必须附带详细文档注释，说明用途、设计考量、性能影响
- 相关常量分组到同一模块，便于统一调整
- 避免跨文件重复定义相同语义的常量

**示例**：
```rust
use crate::tools::constants::{dr_analysis, decoder_performance, defaults};

let window_duration = dr_analysis::WINDOW_DURATION_SECONDS;
let batch_size = decoder_performance::PARALLEL_DECODE_BATCH_SIZE;
let threads = defaults::PARALLEL_THREADS;
```

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

## 📐 代码风格规范

### 🌍 打印语句双语化要求

**所有用户可见的打印语句必须采用双语格式（中文+英文）**

#### 适用范围
- `println!()` - 标准输出
- `eprintln!()` - 错误输出
- CLI参数的 `.help()` 文本
- 错误消息（`AudioError` 的 `Display` 实现）
- 错误分类显示名称（`ErrorCategory::display_name()`）

#### 标准格式

```rust
// ✅ 正确格式1：英文在前
println!("Results saved / 结果已保存到: {}", path.display());
println!("Processing completed / 处理完成");

// ✅ 正确格式2：中文在前（对于更面向中文用户的场景）
println!("扫描目录 / Scanning directory: {}", dir.display());
println!("找到 {} 个音频文件 / Found {} audio files", count, count);

// ✅ 分类标识：使用大写英文前缀
eprintln!("[WARNING] Worker thread panicked / 工作线程发生panic");
eprintln!("[ERROR] File not found / 文件未找到: {}", path);
println!("[PROGRESS] Processing window / 处理窗口 #{}", num);
println!("[EXPERIMENTAL] Enable edge trimming / 启用边缘裁切");
```

#### CLI帮助文本格式

```rust
.help("Audio file or directory path / 音频文件或目录路径 (支持WAV, FLAC等)")
.help("Show detailed processing information / 显示详细处理信息")
.help("[EXPERIMENTAL] Enable silence filtering / 启用静音过滤")
```

#### 错误消息格式

```rust
// AudioError Display 实现
AudioError::InvalidInput(msg) => {
    write!(f, "输入验证失败 / Input validation failed: {msg}")
}

// ErrorCategory 显示名称
ErrorCategory::Format => "FORMAT/格式错误",
ErrorCategory::Decoding => "DECODING/解码错误",
```

#### 参数验证错误

```rust
// parse函数返回的错误消息
Err(format!("'{s}' is not a valid number / 不是有效的数字"))
Err(format!("value must be at least {min} / 值必须至少为 {min}"))
```

### 💬 注释规范

**注释只需保留中文，不需要双语化**

#### 基本原则
- 注释面向开发者，使用中文即可
- 去除所有emoji符号（🚀 🎯 🎵 📄 ⌛ 🧪 🏁 等）
- 避免模糊的阶段描述

#### 注释清理规范

```rust
// ❌ 错误：包含emoji和模糊描述
// 🧪 P0阶段：创建边缘裁切器（如果启用）
// ⌛ 智能缓冲进度

// ✅ 正确：清晰的中文描述
// 创建边缘裁切器（如果启用）
// 智能缓冲流式处理：积累chunk到标准窗口大小，保持算法精度
```

#### 避免模糊描述

```rust
// ❌ 错误：模糊的阶段标识
// P0 phase: Edge trimming
// 阶段D优化禁用时，仅使用阶段B的compact机制

// ✅ 正确：具体的功能描述
// 首尾边缘裁切（如果启用）
// 窗口对齐优化禁用时，仅使用compact阈值机制
```

#### 长注释格式

```rust
// 尾块处理策略说明：
// 末尾不足3秒的尾块直接参与计算（符合多数实现标准）：
// - 尾块样本计入 20% RMS 统计（通过 WindowRmsAnalyzer.process_samples）
// - 尾块峰值参与峰值检测（主Peak、次Peak更新）
// - 此行为与 foobar2000 DR Meter 一致，确保完整音频内容被分析
```

### 🚫 禁止使用的emoji清单

**在代码注释和打印语句中禁止使用以下emoji**：
- 进度/状态：🚀 ⌛ 🏁 📊 ✅ ❌ ⚠️
- 功能标识：🎯 🎵 🎨 🔧 🔍 💡 📝 📄
- 实验性标记：🧪 🚧 🔬
- 其他装饰：🌊 💎 🔥 ⚡ 💻

**例外**：CLAUDE.md文档本身可以使用emoji作为视觉标识，因为它是面向开发者的参考文档。

### 📝 实际清理示例

#### 打印语句清理

```rust
// Before (❌)
println!("📄 结果已保存到: {}", path.display());
println!("⌛ 智能缓冲进度: {:.1}%", progress);

// After (✅)
println!("Results saved / 结果已保存到: {}", path.display());
println!("[PROGRESS] Smart buffer progress / 智能缓冲进度: {:.1}%", progress);
```

#### 注释清理

```rust
// Before (❌)
// 🧪 P0阶段：创建边缘裁切器（如果启用）
// 🌊 智能缓冲流式处理：积累chunk到标准窗口大小

// After (✅)
// 创建边缘裁切器（如果启用）
// 智能缓冲流式处理：积累chunk到标准窗口大小，保持算法精度
```

#### 错误消息清理

```rust
// Before (❌)
return Err(format!("线程池创建失败: {e}"));
return Err(format!("'{s}' 不是有效的数字"));

// After (✅)
return Err(format!("Thread pool creation failed / 线程池创建失败: {e}"));
return Err(format!("'{s}' is not a valid number / 不是有效的数字"));
```

### ✅ 代码清理检查清单

在提交代码前，确保完成以下检查：

- [ ] 所有 `println!`/`eprintln!` 语句已双语化
- [ ] CLI 参数的 `.help()` 文本已双语化
- [ ] `AudioError` 的 `Display` 实现已双语化
- [ ] 错误分类显示名称已双语化
- [ ] 参数验证错误消息已双语化
- [ ] 所有注释已去除emoji符号
- [ ] 注释中没有模糊的阶段描述（如"P0"、"阶段X"）
- [ ] 注释使用清晰、具体的中文描述
- [ ] 代码通过 `cargo fmt` 格式化
- [ ] 代码通过 `cargo clippy -- -D warnings` 检查
- [ ] 所有测试通过 `cargo test`

---

## 📝 Git提交规范：双语化Commit Message

### ✅ 强制要求

**所有commit message必须采用双语格式（中文+英文）**，确保国际协作和代码历史可读性。

### 📋 标准格式

```
<type>: <中文简要描述> / <English brief description>

[可选详细说明 / Optional detailed description]
```

### 🏷️ Type类型

- **feat**: 新功能 / New feature
- **fix**: Bug修复 / Bug fix
- **refactor**: 重构代码 / Code refactoring
- **perf**: 性能优化 / Performance optimization
- **docs**: 文档更新 / Documentation update
- **test**: 测试相关 / Test related
- **chore**: 构建/工具/配置 / Build/tooling/config
- **ci**: CI/CD配置 / CI/CD configuration

### 📖 示例

**✅ 正确示例**：
```bash
# 简单提交
git commit -m "feat: 添加静音过滤功能 / Add silence filtering feature"

# 复杂提交（带详细说明）
git commit -m "feat: 实现边缘裁切算法 / Implement edge trimming algorithm

- 三态状态机（Leading/Passing/Trailing）
- 迟滞机制防止弱音误判
- O(N)时间复杂度，O(min_run)空间复杂度

- Three-state machine (Leading/Passing/Trailing)
- Hysteresis mechanism to prevent weak sound misjudgment
- O(N) time complexity, O(min_run) space complexity"

# 多项改动
git commit -m "refactor: 重构DR计算逻辑并优化性能 / Refactor DR calculation logic and optimize performance

- 使用constants.rs统一管理常量
- 减少15%内存占用
- 提升10%计算速度

- Unify constants management with constants.rs
- Reduce memory usage by 15%
- Improve calculation speed by 10%"
```

**❌ 错误示例**：
```bash
# 纯中文（不符合国际规范）
git commit -m "feat: 添加静音过滤功能"

# 纯英文（缺少中文说明）
git commit -m "feat: Add silence filtering feature"

# 格式错误（未使用斜杠分隔）
git commit -m "feat: 添加静音过滤功能 Add silence filtering feature"
```

### 💡 实用技巧

**使用heredoc避免引号转义**：
```bash
git commit -m "$(cat <<'EOF'
feat: 优化SIMD转换性能 / Optimize SIMD conversion performance

- 使用ARM NEON指令集加速
- 处理速度提升30%

- Accelerate with ARM NEON instruction set
- Improve processing speed by 30%
EOF
)"
```

**查看最近提交格式参考**：
```bash
git log --oneline -10  # 查看简短历史
git log -3 --format="%h %s%n%b"  # 查看详细历史
```

---

## 核心架构

**4层模块化设计** + **2条性能路径**：

### 模块分层
- **tools/**: CLI、格式化输出、文件扫描
  - `constants.rs`: **常量集中管理**（所有魔法数字的唯一来源）
  - `cli.rs`: 命令行参数解析（包含实验性功能开关）
  - `formatter.rs`: 输出格式化（单文件/批量报告）
  - `scanner.rs`: 文件系统扫描和过滤
- **core/**: DR算法引擎（DrCalculator + WindowRmsAnalyzer）
  - `dr_calculator.rs`: DR计算核心逻辑
  - `histogram.rs`: 窗口RMS直方图（20%采样）
  - `peak_selection.rs`: O(n)峰值选择算法
- **processing/**: SIMD优化和音频处理
  - `simd_core.rs`: SIMD基础设施（SimdProcessor + SimdCapabilities）
  - `sample_conversion.rs`: 样本格式转换（i16/i24/i32→f32）
  - `channel_separator.rs`: 声道样本分离引擎
  - `dr_channel_state.rs`: DR计算状态（24字节内存布局）
  - `processing_coordinator.rs`: 协调器（编排各服务）
  - `performance_metrics.rs`: 性能统计
  - `edge_trimmer.rs`: **实验性边缘裁切**（P0样本级实现）
- **audio/**: 解码器（串行BatchPacketReader + 并行OrderedParallelDecoder）
  - `universal_decoder.rs`: 解码器统一入口（自动选择串行/并行）
  - `streaming.rs`: 流式处理接口定义
  - `parallel_decoder.rs`: 并行解码实现（OrderedParallelDecoder）
  - `opus_decoder.rs`: Opus专用解码器（songbird库）

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
5. **实验性增强**:
   - 静音过滤：过滤低于阈值的样本（防止静音虚高DR）
   - 边缘裁切：三态状态机实现首尾静音裁切（保留中段艺术静音）
   - DR边界预警：检测接近X.5四舍五入边界的DR值（如10.45-10.55），标识可能因±0.02-0.05精度差异导致的级别跨越

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

# SIMD性能测试（需显式启用feature，默认跳过编译）
# 避免CI链接器资源耗尽，本地开发专用
cargo test --features simd-perf-tests --test simd_performance_tests

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

**1. 已标记为 `#[ignore]` 的慢速测试**：

这些测试在Debug模式下运行极慢，已标记为忽略以避免CI超时。包括：
- 内存完整性验证
- 并发解码器高负载测试

**使用建议**：
- 常规开发：运行 `cargo test`（自动跳过慢速测试）
- 性能验证：运行 `cargo test --release -- --ignored`（Release模式执行慢速测试）

**2. SIMD性能测试（Feature门控）**：

`tests/simd_performance_tests.rs` 包含大数据集测试（最大25M样本），可能导致CI链接器资源耗尽。

**解决方案**：使用 Cargo feature `simd-perf-tests` 控制编译
- **CI环境**：默认跳过编译（0 tests），避免链接器崩溃
- **本地开发**：显式启用 feature 运行完整性能测试

**运行方式**：
```bash
# 本地性能测试（9个测试，包含大数据集）
cargo test --features simd-perf-tests --test simd_performance_tests

# Release模式（获得真实性能数据）
cargo test --release --features simd-perf-tests --test simd_performance_tests
```

**设计原则**：
- 默认禁用大数据集测试，保护CI环境
- 开发者可按需启用完整性能验证
- 遵循Rust生态的feature flag最佳实践

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

### 为什么需要constants.rs集中管理常量？
**问题**: 为何不能在各模块中直接定义常量？

**答案**: 防止"默认值漂移"和重复定义：
- **漂移问题**: 不同文件中相同语义的常量可能不一致（如批大小64 vs 128）
- **维护成本**: 调整参数需要修改多处，容易遗漏
- **文档缺失**: 散落的魔法数字缺少设计考量说明
- **统一来源**: constants.rs作为"单一事实来源"，所有常量都有详细文档
- **结论**: 集中管理提升一致性、可维护性和可审计性

### 实验性功能的设计原则是什么？
**问题**: 边缘裁切和静音过滤为何标记为"实验性"？

**答案**: 平衡创新与foobar2000兼容性：
- **默认关闭**: 保持与foobar2000的100%结果一致性
- **显式激活**: 用户通过CLI标志主动启用（`--trim-edges`、`--filter-silence`）
- **独立开关**: 每个功能独立控制，互不影响
- **风险标识**: 🧪标记提醒用户这些功能可能改变DR值
- **渐进稳定**: 实验验证后可能移除标记，成为稳定特性
- **结论**: 允许探索新功能，同时保护核心精度保证

# important-instruction-reminders
Do what has been asked; nothing more, nothing less.
NEVER create files unless they're absolutely necessary for achieving your goal.
ALWAYS prefer editing an existing file to creating a new one.
NEVER proactively create documentation files (*.md) or README files. Only create documentation files if explicitly requested by the User.


      IMPORTANT: this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant to your task.
