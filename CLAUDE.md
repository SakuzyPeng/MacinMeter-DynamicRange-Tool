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
- **精度原则**: 与foobar2000结果通常在±0.02-0.05 dB内，接近X.5边界时四舍五入可能相差1级；部分歌曲可能存在~0.1 dB的精度偏差
- **性能参考**: 参见README.md中M4 Pro和i9-13900H的基准数据

---

## 项目概述

MacinMeter DR Tool 是基于foobar2000 DR Meter逆向分析的音频动态范围(DR)分析工具，使用Rust实现。

**核心特性**：
- 完全流式处理，零内存累积（~45MB恒定内存）
- 双路径架构（串行/并行）
- SIMD优化 + 多声道零拷贝
- 实验性增强功能（默认关闭，保持foobar2000兼容）

**详细信息请参考**：
- **功能和CLI参数**：README.md "快速开始"和"常用选项"章节
- **版本更新历史**：RELEASE_NOTES.md
- **性能基准数据**：README.md "性能建议"章节
- **当前版本**：v0.1.0（首个稳定发布）

---

## 核心架构约束

### 格式处理特殊规则

**MP3 - 强制串行解码**：
- MP3是有状态编码格式，并行解码会丢失packet间的状态连续性
- 自动降级到串行解码器（详见"架构决策记录"）

**DSD - FFmpeg桥接**：
- 通过FFmpeg降采样到PCM处理（Symphonia不支持DSD解码）
- 参数：`--dsd-pcm-rate`（默认352.8kHz）、`--dsd-gain-db`（默认+6dB）、`--dsd-filter`（默认teac）
- 报告显示："原生1-bit采样率 → 处理采样率（DSD downsampled）"

**多声道 - 零拷贝优化**：
- 3+声道使用process_samples_strided单次遍历处理交错样本
- 性能收益：7.1(8ch)→8倍，7.1.4(12ch)→12倍，9.1.6(16ch)→16倍
- 1-2声道保持SIMD分离路径

**Opus - 专用解码器**：
- 使用songbird库（Discord音频库）专用解码

**其他格式**：
- 通过Symphonia统一处理（FLAC、WAV、AAC、OGG等）
- 必要时自动回退到FFmpeg（详见README.md "支持的音频格式"）

### 实验性功能设计原则

**设计目标**：平衡创新与foobar2000兼容性

**核心原则**：
- **默认关闭**：保持与foobar2000的100%结果一致性
- **显式激活**：通过CLI标志主动启用（`--filter-silence`、`--trim-edges`、`--exclude-lfe`）
- **独立开关**：每个功能独立控制，互不影响
- **边界预警**：检测DR值接近X.5边界，提醒交叉验证
- **报告标注**：在输出中明确标识已启用的实验性功能及统计信息

**添加新实验性功能的流程**：
1. 在`AppConfig`中添加字段（默认值保持兼容）
2. 在`cli.rs`中添加参数（使用`[EXPERIMENTAL]`标记）
3. 在`ProcessorState`中传递配置
4. 在报告中标注功能已启用及统计信息
5. 经过充分验证后可移除实验标记

**现有实验性功能**（详细参数见README.md）：
- 静音过滤（`--filter-silence`）：窗口级样本过滤
- 边缘裁切（`--trim-edges`）：样本级首尾静音裁切
- LFE剔除（`--exclude-lfe`）：从DR聚合中剔除LFE声道

---

## 构建和运行命令

### 核心命令

```bash
# 构建Release版本
cargo build --release

# 运行工具
./target/release/MacinMeter-DynamicRange-Tool-foo_dr <path>

# 完整测试
cargo test

# Release模式慢速测试
cargo test --release -- --ignored

# SIMD性能测试（Feature门控）
cargo test --features simd-perf-tests --test simd_performance_tests
```

**详细CLI参数和使用示例**：参见README.md

---

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

**功能说明**：
自动执行：代码格式检查、Clippy分析、编译检查、单元测试、安全审计。所有检查必须通过才能提交。

**环境信息**：
- Docker镜像：`macinmeter-ci:standalone`（x86架构）
- 用于标准化的CI和本地测试环境

**⚠️ 重要操作提醒**：

1. **正常提交流程**：
   ```bash
   git commit -m "feat: 你的提交信息 / Your commit message"
   # 预提交钩子会自动运行，等待约3-5分钟完成所有检查
   ```

2. **如果提交长时间无响应**（超过5分钟）：
   - **不要使用 `--no-verify` 跳过检查**（会浪费CI资源）
   - **不要将提交放在后台运行**（无法及时发现错误）
   - 应该手动运行预提交脚本检查问题：
     ```bash
     # 中断当前提交 (Ctrl+C)
     # 手动运行完整检查
     cargo fmt --check && cargo clippy -- -D warnings && cargo check && cargo audit && cargo test
     ```

