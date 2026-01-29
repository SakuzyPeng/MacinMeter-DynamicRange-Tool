[English](BENCHMARKS.md) | [中文](BENCHMARKS_CN.md)

# Performance Benchmarks

This document contains detailed performance benchmark data for MacinMeter DR Tool.

---

## Measurement Method

- macOS scripts: `scripts/benchmark.sh` (single run) and `scripts/benchmark_10x.sh` (10-run statistics).
- Windows scripts: `scripts/benchmark.ps1` and `scripts/benchmark-10x.ps1`, mirroring the macOS sampling logic.
- Sampling: every 0.1 s the scripts record RSS and cumulative CPU time via `ps`/`Get-Process`; total duration uses `date +%s.%N` or Windows timers; throughput = total size (MB) ÷ elapsed time (s).

---

## Test Environment

- macOS 26 with an Apple M4 Pro SoC (12 cores: 8 performance + 4 efficiency) used for all macOS benchmarks.
- Windows 10 laptop with an Intel i7-11800H CPU for the Windows benchmarks below.

---

## Datasets

| Category | Size & Count | Codec/Channels | Sample Rates | Bit Depth | Notes |
| --- | --- | --- | --- | --- | --- |
| Single large file | ~1.6 GB · 1 track · ~94 min | FLAC / 2ch | 96 kHz | 24-bit | via mediainfo |
| 69-track FLAC set | ~1.17 GB · 69 tracks | FLAC / 2ch | 48 kHz × 69 | 24-bit × 69 | typical CD compilation |
| 106-track FLAC set | ~11.7 GB · 106 tracks | FLAC / 2ch | 96×50, 48×46, 192×8, 88.2×2 | 24-bit × 106 | batch benchmark |

---

## macOS M4 Pro Benchmarks

### Single-File Benchmark (10 runs)

| Mode | Avg Time (s) | Median (s) | StdDev (s) | Avg Throughput (MB/s) | Median (MB/s) | StdDev (MB/s) |
| --- | ---:| ---:| ---:| ---:| ---:| ---:|
| Parallel | 2.780 | 2.766 | 0.069 | 721.55 | 724.89 | 17.99 |
| `--serial` | 10.809 | 10.588 | 0.584 | 185.90 | 189.41 | 9.84 |

| Mode | Avg Peak Memory (MB) | Median (MB) | StdDev (MB) | Avg Peak CPU (%) | Avg CPU (%) |
| --- | ---:| ---:| ---:| ---:| ---:|
| Parallel | 47.87 | 44.51 | 7.46 | 33.04 | 32.47 |
| `--serial` | 25.37 | 25.84 | 0.91 | 8.38 | 8.32 |

### Batch Benchmark (10 runs)

| Dataset | Mode | Avg Time (s) | Median (s) | StdDev (s) | Avg Throughput (MB/s) | Median (MB/s) | StdDev (MB/s) |
| --- | --- | ---:| ---:| ---:| ---:| ---:| ---:|
| 69 FLAC tracks | Default parallel | 1.178 | 1.025 | 0.457 | 1113.84 | 1167.73 | 159.40 |
| 106 FLAC tracks | Default parallel | 9.462 | 9.018 | 0.796 | 1241.84 | 1294.15 | 101.56 |
| 106 FLAC tracks | `--no-parallel-files` | 26.997 | 26.035 | 2.591 | 435.34 | 447.81 | 33.43 |

| Dataset | Mode | Avg Peak Memory (MB) | Median (MB) | StdDev (MB) | Avg Peak CPU (%) | Avg CPU (%) |
| --- | --- | ---:| ---:| ---:| ---:| ---:|
| 69 FLAC tracks | Default parallel | 97.65 | 94.22 | 11.72 | 64.62 | 61.99 |
| 106 FLAC tracks | Default parallel | 1743.26 | 1679.38 | 183.64 | 80.54 | 78.69 |
| 106 FLAC tracks | `--no-parallel-files` | 87.41 | 87.71 | 2.73 | 21.69 | 20.02 |

---

## Windows Benchmarks

### Windows 10 · Intel i7-11800H (10 runs)

- Same sampling as macOS scripts; results normalized to seconds and megabytes.
- Windows runs now mirror the deduplicated 106-track set used on macOS (metrics unchanged from the former 107-track set).

| Dataset | Mode | Avg Time (s) | Median (s) | StdDev (s) | Avg Throughput (MB/s) | Median (MB/s) | StdDev (MB/s) |
| --- | --- | ---:| ---:| ---:| ---:| ---:| ---:|
| Single large file | Default parallel | 6.090 | 6.085 | 0.014 | 248.05 | 248.26 | 0.56 |
| 69 FLAC tracks | Default parallel | 3.057 | 3.051 | 0.027 | 381.49 | 382.23 | 3.30 |
| 106 FLAC tracks | Default parallel | 16.411 | 16.213 | 0.416 | 715.07 | 723.44 | 17.49 |

