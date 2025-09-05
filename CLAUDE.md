# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## ⚠️ 重要提醒：专家角色激活

**在开始任何技术工作之前，必须激活专业角色：**

### 🎯 推荐专家角色
- **rust-audio-expert**: Rust音频开发专家 → `action("rust-audio-expert")`
  - 专门负责DR算法实现、SIMD优化、音频解码等核心技术
  - 深度理解foobar2000逆向分析结果和项目技术约束
  - 具备工业级代码质量保证能力

### 📋 角色激活检查清单
- [ ] 确认当前是否已激活专业角色
- [ ] 根据任务类型选择合适的专家（优先rust-audio-expert）
- [ ] 激活角色后确认专家身份和专业能力
- [ ] 在整个会话过程中维持角色状态

### 💡 使用方式
```bash
# 直接对话激活
"我需要激活rust-audio-expert来协助音频开发"

# 或明确指定
action("rust-audio-expert")
```

### 🔍 关键约束提醒
- **Windows验证限制**: foobar2000 DR Meter仅在Windows可用，结果对比只能由用户执行
- **精度第一原则**: 所有实现必须与foobar2000结果100%一致
- **性能目标**: SIMD优化需达到6-7倍性能提升

---

## 项目概述

MacinMeter DR Tool 是一个基于foobar2000 DR Meter完整逆向分析的音频动态范围(DR)分析工具，使用Rust实现，目标是达到100%精度匹配和工业级性能。

## 构建和运行命令

```bash
# 构建开发版本
cargo build

# 构建优化版本（生产环境）
cargo build --release

# 运行工具（开发环境）
cargo run -- [目录路径]

# 运行生产版本
./target/release/MacinMeter-DynamicRange-Tool [目录路径]

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
/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/target/debug/MacinMeter-DynamicRange-Tool
```
- 文件大小: ~10.4 MB
- 包含调试信息，启动快但运行较慢

**Release版本 (生产用)**:
```
/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/target/release/MacinMeter-DynamicRange-Tool
```
- 文件大小: ~1.5 MB  
- 优化编译，启动慢但运行快，用于性能测试和发布

### 快速测试命令
```bash
# 测试release版本
/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/target/release/MacinMeter-DynamicRange-Tool --help

# 测试基本功能
/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/target/release/MacinMeter-DynamicRange-Tool /path/to/audio/file
```

## ⚠️ 重要开发习惯：零警告原则

### 🚨 编译警告清理习惯

**每次代码修改后必须立即检查和清理编译警告！**

### 📋 完整代码质量检查工作流
```bash
# 🎯 每次提交前的完整检查（推荐使用）
cargo fmt --check && \
cargo clippy -- -D warnings && \
cargo check && \
cargo audit && \
cargo test

# 🚀 开发过程中的快速检查
cargo check   # 快速编译检查，发现基本错误和警告

# 🔍 深度代码质量检查
cargo clippy -- -D warnings   # 静态代码分析，将警告视为错误
cargo fmt --check              # 代码格式检查，确保一致性
cargo audit                    # 依赖安全漏洞扫描

# 📊 项目分析和依赖管理
cargo tree                     # 查看依赖关系树
cargo tree --duplicates        # 检查重复依赖

# 🎭 发布前的最终检查
cargo build --release          # 发布模式编译检查
cargo test --release           # 发布模式测试
```

### 🔧 本机已配置的质量工具
- ✅ **rustfmt**: 自动代码格式化，保持一致的代码风格
- ✅ **clippy**: 智能代码建议，发现潜在问题和性能优化点
- ✅ **cargo-audit**: 依赖安全扫描，防范已知漏洞
- ✅ **cargo tree**: 依赖关系可视化，管理依赖复杂度
- ✅ **rust-src**: IDE智能提示支持，提升开发效率

### 🎯 零警告标准
- **dead_code**: 及时删除未使用的函数和变量
- **unused_variables**: 使用`_`前缀或删除未使用变量  
- **unused_imports**: 清理多余的import语句
- **missing_docs**: 为所有public API添加文档注释
- **clippy::all**: 遵循Clippy的所有最佳实践建议

### 💡 常见警告快速修复
```rust
// ❌ 未使用变量警告
let data = read_file();

// ✅ 使用下划线前缀
let _data = read_file();

// ❌ 未使用的导入
use std::collections::HashMap;

// ✅ 删除或移到需要的地方

// ❌ 缺少文档警告
pub fn calculate_dr() {}

// ✅ 添加完整文档
/// 计算音频动态范围值
pub fn calculate_dr() {}
```

