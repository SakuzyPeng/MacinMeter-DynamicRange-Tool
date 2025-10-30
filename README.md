# MacinMeter DR Tool - foobar2000-plugin分支

![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)
![License](https://img.shields.io/badge/license-MIT-blue.svg?style=for-the-badge)
![Branch](https://img.shields.io/badge/branch-foobar2000--plugin-orange.svg?style=for-the-badge)
![Accuracy](https://img.shields.io/badge/foobar2000_accuracy-100%25-brightgreen.svg?style=for-the-badge)
![Performance](https://img.shields.io/badge/performance_gain-85%25-blue.svg?style=for-the-badge)

**尝试提供更好体验的foobar2000兼容实现**

*致敬Janne Hyvärinen的开创性工作*

这是MacinMeter DR Tool的**foobar2000-plugin分支**，学习并实现了foobar2000 DR Meter的算法原理，力求提供**准确的DR分析结果**和**更快的处理速度**。采用流式架构设计，希望能为用户带来便利。

## 🎯 项目概述

本项目是foobar2000 DR Meter插件的衍生实现，基于对原始算法的深入学习和理解。我们通过逆向工程研究了Janne Hyvärinen创作的原始算法逻辑，并将其精神和原理移植到Rust语言实现中，以便为更广泛的平台和用户群体提供服务。

### ✨ 主要特性

- **🎯 算法准确性**: 在56个测试文件上与foobar2000官方结果一致
- **🚀 处理速度**: 在测试中相比foobar2000提升约85%（具体速度取决于文件格式和硬件）
- **🌊 大文件支持**: 流式处理设计，内存占用较小且相对稳定
- **📝 便捷输出**: 单文件/多文件自动适应，结果一目了然
- **🎵 格式支持**: 支持FLAC/WAV/MP3/AAC/ALAC/AIFF/OGG/Opus等12+种常见音频格式
- **🌏 国际化**: 完善的中日英文件名支持
- **⚡ 双重并行**: 支持文件级并行（批量处理多个文件）和解码级并行（单个文件加速）
- **🔧 开箱即用**: 无需复杂配置，默认提供较好的性能表现

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
# 分析单个文件（默认智能缓冲流式处理）
./target/release/MacinMeter-DynamicRange-Tool-foo_dr audio.flac

# 详细输出（显示流式处理统计信息）
./target/release/MacinMeter-DynamicRange-Tool-foo_dr --verbose audio.wav

# 指定输出文件（保存结果到指定路径）
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
  [INPUT]              音频文件或目录路径（可选，未指定时扫描可执行文件所在目录）

选项:
 -v, --verbose               显示详细处理信息（包括流式处理统计）
  -o, --output <FILE>         输出结果到文件
      --filter-silence[=<DB>] 🧪 启用实验性的窗口静音过滤（⚠️ 会打破与foobar2000的兼容性，默认为 -70 dBFS，可选范围 -120~0）
  -h, --help                  显示帮助信息
  -V, --version               显示版本信息

默认行为:
✅ 智能缓冲流式处理 - 包级解码 + 3秒窗口级算法
✅ Sum Doubling补偿 - 自动启用，匹配foobar2000行为
✅ SIMD向量化优化 - 自动启用，无配置选项
✅ 智能输出策略 - 单文件/多文件自适应处理

> 🧪 **实验性提醒**
>
> - 静音过滤功能默认关闭，保持与foobar2000 DR Meter 100%兼容。
> - 若启用 `--filter-silence`，将按阈值剔除低能量窗口，仅用于实验/诊断，不建议作为常规测量；目录场景请写作 `--filter-silence -- <PATH>`，避免路径被误读为阈值。
> - 建议阈值在 **-60 ~ -80 dBFS** 之间，过高会误删真实音乐静音段（如古典弱奏）。
```

### 🎬 P0 阶段：边缘裁切（Edge Trimming）- 实验性功能

**P0 是样本级的首尾边缘裁切**，用于去除编码过程中引入的首尾静音/padding，实现更精确的 DR 测量。

#### 基本用法

```bash
# 启用边缘裁切（使用默认参数）
./target/release/MacinMeter-DynamicRange-Tool-foo_dr --trim-edges audio.aac

# 指定静音阈值（-60 dBFS 为默认值）
./target/release/MacinMeter-DynamicRange-Tool-foo_dr --trim-edges=-60 audio.aac

# 指定最小持续时长（默认 60ms）
./target/release/MacinMeter-DynamicRange-Tool-foo_dr --trim-edges --trim-min-run=120 audio.flac

# 结合详细输出查看处理细节
./target/release/MacinMeter-DynamicRange-Tool-foo_dr --trim-edges --verbose audio.aac

# 批量处理目录
./target/release/MacinMeter-DynamicRange-Tool-foo_dr --trim-edges -- /path/to/audio/dir
```

#### 参数说明

- `--trim-edges[=<DB>]`：启用边缘裁切，可选指定静音阈值（默认 -60 dBFS，范围 -120…0 dB）
- `--trim-min-run=<MS>`：最小连续静音持续时长（默认 60ms，范围 50…2000ms）
  - 仅当首尾连续静音时长 ≥ min_run 时才被裁切；短于该值的首尾静音完整保留
- 结果 TXT 会自动记录裁切统计（首/尾/总时长与样本数）以及静音过滤窗口统计（过滤数、总窗口、百分比），便于复现与追踪

#### 工作原理

1. **三态状态机**
   - **Leading**：累积首部静音帧，直到累积时长 ≥ min_run 才确认丢弃
   - **Passing**：正常通过，输出所有帧
   - **Trailing**：缓冲尾部帧，EOF 时仅丢弃末尾连续静音 ≥ min_run 的帧

2. **声道数量大于1时的策略**：取最大值 `max(|L|, |R|)`
   - 只要任意一声道不是静音，整帧就保留
   - 确保立体声中的单声道信息不被误伤

3. **迟滞机制**：防止古典音乐弱音段被误判为静音
   - 要求连续 N 帧满足条件才转换状态
   - 与 min_run 结合，确保鲁棒性

#### 典型应用场景

- **AAC/MP3 vs WAV 精确对齐**：AAC 编码常引入首尾填充，导致 DR 偏高 0.02…0.05 dB；启用 P0 后可精确对齐
- **无损格式编码测试**：FLAC/ALAC 转码过程中可能引入微小 padding，P0 可自动清理
- **诊断与研究**：了解音频文件的真实边缘特性

#### ⚠️ 注意事项

- **默认关闭**：保持与 foobar2000 DR Meter 100% 兼容，需显式启用
- **阈值选择**：建议 -70 ~ -80 dBFS；过高会误删真实音乐停顿（如古典弱音）
- **短静音保留**：P0 会正确处理短首尾静音，无论多短都不会被裁切，除非达到 min_run 阈值
- **中段静音保留**：中间的任何静音段（艺术表达）完全保留，不做任何处理

#### 诊断输出示例

启用 `--verbose` 时，P0 会输出处理统计：

```
🧪 边缘裁切诊断（P0阶段）:
   阈值: -70.0 dBFS, 最小持续: 300ms, 迟滞: ~100ms
   • 首部: 保留全部（无符合min_run的静音段）
   • 尾部: 保留全部（无符合min_run的静音段）
   📄 结果已保存到: audio/formatTEST/audio_DR_Analysis.txt
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

### 核心算法说明

基于对foobar2000 DR Meter算法的学习理解：

- **DR计算原理**: 基于RMS和峰值的对数比值计算
- **流式处理**: 采用边读边算的方式处理音频数据
- **Sum Doubling**: 参考foobar2000的修正机制（默认启用）
- **容错设计**: 多重峰值系统提升算法稳定性

### ⚡ 双重并行加速

为了提升处理速度，本工具实现了两个层面的并行处理：

**1️⃣ 文件级并行**: 批量处理多个文件时，可以同时分析多个音频文件
**2️⃣ 解码级并行**: 对单个较大的音频文件，解码过程本身也能并行加速

这两个机制结合起来，在实际使用中能带来较为明显的速度提升。当然，具体的性能表现会受到文件格式、大小、硬件配置等因素影响。

### 🌊 智能缓冲流式处理特性

**技术原理**:
```rust
// 智能缓冲流式处理：包级解码 + 窗口级算法
while let Some(chunk) = streaming_decoder.next_chunk()? {
    // 积累chunk到3秒窗口缓冲区
    sample_buffer.extend_from_slice(&chunk);

    // 当积累到完整窗口时，处理并清空缓冲区
    while sample_buffer.len() >= window_size_samples {
        process_window_with_simd_separation(&window_samples, ...);
        sample_buffer.drain(0..window_size_samples);
    }
}
```

**架构优势**:
- ✅ **恒定内存**: 20MB固定使用，与文件大小无关
- ✅ **流式解码**: 包级解码避免全量加载
- ✅ **算法精度**: 3秒标准窗口保持20%采样算法精度
- ✅ **SIMD优化**: 窗口级SIMD声道分离和向量化处理

## 🏆 Windows平台对比测试 (2025-09-27)

### 📊 与foobar2000官方的对比结果

**测试环境**: Windows平台，56个FLAC文件，多语言混合（中日英）
**对比工具**: foobar2000 2.0 + DR Meter 1.1.1 官方插件

#### ✅ **算法一致性测试**

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

**📈 测试结果**: 56个文件的DR值与foobar2000保持一致

#### 🚀 **性能表现**

| 指标 | foobar2000 | MacinMeter | 说明 |
|------|------------|------------|------|
| **处理时间** | 65秒 | 35秒 | 约快85%（具体速度受文件格式影响） |
| **内存使用** | - | ~45MB | 相对稳定的内存占用 |
| **成功率** | 100% | 100% | 稳定处理 |

#### 🎵 **测试文件覆盖范围**
- **古典音乐**: 德沃夏克第九交响曲、贝多芬悲怆奏鸣曲、柴可夫斯基小提琴协奏曲
- **现代流行**: YOASOBI、LiSA、Aimer等知名艺术家作品
- **动漫音乐**: 钢之炼金术师、魔法少女小圆、Fate系列等
- **电子音乐**: VOCALOID作品、初音ミク系列
- **多语言**: 完美支持中日英混合文件名

#### 💎 **项目特点**
- ✅ **跨平台**: Windows/macOS/Linux结果一致
- ✅ **多格式**: 支持FLAC/WAV/MP3/AAC/ALAC/AIFF/OGG/Opus等12+种格式
- ✅ **大文件友好**: 流式处理设计，内存占用相对稳定
- ✅ **稳定性**: 力求提供可靠的处理体验

### 🎵 支持的音频格式

本工具通过优秀的Symphonia和Songbird库，尝试支持常见的音频格式：

**无损格式**: FLAC, ALAC (Apple Lossless), WAV, AIFF
**有损格式**: MP3, AAC, OGG Vorbis, Opus
**其他格式**: MP1 (MPEG Layer I), 以及多种容器格式 (MP4/M4A, MKV等)

**总计**: 12+种常见音频格式，希望能覆盖大部分使用场景

*注: MP3格式由于技术特性采用串行解码，其他大部分格式支持并行加速*

### 📋 最近改进 (2025-10)

- **修复MP3格式**: 解决了并行解码导致的数据问题，现在MP3文件能正确分析了
- **修复AIFF格式**: 改进了样本数据处理，确保AIFF文件的准确性
- **代码质量**: 持续改进代码质量，减少警告和潜在问题

## 🙏 致敬与合规声明

### 🎉 原作者授权确认

**2025年9月8日**:
- ✅ Janne Hyvärinen本人同意我们使用MIT许可证进行项目开发
- ✅ 原作者不介意我们对foobar2000 DR Meter进行学习研究
- 📄 原作者提供了DR测量的技术规范文档
- 🔗 规范文档: [Measuring DR ENv3 (官方PDF)](https://web.archive.org/web/20131206121248/http://www.dynamicrange.de/sites/default/files/Measuring%20DR%20ENv3.pdf)

非常感谢原作者的支持和理解！

### 致敬原创作者
- **原作者**: Janne Hyvärinen - foobar2000 DR Meter插件的创作者
- **贡献**: DR Meter为音频动态范围测量提供了标准方法
- **本项目**: 学习原算法并用Rust重新实现，希望能让更多平台的用户使用这个工具

### 实现方式
- **学习理解**: 通过学习和理解原算法的原理进行实现
- **独立编码**: 所有代码用Rust重新编写，未复制原始源代码
- **遵循规范**: 参考原作者提供的技术规范文档实现

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


## 📝 开发说明

项目遵循基本的代码质量实践：
- 尽量保持代码整洁和可读性
- 使用自动化工具进行格式检查
- 编写测试来验证核心功能

### 🔧 作为库使用 (API 使用指南)

如果你想在自己的 Rust 项目中集成 DR 分析功能，推荐使用统一的解码器 API：

#### **推荐：UniversalDecoder + UniversalStreamingDecoder**

```rust
use macinmeter_dr_tool::audio::{UniversalDecoder, UniversalStreamingDecoder};
use macinmeter_dr_tool::core::DrCalculator;

// 1. 创建解码器工厂和流式解码器（推荐 - 统一接口，支持所有格式）
let universal_decoder = UniversalDecoder::new();
let mut decoder: Box<dyn UniversalStreamingDecoder> =
    universal_decoder.create_streaming("audio.flac")?;

// 2. 获取音频格式信息
let format = decoder.format();
println!("采样率: {}Hz, 声道数: {}", format.sample_rate, format.channels);

// 3. 创建 DR 计算器
let mut calculator = DrCalculator::new(
    format.sample_rate,
    format.channels.into()
)?;

// 4. 流式处理音频数据
while let Some(samples) = decoder.next_chunk()? {
    calculator.process_samples(&samples)?;
}

// 5. 获取 DR 结果
let result = calculator.finalize()?;
println!("官方DR值: DR{}", result.official_dr);
println!("精确DR值: {:.2} dB", result.precise_dr);
```

#### **类型说明**

- **`UniversalDecoder`**: 解码器工厂，提供 `create_streaming()` 方法
- **`UniversalStreamingDecoder`**: 统一的流式解码器接口（trait 别名）
- **`AudioFormat`**: 音频格式信息结构体
- **`DrCalculator`**: DR 计算引擎

#### **并行解码 (可选 - 提升大文件性能)**

```rust
// 启用并行解码（适用于大文件，FLAC/AAC/OGG等无状态格式）
let universal_decoder = UniversalDecoder::new();
let mut decoder = universal_decoder.create_streaming_parallel(
    "large_audio.flac",
    true,  // 启用并行
    None   // 使用默认并行配置
)?;
```

**注意**：
- MP3 格式由于状态依赖会自动降级到串行解码
- Opus 格式使用专用的高性能解码器
- 推荐使用 `UniversalDecoder` 以获得最佳兼容性

详细的 API 文档请查看代码注释和 `src/audio/mod.rs`。

## 📄 许可证

本项目采用 MIT 许可证 - 查看 [LICENSE](LICENSE) 文件了解详情。

## 🤝 贡献

非常欢迎各种形式的贡献和建议！无论是：
- 发现问题并提出Issue
- 改进代码或文档
- 分享使用经验和反馈

我们都非常感谢。在提交代码时，请尽量保持现有的代码风格即可。

## 🌊 智能缓冲流式架构 (foobar2000-plugin分支特色)

### 🎯 包级解码 + 窗口级算法的混合设计

本分支采用创新的混合架构，结合包级流式解码和窗口级算法处理：

| 处理层面 | 实现方式 | 技术优势 | 内存特性 |
|---------|----------|---------|---------|
| **解码层** | 包级流式解码 | 避免全量加载，支持任意大小文件 | 包级临时缓冲 |
| **算法层** | 3秒窗口处理 | 保持20%采样算法精度 | 恒定20MB窗口缓冲 |
| **整体** | 智能缓冲混合 | ✅ 100%与foobar2000算法对齐 | 恒定内存使用 |

### 🔍 详细处理统计

启用`--verbose`模式可查看详细的流式处理统计：

```bash
./target/release/MacinMeter-DynamicRange-Tool-foo_dr --verbose audio.flac

# 输出示例：
🌊 智能缓冲流式处理统计：
   总解码chunk数: 2,847
   窗口处理数: 127个 (3秒/窗口)
   缓冲区峰值: 600KB (3秒窗口)
   平均chunk大小: 2,304.5 样本
   处理效率: 115MB/s

🎯 算法精度保证：
   ✅ 3秒标准窗口，保持20%采样精度
   ✅ WindowRmsAnalyzer流式处理
```

### ⚡ 架构层次设计

#### 🌊 智能缓冲流式处理（当前实现）
```rust
// 包级流式解码 + 窗口级算法处理
while let Some(chunk) = streaming_decoder.next_chunk()? {
    sample_buffer.extend_from_slice(&chunk);

    while sample_buffer.len() >= window_size_samples {
        // 3秒标准窗口处理，保持算法精度
        process_window_with_simd_separation(&window_samples, ...);
        sample_buffer.drain(0..window_size_samples);
    }
}
```

**核心特点**:
- ✅ **流式解码**: 包级解码避免全量内存加载
- ✅ **算法精度**: 3秒窗口保持foobar2000完全对齐
- ✅ **恒定内存**: 20MB固定缓冲，支持GB级文件
- ✅ **SIMD优化**: 窗口级向量化处理

### 🚀 流式架构优势

**内存管理**：
- **解码层**: 包级临时缓冲（KB级）
- **算法层**: 3秒窗口缓冲（~600KB）
- **总体**: 恒定20MB，与文件大小无关

**性能特性**：
- **支持超大文件**: DXD/DSD级别的高分辨率音频
- **实时进度**: 详细的处理进度和统计信息
- **零内存累积**: 处理完窗口立即清空缓冲

## 🚀 后续计划

我们会持续改进这个工具，希望能做得更好：

- 继续优化处理速度和内存使用
- 完善对更多音频格式的支持
- 改进用户体验和错误提示
- 探索多声道音频的支持

详细的优化计划可以查看 [`FUTURE_OPTIMIZATION_ROADMAP.md`](FUTURE_OPTIMIZATION_ROADMAP.md)。

## 🔗 相关链接

- **当前分支**: foobar2000-plugin (智能缓冲流式处理)
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
| **默认处理模式** | 智能缓冲流式处理 | 智能内存管理 |
| **架构设计** | 包级解码+窗口级算法 | 3秒标准块 |
| **内存管理** | 恒定20MB缓冲 | 智能策略选择 |
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
