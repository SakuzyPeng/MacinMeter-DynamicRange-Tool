# MacinMeter DR Tool — Quick Reference

[English](README.md) | [中文](README_CN.md)

![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)
![License](https://img.shields.io/badge/license-MIT-blue.svg?style=for-the-badge)
![Branch](https://img.shields.io/badge/branch-main-green.svg?style=for-the-badge)

**A foobar2000-compatible implementation aiming for better experience**

*Tribute to Janne Hyvärinen's pioneering work*

MacinMeter DR Tool learns and implements the algorithm principles of foobar2000 DR Meter 1.0.3 (foo_dr_meter, by Janne Hyvärinen), striving to provide **accurate DR analysis results** and **faster processing speed**. With streaming architecture design, we hope to bring convenience to users. Performance benchmarks are compared against Dynamic Range Meter 1.1.1 (foo_dynamic_range).

---

## Introduction

- Follows the foobar2000 DR Meter specification, returning both Official DR (rounded) and Precise DR values with SIMD and parallel optimisations.
- Supports 12+ mainstream audio formats (FLAC, WAV, AAC, MP3, Opus, etc.) with automatic fallback to FFmpeg when needed. See "Supported Audio Formats" section for details.
- Precise DR typically differs from foobar2000 by ±0.02–0.05 dB (window selection & rounding). A few tracks may drift by ~0.1 dB (e.g., tail-window participation in the top-20% selection or masters with different leading/trailing samples). The tool surfaces boundary warnings so you can cross-check with foobar2000 when results matter.

## Build & Run

```bash
cargo build --release                                              # Build
cargo run --release -- <audio-or-folder>                           # Run directly
./target/release/MacinMeter-DynamicRange-Tool-foo_dr <path>        # Launch binary
cargo test                                                         # Test
```

## Tauri GUI

The `tauri-app/` directory provides a Tauri 2 GUI that reuses the same DR engine. Select audio via system dialog and view Official/Precise DR, silence filtering and trim reports.

Run with `cd tauri-app && npm install && npm run tauri dev`. Build release with `npm run tauri build`. See `docs/tauri_wrapper.md` for details.

## Quick Start

1. **Double-click launch**: Automatically scans the executable directory. Multiple files produce a batch summary, single files generate `<name>_DR_Analysis.txt`.

2. **CLI examples**:
   ```bash
   ./target/release/MacinMeter-DynamicRange-Tool-foo_dr song.flac      # Single file
   ./target/release/MacinMeter-DynamicRange-Tool-foo_dr album_dir      # Folder (4 concurrent files)
   ```

3. **Verbose logging**: Append `--verbose` to view detailed processing logs.

## Key CLI Options

**Parallel controls** (decode parallelism on by default; multi-file parallelism defaults to 4):
- `--parallel-threads <N>`: number of decoding threads (default 4)
- `--parallel-batch <N>`: decode batch size (default 64)
- `--parallel-files <N>` / `--no-parallel-files`: concurrent files (default 4) / disable
- `--serial`: disable decode parallelism

**Output control**: `--output <file>` for single-file reports; batch mode writes to target directory by default.

**Experimental features** (disabled by default):
- `--trim-edges[=<DB>]`: edge trimming, default −60 dBFS; `--trim-min-run <MS>` (default 60 ms)
- `--filter-silence[=<DB>]`: window-level silence filtering, default −70 dBFS
- `--exclude-lfe`: exclude LFE channels from final DR aggregation
- `--show-rms-peak`: append RMS/Peak diagnostics table in single-file reports

## Output Format

Reports list DR per channel, Official DR, Precise DR, plus audio metadata (sample rate, channels, bit depth, bitrate, codec).

### Single File Example
```markdown
MacinMeter DR Tool vX.X.X | DR15 (15.51 dB)
audio.flac | 7:02 | 48000 Hz | 2ch | FLAC

| Channel | DR       | Peak     |
|---------|----------|----------|
|  Left   | 14.57 dB | -0.12 dB |
|  Right  | 16.46 dB | -0.08 dB |

> Boundary Risk (High): 15.51 dB is 0.01 dB from DR15/DR16 boundary
```

### Batch Example
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

**Markers**: `*` = LFE excluded · `†` = Silent channels excluded

## Accuracy & Compatibility

- Multichannel (≥3 ch) supported; aggregation matches foobar2000: arithmetic mean of per-channel DR over all non-silent channels, rounded to nearest integer.
- Optional LFE exclusion (`--exclude-lfe`): effective only with channel layout metadata. Per-channel DRs are still listed.
- AAC converted from WAV often raises Precise DR by ≈0.01–0.05 dB; other lossy codecs generally decrease or match.

## Performance Summary

For detailed benchmarks, see [docs/BENCHMARKS.md](docs/BENCHMARKS.md).

| Platform | Dataset | Throughput |
|----------|---------|----------:|
| macOS · M4 Pro | 1.6 GB single file | ~725 MB/s |
| macOS · M4 Pro | 69 FLACs (1.17 GB) | ~1168 MB/s |
| Windows · i9-13900H | 69 FLACs (1.17 GB) | ~568 MB/s |

**Tips**: Use release builds; tune `--parallel-threads` and `--parallel-batch` for large files.

---

## Supported Audio Formats

For details, see [docs/SUPPORTED_FORMATS.md](docs/SUPPORTED_FORMATS.md).

| Category | Formats | Decoder |
|----------|---------|---------|
| Lossless | FLAC, ALAC, WAV, AIFF, PCM | Symphonia |
| Lossy | AAC, OGG Vorbis, MP1, MP3, Opus | Symphonia / songbird |
| Video Codec | AC-3, E-AC-3, DTS, DSD | FFmpeg (auto) |
| Containers | MP4/M4A, MKV, WebM | Smart routing |

**FFmpeg Installation**: macOS `brew install ffmpeg` · Windows `winget install Gyan.FFmpeg` · Linux package manager

---

## License & Acknowledgements

**MIT License** - See [LICENSE](LICENSE) for details.

For legal compliance, third-party notices, and disclaimer, see [docs/LEGAL.md](docs/LEGAL.md).

---

## Related Links

- **Reference Implementation**: foobar2000 DR Meter 1.0.3 (foo_dr_meter)
  - **Author**: Janne Hyvärinen
  - **Official**: https://foobar.hyv.fi/?view=foo_dr_meter
- **Performance Benchmark**: Dynamic Range Meter 1.1.1 (foo_dynamic_range)
  - **Based on**: TT Dynamic Range Offline Meter from the Pleasurize Music Foundation

---

**Built for Professional Audio Production**
