# MacinMeter DR Tool — 快速指南 / Quick Reference

![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)
![License](https://img.shields.io/badge/license-MIT-blue.svg?style=for-the-badge)
![Branch](https://img.shields.io/badge/branch-main-green.svg?style=for-the-badge)

**尝试提供更好体验的foobar2000兼容实现 / A foobar2000-compatible implementation aiming for better experience**

*致敬Janne Hyvärinen的开创性工作 / Tribute to Janne Hyvärinen's pioneering work*

MacinMeter DR Tool 学习并实现了 foobar2000 DR Meter 1.0.3（foo_dr_meter，作者 Janne Hyvärinen）的算法原理，力求提供**准确的DR分析结果**和**更快的处理速度**。采用流式架构设计，希望能为用户带来便利。性能基准对比参考 Dynamic Range Meter 1.1.1（foo_dynamic_range）。

MacinMeter DR Tool learns and implements the algorithm principles of foobar2000 DR Meter 1.0.3 (foo_dr_meter, by Janne Hyvärinen), striving to provide **accurate DR analysis results** and **faster processing speed**. With streaming architecture design, we hope to bring convenience to users. Performance benchmarks are compared against Dynamic Range Meter 1.1.1 (foo_dynamic_range).

---

## 简介（Introduction）
- 本工具遵循 foobar2000 DR Meter 官方算法，可输出官方四舍五入值（Official DR）与精确小数（Precise DR），并结合 SIMD + 并行优化。
- The tool follows the foobar2000 DR Meter specification, returning both Official DR (rounded) and Precise DR values with SIMD and parallel optimisations.
- 支持 12+ 种常见音频格式（FLAC、WAV、AAC、MP3、Opus 等），并在必要时自动切换至 FFmpeg 解码。详见"支持的音频格式"章节。
- Supports 12+ mainstream audio formats (FLAC, WAV, AAC, MP3, Opus, etc.) with automatic fallback to FFmpeg when needed. See "Supported Audio Formats" section for details.
- Precise DR 相比 foobar2000 通常在 ±0.02–0.05 dB 内波动（窗口抽样及舍入口径差异）；个别曲目会出现约 0.1 dB 的偏差（常见原因是尾窗是否纳入 20% 窗口或不同母带的首尾采样差异）。预警功能会在 Precise DR 接近四舍五入边界时提醒交叉验证。
- Precise DR typically differs from foobar2000 by ±0.02–0.05 dB (window selection & rounding). A few tracks may drift by ~0.1 dB (e.g., tail-window participation in the top-20% selection or masters with different leading/trailing samples). The tool surfaces boundary warnings so you can cross-check with foobar2000 when results matter.

## 构建与运行（Build & Run）
- Release 构建：`cargo build --release`
- Build release artifacts with `cargo build --release`.
- 单文件执行：`cargo run --release -- <audio-or-folder>`
- Analyse a file or folder directly with `cargo run --release -- <audio-or-folder>`.
- 直接运行可执行文件：`./target/release/MacinMeter-DynamicRange-Tool-foo_dr <path>`
- Launch the compiled binary via `./target/release/MacinMeter-DynamicRange-Tool-foo_dr <path>`.
- 运行测试：`cargo test`
- Execute the test suite with `cargo test`.

## 图形界面 / Tauri GUI
- `tauri-app/` 目录提供了一个 Tauri 2 GUI——复用同一套 DR 引擎，可通过系统对话框选择音频并查看官方/精确 DR、静音过滤与裁切报告。
- 运行方式（首次先 `npm install`）：`cd tauri-app && npm run tauri dev`；构建发行版：`npm run tauri build`。
- 详情、命令说明与安全权限请见 `docs/tauri_wrapper.md`。

## 快速开始（Quick Start）
1. **双击运行 / Double-click launch**
   - 默认扫描可执行文件所在目录：若存在多首音频，生成一份批量汇总 TXT；仅 1 首则写出 `<name>_DR_Analysis.txt`。
   - Automatically scans the executable directory. Multiple files produce a batch summary, single files generate `<name>_DR_Analysis.txt`.
2. **命令行示例 / CLI examples**
   - 单文件：`./target/release/MacinMeter-DynamicRange-Tool-foo_dr song.flac`
   - Single file: `./target/release/MacinMeter-DynamicRange-Tool-foo_dr song.flac`
   - 目录（默认 4 文件并行）：`./target/release/MacinMeter-DynamicRange-Tool-foo_dr album_dir`
   - Folder (4 concurrent files by default): `./target/release/MacinMeter-DynamicRange-Tool-foo_dr album_dir`
   - 实验功能（默认关闭，注意 `--` 分隔路径）：
     `./target/release/MacinMeter-DynamicRange-Tool-foo_dr --trim-edges --trim-min-run=60 --filter-silence -- --parallel-files=4 --parallel-threads=4 --parallel-batch=64 --output ./report.txt album_dir`
   - Experimental options (disabled by default, `--` separates the directory argument).
3. **详细日志 / Verbose logging**
   - 追加 `--verbose` 展示完整分析过程。
   - Append `--verbose` to view detailed processing logs.

## 常用选项（Key CLI Options）
- 并行相关（默认启用解码并行；文件级并行默认 4，可用 `--serial` 串行化解码 / `--no-parallel-files` 串行化文件处理）
- Parallel controls (decode parallelism on by default; multi-file parallelism defaults to 4 – use `--serial` for decode-only serial mode or `--no-parallel-files` for per-file serial processing)
  - `--parallel-threads <N>`：解码线程数（默认 4）。
  - `--parallel-threads <N>`: number of decoding threads (default 4).
  - `--parallel-batch <N>`：解码批大小（默认 64）。
  - `--parallel-batch <N>`: decode batch size (default 64).
  - `--parallel-files <N>` / `--no-parallel-files`：多文件并行度（默认 4）/ 禁用多文件并行。
 - `--parallel-files <N>` / `--no-parallel-files`: number of concurrent files (default 4) / disable multi-file parallelism.
- 输出文件：`--output <file>` 指定单文件结果路径；批量模式默认写入目标目录。
- Output control: use `--output <file>` for single-file reports; batch mode writes to the target directory by default.
- **实验性功能 / Experimental features**（默认关闭）
  - `--trim-edges[=<DB>]`：首尾边缘裁切，默认阈值 -60 dBFS；配合 `--trim-min-run <MS>`（默认 60 ms）控制最小连贯静音长度。
  - `--trim-edges[=<DB>]`: edge trimming with default threshold −60 dBFS; use `--trim-min-run <MS>` (default 60 ms) to set minimum sustained silence.
  - `--filter-silence[=<DB>]`：窗口级静音过滤，默认阈值 -70 dBFS；目录模式建议写作 `--filter-silence -- /path/to/dir` 避免路径被解析为阈值。
  - `--filter-silence[=<DB>]`: window-level silence filtering, default −70 dBFS; for directories use `--filter-silence -- /path/to/dir` to prevent path parsing as threshold.
- `--exclude-lfe`：从最终 DR 聚合中剔除 LFE 声道（仅在存在声道布局元数据时生效）；单声道明细仍保留。
- `--exclude-lfe`: exclude LFE channels from final DR aggregation (effective only with channel layout metadata); per‑channel details remain.
- `--show-rms-peak`：在单文件报告中附加 RMS/Peak 诊断表（默认隐藏，批量汇总暂不支持）。
- `--show-rms-peak`: append the RMS/Peak diagnostics table in single-file reports (hidden by default; batch summaries not yet supported).

## 输出说明（Output Format）
- 每声道 DR 值、Official DR（整数）、Precise DR（小数）及音频信息（采样率/声道/位深/比特率/编解码器）。
- The report lists DR per channel, Official DR, Precise DR, plus audio metadata (sample rate, channels, bit depth, bitrate, codec).
- 若使用边缘裁切或窗口过滤，TXT 头部会展示配置与统计（裁切样本数、过滤窗口数等）。
- When trimming/filtering is enabled, configuration and statistics (trimmed samples, filtered windows) appear in the header.

### 单文件输出示例 / Single File Output Example
```markdown
MacinMeter DR Tool vX.X.X | DR15 (15.51 dB)
audio.flac | 7:02 | 48000 Hz | 2ch | FLAC

| Channel | DR       | Peak     |
|---------|----------|----------|
|  Left   | 14.57 dB | -0.12 dB |
|  Right  | 16.46 dB | -0.08 dB |

> Boundary Risk (High): 15.51 dB is 0.01 dB from DR15/DR16 boundary
```

## 输出策略（Output File Policy）
- 单文件输入：生成同目录 `<name>_DR_Analysis.txt`。
- Single-file input: generates `<name>_DR_Analysis.txt` next to the audio.
- 多文件输入：生成单独的批量汇总 TXT。
- Folder input: produces a batch summary file.
- 可自定义输出路径：`--output <file>`（对批量模式同样适用）。
- Custom output path: supply `--output <file>` (works for batch mode).

### 批量输出示例 / Batch Output Example
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

### Boundary Warnings (1)

| DR | Precise | Risk | Potential | File |
|----|---------|------|-----------|------|
| 16 |   15.51 | High | DR15      | track04.flac |

### Summary

| Metric  | Value      |
|---------|------------|
| Total   | 5          |
| Success | 5 (100%)   |

---
*MacinMeter DR Tool vX.X.X*
```

**标记说明 / Markers**: `*` = LFE excluded · `†` = Silent channels excluded

## 并行模式简介（Parallel Modes）
- 解码并行：默认针对单文件执行多线程解码 / 窗口处理；提升吞吐而内存占用较小。可使用 `--serial` 禁用。
- Decode parallelism: splits decoding and window processing across threads for a single file (default on); disable with `--serial`.
- 文件级并行：默认同时处理 4 个文件（macOS），可用 `--parallel-files` 调整，或 `--no-parallel-files` 禁用以降低内存。
- File-level parallelism: processes up to four files concurrently (default on macOS); adjust via `--parallel-files` or disable with `--no-parallel-files` to reduce memory footprint.

## 准确性与兼容性（Accuracy & Compatibility）
- 支持多声道（≥3）分析；官方聚合口径与 foobar2000 一致：对所有“非静音”声道的单声道 DR 做算术平均并四舍五入得到 Official DR。
- Multichannel (≥3 ch) is supported; aggregation matches foobar2000: arithmetic mean of per‑channel DR over all non‑silent channels, rounded to the nearest integer for Official DR.
- 可选 LFE 剔除（`--exclude-lfe`）：当容器提供声道布局元数据（如 WAV WAVEFORMATEXTENSIBLE 掩码或解码器的通道位图）时，从“聚合计算”中剔除 LFE；单声道明细仍保留 LFE 的 DR 以供查看。
- Optional LFE exclusion (`--exclude-lfe`): when the container exposes channel layout metadata (e.g. WAV WAVEFORMATEXTENSIBLE mask or a decoder channel bitset), LFE channels are excluded from the final aggregation; per‑channel DRs are still listed for inspection.
- 无元数据时，为避免误判，将不会执行 LFE 剔除（报告会提示“请求剔除 LFE，但未检测到声道布局元数据”）；FLAC 5.1/7.1 在缺少元数据时按规范回退将 LFE 视作 index=3。
- If no layout metadata is available, LFE exclusion is not performed (a note is emitted); for FLAC 5.1/7.1 a spec fallback maps LFE to index=3 when metadata is missing.
- AAC 从 WAV 转码常使 Precise DR 略有升高（≈0.01–0.05 dB）；其他有损格式通常略降或持平，无损基本不变。
- AAC converted from WAV often raises Precise DR by ≈0.01–0.05 dB; other lossy codecs generally decrease or match, while lossless conversions stay aligned.

## 已知限制（Known Limitations）
- 若容器未提供可用的声道布局元数据，`--exclude-lfe` 将不会生效（会在报告中提示）。
- If the container does not expose a usable channel layout, `--exclude-lfe` will not take effect (a note is printed in the report).
- 对于部分容器/变体（如 ADM/RF64、部分 MP4/MKV），布局位图解析仍在完善中；WAV/PCM 的非常规变体可能需要先“重新封装”为标准 WAV/PCM 再分析。
- For certain containers/variants (e.g. ADM/RF64, some MP4/MKV), layout parsing is still in progress; uncommon WAV/PCM variants may require “rewrapping” into standard WAV/PCM first.
- 部分容器的真实比特率可能无法获取，将显示 N/A。
- Some containers may not expose a reliable bitrate, resulting in N/A.

## 多声道与 LFE 剔除简述（Multichannel & LFE Exclusion）
- 聚合口径：所有“非静音”声道参与；静音声道（峰值与 RMS 近 0）自动排除，不计入平均。
- Aggregation: all non‑silent channels participate; silent channels (near‑zero peak and RMS) are dropped.
- LFE 剔除：使用 `--exclude-lfe` 开启，仅在存在可信布局元数据时生效；剔除仅作用于最终聚合，单声道 DR 明细保留。
- LFE exclusion: enable with `--exclude-lfe`; effective only with reliable layout metadata. Exclusion affects only the aggregate, per‑channel DR rows remain.
- FLAC 回退：5.1/7.1 若缺少元数据，按规范将 LFE 视作 index=3（0‑based）。7.1.4/9.1.6 等更高阶布局须依赖容器提供的布局或位图。
- FLAC fallback: for 5.1/7.1 without metadata, LFE is mapped to index=3 (0‑based). Higher layouts like 7.1.4/9.1.6 require container metadata.
- 示例 / Example：`./target/release/MacinMeter-DynamicRange-Tool-foo_dr --exclude-lfe -- /path/to/file-or-dir`

## 性能概要（Performance Summary）

详细基准数据请参见 [docs/BENCHMARKS.md](docs/BENCHMARKS.md)。

For detailed benchmark data, see [docs/BENCHMARKS.md](docs/BENCHMARKS.md).

**快速参考 / Quick Reference**:

| 平台 / Platform | 数据集 / Dataset | 吞吐 Throughput |
| --- | --- | ---:|
| macOS · M4 Pro | 1.6 GB 单文件 / Single file | ~725 MB/s |
| macOS · M4 Pro | 69 首 FLAC (1.17 GB) | ~1168 MB/s |
| Windows · i9-13900H | 69 首 FLAC (1.17 GB) | ~568 MB/s |

**性能建议 / Performance Tips**:
- 建议使用 Release 构建；并行解码 + SIMD 可显著提升吞吐。
- Prefer release builds; parallel decoding + SIMD dramatically improve throughput.
- 大文件可调整 `--parallel-threads` 与 `--parallel-batch` 平衡吞吐与资源。
- Tune `--parallel-threads` and `--parallel-batch` for large files.

---

## 支持的音频格式（Supported Audio Formats）

详细文档请参见 [docs/SUPPORTED_FORMATS.md](docs/SUPPORTED_FORMATS.md)。

For detailed format documentation, see [docs/SUPPORTED_FORMATS.md](docs/SUPPORTED_FORMATS.md).

| 分类 / Category | 格式 / Formats | 解码器 / Decoder |
|-----------------|----------------|------------------|
| 无损 Lossless | FLAC, ALAC, WAV, AIFF, PCM | Symphonia |
| 有损 Lossy | AAC, OGG Vorbis, MP1, MP3, Opus | Symphonia / songbird |
| 影音编码 Video | AC-3, E-AC-3, DTS, DSD | FFmpeg (自动回退) |
| 容器 Containers | MP4/M4A, MKV, WebM | 智能路由 Smart routing |

**FFmpeg 安装 / Installation**: macOS `brew install ffmpeg` · Windows `winget install Gyan.FFmpeg` · Linux 包管理器

---

## 许可证与致谢（License & Acknowledgements）

**MIT License** - 查看 [LICENSE](LICENSE) 了解详情 / See [LICENSE](LICENSE) for details.

致敬与合规声明、第三方许可、免责声明请参见 [docs/LEGAL.md](docs/LEGAL.md)。

For legal compliance, third-party notices, and disclaimer, see [docs/LEGAL.md](docs/LEGAL.md).

---

## 相关链接（Related Links）

- **当前分支 / Current Branch**: foobar2000-plugin (智能缓冲流式处理 / smart buffered streaming)
- **参考实现 / Reference Implementation**: foobar2000 DR Meter 1.0.3 (foo_dr_meter 插件 / plugin)
  - **作者 / Author**: Janne Hyvärinen
  - **官方主页 / Official**: https://foobar.hyv.fi/?view=foo_dr_meter
- **性能对比 / Performance Benchmark**: Dynamic Range Meter 1.1.1 (foo_dynamic_range 插件 / plugin)
  - **基于 / Based on**: TT Dynamic Range Offline Meter from the Pleasurize Music Foundation www.pleasurizemusic.com
  - **foobar2000 组件作者 / foobar2000 Component Author**: Soerin Jokhan

---

**为专业音频制作而生 / Built for Professional Audio Production**
