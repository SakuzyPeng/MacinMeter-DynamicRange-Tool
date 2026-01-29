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

# 运行单个测试
cargo test <test_name>
cargo test test_dr_calculation  # 示例

# Release模式慢速测试
cargo test --release -- --ignored

# SIMD性能测试（Feature门控）
cargo test --features simd-perf-tests --test simd_performance_tests
```

### 代码质量检查

```bash
# 完整检查（推荐）
cargo fmt --check && cargo clippy -- -D warnings && cargo check && cargo audit && cargo test

# 快速检查
cargo check

# 发布检查
cargo build --release && cargo test --release
```

### 性能基准测试

```bash
# 10次取平均值，消除系统噪声
./scripts/benchmark_10x.sh              # macOS
./scripts/benchmark-10x.ps1             # Windows
```

**详细CLI参数和使用示例**：参见README.md

### Tauri GUI

```bash
cd tauri-app

# 安装依赖（首次）
npm install

# 开发模式
npm run tauri dev

# 构建发行版
npm run tauri build
```

GUI 直接复用 CLI 的 DR 计算引擎，详见 `docs/tauri_wrapper.md`。

---

## 核心架构

**4层模块化设计** + **2条性能路径**：

### 模块分层
- **tauri-app/**: Tauri 2 GUI 包装层
  - `src/`: 前端 UI（Vite + TypeScript）
  - `src-tauri/`: Rust 后端命令，调用 `macinmeter_dr_tool::analyze_file`
  - 三个核心命令：`analyze_audio`、`scan_audio_directory`、`load_app_metadata`
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
  - `edge_trimmer.rs`: **实验性边缘裁切**（样本级实现）
- **audio/**: 解码器（串行BatchPacketReader + 并行OrderedParallelDecoder）
  - `universal_decoder.rs`: 解码器统一入口（自动选择串行/并行/FFmpeg）
  - `streaming.rs`: 流式处理接口定义
  - `parallel_decoder.rs`: 并行解码实现（OrderedParallelDecoder）
  - `opus_decoder.rs`: Opus专用解码器（songbird库）
  - `ffmpeg_bridge.rs`: FFmpeg桥接（DSD、格式回退）

### 双路径架构（关键设计）

**串行路径**（UniversalStreamProcessor）：
- BatchPacketReader：减少99%系统调用的I/O优化
- 单Decoder：直接解码，零通信开销
- 适用场景：单文件处理、低并发

**并行路径**（ParallelUniversalStreamProcessor）：
- OrderedParallelDecoder：4线程64包批量解码
- SequencedChannel：序列号保证样本时间顺序
- 性能提升：累积 3.3倍（详见README.md性能数据）
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

## 格式处理特殊规则

**MP3 - 强制串行解码**：
- MP3是有状态编码格式，并行解码会丢失packet间的状态连续性
- 自动降级到串行解码器

**DSD - FFmpeg桥接**：
- 通过FFmpeg降采样到PCM处理（Symphonia不支持DSD解码）
- 参数：`--dsd-pcm-rate`（默认352.8kHz）、`--dsd-gain-db`（默认+6dB）、`--dsd-filter`（默认teac）

**多声道 - 零拷贝优化**：
- 3+声道使用process_samples_strided单次遍历处理交错样本
- 性能收益：7.1(8ch)→8倍，7.1.4(12ch)→12倍，9.1.6(16ch)→16倍
- 1-2声道保持SIMD分离路径

**Opus - 专用解码器**：
- 使用songbird库（Discord音频库）专用解码

**其他格式**：
- 通过Symphonia统一处理（FLAC、WAV、AAC、OGG等）
- 必要时自动回退到FFmpeg（详见README.md "支持的音频格式"）

---

## 实验性功能设计原则

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

## 开发规范

### 零警告原则

**每次代码修改后必须立即检查和清理编译警告！**

**零警告标准**：
- **dead_code**: 及时删除未使用的函数、结构体、常量和变量
- **unused_variables**: 优先确认变量是否可以安全删除，能删则删
- **unused_imports**: 清理多余的import语句
- **missing_docs**: 为所有 public API 添加文档注释
- **clippy::all**: 遵循Clippy的所有最佳实践建议

### 常量管理：src/tools/constants.rs

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
- 每个常量必须附带详细文档注释
- 相关常量分组到同一模块

**示例**：
```rust
use crate::tools::constants::{dr_analysis, decoder_performance, defaults};

let window_duration = dr_analysis::WINDOW_DURATION_SECONDS;
let batch_size = decoder_performance::PARALLEL_DECODE_BATCH_SIZE;
let threads = defaults::PARALLEL_THREADS;
```

### 双语化规范

**核心原则**：本项目面向国际开发者，所有用户可见内容必须采用双语格式（中文+英文）

**必须双语化的内容**：
- `println!()` / `eprintln!()` - 标准输出和错误输出
- CLI参数的 `.help()` 文本
- 错误消息（`AudioError` 的 `Display` 实现）
- Git提交信息

**标准格式**：
```rust
println!("Results saved / 结果已保存到: {}", path.display());
eprintln!("[ERROR] File not found / 文件未找到: {}", path);
.help("Audio file or directory path / 音频文件或目录路径")
```

**Git提交信息格式**：
```
<type>: <中文简要描述> / <English brief description>
```

**Type类型**：feat、fix、refactor、perf、docs、test、chore、ci

**不需要双语化**：代码注释（仅中文）、内部日志

### 代码风格

- **注释语言**：仅使用中文
- **禁止emoji**：代码注释和打印语句中不使用emoji
- 代码通过 `cargo fmt` 格式化
- 代码通过 `cargo clippy -- -D warnings` 检查

### 预提交钩子

自动执行：代码格式检查、Clippy分析、编译检查、单元测试、安全审计。

**正常提交流程**：
```bash
git commit -m "feat: 你的提交信息 / Your commit message"
# 预提交钩子会自动运行，等待约3-5分钟完成所有检查
```

**如果提交长时间无响应**（超过5分钟），手动运行检查：
```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo check && cargo audit && cargo test
```

**禁止操作**：
- `git commit --no-verify` - 跳过预提交检查
- 将提交命令放在后台运行

---

## 测试策略

```bash
# 单元测试
cargo test --lib

