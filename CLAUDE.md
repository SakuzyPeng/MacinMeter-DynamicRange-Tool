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

### 🎵 音频格式支持 (2025年最新)

**通过Symphonia支持**：
- **无损格式**: FLAC, ALAC (Apple Lossless), WAV, AIFF, PCM (AU, CAF等)
- **有损格式**: MP3, MP1 (MPEG Layer I), AAC, OGG Vorbis
- **容器格式**: MP4/M4A, MKV/WebM

**专用解码器**：
- **Opus**: 通过songbird专用解码器 (Discord音频库)
- **WAV**: 通过hound库额外支持

**总计支持格式**: 12+种主流音频格式，覆盖90%+用户需求

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

该项目采用严格的模块化架构，基于foobar2000 DR Meter的逆向工程分析：

### 核心架构

**4层模块化设计**:
- **tools/**: UI和工具层 - 命令行接口、格式化输出、文件处理
- **core/**: DR计算核心 - 算法引擎、RMS分析、峰值策略
- **processing/**: 性能优化层 - SIMD加速、声道分离、协调器
- **audio/**: 音频解码层 - 通用解码器、流式处理、格式支持

### 🔥 流式架构特性

**零内存累积处理**:
- 恒定~50MB内存使用，支持任意大小文件(1MB→10GB+)
- SIMD优化：立体声SSE2/NEON分离，单声道零开销直通
- 智能缓冲：3秒标准窗口，与foobar2000算法完全对齐

### 核心算法

1. **20%采样算法**: 从窗口RMS值中选择最响20%计算DR
2. **峰值选择策略**: 4种策略(PreferSecondary/ClippingAware/AlwaysPrimary/AlwaysSecondary)
3. **SIMD优化**: SSE2/NEON向量化，4样本并行处理
4. **双峰值系统**: 主Peak失效时智能切换到次Peak

## 关键API

**DrCalculator主要方法**:
```rust
// 构造函数
DrCalculator::new(channel_count: usize, sum_doubling: bool, sample_rate: u32, block_duration: f64)

// 主计算方法
calculate_dr_from_samples(&self, samples: &[f32], channel_count: usize) -> Vec<DrResult>

// 流式处理
process_decoder_chunk(&mut self, chunk_samples: &[f32], channels: usize)
```

**核心数据结构**:
```rust
pub struct DrResult {
    pub dr_value: f64,        // DR值
    pub rms: f64,            // RMS值
    pub peak: f64,           // 选中的峰值
    pub primary_peak: f64,   // 主峰
    pub secondary_peak: f64, // 次峰
}
```

---

## 开发原则

### 🎯 声道支持边界
- **支持**: 单声道(1)和立体声(2)，SIMD优化
- **拒绝**: 3+声道（友好错误提示）

### 💎 性能优先
- 默认启用所有优化：SIMD、多线程、Sum Doubling
- 零配置原则：智能默认值，自动检测最优策略

### 🔍 代码质量
- 删除未使用参数，不要简单加下划线
- 方法命名要诚实反映实际功能
- 统一API设计，避免向后兼容混乱

## 测试指引

### 关键测试命令
```bash
# 核心模块测试
cargo test core::dr_calculator::tests
cargo test processing::channel_extractor::tests
cargo test --release simd_precision_test

# 性能基准测试
cargo test --release benchmark_streaming -- --nocapture
```

### 测试数据要求
- **Peak值 >> 20%RMS值**: 确保算法不会出现RMS > Peak错误
- **足够的小信号**: 降低20%采样的RMS基准
- **次Peak验证**: foobar2000优先选择次Peak

---

## 🔌 foobar2000插件

位于 `foobar2000_plugin/` 目录，100%复用主项目DR算法。

### 架构设计
- **UI层**: 右键菜单 + 结果显示窗口
- **控制器层**: DrAnalysisController (业务编排)
- **服务层**: AudioAccessor (foobar2000解码)
- **FFI层**: rust_bridge + rust_core (C++↔Rust接口)

### 构建使用
```bash
# 构建插件
cd foobar2000_plugin && mkdir build && cd build
cmake .. -DCMAKE_BUILD_TYPE=Release
cmake --build . --config Release

# 安装使用
# 1. 拖入 foo_dr_macinmeter.fb2k-component 到foobar2000
# 2. 右键音频文件 → "Analyze Dynamic Range"
```

### 核心特性
- ✅ 1-2声道支持，3+声道友好拒绝
- ✅ 零重复代码，100%复用主项目算法
- ✅ FFI安全，内存边界检查
- ✅ 结果兼容foobar2000 DR Meter

# important-instruction-reminders
Do what has been asked; nothing more, nothing less.
NEVER create files unless they're absolutely necessary for achieving your goal.
ALWAYS prefer editing an existing file to creating a new one.
NEVER proactively create documentation files (*.md) or README files. Only create documentation files if explicitly requested by the User.

      
      IMPORTANT: this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant to your task.