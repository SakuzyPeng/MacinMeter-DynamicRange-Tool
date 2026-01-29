# 支持的音频格式 / Supported Audio Formats

## 解码器路由 / Decoder Routing

工具采用智能自动路由，优先使用 Symphonia，必要时自动切换 FFmpeg：

The tool uses smart auto-routing, preferring Symphonia with automatic FFmpeg fallback when needed.

### Symphonia 原生支持 / Native Symphonia Support

- **无损格式 / Lossless**: FLAC, ALAC (Apple Lossless), WAV, AIFF, PCM
- **有损格式 / Lossy**: AAC, OGG Vorbis, MP1 (MPEG Layer I)
- **容器格式 / Containers**: MP4/M4A（仅限 Symphonia 支持的编码），MKV/WebM

### 专用解码器 / Dedicated Decoders

- **Opus**: 通过 songbird 专用解码器 (Discord 音频库) / Via songbird decoder (Discord audio library)
- **MP3**: 有状态解码格式，强制串行处理 / Stateful format, forced serial decoding

### FFmpeg 自动回退 / Auto Fallback to FFmpeg

当 Symphonia 无法支持时，工具会自动切换到 FFmpeg 进行解码。

When Symphonia cannot decode a format, the tool automatically falls back to FFmpeg.

**典型场景 / Typical cases**:
- 扩展名为 `.ac3`、`.ec3`、`.eac3`、`.dts`、`.dsf`、`.dff` → 直接使用 FFmpeg
- For extensions `.ac3`, `.ec3/.eac3`, `.dts`, `.dsf`, `.dff` → use FFmpeg directly
- MP4/M4A 容器包含 AC-3、E-AC-3（含 Dolby Atmos）、DTS → 自动切换 FFmpeg
- MP4/M4A containers with AC-3/E-AC-3 (incl. Atmos) or DTS → auto-switch to FFmpeg
- 其他容器（部分 MKV/MP4 变体）内的不兼容编码 → 自动回退 FFmpeg
- Incompatible codecs inside containers (some MKV/MP4 variants) → auto fallback to FFmpeg

---

## FFmpeg 安装 / FFmpeg Installation

如需使用 FFmpeg 功能，请确保系统已安装 `ffmpeg` 和 `ffprobe`：

To use FFmpeg features, make sure both `ffmpeg` and `ffprobe` are installed:

| 平台 / Platform | 安装命令 / Install Command |
|-----------------|---------------------------|
| **macOS** | `brew install ffmpeg` |
| **Windows** | `winget install Gyan.FFmpeg`（或 Chocolatey） |
| **Ubuntu/Debian** | `sudo apt install ffmpeg` |
| **Fedora/RHEL** | `sudo dnf install ffmpeg` |
| **Arch** | `sudo pacman -S ffmpeg` |

验证安装：`ffmpeg -version` 与 `ffprobe -version` 应返回版本号；工具会自动检测 PATH 中的二者。

Verify: both `ffmpeg -version` and `ffprobe -version` should print a version; the tool auto-detects them from PATH.

---

## 多声道与 LFE 支持 / Multichannel & LFE Support

- **多声道分析**：支持 3-32 声道音频，每声道独立计算 DR，输出详细的 per-channel 结果
- Multichannel analysis: supports 3–32 channels; per-channel DR is computed and listed.

- **Official DR 聚合**：对所有"非静音"声道的 DR 值进行算术平均并四舍五入（foobar2000 口径）
- Official aggregation: arithmetic mean of all non-silent channel DRs, rounded (foobar2000 style).

- **LFE 识别**：
  - 通过 Symphonia：自动检测声道布局元数据（如 WAV WAVEFORMATEXTENSIBLE 掩码、部分 MP4/MKV）
  - Via Symphonia: auto-detects layout metadata (e.g., WAV WAVEFORMATEXTENSIBLE masks, some MP4/MKV).
  - 通过 FFmpeg：读取 ffprobe JSON 标签序列（如 `FL+FR+FC+LFE+…`），精确定位 LFE 位置
  - Via FFmpeg: parses ffprobe JSON label sequences (e.g., `FL+FR+FC+LFE+…`) to locate LFE accurately.

- **LFE 剔除（可选）**：使用 `--exclude-lfe` 在最终聚合中排除 LFE；单声道 DR 明细仍保持输出
- LFE exclusion (optional): enable `--exclude-lfe` to drop LFE from the aggregate; per-channel DR lines remain.

---

## DSD 处理 / DSD Processing

### 参数选项 / Options

| 参数 / Flag | 说明 / Description | 默认值 / Default |
|-------------|-------------------|-----------------|
| `--dsd-pcm-rate` | 目标采样率 / Target sample rate | 352800 Hz |
| `--dsd-gain-db` | 线性增益 / Linear gain (0 to disable) | +6.0 dB |
| `--dsd-filter` | 低通滤波器 / Low-pass filter | teac |

### 滤波器模式 / Filter Modes

**teac (TEAC Narrow)**:
- DSD64 → 39 kHz
- DSD128 → 78 kHz
- DSD256 → 156 kHz
- DSD512 → 312 kHz
- DSD1024 → 624 kHz
- 并按 0.45×Fs（目标采样率）限顶 / capped at 0.45×Fs (target rate)

**studio**:
- 固定 20 kHz（仅可听带宽）/ fixed 20 kHz (audible-band only)

**off**:
- 关闭低通（仅诊断；超声噪声进入 RMS 可能降低 DR；与 +6 dB 同用时存在削顶风险）
- No extra low-pass (diagnostic; ultrasonics enter RMS and may reduce DR; clipping risk with +6 dB)

### 输出格式 / Output Format

- 统一输出 32-bit float（F32LE），便于后续计算与一致性
- Unified F32LE output for consistency and easy processing
- 报告显示 DSD 源："原生一位采样率与档位 → 处理采样率"，位深显示为 "1 (DSD 1-bit, processed as f32)"
- Reports show "native 1-bit rate & tier → processed rate"; bit depth printed as "1 (DSD 1-bit, processed as f32)"

---

## 格式汇总 / Format Summary

**12+ 种主流音频格式** / 12+ mainstream formats，覆盖 90%+ 用户需求：

| 分类 / Category | 格式 / Formats | 解码器 / Decoder |
|-----------------|----------------|------------------|
| 无损 Lossless | FLAC, ALAC, WAV, AIFF, PCM | Symphonia |
| 有损 Lossy | AAC, OGG Vorbis, MP1 | Symphonia |
| 音乐编码 Proprietary | MP3 | Symphonia (串行 Serial) |
| 音乐编码 Proprietary | Opus | songbird (专用 Dedicated) |
| 影音编码 Video Codec | AC-3, E-AC-3, DTS, DSD | FFmpeg (自动回退 Auto) |
| 容器 Containers | MP4/M4A, MKV, WebM | Symphonia / FFmpeg (智能路由 Smart) |

### 并行性能说明 / Parallelism Notes

- **MP3** 采用串行处理（有状态格式），其他格式均支持并行加速
- MP3 uses serial decoding (stateful format); other formats support parallel acceleration.
- **多声道** 使用零拷贝跨步优化，3+ 声道性能提升 8-16 倍
- Multichannel uses zero-copy strided optimization with 8–16× performance gain for 3+ channels.
