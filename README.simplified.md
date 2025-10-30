# MacinMeter DR Tool — 快速指南 / Quick Reference

## 简介（Introduction）
- 本工具遵循 foobar2000 DR Meter 官方算法，可输出官方四舍五入值（Official DR）与精确小数（Precise DR），并结合 SIMD + 并行优化。
  The tool follows the foobar2000 DR Meter specification, returning both Official DR (rounded) and Precise DR values with SIMD and parallel optimisations.
- 支持 WAV/PCM、FLAC/ALAC、AAC/MP3、Opus 等常见格式，依赖 Symphonia 解码库。
  Supported formats include WAV/PCM, FLAC/ALAC, AAC/MP3, Opus, and other codecs via the Symphonia decoding stack.
- Precise DR 相比 foobar2000 通常在 ±0.02–0.05 dB 内波动（窗口抽样及舍入口径差异）；106 首批量测试中有 3 首 Official DR 与 foobar 不一致。预警功能将在 Precise DR 接近四舍五入边界时提醒交叉验证。
  Precise DR typically differs from foobar2000 by ±0.02–0.05 dB (window selection & rounding); in the 106-track batch, 3 tracks diverged from foobar’s Official DR. A rounding-boundary warning highlights potential discrepancies.

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
     Automatically scans the executable directory. Multiple files produce a batch summary, single files generate `<name>_DR_Analysis.txt`.
2. **命令行示例 / CLI examples**
   - 单文件：`./target/release/MacinMeter-DynamicRange-Tool-foo_dr song.flac`
     Single file: `./target/release/MacinMeter-DynamicRange-Tool-foo_dr song.flac`
   - 目录（默认 4 文件并行）：`./target/release/MacinMeter-DynamicRange-Tool-foo_dr album_dir`
     Folder (4 concurrent files by default): `./target/release/MacinMeter-DynamicRange-Tool-foo_dr album_dir`
   - 实验功能（默认关闭，注意 `--` 分隔路径）：
     `./target/release/MacinMeter-DynamicRange-Tool-foo_dr --trim-edges --trim-min-run=60 --filter-silence -- --parallel-files=4 --parallel-threads=4 --parallel-batch=64 --output ./report.txt album_dir`
     Experimental options (disabled by default, `--` separates the directory argument).
3. **详细日志 / Verbose logging**
   - 追加 `--verbose` 展示完整分析过程。
     Append `--verbose` to view detailed processing logs.

## 常用选项（Key CLI Options）
- 并行相关（默认启用解码并行；文件级并行默认 4，可用 `--serial` 串行化解码 / `--no-parallel-files` 串行化文件处理）
- Parallel controls (decode parallelism on by default; multi-file parallelism defaults to 4 – use `--serial` for decode-only serial mode or `--no-parallel-files` for per-file serial processing)
  - `--parallel-threads <N>`：解码线程数（默认 4）。
  - `--parallel-threads <N>`: number of decoding threads (default 4).
  - `--parallel-batch <N>`：解码批大小（默认 64）。
  - `--parallel-batch <N>`: decode batch size (default 64).
  - `--parallel-files <N>` / `--no-parallel-files`：多文件并行度（默认 4）/ 禁用多文件并行。
  - `--parallel-files <N>` / `--no-parallel-files`: number of concurrent files (default 4) / disable multi-file parallelism.
- 边缘裁切（默认关闭）：`--trim-edges[=<DB>]`，默认阈值 -60 dBFS；配合 `--trim-min-run <MS>`（默认 60 ms）控制首尾连贯静音裁切。
- Edge trimming (disabled by default): `--trim-edges[=<DB>]`, default threshold −60 dBFS, with `--trim-min-run <MS>` (default 60 ms) to require sustained silence before trimming.
- 窗口静音过滤（默认关闭）：`--filter-silence[=<DB>]`，默认阈值 -70 dBFS；目录模式建议写作 `--filter-silence -- /path/to/dir`（示例 `./tool --filter-silence -- ./album`），避免路径被解析为阈值。
- Window silence filter (disabled by default): `--filter-silence[=<DB>]`, default −70 dBFS; when scanning a directory use `--filter-silence -- /path/to/dir` to prevent the path from being parsed as the threshold.
- 输出文件：`--output <file>` 指定单文件结果路径；批量模式默认写入目标目录。
- Output control: use `--output <file>` for single-file reports; batch mode writes to the target directory by default.