3. **发现预提交脚本问题时**：
   - 优先考虑完善脚本问题
   - 确认脚本正常后再提交代码
   - 避免绕过检查导致CI失败

4. **禁止操作**：
   - ❌ `git commit --no-verify` - 跳过预提交检查
   - ❌ 将提交命令放在后台运行
   - ❌ 不等待检查完成就强制提交

**检查超时处理**：
- 预提交钩子建议5分钟内完成
- 超时时先手动运行检查命令定位问题
- 解决问题后重新提交

---

## 🌍 双语化规范 (Bilingualization)

**核心原则**：本项目面向国际开发者，所有用户可见内容必须采用双语格式（中文+英文）

### 1. 用户可见输出

**必须双语化的内容**：
- `println!()` / `eprintln!()` - 标准输出和错误输出
- CLI参数的 `.help()` 文本
- 错误消息（`AudioError` 的 `Display` 实现）
- 错误分类显示名称（`ErrorCategory::display_name()`）
- 参数验证错误消息

**标准格式**：

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

**CLI帮助文本格式**：

```rust
.help("Audio file or directory path / 音频文件或目录路径 (支持WAV, FLAC等)")
.help("Show detailed processing information / 显示详细处理信息")
.help("[EXPERIMENTAL] Enable silence filtering / 启用静音过滤")
```

**错误消息格式**：

```rust
// AudioError Display 实现
AudioError::InvalidInput(msg) => {
    write!(f, "输入验证失败 / Input validation failed: {msg}")
}

// ErrorCategory 显示名称
ErrorCategory::Format => "FORMAT/格式错误",
ErrorCategory::Decoding => "DECODING/解码错误",
```

**参数验证错误**：

```rust
// parse函数返回的错误消息
Err(format!("'{s}' is not a valid number / 不是有效的数字"))
Err(format!("value must be at least {min} / 值必须至少为 {min}"))
```

### 2. Git提交信息

**强制要求**：所有commit message必须采用双语格式（中文+英文）

**格式规范**：

```
<type>: <中文简要描述> / <English brief description>

[可选详细说明 / Optional detailed description]
```

**Type类型**：

- **feat**: 新功能 / New feature
- **fix**: Bug修复 / Bug fix
- **refactor**: 重构代码 / Code refactoring
- **perf**: 性能优化 / Performance optimization
- **docs**: 文档更新 / Documentation update
- **test**: 测试相关 / Test related
- **chore**: 构建/工具/配置 / Build/tooling/config
- **ci**: CI/CD配置 / CI/CD configuration

**提交示例**：

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

**实用技巧**：

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

### 3. 例外说明

**不需要双语化的内容**：
- **代码注释**：仅使用中文即可，面向开发者
- **内部日志**：调试和开发用的日志信息
- **文档内容**：CLAUDE.md、README.md等文档本身的正文内容（已经是双语）

---

## 📐 代码风格规范

### 💬 注释规范

**注释只需保留中文，不需要双语化**

**基本原则**：
- 注释面向开发者，使用中文即可
- 去除所有emoji符号（🚀 🎯 🎵 📄 ⌛ 🧪 🏁 等）
- 避免模糊的阶段描述

**注释示例**：

```rust
// 创建边缘裁切器（如果启用）
// 智能缓冲流式处理：积累chunk到标准窗口大小，保持算法精度

// 首尾边缘裁切（如果启用）
// 窗口对齐优化禁用时，仅使用compact阈值机制
```

**长注释格式**：

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

### ✅ 代码清理检查清单

在提交代码前，确保完成以下检查：

**双语化检查**：
- [ ] 所有 `println!`/`eprintln!` 语句已双语化
- [ ] CLI 参数的 `.help()` 文本已双语化
- [ ] `AudioError` 的 `Display` 实现已双语化
- [ ] 错误分类显示名称已双语化
- [ ] 参数验证错误消息已双语化
- [ ] Git提交信息已双语化

**代码风格检查**：
- [ ] 所有注释已去除emoji符号
- [ ] 注释中没有模糊的阶段描述（如"P0"、"阶段X"）
- [ ] 注释使用清晰、具体的中文描述
- [ ] 代码通过 `cargo fmt` 格式化
- [ ] 代码通过 `cargo clippy -- -D warnings` 检查
- [ ] 所有测试通过 `cargo test`

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
  - `edge_trimmer.rs`: **实验性边缘裁切**（样本级实现）
