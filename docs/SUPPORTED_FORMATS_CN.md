[English](SUPPORTED_FORMATS.md) | [中文](SUPPORTED_FORMATS_CN.md)

# 支持的音频格式

## 解码器路由

工具采用智能自动路由，优先使用 Symphonia，必要时自动切换 FFmpeg。

### Symphonia 原生支持

- **无损格式**: FLAC, ALAC (Apple Lossless), WAV, AIFF, PCM
- **有损格式**: AAC, OGG Vorbis, MP1 (MPEG Layer I)
- **容器格式**: MP4/M4A（仅限 Symphonia 支持的编码），MKV/WebM

### 专用解码器

- **Opus**: 通过 songbird 专用解码器 (Discord 音频库)
- **MP3**: 有状态解码格式，强制串行处理

### FFmpeg 自动回退

当 Symphonia 无法支持时，工具会自动切换到 FFmpeg 进行解码。

**典型场景**:
- 扩展名为 `.ac3`、`.ec3`、`.eac3`、`.dts`、`.dsf`、`.dff` → 直接使用 FFmpeg
- MP4/M4A 容器包含 AC-3、E-AC-3（含 Dolby Atmos）、DTS → 自动切换 FFmpeg
- 其他容器（部分 MKV/MP4 变体）内的不兼容编码 → 自动回退 FFmpeg

---

## FFmpeg 安装

如需使用 FFmpeg 功能，请确保系统已安装 `ffmpeg` 和 `ffprobe`：

| 平台 | 安装命令 |
|------|----------|
| **macOS** | `brew install ffmpeg` |
| **Windows** | `winget install Gyan.FFmpeg`（或 Chocolatey） |
| **Ubuntu/Debian** | `sudo apt install ffmpeg` |
| **Fedora/RHEL** | `sudo dnf install ffmpeg` |
| **Arch** | `sudo pacman -S ffmpeg` |

验证安装：`ffmpeg -version` 与 `ffprobe -version` 应返回版本号；工具会自动检测 PATH 中的二者。

---

## 多声道与 LFE 支持

- **多声道分析**：支持 3-32 声道音频，每声道独立计算 DR，输出详细的 per-channel 结果

- **Official DR 聚合**：对所有"非静音"声道的 DR 值进行算术平均并四舍五入（foobar2000 口径）

- **LFE 识别**：
  - 通过 Symphonia：自动检测声道布局元数据（如 WAV WAVEFORMATEXTENSIBLE 掩码、部分 MP4/MKV）
  - 通过 FFmpeg：读取 ffprobe JSON 标签序列（如 `FL+FR+FC+LFE+…`），精确定位 LFE 位置

- **LFE 剔除（可选）**：使用 `--exclude-lfe` 在最终聚合中排除 LFE；单声道 DR 明细仍保持输出

---

## DSD 处理

### 参数选项

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `--dsd-pcm-rate` | 目标采样率 | 352800 Hz |
| `--dsd-gain-db` | 线性增益 (0 禁用) | +6.0 dB |
| `--dsd-filter` | 低通滤波器 | teac |

### 滤波器模式

**teac (TEAC Narrow)**:
- DSD64 → 39 kHz
- DSD128 → 78 kHz
- DSD256 → 156 kHz
- DSD512 → 312 kHz
- DSD1024 → 624 kHz
- 并按 0.45×Fs（目标采样率）限顶

**studio**:
- 固定 20 kHz（仅可听带宽）

**off**:
- 关闭低通（仅诊断；超声噪声进入 RMS 可能降低 DR；与 +6 dB 同用时存在削顶风险）

### 输出格式

- 统一输出 32-bit float（F32LE），便于后续计算与一致性
- 报告显示 DSD 源："原生一位采样率与档位 → 处理采样率"，位深显示为 "1 (DSD 1-bit, processed as f32)"

---

## 格式汇总

**12+ 种主流音频格式**，覆盖 90%+ 用户需求：

| 分类 | 格式 | 解码器 |
|------|------|--------|
| 无损 | FLAC, ALAC, WAV, AIFF, PCM | Symphonia |
| 有损 | AAC, OGG Vorbis, MP1 | Symphonia |
| 音乐编码 | MP3 | Symphonia (串行) |
| 音乐编码 | Opus | songbird (专用) |
| 影音编码 | AC-3, E-AC-3, DTS, DSD | FFmpeg (自动回退) |
| 容器 | MP4/M4A, MKV, WebM | Symphonia / FFmpeg (智能路由) |

### 并行性能说明

- **MP3** 采用串行处理（有状态格式），其他格式均支持并行加速
- **多声道** 使用零拷贝跨步优化，3+ 声道性能提升 8-16 倍
