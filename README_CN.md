# MacinMeter DR Tool — 快速指南

[English](README.md) | [中文](README_CN.md)

![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)
![License](https://img.shields.io/badge/license-MIT-blue.svg?style=for-the-badge)
![Branch](https://img.shields.io/badge/branch-main-green.svg?style=for-the-badge)

**正在重新验证参考行为的独立 DR 分析实现**

*致敬 Janne Hyvärinen 的开创性工作*

MacinMeter DR Tool 是一个参考 foobar2000 DR Meter 1.0.3（`foo_dr_meter`，作者 Janne Hyvärinen）行为而独立编写的 Rust 实现。项目正在进行架构整改，以及新一轮黑盒实验和逆向对齐。当前算法与参考插件存在已知的系统性偏差，因此现阶段不能将结果视为已经通过参考一致性认证。

---

## 简介

- 提供流式 DR 分析、整数聚合值和小数诊断值。现有报告仍沿用 `Official DR` 与 `Precise DR` 字段名，但这些名称目前不代表已经验证参考一致性。
- 为 12+ 种常见格式（FLAC、WAV、AAC、MP3、Opus 等）声明了解码路径，并在部分外部/回退场景使用 FFmpeg；实际可用性和正确性随 backend 而异。
- 正在通过可重复的参考观测与重新逆向建立兼容性证据；当前输出不能视为可与 `foo_dr_meter` 互换。

> **兼容性警告：** 当前多声道、并行解码、DSD 和部分实验性前处理路径存在已知正确性问题。重要结果请与参考插件交叉验证，并参见[架构整改与参考插件重新对齐路线图](docs/ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md)。

## 构建与运行

```bash
cargo build --release                                              # 构建
cargo run --release -- <音频文件或目录>                              # 直接运行
./target/release/MacinMeter-DynamicRange-Tool-foo_dr <路径>         # 启动可执行文件
cargo test                                                         # 测试
```

## 图形界面

`tauri-app/` 目录提供了一个 Tauri 2 GUI，复用同一套 DR 引擎。可通过系统对话框选择音频并查看整数/小数 DR 字段，以及实验性的静音过滤与裁切报告。

运行方式：`cd tauri-app && npm install && npm run tauri dev`；构建发行版：`npm run tauri build`。详见 `docs/tauri_wrapper.md`。

## 快速开始

1. **双击运行**：默认扫描可执行文件所在目录。若存在多首音频，生成一份批量汇总 TXT；仅 1 首则写出 `<name>_DR_Analysis.txt`。

2. **命令行示例**：
   ```bash
   ./target/release/MacinMeter-DynamicRange-Tool-foo_dr song.flac      # 单文件
   ./target/release/MacinMeter-DynamicRange-Tool-foo_dr album_dir      # 目录（默认 4 文件并行）
   ```

3. **详细日志**：追加 `--verbose` 展示完整分析过程。

## 常用选项

**并行相关**（默认启用解码并行；文件级并行默认 4）：
- `--parallel-threads <N>`：解码线程数（默认 4）
- `--parallel-batch <N>`：解码批大小（默认 64）
- `--parallel-files <N>` / `--no-parallel-files`：多文件并行度（默认 4）/ 禁用
- `--serial`：禁用解码并行

> **当前可靠性提示：** 包级并行解码存在已知的 EOF 和错误传播风险。在路线图 `COR-001`、`COR-002` 关闭前，重要分析请使用 `--serial`。

**输出控制**：`--output <file>` 指定单文件结果路径；批量模式默认写入目标目录。

**实验性功能**（默认关闭）：
- `--trim-edges[=<DB>]`：首尾边缘裁切，默认阈值 -60 dBFS；`--trim-min-run <MS>`（默认 60 ms）。该路径存在已知状态机缺陷，目前不应供可信分析使用。
- `--filter-silence[=<DB>]`：窗口级静音过滤，默认阈值 -70 dBFS
- `--exclude-lfe`：从最终 DR 聚合中剔除 LFE 声道；布局可信度及回退行为正在复核。
- `--show-rms-peak`：在单文件报告中附加 RMS/Peak 诊断表

## 输出说明

报告包含每声道 DR 值、当前名为 Official DR（整数）和 Precise DR（小数）的字段，以及音频信息（采样率/声道/位深/比特率/编解码器）。这些字段名描述现有报告结构，不代表已经验证参考兼容性。

### 单文件示例
```markdown
MacinMeter DR Tool vX.X.X | DR15 (15.51 dB)
audio.flac | 7:02 | 48000 Hz | 2ch | FLAC

| Channel | DR       | Peak     |
|---------|----------|----------|
|  Left   | 14.57 dB | -0.12 dB |
|  Right  | 16.46 dB | -0.08 dB |

> Boundary Risk (High): 15.51 dB is 0.01 dB from DR15/DR16 boundary
```

### 批量示例
```markdown
## MacinMeter DR Batch Report

**Generated**: 2025-01-29 12:00:00 | **Files**: 5 | **Directory**: /path/to/album

| DR | Precise | File |
|----|---------|------|
| 11 | 10.71 | track01.flac |
| 12 | 12.15 | track02.flac |
| 13 | 12.64 | track03.flac * |
| 16 | 15.51 | track04.flac |
| 15 | 15.19 | track05.flac |

*LFE excluded

### Summary

| Metric  | Value      |
|---------|------------|
| Total   | 5          |
| Success | 5 (100%)   |

---
*MacinMeter DR Tool vX.X.X*
```

**标记说明**：`*` = LFE 已剔除 · `†` = 静音声道已剔除

## 当前准确性与兼容性状态

- 参考一致性尚未重新建立；已知差异具有系统性，不能归纳为一个通用的小幅浮点误差范围。
- 当前 3+ 声道路径存在尾窗未结算缺陷；在 `ALG-001` 关闭前，不应依赖多声道结果。
- 可选 LFE 剔除（`--exclude-lfe`）仍属实验功能，声道布局可信度和未知布局行为正在修正。
- 不同 codec 的结果差异不能证明算法兼容，因为解码和重采样可能改变送入分析器的 PCM。
- 证据计划、验收标准和整改事项详见[架构与参考对齐路线图](docs/ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md)。

## 性能概要

历史基准数据请参见 [docs/BENCHMARKS_CN.md](docs/BENCHMARKS_CN.md)。相关方法和受影响的并行路径正在重新验证；这些数字不构成正确性保证或当前性能承诺。

| 平台 | 数据集 | 吞吐 |
|------|--------|-----:|
| macOS · M4 Pro | 1.6 GB 单文件 | ~725 MB/s |
| macOS · M4 Pro | 69 首 FLAC (1.17 GB) | ~1168 MB/s |
| Windows · i9-13900H | 69 首 FLAC (1.17 GB) | ~568 MB/s |

**当前建议**：使用 Release 构建；结果完整性重要时优先使用 `--serial`。包级并行契约修复并重新验证后，再调整并行参数。

---

## 声明的音频格式覆盖

详细文档请参见 [docs/SUPPORTED_FORMATS_CN.md](docs/SUPPORTED_FORMATS_CN.md)。

下表记录预期路由，不代表已经验证的能力保证。FFmpeg 回退、Opus 和容器内特定 codec 路径均属于本轮解码可靠性审计范围。

| 分类 | 格式 | 解码器 |
|------|------|--------|
| 无损 | FLAC, ALAC, WAV, AIFF, PCM | Symphonia |
| 有损 | AAC, OGG Vorbis, MP1, MP3, Opus | Symphonia / songbird |
| 影音编码 | AC-3, E-AC-3, DTS, DSD | FFmpeg（自动回退） |
| 容器 | MP4/M4A, MKV, WebM | 智能路由 |

**FFmpeg 安装**：macOS `brew install ffmpeg` · Windows `winget install Gyan.FFmpeg` · Linux 包管理器

---

## 许可证与致谢

**MIT License** - 查看 [LICENSE](LICENSE) 了解详情。

致敬与合规声明、第三方许可、免责声明请参见 [docs/LEGAL_CN.md](docs/LEGAL_CN.md)。

---

## 相关链接

- **项目整改路线图**：[架构整改与参考插件重新对齐](docs/ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md)
- **参考目标**：foobar2000 DR Meter 1.0.3 (foo_dr_meter)
  - **作者**：Janne Hyvärinen
  - **官方主页**：https://foobar.hyv.fi/?view=foo_dr_meter
- **性能对比**：Dynamic Range Meter 1.1.1 (foo_dynamic_range)
  - **基于**：Pleasurize Music Foundation 的 TT Dynamic Range Offline Meter

---

**独立音频分析研究与工程项目**