- **audio/**: 解码器（串行BatchPacketReader + 并行OrderedParallelDecoder）
  - `universal_decoder.rs`: 解码器统一入口（自动选择串行/并行/FFmpeg）
  - `streaming.rs`: 流式处理接口定义
  - `parallel_decoder.rs`: 并行解码实现（OrderedParallelDecoder）
  - `opus_decoder.rs`: Opus专用解码器（songbird库）
  - `ffmpeg_bridge.rs`: FFmpeg桥接（DSD、格式回退）

### 🚀 双路径架构（关键设计）

**串行路径**（UniversalStreamProcessor）：
- BatchPacketReader：减少99%系统调用的I/O优化
- 单Decoder：直接解码，零通信开销
- 适用场景：单文件处理、低并发

**并行路径**（ParallelUniversalStreamProcessor）：
- OrderedParallelDecoder：4线程64包批量解码
- SequencedChannel：序列号保证样本时间顺序
- 性能提升：累积 3.3倍（115MB/s → 705MB/s，2025-10-25 基准数据）
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

**详细性能数据**：参见README.md "性能建议"章节

---

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
./scripts/benchmark_10x.sh              # macOS
./scripts/benchmark-10x.ps1             # Windows
```

### 📊 性能基准脚本说明

**benchmark_10x.sh / benchmark-10x.ps1**: 10次重复测试取统计数据
- **功能**: 运行10次基准测试，计算平均值、中位数、标准差
- **输出指标**: 运行时间、处理速度、内存峰值、CPU使用率
- **用途**: 消除系统缓存/调度的随机性，获得可靠的性能数据

**推荐工作流**:
```bash
# 1. 性能优化后，重新构建Release版本
cargo build --release

# 2. 运行10x基准测试（生成当前版本性能数据）
./scripts/benchmark_10x.sh

# 3. 观察指标：看中位数而非平均值（更稳定）
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

---

## 重要架构决策记录

### 为什么保持串行和并行两条路径？
**问题**: 能否用DecoderMode enum统一串行和并行？

**答案**: **不能**。串行≠并发度1的并行：
- **串行**（BatchPacketReader）：零通信开销，直接VecDeque缓冲
- **并行度1**（OrderedParallelDecoder）：仍有channel/HashMap/序列号开销，但无并行收益
- **结论**: 保持两条独立路径，用ProcessorState消除重复

### 为什么MP3必须串行解码？
MP3是有状态编码格式，每个packet的解码依赖前一个packet的decoder状态。并行解码会创建独立decoder，丢失packet间的状态连续性导致样本错误。因此通过文件扩展名检测，自动降级到串行解码器。

### 为什么DSD需要FFmpeg桥接？
Symphonia不支持DSD解码。FFmpeg提供成熟的DSD→PCM转换，降采样到标准PCM后走统一DR计算路径，无需特殊处理。参数：`--dsd-pcm-rate`/`--dsd-gain-db`/`--dsd-filter`（详见README.md）。

### 实验性功能的设计原则是什么？
**问题**: 边缘裁切和静音过滤为何标记为"实验性"？

**答案**: 平衡创新与foobar2000兼容性：
- **默认关闭**: 保持与foobar2000的100%结果一致性
- **显式激活**: 用户通过CLI标志主动启用（`--trim-edges`、`--filter-silence`）
- **独立开关**: 每个功能独立控制，互不影响
- **风险标识**: 🧪标记提醒用户这些功能可能改变DR值
- **渐进稳定**: 实验验证后可能移除标记，成为稳定特性
- **结论**: 允许探索新功能，同时保护核心精度保证

### 为什么多声道采用零拷贝跨步处理？
**问题**: 为何3+声道使用process_samples_strided而非传统的extract+process？

**答案**: 性能和内存优化的关键设计：
- **内存问题**: 传统方法需要N次extract创建N个Vec，对7.1(8ch)文件产生8倍内存占用
- **CPU效率**: 单次遍历vs N次遍历，显著提升缓存局部性
- **性能收益**: 7.1→8倍，7.1.4→12倍，9.1.6→16倍内存和CPU效率提升
- **混合策略**: 1-2声道保持ProcessingCoordinator路径，享受SIMD分离优势
- **结论**: 零拷贝是多声道高性能处理的必选路径

### 为什么LFE剔除需要元数据支持？
无元数据时无法确定哪个物理声道是LFE（5.1中可能在index 3/4/5），不同容器对声道顺序定义不同。宁可保守不剔除，避免误删非LFE声道。无元数据时报告会提示用户。

### 为什么静音声道使用DR_ZERO_EPS阈值？
解码器可能产生近零噪声（1e-15），浮点运算有累积误差。阈值1e-12足够严格（远小于16bit量化噪声3e-5）且容忍合理误差，是处理实际音频数据的工业级实践

---

## 文档维护策略

### 文档职责分工

| 文档 | 职责 | 读者 | 更新时机 |
|------|------|------|----------|
| **CLAUDE.md** | 架构约束、设计原则、Why&How | AI助手 | 架构变更、新增设计原则 |
| **README.md** | 功能列表、CLI参数、性能数据、使用指南 | 用户 | 每次发布前 |
| **RELEASE_NOTES.md** | 版本历史、新功能、Breaking Changes | 用户 | 每次发布时 |
| **changelogs/** | 重要修复和改进的详细技术分析 | 开发者 | 重大变更时 |
| **代码注释** | 具体实现细节、算法说明 | 开发者 | 代码变更时 |

### ✅ CLAUDE.md 更新原则

**应该更新的情况**：
- 核心架构发生变化（如新增第三条解码路径）
- 添加新的设计原则或约束（如新的格式处理规则）
- 重要架构决策需要记录（添加到"架构决策记录"章节）
- 开发规范调整（如新的代码风格要求）

**不应该更新的情况**：
- 添加新的CLI参数（应更新README.md）
- 支持新的音频格式（应更新README.md）
- 性能数据变化（应更新README.md）
- Bug修复（应更新RELEASE_NOTES.md）
- 实验性功能的具体参数调整（应更新README.md）

### 性能数据更新流程

**每次发布前**：
1. 运行性能基准测试：`./scripts/benchmark_10x.sh`或`./scripts/benchmark-10x.ps1`
2. 将结果更新到README.md "性能建议"章节
3. 在RELEASE_NOTES.md中记录性能变化
4. CLAUDE.md仅通过引用指向README.md，不硬编码性能数据

### Changelog规范（changelogs/文件夹）

**目的**: 记录重大修复和改进的详细技术分析，供开发者参考

**文件命名**: `CHANGELOG_v{版本号}_{YYYY-MM-DD-HH-MM}.md`

**示例**: `CHANGELOG_v0.1.1_2025-11-08-14-20.md`

**内容结构**（按顺序）:

1. **标题和版本信息**
   ```markdown
   # Changelog v0.1.1 (2025-11-08-14-20)

   ## 修复：问题简述 / Fix: Brief Issue Description
   ```

2. **问题描述 / Problem Description**
   - 错误现象 / Symptom: 具体症状和测试数据
   - 根本原因 / Root Cause: 代码逻辑错误的分析

3. **修复方案 / Fix Solution**
   - 代码变更: 修改的文件和具体代码
   - 逻辑重构: 需要说明"为什么"不仅仅是"做了什么"

4. **验证结果 / Verification Results**
   - 多个测试场景的对比结果
   - 与参考实现（如foobar2000）的对比数据
   - 精度分析和误差范围说明

5. **技术细节 / Technical Details**
   - 公式和计算方法
   - 误差来源分析
   - 性能影响评估

**格式要求**:
- 删除所有emoji符号（改用文本标记）
- 使用markdown表格展示数据对比
- 代码块用```rust或```bash标记
- 中英双语说明
- 逻辑清晰，便于理解修复的"为什么"

**示例差异对比表**:
```markdown
| 工具 | 比特率 | 差异 |
|------|--------|------|
| MacinMeter (修复后) | 5561 kbps | - |
| foobar2000 v1.0.3 | 5558 kbps | +3 kbps (+0.05%) |
| ffprobe | 5560 kbps | +1 kbps (+0.02%) |
```

**不包含内容**:
- 不写"下一步计划"（规划性内容）
- 不写"参考资料"（链接和引用）
- 不写"质量保证"和"Git提交记录"细节（属于开发过程）
- 不写"影响范围"（属于RELEASE_NOTES职责）

### 🔄 版本发布检查清单

发布新版本前，确保完成：

- [ ] 运行完整测试：`cargo test`和`cargo test --release -- --ignored`
- [ ] 更新README.md中的性能基准数据
- [ ] 在RELEASE_NOTES.md中添加版本记录
- [ ] 检查CLAUDE.md是否需要更新架构决策
- [ ] 确认所有代码通过`cargo clippy -- -D warnings`
- [ ] 确认所有commit message符合双语化规范
- [ ] 更新Cargo.toml中的版本号

---

# important-instruction-reminders
Do what has been asked; nothing more, nothing less.
NEVER create files unless they're absolutely necessary for achieving your goal.
ALWAYS prefer editing an existing file to creating a new one.
NEVER proactively create documentation files (*.md) or README files. Only create documentation files if explicitly requested by the User.