### 🎵 音频项目特定检查建议
```bash
# 性能关键检查（适用于音频处理）
cargo clippy -- -W clippy::cast_lossless     # 检查可能的精度损失转换
cargo clippy -- -W clippy::float_arithmetic  # 检查浮点数运算潜在问题
cargo clippy -- -W clippy::indexing_slicing  # 检查数组越界风险

# SIMD代码检查
cargo rustc -- --emit=asm                    # 生成汇编代码检查向量化效果
cargo build --release                        # 确保优化版本编译成功

# 内存布局验证（对24字节结构很重要）
cargo test -- --nocapture layout_tests       # 运行内存布局相关测试
```

### 🔧 IDE集成建议
- **配置rust-analyzer**: 实时显示警告和类型提示
- **保存时自动格式化**: 设置保存时运行`cargo fmt`
- **实时clippy检查**: 在代码编辑时显示clippy建议
- **持续集成**: CI流水线中启用`-D warnings`阻止警告代码合并

### ⚡ 自动化脚本建议
创建快捷脚本来运行完整检查：
```bash
# scripts/quality-check.sh
#!/bin/bash
echo "🔍 运行完整代码质量检查..."
cargo fmt --check && \
cargo clippy -- -D warnings && \
cargo audit && \
cargo test && \
echo "✅ 所有检查通过！"
```

**⚠️ 记住：Rust编译器的警告都很有价值，忽略警告往往会导致潜在的bug或性能问题！对于音频处理这种性能敏感的应用，警告检查更加重要。**

---

## 核心架构

该项目采用严格的模块化架构，基于foobar2000 DR Meter的逆向工程分析：

### 模块结构
- **core/**: DR计算核心算法
  - `dr_calculator.rs` - 主DR计算引擎，实现`DR = log10(RMS / Peak) * -20.0`公式
  - `channel_data.rs` - 24字节ChannelData结构（8字节RMS累积+8字节主Peak+8字节次Peak）
  - `histogram.rs` - 10001-bin直方图和20%采样算法

- **audio/**: 音频解码层
  - `decoder.rs` - 音频解码器trait抽象
  - `wav_decoder.rs` - WAV格式支持（使用hound）
  - `multi_decoder.rs` - 多格式支持（使用symphonia）

- **processing/**: 性能优化层
  - `batch.rs` - 批量处理和并行化
  - `simd.rs` - SSE向量化优化（4样本并行处理）

- **output/**: 输出格式化
  - `report.rs` - DR分析报告生成，兼容foobar2000格式

- **utils/**: 辅助工具
  - `safety.rs` - 8层防御性异常处理机制

### 关键技术要点

1. **24字节数据结构**: 每声道精确的内存布局，支持8字节对齐
2. **Sum Doubling机制**: 专为交错音频数据设计的2倍RMS修正算法
3. **双Peak回退系统**: 主Peak失效时智能切换到次Peak的容错设计
4. **10001-bin直方图**: 超高精度DR分布统计（覆盖0-10000索引）
5. **逆向遍历20%采样**: 从高RMS向低RMS遍历，符合"最响20%"标准
6. **SSE向量化**: 4样本并行处理，预期6-7倍性能提升

### 依赖说明

- `hound` - WAV文件解码
- `symphonia` - 多格式音频解码（FLAC/MP3/AAC等）
- `walkdir` - 目录遍历和批量文件处理
- `anyhow` - 统一错误处理
- `clap` - 命令行参数解析
- `rayon` - 并行计算优化

### 输出二进制

项目生成名为`dr-meter`的可执行文件，支持：
- 自动扫描指定目录的音频文件
- 输出与foobar2000格式兼容的DR分析报告
- 保存结果到txt文件

### 开发重点

本项目的核心价值在于算法精度和性能优化：
1. **精度优先**: 所有算法实现必须与foobar2000 DR Meter的结果100%一致
2. **性能关键**: SSE向量化和并行处理是核心竞争优势
3. **工业级稳定性**: 8层防御机制确保异常情况下的安全处理
4. **跨平台兼容**: 单一可执行文件，支持主流操作系统

### 验证标准

所有功能实现都必须通过以下验证：
- 与foobar2000 DR Meter的计算结果对比测试
- 性能基准测试（SSE优化效果验证）
- 多格式音频文件兼容性测试
- 边界条件和异常情况处理测试

详细的技术分析和开发计划参见：
- `docs/DR_Meter_Deep_Analysis_Enhanced.md` - 完整的foobar2000逆向分析
- `docs/DEVELOPMENT_PLAN.md` - 15天开发计划和技术规格