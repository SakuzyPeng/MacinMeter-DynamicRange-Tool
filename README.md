# MacinMeter DR Tool — 快速指南 / Quick Reference

![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)
![License](https://img.shields.io/badge/license-MIT-blue.svg?style=for-the-badge)
![Branch](https://img.shields.io/badge/branch-foobar2000--plugin-orange.svg?style=for-the-badge)

**尝试提供更好体验的foobar2000兼容实现 / A foobar2000-compatible implementation aiming for better experience**

*致敬Janne Hyvärinen的开创性工作 / Tribute to Janne Hyvärinen's pioneering work*

这是MacinMeter DR Tool的**foobar2000-plugin分支**，学习并实现了foobar2000 DR Meter的算法原理，力求提供**准确的DR分析结果**和**更快的处理速度**。采用流式架构设计，希望能为用户带来便利。

This is the **foobar2000-plugin branch** of MacinMeter DR Tool, which learns and implements the algorithm principles of foobar2000 DR Meter, striving to provide **accurate DR analysis results** and **faster processing speed**. With streaming architecture design, we hope to bring convenience to users.

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
```
MacinMeter DR Tool v0.1.0 / Dynamic Range Meter (foobar2000 compatible)
日志时间 / Log date: 2025-10-30 23:41:09

--------------------------------------------------------------------------------
统计对象 / Statistics for: audio.flac
样本总数 / Number of samples: 20256768
时长 / Duration: 7:02
--------------------------------------------------------------------------------

                         左声道 / Left      右声道 / Right

DR通道 / DR Channel:      14.57 dB   ---    16.46 dB
--------------------------------------------------------------------------------

Official DR Value: DR16
Precise DR Value: 15.51 dB

边界风险（高） / Boundary Risk (High)
Precise DR 15.51 dB 距离 DR15/DR16 下边界 0.01 dB
建议 / Recommendation: 使用 foobar2000 DR Meter 交叉验证

采样率 / Sample rate:    48000 Hz
声道数 / Channels:       2
位深 / Bits per sample: 24
比特率 / Bitrate:        2304 kbps
编码 / Codec:           FLAC
================================================================================
```

## 输出策略（Output File Policy）
- 单文件输入：生成同目录 `<name>_DR_Analysis.txt`。
- Single-file input: generates `<name>_DR_Analysis.txt` next to the audio.
- 多文件输入：生成单独的批量汇总 TXT。
- Folder input: produces a batch summary file.
- 可自定义输出路径：`--output <file>`（对批量模式同样适用）。
- Custom output path: supply `--output <file>` (works for batch mode).

### 批量输出示例 / Batch Output Example
```
====================================================================================
   MacinMeter DR Analysis Report / MacinMeter DR分析报告
   批量分析结果 (foobar2000兼容版) / Batch Analysis Results (foobar2000 Compatible)
====================================================================================

Git分支 / Git Branch: foobar2000-plugin (默认批处理模式)
基于foobar2000 DR Meter逆向分析 (Measuring_DR_ENv3规范)
扫描目录 / Scanned Directory: /path/to/audio
处理文件数 / Files to Process: 106

Official DR      Precise DR        文件名 / File Name
================================================================================
DR11             10.71 dB         track01.flac
DR12             12.15 dB         track02.flac
DR13             12.64 dB         track03.flac
DR16             16.16 dB         track04.flac
DR15             15.19 dB         track05.flac
...

=====================================
   边界风险警告 / Boundary Risk Warnings
=====================================

以下文件的DR值接近四舍五入边界，可能与foobar2000结果相差±1级：

Official DR  Precise DR   风险等级           边界方向         Δ距离        foobar2000 可能值
==============================================================================================
DR11         11.49 dB     高风险 / High     上边界 / Upper   Δ0.01 dB     DR12
DR16         15.51 dB     高风险 / High     下边界 / Lower   Δ0.01 dB     DR15

批量处理统计 / Batch Processing Statistics:
   总文件数 / Total Files: 106
   成功处理 / Processed Successfully: 106
   处理失败 / Failed: 0
   处理成功率 / Success Rate: 100.0%
```

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

## 测量方法（Measurement Method）
- macOS 脚本：`scripts/benchmark.sh`（单次）、`scripts/benchmark_10x.sh`（10 次统计）。
- macOS scripts: `scripts/benchmark.sh` (single run) and `scripts/benchmark_10x.sh` (10-run statistics).
- Windows 脚本：`scripts/benchmark.ps1`、`scripts/benchmark-10x.ps1`（与 macOS 采样逻辑一致）。
- Windows scripts: `scripts/benchmark.ps1` and `scripts/benchmark-10x.ps1`, mirroring the macOS sampling logic.
- 指标采集：每 0.1 s 通过 `ps` (或 PowerShell `Get-Process`) 记录 RSS 与累计 CPU 时间；使用 `date +%s.%N` 或 Windows 计时器测量总时长；处理速度＝音频总大小（MB）÷运行时长（s）。
- Sampling: every 0.1 s the scripts record RSS and cumulative CPU time via `ps`/`Get-Process`; total duration uses `date +%s.%N` or Windows timers; throughput = total size (MB) ÷ elapsed time (s).

## 测试环境（Test Environment）
- macOS 26 · Apple M4 Pro · 12 cores (8P + 4E)
  macOS 26 with an Apple M4 Pro SoC (12 cores: 8 performance + 4 efficiency) used for all macOS benchmarks.
- Windows 10 · Intel i7-11800H
  Windows 10 laptop with an Intel i7-11800H CPU for the Windows benchmarks below.

## 数据集（Datasets）

### 中文
| 类别 | 规模 | 编码/声道 | 采样率分布 | 位深分布 | 备注 |
| --- | --- | --- | --- | --- | --- |
| 单大型文件 | 约 1.6 GB · 1 首 · 约 94 分钟 | FLAC / 2ch | 96 kHz | 24-bit | mediainfo 提取 |
| 69 首 FLAC 集 | 约 1.17 GB · 69 首 | FLAC / 2ch | 48 kHz × 69 | 24-bit × 69 | 常见 CD 合集 |
| 106 首 FLAC | 约 11.7 GB · 106 首 | FLAC / 2ch | 96×50，48×46，192×8，88.2×2 | 24-bit × 106 | 用于文件级并行基准 |

### English
| Category | Size & Count | Codec/Channels | Sample Rates | Bit Depth | Notes |
| --- | --- | --- | --- | --- | --- |
| Single large file | ~1.6 GB · 1 track · ~94 min | FLAC / 2ch | 96 kHz | 24-bit | via mediainfo |
| 69-track FLAC set | ~1.17 GB · 69 tracks | FLAC / 2ch | 48 kHz × 69 | 24-bit × 69 | typical CD compilation |
| 106-track FLAC set | ~11.7 GB · 106 tracks | FLAC / 2ch | 96×50, 48×46, 192×8, 88.2×2 | 24-bit × 106 | batch benchmark |

## 单文件基准（10 次，解码并行 vs 串行） / Single-File Benchmark (10 runs)

| 模式 / Mode | 平均时间 Avg Time (s) | 中位数 Median (s) | 标准差 StdDev (s) | 平均吞吐 Avg Throughput (MB/s) | 中位数 Median (MB/s) | 标准差 StdDev (MB/s) |
| --- | ---:| ---:| ---:| ---:| ---:| ---:|
| 并行解码 / Parallel | 2.780 | 2.766 | 0.069 | 721.55 | 724.89 | 17.99 |
| 串行解码 / `--serial` | 10.809 | 10.588 | 0.584 | 185.90 | 189.41 | 9.84 |

| 模式 / Mode | 峰值内存 平均 Avg Peak Memory (MB) | 峰值内存 中位 Median (MB) | 峰值内存 标准差 StdDev (MB) | CPU 峰值 平均 Avg Peak CPU (%) | CPU 平均 Avg CPU (%) |
| --- | ---:| ---:| ---:| ---:| ---:|
| 并行解码 / Parallel | 47.87 | 44.51 | 7.46 | 33.04 | 32.47 |
| 串行解码 / `--serial` | 25.37 | 25.84 | 0.91 | 8.38 | 8.32 |

## 批量基准（10 次，文件级并行 vs 禁用） / Batch Benchmark (10 runs)

| 数据集 / Dataset | 模式 / Mode | 平均时间 Avg Time (s) | 中位数 Median (s) | 标准差 StdDev (s) | 平均吞吐 Avg Throughput (MB/s) | 中位数 Median (MB/s) | 标准差 StdDev (MB/s) |
| --- | --- | ---:| ---:| ---:| ---:| ---:| ---:|
| 69 首 FLAC | 默认并行 / Default parallel | 1.178 | 1.025 | 0.457 | 1113.84 | 1167.73 | 159.40 |
| 106 首 FLAC | 默认并行 / Default parallel | 9.462 | 9.018 | 0.796 | 1241.84 | 1294.15 | 101.56 |
| 106 首 FLAC | 禁用文件并行 / `--no-parallel-files` | 26.997 | 26.035 | 2.591 | 435.34 | 447.81 | 33.43 |

| 数据集 / Dataset | 模式 / Mode | 峰值内存 平均 Avg Peak Memory (MB) | 峰值内存 中位 Median (MB) | 峰值内存 标准差 StdDev (MB) | CPU 峰值 平均 Avg Peak CPU (%) | CPU 平均 Avg CPU (%) |
| --- | --- | ---:| ---:| ---:| ---:| ---:|
| 69 首 FLAC | 默认并行 / Default parallel | 97.65 | 94.22 | 11.72 | 64.62 | 61.99 |
| 106 首 FLAC | 默认并行 / Default parallel | 1743.26 | 1679.38 | 183.64 | 80.54 | 78.69 |
| 106 首 FLAC | 禁用文件并行 / `--no-parallel-files` | 87.41 | 87.71 | 2.73 | 21.69 | 20.02 |

## Windows 10 Benchmark (Intel i7-11800H, 10 runs)
- 测试脚本与 macOS 相同采样逻辑，数值统一换算为秒与 MB。
- Same sampling as macOS scripts; results normalized to seconds and megabytes.
- Windows 数据与 macOS 相同，均基于去重后的 106 首集合（仅数值与此前 107 首版本相同）。
- Windows runs now mirror the deduplicated 106-track set used on macOS (metrics unchanged from the former 107-track set).

| 数据集 / Dataset | 模式 / Mode | 平均时间 Avg Time (s) | 中位数 Median (s) | 标准差 StdDev (s) | 平均吞吐 Avg Throughput (MB/s) | 中位数 Median (MB/s) | 标准差 StdDev (MB/s) |
| --- | --- | ---:| ---:| ---:| ---:| ---:| ---:|
| 单大型文件 | 默认并行 / Default parallel | 6.090 | 6.085 | 0.014 | 248.05 | 248.26 | 0.56 |
| 69 首 FLAC | 默认并行 / Default parallel | 3.057 | 3.051 | 0.027 | 381.49 | 382.23 | 3.30 |
| 106 首 FLAC | 默认并行 / Default parallel | 16.411 | 16.213 | 0.416 | 715.07 | 723.44 | 17.49 |

| 数据集 / Dataset | 峰值内存 平均 Avg Peak Memory (MB) | 峰值内存 中位 Median (MB) | 峰值内存 标准差 StdDev (MB) | 平均内存 Avg Memory (MB) | 平均内存 中位 Median (MB) | 平均内存 标准差 StdDev (MB) |
| --- | ---:| ---:| ---:| ---:| ---:| ---:|
| 单大型文件 / Single large file | 217.89 | 217.87 | 12.90 | 135.39 | 136.78 | 8.01 |
| 69 首 FLAC / 69 FLAC tracks | 40.17 | 40.21 | 1.05 | 26.84 | 26.75 | 0.55 |
| 106 首 FLAC / 106 FLAC tracks | 178.67 | 175.07 | 23.57 | 98.82 | 99.24 | 10.97 |

**Intel 13 代 i9-13900H + Windows 11 性能数据**：69 首 FLAC 集在默认 4 并发下，通过线程优先级优化后性能表现稳定，测试数据如下（10 次运行统计）：

**Intel 13th Gen i9-13900H + Windows 11 Performance**: the 69-track FLAC set at default 4-way parallelism, with thread priority optimization, shows stable performance. 10-run statistics:

| 指标 / Metric | 中位数 Median | 平均值 Average | 标准差 StdDev |
| --- | ---:| ---:| ---:|
| 运行时间 / Time | 2.052 s | 2.056 s | 0.016 s |
| 处理速度 / Throughput | 568.18 MB/s | 567.23 MB/s | 4.28 MB/s |
| 峰值内存 / Peak Memory | 41.43 MB | 40.09 MB | 4.49 MB |
| 平均内存 / Avg Memory | 21.98 MB | 21.31 MB | 2.25 MB |
| CPU 平均占用 / Avg CPU | 36.57% | 36.30% | 1.32% |
| CPU 峰值占用 / Peak CPU | 43.19% | 43.54% | 2.09% |

相比 macOS M4 Pro（中位 1.025 秒、1167.73 MB/s），i9-13900H 在相同 69 首 FLAC 数据集（1.17GB）上的表现为中位 2.05 秒、568 MB/s，整体性能约为 M4 Pro 的 49%。混合 P/E 核调度已通过线程优先级优化得到改善。

Compared to macOS M4 Pro (median 1.025 s / 1167.73 MB/s), the i9-13900H achieves median 2.05 s / 568 MB/s on the same 69-track FLAC set (1.17GB), reaching approximately 49% of M4 Pro's performance. Hybrid P/E core scheduling has been improved through thread priority optimization.

| 平台 / Platform | 模式 / Mode | 106 首 FLAC 时间 Time (s) | 吞吐 Throughput (MB/s) | 峰值内存 Peak (MB) | 平均内存 Avg (MB) | 平均 CPU Avg CPU (%) |
| --- | --- | ---:| ---:| ---:| ---:| ---:|
| Windows 10 · i7-11800H | `--serial --no-parallel-files` | 127.452 | 89.4 | 23.38 | 17.76 | 20.0 |
| Windows 10 · i7-11800H | foobar2000 DR Meter | 162.0 | 70.4 | <15 | <15 | 15.0 |

> Windows 完全串行为极端调度或内存受限环境提供兜底，foobar2000 记录列供第三方基准参考。

> Fully serial mode offers a fallback when scheduler pressure or memory limits are severe; foobar2000 numbers are listed for baseline comparison.

## 性能建议（Performance Tips）
- 建议优先使用 Release 构建；并行解码与 SIMD 可显著提升吞吐。
- Prefer release builds; decode parallelism plus SIMD dramatically improve throughput.
- 对大文件可调整 `--parallel-threads` 与 `--parallel-batch` 平衡吞吐与资源。
- Tune `--parallel-threads` and `--parallel-batch` for large files to balance throughput and resource usage.

---

## 支持的音频格式（Supported Audio Formats）

### 解码器路由（Decoder Routing）

工具采用智能自动路由，优先使用 Symphonia，必要时自动切换 FFmpeg：

**Symphonia 原生支持 / Native Symphonia Support**:
- **无损格式 / Lossless**: FLAC, ALAC (Apple Lossless), WAV, AIFF, PCM
- **有损格式 / Lossy**: AAC, OGG Vorbis, MP1 (MPEG Layer I)
- **容器格式 / Containers**: MP4/M4A（仅限 Symphonia 支持的编码），MKV/WebM

**专用解码器 / Dedicated Decoders**:
- **Opus**: 通过songbird专用解码器 (Discord音频库) / Via songbird decoder (Discord audio library)
- **MP3**: 有状态解码格式，强制串行处理 / Stateful format, forced serial decoding

**FFmpeg 自动回退 / Auto Fallback to FFmpeg**:
当 Symphonia 无法支持时，工具自动切换至 FFmpeg 进行解码。典型场景：
- 扩展名为 `.ac3`、`.ec3`、`.eac3`、`.dts`、`.dsf`、`.dff` → 直接使用 FFmpeg
- MP4/M4A 容器内包含 AC-3、E-AC-3（含 Dolby Atmos）、DTS → 自动切换 FFmpeg
- 其他容器格式（部分 MKV、MP4 变体）内的不兼容编码 → 自动回退 FFmpeg

### FFmpeg 安装（FFmpeg Installation）

如需使用 FFmpeg 功能，请确保系统已安装 `ffmpeg` 和 `ffprobe`：

- **macOS**: `brew install ffmpeg`
- **Windows**: `winget install Gyan.FFmpeg`（或通过 Chocolatey、其他发行渠道）
- **Linux**:
  - Ubuntu/Debian: `sudo apt install ffmpeg`
  - Fedora/RHEL: `sudo dnf install ffmpeg`
  - Arch: `sudo pacman -S ffmpeg`

验证安装：`ffmpeg -version` 和 `ffprobe -version` 应返回版本号。工具会自动检测 PATH 中的 ffmpeg 和 ffprobe。

### 多声道与 LFE 支持（Multichannel & LFE Support）

- **多声道分析**: 支持 3-32 声道音频，每声道独立计算 DR，输出详细的 per-channel 结果
- **Official DR 聚合**: 对所有"非静音"声道的 DR 值进行算术平均并四舍五入，符合 foobar2000 标准
- **LFE 识别**:
  - 通过 Symphonia：自动检测声道布局元数据（WAV、某些 MP4/MKV）
  - 通过 FFmpeg：读取 ffprobe JSON 标签序列（如 `FL+FR+FC+LFE+...`），精确定位 LFE 位置
- **LFE 剔除**（可选）：使用 `--exclude-lfe` 在最终聚合中排除 LFE，单声道 DR 明细仍保留

### 总计（Summary）

**12+种主流音频格式** / 12+ mainstream formats，覆盖 90%+ 用户需求：

| 分类 / Category | 格式 / Formats | 解码器 / Decoder |
| --- | --- | --- |
| 无损 Lossless | FLAC, ALAC, WAV, AIFF, PCM | Symphonia |
| 有损 Lossy | AAC, OGG Vorbis, MP1 | Symphonia |
| 音乐编码 Proprietary | MP3 | Symphonia (串行 Serial) |
| 音乐编码 Proprietary | Opus | songbird (专用 Dedicated) |
| 影音编码 Video Codec | AC-3, E-AC-3, DTS, DSD | FFmpeg (自动回退 Auto) |
| 容器 Containers | MP4/M4A, MKV, WebM | Symphonia / FFmpeg (智能路由 Smart) |

**并行性能**：MP3 采用串行处理（有状态格式），其他格式均支持并行加速；多声道使用零拷贝跨步优化，3+ 声道性能提升 8-16 倍。

MP3 uses serial decoding (stateful format); other formats support parallel acceleration. Multichannel uses zero-copy strided optimization with 8–16× performance gain for 3+ channels.

---

## 致敬与合规声明（Legal Compliance）

### 原作者授权确认 / Author Authorization
**2025年9月8日 / September 8, 2025**:
- Janne Hyvärinen（原作者）同意使用MIT许可证进行项目开发 / Author agreed to MIT license development
- 原作者不介意对foobar2000 DR Meter进行学习研究 / Author has no objection to studying foobar2000 DR Meter
- 提供了DR测量的技术规范文档 / Provided technical specification document for DR measurement
- 规范文档 / Specification: [Measuring DR ENv3 (官方PDF)](https://web.archive.org/web/20131206121248/http://www.dynamicrange.de/sites/default/files/Measuring%20DR%20ENv3.pdf)

非常感谢原作者的支持和理解！/ Special thanks to the original author for support and understanding!

### 实现方式 / Implementation Approach
- 完全使用Rust重新编写（原版为C++）/ Completely rewritten in Rust (original in C++)
- 独立的模块化设计和代码结构 / Independent modular design and code structure
- 基于数学公式的原创实现 / Original implementation based on mathematical formulas
- 通过输入/输出对比验证算法正确性 / Algorithm verified through input/output comparison

### 智能助力 / AI Collaboration
- 感谢 Anthropic Claude 4.5 系列模型（Sonnet / Haiku）完成了项目的大部分代码编写  
  Thanks to Anthropic Claude 4.5 models (Sonnet / Haiku) for implementing the majority of the codebase
- 感谢 OpenAI GPT-5 与 Codex 模型协助补充部分代码并负责大部分审阅与改进建议  
  Thanks to OpenAI GPT-5 and Codex models for contributing additional code and providing the bulk of reviews & refinements

### 逆向工程合法性 / Reverse Engineering Legality
根据相关法律判例，以下行为通常被认为是合法的 / Based on legal precedents, the following are generally legal:
- 为了互操作性目的的逆向分析 / Reverse analysis for interoperability
- 理解算法逻辑用于独立实现 / Understanding algorithms for independent implementation
- 通过合法工具进行技术研究 / Technical research using legal tools

本项目严格避免 / This project strictly avoids:
- 直接复制或使用原始源代码 / Direct copying or use of original source code
- 侵犯商标或品牌标识 / Trademark or brand infringement
- 恶意商业竞争行为 / Malicious commercial competition

---

## 许可证（License）

本项目采用 MIT 许可证 - 查看 [LICENSE](LICENSE) 文件了解详情。

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

---

## 相关链接（Related Links）

- **当前分支 / Current Branch**: foobar2000-plugin (智能缓冲流式处理 / smart buffered streaming)
- **参考实现 / Reference**: foobar2000 DR Meter (foo_dr_meter插件 / plugin)
- **官方主页 / Official**: https://foobar.hyv.fi/?view=foo_dr_meter
- **原作者 / Original Author**: Janne Hyvärinen

---

## 免责声明（Disclaimer）

本项目仅供技术研究和学习使用。所有逆向工程活动均符合相关法律法规。如有法律疑问，建议咨询专业律师。

This project is for technical research and educational purposes only. All reverse engineering activities comply with relevant laws and regulations. For legal questions, please consult a professional lawyer.

**为专业音频制作而生 / Built for Professional Audio Production**
