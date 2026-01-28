# Performance Benchmarks / 性能基准

本文档包含 MacinMeter DR Tool 的详细性能测试数据。

This document contains detailed performance benchmark data for MacinMeter DR Tool.

---

## 测量方法（Measurement Method）

- macOS 脚本：`scripts/benchmark.sh`（单次）、`scripts/benchmark_10x.sh`（10 次统计）。
- macOS scripts: `scripts/benchmark.sh` (single run) and `scripts/benchmark_10x.sh` (10-run statistics).
- Windows 脚本：`scripts/benchmark.ps1`、`scripts/benchmark-10x.ps1`（与 macOS 采样逻辑一致）。
- Windows scripts: `scripts/benchmark.ps1` and `scripts/benchmark-10x.ps1`, mirroring the macOS sampling logic.
- 指标采集：每 0.1 s 通过 `ps` (或 PowerShell `Get-Process`) 记录 RSS 与累计 CPU 时间；使用 `date +%s.%N` 或 Windows 计时器测量总时长；处理速度＝音频总大小（MB）÷运行时长（s）。
- Sampling: every 0.1 s the scripts record RSS and cumulative CPU time via `ps`/`Get-Process`; total duration uses `date +%s.%N` or Windows timers; throughput = total size (MB) ÷ elapsed time (s).

---

## 测试环境（Test Environment）

- macOS 26 · Apple M4 Pro · 12 cores (8P + 4E)
  macOS 26 with an Apple M4 Pro SoC (12 cores: 8 performance + 4 efficiency) used for all macOS benchmarks.
- Windows 10 · Intel i7-11800H
  Windows 10 laptop with an Intel i7-11800H CPU for the Windows benchmarks below.

---

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

---

## macOS M4 Pro Benchmarks

### 单文件基准（10 次，解码并行 vs 串行） / Single-File Benchmark (10 runs)

| 模式 / Mode | 平均时间 Avg Time (s) | 中位数 Median (s) | 标准差 StdDev (s) | 平均吞吐 Avg Throughput (MB/s) | 中位数 Median (MB/s) | 标准差 StdDev (MB/s) |
| --- | ---:| ---:| ---:| ---:| ---:| ---:|
| 并行解码 / Parallel | 2.780 | 2.766 | 0.069 | 721.55 | 724.89 | 17.99 |
| 串行解码 / `--serial` | 10.809 | 10.588 | 0.584 | 185.90 | 189.41 | 9.84 |

| 模式 / Mode | 峰值内存 平均 Avg Peak Memory (MB) | 峰值内存 中位 Median (MB) | 峰值内存 标准差 StdDev (MB) | CPU 峰值 平均 Avg Peak CPU (%) | CPU 平均 Avg CPU (%) |
| --- | ---:| ---:| ---:| ---:| ---:|
| 并行解码 / Parallel | 47.87 | 44.51 | 7.46 | 33.04 | 32.47 |
| 串行解码 / `--serial` | 25.37 | 25.84 | 0.91 | 8.38 | 8.32 |

### 批量基准（10 次，文件级并行 vs 禁用） / Batch Benchmark (10 runs)

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

---

## Windows Benchmarks

### Windows 10 · Intel i7-11800H (10 runs)

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

### Windows 11 · Intel i9-13900H (10 runs)

69 首 FLAC 集在默认 4 并发下，通过线程优先级优化后性能表现稳定：

The 69-track FLAC set at default 4-way parallelism, with thread priority optimization, shows stable performance:

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

---

## 串行模式对比（Serial Mode Comparison）

| 平台 / Platform | 模式 / Mode | 106 首 FLAC 时间 Time (s) | 吞吐 Throughput (MB/s) | 峰值内存 Peak (MB) | 平均内存 Avg (MB) | 平均 CPU Avg CPU (%) |
| --- | --- | ---:| ---:| ---:| ---:| ---:|
| Windows 10 · i7-11800H | `--serial --no-parallel-files` | 127.452 | 89.4 | 23.38 | 17.76 | 20.0 |
| Windows 10 · i7-11800H | Dynamic Range Meter 1.1.1 | 162.0 | 70.4 | <15 | <15 | 15.0 |

> Windows 完全串行为极端调度或内存受限环境提供兜底，Dynamic Range Meter 1.1.1 记录列供第三方基准参考。

> Fully serial mode offers a fallback when scheduler pressure or memory limits are severe; Dynamic Range Meter 1.1.1 numbers are listed for baseline comparison.

---

## 性能建议（Performance Tips）

- 建议优先使用 Release 构建；并行解码与 SIMD 可显著提升吞吐。
- Prefer release builds; decode parallelism plus SIMD dramatically improve throughput.
- 对大文件可调整 `--parallel-threads` 与 `--parallel-batch` 平衡吞吐与资源。
- Tune `--parallel-threads` and `--parallel-batch` for large files to balance throughput and resource usage.