## 输出说明（Output Format）
- 每声道 DR 值、Official DR（整数）、Precise DR（小数）及音频信息（采样率/声道/位深/比特率/编解码器）。
- The report lists DR per channel, Official DR, Precise DR, plus audio metadata (sample rate, channels, bit depth, bitrate, codec).
- 若使用边缘裁切或窗口过滤，TXT 头部会展示配置与统计（裁切样本数、过滤窗口数等）。
- When trimming/filtering is enabled, configuration and statistics (trimmed samples, filtered windows) appear in the header.

## 输出策略（Output File Policy）
- 单文件输入：生成同目录 `<name>_DR_Analysis.txt`。
- Single-file input: generates `<name>_DR_Analysis.txt` next to the audio.
- 多文件输入：生成单独的批量汇总 TXT。
- Folder input: produces a batch summary file.
- 可自定义输出路径：`--output <file>`（对批量模式同样适用）。
- Custom output path: supply `--output <file>` (works for batch mode).

## 并行模式简介（Parallel Modes）
- 解码并行：默认针对单文件执行多线程解码 / 窗口处理；提升吞吐而内存占用较小。可使用 `--serial` 禁用。
- Decode parallelism: splits decoding and window processing across threads for a single file (default on); disable with `--serial`.
- 文件级并行：默认同时处理 4 个文件（macOS），可用 `--parallel-files` 调整，或 `--no-parallel-files` 禁用以降低内存。
- File-level parallelism: processes up to four files concurrently (default on macOS); adjust via `--parallel-files` or disable with `--no-parallel-files` to reduce memory footprint.

## 准确性与兼容性（Accuracy & Compatibility）
- 仅支持 1–2 声道输入；多声道音频会被拒绝，并计划后续推出兼容流程（DR 仍排除 LFE）。
- Only mono/stereo sources are accepted—multichannel files are rejected; multichannel support (still excluding LFE) is planned.
- AAC 从 WAV 转码常使 Precise DR 稍微升高（≈0.01–0.05 dB）；其他有损格式通常略降或持平，无损基本不变。
- AAC converted from WAV often raises Precise DR by ≈0.01–0.05 dB; other lossy codecs generally decrease or match, while lossless conversions stay aligned.

## 已知限制（Known Limitations）
- 多声道文件会被拒绝；未来会引入兼容模式。
- Multichannel input is currently rejected; a compatible mode is planned.
- 部分容器的真实比特率可能无法获取，将显示 N/A。
- Some containers may not expose a reliable bitrate, resulting in N/A.

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
| 69 首 FLAC | 默认并行 / Default parallel | 2.959 | 2.764 | 0.503 | 697.99 | 732.11 | 83.94 |
| 106 首 FLAC | 默认并行 / Default parallel | 9.462 | 9.018 | 0.796 | 1241.84 | 1294.15 | 101.56 |
| 106 首 FLAC | 禁用文件并行 / `--no-parallel-files` | 26.997 | 26.035 | 2.591 | 435.34 | 447.81 | 33.43 |

| 数据集 / Dataset | 模式 / Mode | 峰值内存 平均 Avg Peak Memory (MB) | 峰值内存 中位 Median (MB) | 峰值内存 标准差 StdDev (MB) | CPU 峰值 平均 Avg Peak CPU (%) | CPU 平均 Avg CPU (%) |
| --- | --- | ---:| ---:| ---:| ---:| ---:|
| 69 首 FLAC | 默认并行 / Default parallel | 44.27 | 43.12 | 3.47 | 31.58 | 31.05 |
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

**性能警告（Intel 13 代 i9-13900H + Windows 11）**：69 首 FLAC 集在默认 4 并发下耗时约 54.18 秒，吞吐仅 ~21.5 MB/s；较 macOS 同数据集（中位 2.76 秒、732 MB/s）退化显著，推测与混合 P/E 核调度有关，建议暂时降低并发或等待驱动更新。为对照，Windows 10 · i7-11800H 同数据集耗时约 3.061 秒（吞吐 ~732 MB/s）。后续版本会继续针对异构 Intel 平台优化调度策略。
**Performance warning (Intel 13th Gen i9-13900H + Windows 11)**: the 69-track set takes ~54.18 s at default 4-way parallelism (only ~21.5 MB/s), versus macOS’s 2.76 s / 732 MB/s. Hybrid P/E scheduling is the likely culprit; reduce parallelism or await driver fixes. For comparison, Windows 10 on an i7-11800H finishes the same batch in ~3.061 s (~732 MB/s). Future releases will keep iterating on heterogenous Intel scheduling.

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
