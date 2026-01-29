# MacinMeter DR Tool — 快速指南

[English](README.md) | [中文](README_CN.md)

![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)
![License](https://img.shields.io/badge/license-MIT-blue.svg?style=for-the-badge)
![Branch](https://img.shields.io/badge/branch-main-green.svg?style=for-the-badge)

**尝试提供更好体验的 foobar2000 兼容实现**

*致敬 Janne Hyvärinen 的开创性工作*

MacinMeter DR Tool 学习并实现了 foobar2000 DR Meter 1.0.3（foo_dr_meter，作者 Janne Hyvärinen）的算法原理，力求提供**准确的 DR 分析结果**和**更快的处理速度**。采用流式架构设计，希望能为用户带来便利。性能基准对比参考 Dynamic Range Meter 1.1.1（foo_dynamic_range）。

---

## 简介

- 遵循 foobar2000 DR Meter 官方算法，可输出官方四舍五入值（Official DR）与精确小数（Precise DR），并结合 SIMD + 并行优化。
- 支持 12+ 种常见音频格式（FLAC、WAV、AAC、MP3、Opus 等），并在必要时自动切换至 FFmpeg 解码。详见"支持的音频格式"章节。
- Precise DR 相比 foobar2000 通常在 ±0.02–0.05 dB 内波动（窗口抽样及舍入口径差异）；个别曲目会出现约 0.1 dB 的偏差。预警功能会在 Precise DR 接近四舍五入边界时提醒交叉验证。

## 构建与运行

```bash
cargo build --release                                              # 构建
cargo run --release -- <音频文件或目录>                              # 直接运行
./target/release/MacinMeter-DynamicRange-Tool-foo_dr <路径>         # 启动可执行文件
cargo test                                                         # 测试
```

## 图形界面

`tauri-app/` 目录提供了一个 Tauri 2 GUI，复用同一套 DR 引擎。可通过系统对话框选择音频并查看官方/精确 DR、静音过滤与裁切报告。

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

**输出控制**：`--output <file>` 指定单文件结果路径；批量模式默认写入目标目录。

**实验性功能**（默认关闭）：
- `--trim-edges[=<DB>]`：首尾边缘裁切，默认阈值 -60 dBFS；`--trim-min-run <MS>`（默认 60 ms）
- `--filter-silence[=<DB>]`：窗口级静音过滤，默认阈值 -70 dBFS
- `--exclude-lfe`：从最终 DR 聚合中剔除 LFE 声道
- `--show-rms-peak`：在单文件报告中附加 RMS/Peak 诊断表

## 输出说明

报告包含每声道 DR 值、Official DR（整数）、Precise DR（小数）及音频信息（采样率/声道/位深/比特率/编解码器）。

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

## 准确性与兼容性

- 支持多声道（≥3）分析；官方聚合口径与 foobar2000 一致：对所有"非静音"声道的单声道 DR 做算术平均并四舍五入。
- 可选 LFE 剔除（`--exclude-lfe`）：仅在存在声道布局元数据时生效；单声道明细仍保留。
- AAC 从 WAV 转码常使 Precise DR 略有升高（≈0.01–0.05 dB）；其他有损格式通常略降或持平。

## 性能概要

详细基准数据请参见 [docs/BENCHMARKS.md](docs/BENCHMARKS.md)。

| 平台 | 数据集 | 吞吐 |
|------|--------|-----:|
| macOS · M4 Pro | 1.6 GB 单文件 | ~725 MB/s |
| macOS · M4 Pro | 69 首 FLAC (1.17 GB) | ~1168 MB/s |
| Windows · i9-13900H | 69 首 FLAC (1.17 GB) | ~568 MB/s |

**建议**：使用 Release 构建；大文件可调整 `--parallel-threads` 与 `--parallel-batch`。

---

## 支持的音频格式

详细文档请参见 [docs/SUPPORTED_FORMATS.md](docs/SUPPORTED_FORMATS.md)。

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

致敬与合规声明、第三方许可、免责声明请参见 [docs/LEGAL.md](docs/LEGAL.md)。

---

## 相关链接

- **参考实现**：foobar2000 DR Meter 1.0.3 (foo_dr_meter)
  - **作者**：Janne Hyvärinen
  - **官方主页**：https://foobar.hyv.fi/?view=foo_dr_meter
- **性能对比**：Dynamic Range Meter 1.1.1 (foo_dynamic_range)
  - **基于**：Pleasurize Music Foundation 的 TT Dynamic Range Offline Meter

---

**为专业音频制作而生**
