# MacinMeter 0.3.0 release draft / 发布草案

> Draft only. No tag, GitHub Release, signed artifact, or notarized build has
> been created. / 仅为草案；尚未创建 tag、GitHub Release、签名制品或公证构建。

## English

MacinMeter 0.3.0 adds a constrained, stable in-process MP4/M4A + ALAC route.

Highlights:

- analyzes non-fragmented, audio-only `.m4a` and `.mp4` files containing one
  ALAC compatible-version-0 track;
- supports 16- and 24-bit ALAC with 1–8 standard-layout channels;
- probes ISO BMFF structures before decoder creation and cross-checks the ALAC
  cookie, sample entry, media timing, sample tables, backend codec identity,
  sample rate, and exact decoded frame count;
- adds `.m4a` and `.mp4` to capability-driven directory discovery;
- adds bounded packet-level decoding for the ALAC route and for FLAC streams
  whose packet geometry fits the granted reservation, under the single
  application-owned worker and memory plan accepted by
  [ADR-0014](adr/0014-deterministic-decode-analysis-pipeline.md); a report and
  its decoded PCM are identical whatever worker count a host grants, and
  file-level parallelism remains disabled in the product while window-level
  parallelism remains unimplemented;
- retains the in-process Symphonia implementation, finite interleaved `f64`,
  the single `Application` façade, and the fixed analysis rules;
- upgrades the shared CLI/Tauri `WireEnvelope` to schema v4 for the new
  `mp4` container and `alac` codec identifiers.

The first stable slice does not include AAC, fragmented MP4, video or extra
tracks, multiple audio tracks, cropped edit lists, ALAC 20/32-bit, nonstandard
channel layouts, CAF/raw ALAC, AIFC, resampling, or an FFmpeg runtime. FFmpeg
8.0.1 is used only to regenerate the committed synthetic ALAC corpus.

The GUI release boundary is unchanged: unsigned and unnotarized Apple Silicon
macOS 11.0+, with no Intel/universal, Windows, or Linux GUI artifact. CLI
source builds remain available on supported Rust hosts. Checksums and artifact
identities will be added only after an explicit release-staging operation.

## 中文

MacinMeter 0.3.0 新增一条受限、稳定、进程内的 MP4/M4A + ALAC 路径。

主要变化：

- 分析仅包含一条 ALAC compatible-version-0 音轨的非 fragmented、纯音频 `.m4a`
  与 `.mp4` 文件；
- 支持 16/24-bit、1–8 个标准布局声道；
- 在创建 decoder 前首检 ISO BMFF，并交叉核对 ALAC cookie、sample entry、媒体
  时序、sample tables、backend codec 身份、采样率和最终精确解码帧数；
- capability 驱动的目录发现新增 `.m4a` 与 `.mp4`；
- 在 [ADR-0014](adr/0014-deterministic-decode-analysis-pipeline.md) 接受的唯一
  application 自有 worker 与内存计划下，为 ALAC route 以及 packet 几何能落入已授予
  reservation 的 FLAC 流启用有界 packet 级解码；无论宿主授予多少 worker，报告与
  解码 PCM 完全相同；文件级并行仍未在产品中启用，窗口级并行仍未实现；
- 保持进程内 Symphonia 实现、有限交错 `f64`、唯一 `Application` façade 和固定
  分析规则；
- 共享 CLI/Tauri `WireEnvelope` 因新增 `mp4` container 与 `alac` codec 标识升级
  到 schema v4。

首批稳定范围不包含 AAC、fragmented MP4、视频或额外 track、多音轨、裁剪 edit
list、ALAC 20/32-bit、非标准声道布局、CAF/raw ALAC、AIFC、重采样或 FFmpeg
runtime。FFmpeg 8.0.1 只用于再生成仓库提交的合成 ALAC 语料。

GUI 发行边界不变：仅面向 Apple Silicon macOS 11.0+，仍未签名、未公证；不提供
Intel/universal、Windows 或 Linux GUI 制品。只有显式执行 release staging 后，才会
把校验和及制品身份补入正式发布材料。