# 完整测试（跳过慢速性能测试）
cargo test

# 慢速性能测试（Release模式）
cargo test --release -- --ignored

# SIMD性能测试（本地开发专用）
cargo test --features simd-perf-tests --test simd_performance_tests
```

### 慢速测试说明

**已标记为 `#[ignore]` 的测试**：
- 内存完整性验证
- 并发解码器高负载测试

**使用建议**：
- 常规开发：`cargo test`（自动跳过）
- 性能验证：`cargo test --release -- --ignored`

**SIMD性能测试**：使用 Cargo feature `simd-perf-tests` 控制编译，避免CI链接器资源耗尽。

---

## 重要架构决策记录

### 为什么保持串行和并行两条路径？
**答案**: 串行≠并发度1的并行：
- **串行**（BatchPacketReader）：零通信开销，直接VecDeque缓冲
- **并行度1**（OrderedParallelDecoder）：仍有channel/HashMap/序列号开销
- **结论**: 保持两条独立路径，用ProcessorState消除重复

### 为什么MP3必须串行解码？
MP3是有状态编码格式，每个packet的解码依赖前一个packet的decoder状态。并行解码会创建独立decoder，丢失packet间的状态连续性导致样本错误。

### 为什么DSD需要FFmpeg桥接？
Symphonia不支持DSD解码。FFmpeg提供成熟的DSD→PCM转换，降采样到标准PCM后走统一DR计算路径。

### 为什么多声道采用零拷贝跨步处理？
- **内存问题**: 传统方法需要N次extract创建N个Vec，对7.1(8ch)文件产生8倍内存占用
- **CPU效率**: 单次遍历vs N次遍历，显著提升缓存局部性
- **混合策略**: 1-2声道保持ProcessingCoordinator路径，享受SIMD分离优势

### 为什么LFE剔除需要元数据支持？
无元数据时无法确定哪个物理声道是LFE（5.1中可能在index 3/4/5），不同容器对声道顺序定义不同。宁可保守不剔除，避免误删非LFE声道。

### 为什么静音声道使用DR_ZERO_EPS阈值？
解码器可能产生近零噪声（1e-15），浮点运算有累积误差。阈值1e-12足够严格（远小于16bit量化噪声3e-5）且容忍合理误差。

---

## 发布流程

### 自动发布（推送 Tag）

```bash
git tag v0.1.x
git push origin v0.1.x
```

CI 会自动构建并创建 Release（需要代码变更触发构建）。

### 手动发布（从 Actions 产物）

```bash
# 1. 下载最新成功构建的产物
gh run list --limit 5                    # 查看最近的 workflow runs
gh run download <run-id> --dir artifacts # 下载产物

# 2. 准备发布文件（解压 .gz，重命名，打包 .zip）
cd artifacts
# CLI: gunzip → chmod +x → zip
# GUI: 直接使用 .dmg / .exe

# 3. 创建 Release
gh release create v0.1.x \
  --title "v0.1.x – 标题" \
  --notes-file release-notes.md \
  file1.zip file2.zip ...
```

### 发布资产命名规范

- CLI: `MacinMeter-DR-Tool-v{版本}-{平台}.zip`
  - 平台: `windows-x64`, `macos-intel`, `macos-arm64`, `linux-x64`
- GUI: `MacinMeter-DR-GUI-v{版本}-{平台}.{ext}`
  - macOS: `.dmg`, Windows: `.exe`

### Release Notes 模板

参考 RELEASE_NOTES.md 对应版本章节，包含：
- CLI / 命令行变更
- GUI / 图形界面变更
- 平台产物列表
- macOS 未签名提示

---

## 文档维护策略

### 文档职责分工

| 文档 | 职责 | 读者 | 更新时机 |
|------|------|------|----------|
| **CLAUDE.md** | 架构约束、设计原则、Why&How | AI助手 | 架构变更、新增设计原则 |
| **README.md** | 功能列表、CLI参数、性能数据、使用指南 | 用户 | 每次发布前 |
| **RELEASE_NOTES.md** | 版本历史、新功能、Breaking Changes | 用户 | 每次发布时 |
| **changelogs/** | 重要修复和改进的详细技术分析 | 开发者 | 重大变更时 |

### CLAUDE.md 更新原则

**应该更新**：
- 核心架构变化、新设计原则、重要架构决策、开发规范调整

**不应该更新**：
- 新CLI参数、新音频格式支持、性能数据变化、Bug修复

### Changelog规范

**文件命名**: `CHANGELOG_v{版本号}_{YYYY-MM-DD-HH-MM}.md`

**内容结构**：
1. 问题描述（症状、根本原因）
2. 修复方案（代码变更、逻辑说明）
3. 验证结果（测试数据、精度分析）
4. 技术细节（公式、误差来源）

---

# important-instruction-reminders
Do what has been asked; nothing more, nothing less.
NEVER create files unless they're absolutely necessary for achieving your goal.
ALWAYS prefer editing an existing file to creating a new one.
NEVER proactively create documentation files (*.md) or README files. Only create documentation files if explicitly requested by the User.
