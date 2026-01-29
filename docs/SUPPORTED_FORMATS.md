[English](SUPPORTED_FORMATS.md) | [中文](SUPPORTED_FORMATS_CN.md)

# Supported Audio Formats

## Decoder Routing

The tool uses smart auto-routing, preferring Symphonia with automatic FFmpeg fallback when needed.

### Native Symphonia Support

- **Lossless**: FLAC, ALAC (Apple Lossless), WAV, AIFF, PCM
- **Lossy**: AAC, OGG Vorbis, MP1 (MPEG Layer I)
- **Containers**: MP4/M4A (Symphonia-supported codecs only), MKV/WebM

### Dedicated Decoders

- **Opus**: Via songbird decoder (Discord audio library)
- **MP3**: Stateful format, forced serial decoding

### Auto Fallback to FFmpeg

When Symphonia cannot decode a format, the tool automatically falls back to FFmpeg.

**Typical cases**:
- For extensions `.ac3`, `.ec3/.eac3`, `.dts`, `.dsf`, `.dff` → use FFmpeg directly
- MP4/M4A containers with AC-3/E-AC-3 (incl. Atmos) or DTS → auto-switch to FFmpeg
- Incompatible codecs inside containers (some MKV/MP4 variants) → auto fallback to FFmpeg

---

## FFmpeg Installation

To use FFmpeg features, make sure both `ffmpeg` and `ffprobe` are installed:

| Platform | Install Command |
|----------|-----------------|
| **macOS** | `brew install ffmpeg` |
| **Windows** | `winget install Gyan.FFmpeg` (or Chocolatey) |
| **Ubuntu/Debian** | `sudo apt install ffmpeg` |
| **Fedora/RHEL** | `sudo dnf install ffmpeg` |
| **Arch** | `sudo pacman -S ffmpeg` |

Verify: both `ffmpeg -version` and `ffprobe -version` should print a version; the tool auto-detects them from PATH.

---

## Multichannel & LFE Support

- **Multichannel analysis**: supports 3–32 channels; per-channel DR is computed and listed.

- **Official aggregation**: arithmetic mean of all non-silent channel DRs, rounded (foobar2000 style).

- **LFE detection**:
  - Via Symphonia: auto-detects layout metadata (e.g., WAV WAVEFORMATEXTENSIBLE masks, some MP4/MKV).
  - Via FFmpeg: parses ffprobe JSON label sequences (e.g., `FL+FR+FC+LFE+…`) to locate LFE accurately.

- **LFE exclusion (optional)**: enable `--exclude-lfe` to drop LFE from the aggregate; per-channel DR lines remain.

---

## DSD Processing

### Options

| Flag | Description | Default |
|------|-------------|---------|
| `--dsd-pcm-rate` | Target sample rate | 352800 Hz |
| `--dsd-gain-db` | Linear gain (0 to disable) | +6.0 dB |
| `--dsd-filter` | Low-pass filter | teac |

### Filter Modes

**teac (TEAC Narrow)**:
- DSD64 → 39 kHz
- DSD128 → 78 kHz
- DSD256 → 156 kHz
- DSD512 → 312 kHz
- DSD1024 → 624 kHz
- Capped at 0.45×Fs (target rate)

**studio**:
- Fixed 20 kHz (audible-band only)

**off**:
- No extra low-pass (diagnostic; ultrasonics enter RMS and may reduce DR; clipping risk with +6 dB)

### Output Format

- Unified F32LE output for consistency and easy processing
- Reports show "native 1-bit rate & tier → processed rate"; bit depth printed as "1 (DSD 1-bit, processed as f32)"

---

## Format Summary

**12+ mainstream formats**, covering 90%+ user needs:

| Category | Formats | Decoder |
|----------|---------|---------|
| Lossless | FLAC, ALAC, WAV, AIFF, PCM | Symphonia |
| Lossy | AAC, OGG Vorbis, MP1 | Symphonia |
| Proprietary | MP3 | Symphonia (Serial) |
| Proprietary | Opus | songbird (Dedicated) |
| Video Codec | AC-3, E-AC-3, DTS, DSD | FFmpeg (Auto) |
| Containers | MP4/M4A, MKV, WebM | Symphonia / FFmpeg (Smart) |

### Parallelism Notes

- MP3 uses serial decoding (stateful format); other formats support parallel acceleration.
- Multichannel uses zero-copy strided optimization with 8–16× performance gain for 3+ channels.
