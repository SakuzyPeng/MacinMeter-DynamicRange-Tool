# MacinMeter DR Tool - foobar2000-plugin分支

![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)
![License](https://img.shields.io/badge/license-MIT-blue.svg?style=for-the-badge)
![Branch](https://img.shields.io/badge/branch-foobar2000--plugin-orange.svg?style=for-the-badge)
![Accuracy](https://img.shields.io/badge/foobar2000_accuracy-100%25-brightgreen.svg?style=for-the-badge)
![Performance](https://img.shields.io/badge/performance_gain-18.5%25-blue.svg?style=for-the-badge)

**超越原版性能的foobar2000完全兼容实现**

*致敬Janne Hyvärinen的开创性工作*

这是MacinMeter DR Tool的**foobar2000-plugin分支**，通过深度逆向工程实现与foobar2000 DR Meter的**100%算法精度对齐**，同时提供**18.5%性能提升**和**恒定内存使用**的工业级优化。采用流式处理架构，支持任意大小音频文件。

## 🎯 项目概述

本项目是foobar2000 DR Meter插件的衍生实现，基于对原始算法的深入学习和理解。我们通过逆向工程研究了Janne Hyvärinen创作的原始算法逻辑，并将其精神和原理移植到Rust语言实现中，以便为更广泛的平台和用户群体提供服务。

### ✨ foobar2000-plugin分支特性

- **🏆 100%算法精度**: 56个文件与foobar2000官方DR值完全一致
- **🚀 18.5%性能提升**: 53秒 vs 65秒，同时保持恒定20MB内存使用
- **🌊 工业级流式处理**: 支持GB级大文件，内存使用与文件大小无关
- **📝 智能输出策略**: 单文件/多文件自适应，DR值第一列便于对齐
- **🎵 全格式多语言**: WAV/FLAC/MP3/AAC/OGG + 完美中日英文件名支持
- **🔥 逐包直通处理**: 与foobar2000块边界完美对齐的原生包处理
- **⚡ SIMD向量化**: SSE2/NEON优化的立体声声道分离，单声道零开销
- **🔧 零配置优化**: 默认启用所有性能优化，无需复杂参数调节

## 🚀 快速开始

### 构建

```bash
# 克隆仓库
git clone https://github.com/SakuzyPeng/MacinMeter-DynamicRange-Tool.git
cd MacinMeter-DynamicRange-Tool

# 切换到foobar2000-plugin分支
git checkout foobar2000-plugin

# 构建release版本
cargo build --release
```

### 基本使用

```bash
# 分析单个文件（默认逐包直通模式）
./target/release/MacinMeter-DynamicRange-Tool-foo_dr audio.flac

# 详细输出（显示逐包处理信息）
./target/release/MacinMeter-DynamicRange-Tool-foo_dr --verbose audio.wav

# 禁用逐包模式（使用传统700ms固定块）
./target/release/MacinMeter-DynamicRange-Tool-foo_dr --disable-packet-chunk audio.flac

# 保存结果到文件
./target/release/MacinMeter-DynamicRange-Tool-foo_dr --output result.txt audio.flac

# 批量处理目录
./target/release/MacinMeter-DynamicRange-Tool-foo_dr /path/to/audio/directory
```

### 📝 智能输出策略 (2025-09-27优化)

MacinMeter 采用智能输出策略，根据文件数量自动优化输出格式：

#### 🎯 **单文件处理**
```bash
./target/release/MacinMeter-DynamicRange-Tool-foo_dr audio.flac
# 输出: audio_DR_Analysis.txt (仅生成单独结果文件)
```

#### 📊 **多文件批量处理**
```bash
./target/release/MacinMeter-DynamicRange-Tool-foo_dr /audio/directory
# 输出: directory_BatchDR_2025-09-27_13-22-30.txt (仅生成批量汇总)
```

#### 📋 **优化的Batch格式**
```
Official DR	Precise DR	文件名
--------------------------------------------------------
DR16	16.16 dB	贝多芬第八钢琴奏鸣曲「悲怆」.flac
DR15	15.19 dB	柴可夫斯基第一小提琴协奏曲.flac
DR13	13.17 dB	肖邦第一钢琴协奏曲.flac
```

**🎯 新格式优势**:
- ✅ **DR值第一列**: 便于视觉对齐和比较
- ✅ **智能文件名**: `{目录名}_BatchDR_{YYYY-MM-DD_HH-MM-SS}.txt`
- ✅ **制表符分隔**: 完美兼容Excel/文本编辑器
- ✅ **避免重复**: 单文件不生成batch，多文件不生成单独文件

### 编译产物路径

```bash
# Release版本 (生产使用，推荐)
./target/release/MacinMeter-DynamicRange-Tool-foo_dr

# Debug版本 (开发调试)
./target/debug/MacinMeter-DynamicRange-Tool-foo_dr
```

## 📋 命令行选项 (foobar2000-plugin分支)

```bash
参数:
  [INPUT]              音频文件或目录路径（可选，未指定时扫描当前目录）

选项:
  -v, --verbose               显示详细处理信息（包括逐包统计）
  -o, --output <FILE>         输出结果到文件
      --disable-simd          禁用SIMD向量化优化（降低性能但提高兼容性）
      --single-thread         禁用多线程处理（单线程模式）
      --disable-packet-chunk  禁用逐包直通模式（改用传统固定时长块模式）
  -h, --help                  显示帮助信息
  -V, --version               显示版本信息

默认行为:
✅ 逐包直通模式（packet-chunk）- 与foobar2000完美对齐
✅ Sum Doubling补偿 - 自动启用，匹配foobar2000行为
✅ SIMD向量化优化 - 自动启用，可手动禁用
✅ 多线程处理 - 自动启用，可切换单线程
```

## 🔬 技术实现说明

### 逆向工程方法

**研究工具**: IDA Pro 专业反汇编分析
- **分析目的**: 理解foobar2000 DR Meter的算法逻辑和数学公式  
- **分析范围**: 仅研究算法行为，未复制任何原始源代码
- **实现方式**: 基于算法理解的完全独立Rust实现

### 独立实现原则

- **🦀 编程语言**: 完全使用Rust重新编写（原版为C++）
- **🏗️ 架构设计**: 独立的模块化设计和代码结构
- **📐 算法实现**: 基于数学公式的原创实现
- **🔍 验证方法**: 通过输入/输出对比验证算法正确性

### 核心算法 (foobar2000-plugin分支)

- **DR计算公式**: `DR = -20 × log₁₀(RMS_20% / Peak_2nd)`
- **24字节ChannelData结构**: 8字节RMS累积 + 8字节主Peak + 8字节次Peak
- **逐包直通处理**: 每个解码包直接作为独立块，与foobar2000块边界完美对齐
- **Sum Doubling机制**: 专为交错音频数据设计的2倍RMS修正算法（默认启用）
- **双Peak回退系统**: 主Peak失效时智能切换到次Peak的容错设计

### 🔥 逐包直通模式特性

**技术原理**:
```rust
// 逐包模式（默认）：每个解码包直接处理
while let Some(chunk) = decoder.next_chunk() {
    dr_calculator.process_decoder_chunk(&chunk, channels);
}

// 传统模式（--disable-packet-chunk）：固定700ms块
while accumulated_samples.len() >= samples_per_block {
    let block = accumulated_samples.drain(..samples_per_block).collect();
    dr_calculator.process_decoder_chunk(&block, channels);
}
```

**优势**:
- ✅ **块边界对齐**: 与foobar2000原版完全相同的块分割方式
- ✅ **格式原生**: 每种音频格式使用其原生包结构（如FLAC frames）
- ✅ **最高精度**: 消除人工块分割带来的边界效应
- ✅ **内存高效**: 恒定内存使用，支持任意大小文件

## 🏆 Windows平台对比验证 (2025-09-27)

### 📊 与foobar2000官方的终极对比

**测试环境**: Windows平台，56个FLAC文件，多语言混合（中日英）
**对比工具**: foobar2000 2.0 + DR Meter 1.1.1 官方插件

#### ✅ **算法精度：100%完美对齐**

| DR值 | foobar2000 | MacinMeter | 文件数量 | 状态 |
|------|------------|------------|----------|------|
| DR8  | 1个        | 1个        | 1        | ✅ 完全一致 |
| DR9  | 3个        | 3个        | 3        | ✅ 完全一致 |
| DR10 | 17个       | 17个       | 17       | ✅ 完全一致 |
| DR11 | 23个       | 23个       | 23       | ✅ 完全一致 |
| DR12 | 6个        | 6个        | 6        | ✅ 完全一致 |
| DR13 | 3个        | 3个        | 3        | ✅ 完全一致 |
| DR15 | 2个        | 2个        | 2        | ✅ 完全一致 |
| DR16 | 1个        | 1个        | 1        | ✅ 完全一致 |

**📈 核心结论**: **56个文件DR值100%一致，算法逆向工程完全成功**

#### 🚀 **性能对比：显著优势**

| 指标 | foobar2000 | MacinMeter | 提升幅度 |
|------|------------|------------|----------|
| **处理时间** | 65秒 | 53秒 | **18.5%更快** |
| **内存使用** | 未知 | 恒定20.96MB | **恒定内存优势** |
| **成功率** | 100% | 100% | 一致稳定 |

#### 🎵 **测试文件覆盖范围**
- **古典音乐**: 德沃夏克第九交响曲、贝多芬悲怆奏鸣曲、柴可夫斯基小提琴协奏曲
- **现代流行**: YOASOBI、LiSA、Aimer等知名艺术家作品
- **动漫音乐**: 钢之炼金术师、魔法少女小圆、Fate系列等
- **电子音乐**: VOCALOID作品、初音ミク系列
- **多语言**: 完美支持中日英混合文件名

#### 💎 **工程质量验证**
- ✅ **跨平台一致性**: Windows/macOS结果完全相同
- ✅ **多格式兼容**: FLAC/WAV/MP3/AAC等主流格式
- ✅ **流式处理**: 支持GB级大文件，内存使用恒定
- ✅ **工业级稳定**: 零崩溃，完善错误处理

## 🙏 致敬与合规声明

### 🎉 原作者授权确认 (重大突破!)

**2025年9月8日 - 历史性时刻**: 
- ✅ **Janne Hyvärinen本人明确同意**我们使用MIT许可证进行项目开发
- ✅ **官方认可**：原作者不介意我们对foobar2000 DR Meter进行逆向工程研究  
- 📄 **官方技术规范**: 原作者提供了DR测量的标准规范文档
- 🔗 **规范文档来源**: [Measuring DR ENv3 (官方PDF)](https://web.archive.org/web/20131206121248/http://www.dynamicrange.de/sites/default/files/Measuring%20DR%20ENv3.pdf)

这一授权确认了我们项目的完全合法性，为技术研究和开源贡献扫清了所有法律障碍！

### 对原创工作的深深敬意
- **原作者**: Janne Hyvärinen - foobar2000 DR Meter插件的创作者
- **开创性贡献**: DR Meter为音频工程领域带来了标准化的动态范围测量方法
- **技术传承**: 本项目作为衍生实现，致力于传承和发扬原始算法的技术价值
- **跨平台使命**: 将这一重要工具的价值扩展到更多平台和用户群体

### 衍生实现原则
- **学习致敬**: 基于对原始算法深入学习和理解的衍生实现
- **独立构建**: 所有代码均为原创Rust实现，未复制任何原始源代码
- **算法传承**: 忠实保持原始算法的数学原理和设计理念
- **官方规范遵循**: 严格按照原作者提供的技术规范文档实现算法

### 逆向工程合法性
根据相关法律判例，以下行为通常被认为是合法的：
- ✅ 为了互操作性目的的逆向分析
- ✅ 理解算法逻辑用于独立实现  
- ✅ 通过合法工具进行技术研究

### 避免的行为
本项目严格避免以下可能有争议的行为：
- ❌ 直接复制或使用原始源代码
- ❌ 侵犯商标或品牌标识  
- ❌ 恶意商业竞争行为

## 🛠️ 开发环境

### 依赖要求
- Rust 1.70+
- 支持SIMD的CPU (可选，用于性能优化)

### 构建配置
```toml
[profile.release]
opt-level = 3
lto = true
codegen-units = 1
panic = "abort"
```

## 🧪 测试验证

```bash
# 运行所有测试 (47个单元测试 + 14个文档测试)
cargo test

# 运行SIMD精度测试
cargo test --release simd_precision_test

# 性能基准测试
cargo test --release benchmark

# 完整质量检查 (格式化 + Clippy + 编译 + 测试)
cargo fmt --check && cargo clippy -- -D warnings && cargo build --release && cargo test
```

### 📊 测试覆盖
- ✅ **47个单元测试**: 覆盖核心算法、SIMD优化、流式处理
- ✅ **14个文档测试**: 确保API文档的示例代码正确
- ✅ **零编译警告**: 严格的代码质量标准
- ✅ **跨平台验证**: Windows/macOS双平台测试通过

## 📝 开发规范

项目采用严格的代码质量标准：
- **零警告原则**: 所有Clippy警告必须修复
- **预提交钩子**: 自动进行格式、编译、测试检查
- **完整测试覆盖**: 75+单元测试覆盖核心功能

## 📄 许可证

本项目采用 MIT 许可证 - 查看 [LICENSE](LICENSE) 文件了解详情。

## 🤝 贡献

欢迎贡献代码和建议！请注意：
- 遵循现有的代码风格和架构设计
- 确保所有测试通过
- 维持高精度和算法一致性

## 🔥 逐包直通处理优势 (foobar2000-plugin分支特色)

### 🎯 与foobar2000完全对齐的块处理

本分支专注实现与foobar2000 DR Meter**完全相同**的音频块处理方式：

| 处理模式 | 块分割方式 | 精度对齐 | 内存使用 |
|---------|----------|---------|---------|
| **逐包直通**（默认） | 解码器原生包 | ✅ 100%与foobar2000对齐 | 恒定 ~50MB |
| **传统固定块** | 700ms人工分割 | ⚠️ 可能存在边界差异 | 恒定 ~50MB |

### 🔍 逐包统计信息

启用`--verbose`模式可查看详细的包处理统计：

```bash
./target/release/MacinMeter-DynamicRange-Tool-foo_dr --verbose audio.flac

# 输出示例：
📊 逐包模式统计报告：
   总解码包数: 2,847
   包大小范围: 1152 - 4608 样本
   平均包大小: 2,304.5 样本
   中位数包大小: 2304 样本
   包时长范围: 26.12ms - 104.49ms
   平均包时长: 52.24ms

🔍 块边界分析：
   ✅ 包大小相对稳定，有利于Top 20%统计一致性
```

### ⚡ 处理模式对比

#### 🔥 逐包直通模式（默认）
```rust
// 每个FLAC frame直接作为一个处理块
while let Some(flac_frame) = decoder.next_frame() {
    dr_calculator.process_decoder_chunk(&flac_frame.samples, channels);
}
```

**特点**:
- ✅ 完美复现foobar2000的块边界
- ✅ 每种格式使用其原生结构（FLAC frame、MP3 frame等）
- ✅ 消除人工切割引入的统计偏差
- ✅ 最高的Top 20%采样精度

#### 📦 传统固定块模式
```bash
# 使用--disable-packet-chunk启用
./target/release/MacinMeter-DynamicRange-Tool-foo_dr --disable-packet-chunk audio.flac
```

**特点**:
- ⚠️ 700ms固定时长切割，可能与foobar2000存在微小差异
- 🔄 适用于对比其他传统DR工具
- 📐 人工块边界可能影响边缘采样精度

### 🌊 流式处理特性

两种模式都采用**流式处理架构**：
- **恒定内存**: ~50MB，与文件大小无关
- **支持超大文件**: DXD/DSD级别的高分辨率音频
- **实时进度**: 详细的处理进度和统计信息

## 🚀 未来规划

详见 [`FUTURE_OPTIMIZATION_ROADMAP.md`](FUTURE_OPTIMIZATION_ROADMAP.md) - 完整的性能优化路线图，目标从13.669秒优化至6-8秒（45-60%性能提升）。

### 🎯 近期目标
- [ ] **解码层优化**: Symphonia参数调优，预期25-35%性能提升
- [ ] **算法层优化**: WindowRmsAnalyzer简化，预期15-25%性能提升
- [ ] **编译层优化**: PGO + Apple Silicon特定优化，预期10-20%性能提升
- [ ] **多声道支持**: 智能LFE排除和声道权重系统

## 🔗 相关链接

- **当前分支**: foobar2000-plugin (默认逐包直通模式)
- **主线分支**: early-version (通用高精度版本)
- **参考实现**: foobar2000 DR Meter (foo_dr_meter插件)
- **官方主页**: https://foobar.hyv.fi/?view=foo_dr_meter
- **原作者**: Janne Hyvärinen
- **SDK集成计划**: [foobar2000 SDK集成计划文档](docs/FOOBAR2000_SDK_INTEGRATION_PLAN.md)

## 🌿 分支差异说明

### foobar2000-plugin分支 vs early-version分支

| 特性 | foobar2000-plugin | early-version |
|-----|-------------------|---------------|
| **主要目标** | 与foobar2000完全兼容 | 通用高精度DR分析 |
| **默认处理模式** | 逐包直通 | 智能内存管理 |
| **块分割方式** | 解码器原生包 | 3秒标准块 |
| **内存管理** | 简化流式处理 | 智能策略选择 |
| **参数复杂度** | 简化（4个主要选项） | 完整（8+选项） |
| **精度对齐目标** | foobar2000 DR Meter | 通用DR标准 |
| **适用场景** | foobar2000用户迁移 | 通用DR分析工具 |

### 选择建议

- **选择 foobar2000-plugin**: 如果你是foobar2000用户，希望获得与原版完全相同的结果
- **选择 early-version**: 如果你需要通用的DR分析工具，支持更多配置选项

---

## ⚠️ 免责声明

本项目仅供技术研究和学习使用。所有逆向工程活动均符合相关法律法规。如有法律疑问，建议咨询专业律师。

**为专业音频制作而生 | Built for Professional Audio Production**