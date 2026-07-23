# MacinMeter 0.2.0 — Apple Silicon macOS

> **Unsigned Apple Silicon release / 未签名 Apple Silicon 版本**
>
> This release is built only for Apple Silicon Macs (`arm64`) running macOS
> 11.0 or newer. It has no Apple Developer ID signature and has not been
> notarized. macOS may block the first launch or require an explicit **Open** /
> **Open Anyway** confirmation.
>
> 本版本只面向运行 macOS 11.0 或更新系统的 Apple Silicon（M 系列）Mac。应用没有
> Apple Developer ID 签名，也未经过 Apple 公证。macOS 可能阻止首次启动，或要求
> 用户显式选择**打开** / **仍要打开**。

## Downloads / 下载

- `macinmeter-gui-0.2.0-aarch64-apple-darwin.dmg` — desktop GUI / 桌面 GUI
- `macinmeter-cli-0.2.0-aarch64-apple-darwin.tar.gz` — command-line tool / CLI
- `RELEASE_MANIFEST.json` — source, toolchain, target and artifact identity
- `SHA256SUMS` — SHA-256 for the manifest and both artifacts

There is no Intel or universal macOS build in 0.2.0. Historical Windows,
Linux, and Intel assets belong to earlier releases and are not 0.2.0 packages.

0.2.0 不提供 Intel 或 universal macOS 构建。历史 Release 中的 Windows、Linux
及 Intel 制品属于旧版本，不是 0.2.0 安装包。

## What changed / 主要变化

- Rebuilt the project as a safe Rust workspace with one streaming analyzer and
  one shared application façade for the library, CLI, and GUI.
- Re-established the numeric algorithm against the fixed `foo_dr_meter` 1.0.8
  x64 target and recorded the exact evidence boundary.
- Limited stable decoding to content-probed WAV PCM, FLAC, and AIFF PCM.
- Added schema-v3 reports, bounded cancellation/execution, strict decoder
  contracts, deterministic fixtures, and three-platform correctness CI.
- Added a native Tauri 2 GUI with whole-window file and directory drag-and-drop,
  bilingual presentation, result search and sorting, path hiding, and
  Markdown, JSON, PNG, and SVG export. The final arm64 DMG structure is
  verified on a clean hosted macOS runner.

- 项目已重建为安全 Rust workspace；库、CLI 与 GUI 共用唯一流式分析器和 application
  façade。
- 围绕固定 `foo_dr_meter` 1.0.8 x64 目标重建数值算法，并记录准确的证据边界。
- 稳定解码范围限定为按内容探测的 WAV PCM、FLAC 与 AIFF PCM。
- 新增 schema-v3 报告、有界取消/执行、严格解码契约、确定性 fixture 与三平台
  正确性 CI。
- 新增原生 Tauri 2 GUI，支持整窗文件与目录拖放、中英文界面、结果搜索与排序、
  路径隐藏，以及 Markdown、JSON、PNG 和 SVG 导出；最终 arm64 DMG 结构会在
  clean hosted macOS runner 上验证。

## Opening the unsigned GUI / 打开未签名 GUI

After copying the app from the DMG, macOS may require Control-clicking the app
and choosing **Open**, or approving it from **System Settings → Privacy &
Security**. Only continue if the downloaded SHA-256 matches `SHA256SUMS` from
this Release.

从 DMG 复制应用后，macOS 可能要求按住 Control 点击应用并选择**打开**，或在
**系统设置 → 隐私与安全性**中确认。只有当下载文件的 SHA-256 与本 Release 的
`SHA256SUMS` 一致时才应继续。

## Known boundaries / 已知边界

- No Developer ID signing, notarization, or Gatekeeper readiness claim.
- No Intel/universal macOS, Windows GUI, or Linux GUI package.
- No FFmpeg, DSD, lossy-codec, preprocessing, SIMD, or file-parallel backend.
- Batch processing remains serial and produces independent track reports.

- 不包含 Developer ID 签名、公证，也不声明已通过 Gatekeeper 分发要求。
- 不提供 Intel/universal macOS、Windows GUI 或 Linux GUI 包。
- 不包含 FFmpeg、DSD、有损格式、预处理、SIMD 或文件级并行 backend。
- batch 继续串行运行，并产生相互独立的逐轨报告。