| Dataset | Avg Peak Memory (MB) | Median (MB) | StdDev (MB) | Avg Memory (MB) | Median (MB) | StdDev (MB) |
| --- | ---:| ---:| ---:| ---:| ---:| ---:|
| Single large file | 217.89 | 217.87 | 12.90 | 135.39 | 136.78 | 8.01 |
| 69 FLAC tracks | 40.17 | 40.21 | 1.05 | 26.84 | 26.75 | 0.55 |
| 106 FLAC tracks | 178.67 | 175.07 | 23.57 | 98.82 | 99.24 | 10.97 |

### Windows 11 · Intel i9-13900H (10 runs)

The 69-track FLAC set at default 4-way parallelism, with thread priority optimization, shows stable performance:

| Metric | Median | Average | StdDev |
| --- | ---:| ---:| ---:|
| Time | 2.052 s | 2.056 s | 0.016 s |
| Throughput | 568.18 MB/s | 567.23 MB/s | 4.28 MB/s |
| Peak Memory | 41.43 MB | 40.09 MB | 4.49 MB |
| Avg Memory | 21.98 MB | 21.31 MB | 2.25 MB |
| Avg CPU | 36.57% | 36.30% | 1.32% |
| Peak CPU | 43.19% | 43.54% | 2.09% |

Compared to macOS M4 Pro (median 1.025 s / 1167.73 MB/s), the i9-13900H achieves median 2.05 s / 568 MB/s on the same 69-track FLAC set (1.17GB), reaching approximately 49% of M4 Pro's performance. Hybrid P/E core scheduling has been improved through thread priority optimization.

---

## Serial Mode Comparison

| Platform | Mode | 106 FLAC Time (s) | Throughput (MB/s) | Peak (MB) | Avg (MB) | Avg CPU (%) |
| --- | --- | ---:| ---:| ---:| ---:| ---:|
| Windows 10 · i7-11800H | `--serial --no-parallel-files` | 127.452 | 89.4 | 23.38 | 17.76 | 20.0 |
| Windows 10 · i7-11800H | Dynamic Range Meter 1.1.1 | 162.0 | 70.4 | <15 | <15 | 15.0 |

> Fully serial mode offers a fallback when scheduler pressure or memory limits are severe; Dynamic Range Meter 1.1.1 numbers are listed for baseline comparison.

---

## DSD/FFmpeg Pipe Optimization

### Test Dataset

| Category | Size | Codec | Sample Rate | Notes |
| --- | --- | --- | --- | --- |
| 10 DSD tracks | ~3.85 GB · 10 tracks | DSF / 2ch | DSD128 (5.6 MHz) | Downsampled to 352.8 kHz via FFmpeg |

### Optimization Details

Async double-buffering architecture:

```
FFmpeg → [Pipe] → Reader thread (1MB BufReader) → [Channel 4×512KB] → Main thread
```

- BufReader increased from 128KB to 1MB
- New async reader thread for continuous pre-reading
- Data chunks passed via crossbeam-channel (capacity: 4 chunks × 512KB)
- Reduces FFmpeg write-side blocking, mitigating Windows 4KB pipe buffer limitation

### macOS M4 Pro · DSD Benchmark (10 runs)

| Version | Avg Time (s) | Median (s) | Min (s) | Max (s) | Improvement |
| --- | ---:| ---:| ---:| ---:| ---:|
| Before (no BufReader) | 17.914 | 17.927 | 17.274 | 18.919 | — |
| **After (async 1MB)** | **11.603** | **11.545** | **11.302** | **12.020** | **+35%** |

### Windows 10 · Intel i7-11800H · DSD Benchmark (10 runs)

| Version | Avg Time (s) | Median (s) | Throughput (MB/s) | Improvement |
| --- | ---:| ---:| ---:| ---:|
| Before (2025-11-04) | 245.815 | 244.557 | 15.68 | — |
| **After (2026-01-29)** | **16.824** | **16.793** | **229.07** | **14.6× faster** |

| Version | Peak Memory (MB) | Avg Memory (MB) | Avg CPU (%) | Peak CPU (%) |
| --- | ---:| ---:| ---:| ---:|
| Before (2025-11-04) | 201.41 | 153.46 | 22.48 | 26.93 |
| **After (2026-01-29)** | **316.45** | **197.69** | **10.70** | **14.22** |

### Cross-Platform Comparison

After optimization, the macOS vs Windows performance gap narrowed from **13.7×** to **1.45×**:

| Platform | 10 DSD Median (s) | Throughput (MB/s) | Relative |
| --- | ---:| ---:| ---:|
| macOS M4 Pro | 11.545 | ~333 | 1.00× |
| Windows i7-11800H | 16.793 | 229 | 0.69× |

> Windows DSD processing improved **14.6×**, primarily due to async double-buffering eliminating FFmpeg write stalls caused by the 4KB pipe buffer limitation.

---

## Performance Tips

- Prefer release builds; decode parallelism plus SIMD dramatically improve throughput.
- Tune `--parallel-threads` and `--parallel-batch` for large files to balance throughput and resource usage.
