# MacinMeter DR Tool (foobar2000兼容版)

![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)
![License](https://img.shields.io/badge/license-MIT-blue.svg?style=for-the-badge)

**基于foobar2000 DR Meter的Rust衍生实现**

*致敬Janne Hyvärinen的开创性工作*

一个基于foobar2000 DR Meter算法逻辑深入研究的高性能音频动态范围(DR)测量工具衍生实现，使用Rust语言重新构建，旨在为跨平台环境提供高精度的DR分析能力。

## 🎯 项目概述

本项目是foobar2000 DR Meter插件的衍生实现，基于对原始算法的深入学习和理解。我们通过逆向工程研究了Janne Hyvärinen创作的原始算法逻辑，并将其精神和原理移植到Rust语言实现中，以便为更广泛的平台和用户群体提供服务。

### ✨ 衍生实现特性

- **🌿 致敬原创**: 忠实传承foobar2000 DR Meter的核心算法精神
- **🎯 高精度实现**: Peak检测100%精确，DR分类高度准确
- **🎵 多格式支持**: WAV, FLAC, MP3, AAC, OGG等主流音频格式
- **🚀 现代化优化**: Rust语言重构，SIMD向量化，多线程并行
- **🔧 跨平台扩展**: 将原本Windows专有的功能扩展至Linux、macOS
- **⚡ 算法传承**: 保持Sum Doubling、双Peak回退等原始设计理念

## 🚀 快速开始

### 构建

```bash
# 克隆仓库
git clone https://github.com/SakuzyPeng/MacinMeter-DynamicRange-Tool.git
cd MacinMeter-DynamicRange-Tool

# 切换到foobar2000兼容分支
git checkout early-version

# 构建release版本
cargo build --release
```

### 基本使用

```bash
# 分析单个文件
./target/release/MacinMeter-DynamicRange-Tool-foo_dr audio.flac

# 详细输出
./target/release/MacinMeter-DynamicRange-Tool-foo_dr --verbose audio.wav

# 启用Sum Doubling补偿
./target/release/MacinMeter-DynamicRange-Tool-foo_dr --sum-doubling audio.mp3

# 保存结果到文件
./target/release/MacinMeter-DynamicRange-Tool-foo_dr --output result.txt audio.flac
```

### Mac用户便利路径

```bash
# Release版本 (推荐)
/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/target/release/MacinMeter-DynamicRange-Tool-foo_dr

# Debug版本 (开发用)  
/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/target/debug/MacinMeter-DynamicRange-Tool-foo_dr
```

## 📋 命令行选项

```bash
选项:
  -s, --sum-doubling   启用Sum Doubling补偿（交错数据）
  -v, --verbose        显示详细处理信息
  -o, --output <FILE>  输出结果到文件
      --disable-simd   禁用SIMD向量化优化
      --single-thread  禁用多线程处理
  -h, --help           显示帮助信息
  -V, --version        显示版本信息
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

### 核心算法

- **DR计算公式**: `DR = -20 × log₁₀(RMS_20% / Peak_2nd)`
- **24字节ChannelData结构**: 8字节RMS累积 + 8字节主Peak + 8字节次Peak
- **Sum Doubling机制**: 专为交错音频数据设计的2倍RMS修正算法  
- **双Peak回退系统**: 主Peak失效时智能切换到次Peak的容错设计

## 📊 精度验证

使用标准测试音频文件验证：
- **测试文件**: `Ver2-adm-master-from-DAW-spatialmix-noreverb-peaklimited-0-2025-08-29-00-00-55.flac`
- **期望结果**: DR10 (与foobar2000 DR Meter一致)
- **验证状态**: ✅ Peak检测100%精确，官方DR值高度一致
- **详细分析**: 参见 [`docs/PRECISION_ANALYSIS.md`](docs/PRECISION_ANALYSIS.md)

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
# 运行所有测试
cargo test

# 运行SIMD精度测试
cargo test --release simd_precision_test

# 性能基准测试
cargo test --release benchmark
```

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

## 🔗 相关链接

- **主线分支**: MacinMeter DR Tool 标准版本
- **参考实现**: foobar2000 DR Meter (foo_dr_meter插件)
- **官方主页**: https://foobar.hyv.fi/?view=foo_dr_meter
- **原作者**: Janne Hyvärinen

---

## ⚠️ 免责声明

本项目仅供技术研究和学习使用。所有逆向工程活动均符合相关法律法规。如有法律疑问，建议咨询专业律师。

**为专业音频制作而生 | Built for Professional Audio Production**