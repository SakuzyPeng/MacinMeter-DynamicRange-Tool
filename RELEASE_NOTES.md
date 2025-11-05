# Release Notes / 发布说明

## v0.1.0 – Release / 正式发布

- Overview / 概览
  - First public release of a foobar2000‑compatible Dynamic Range (DR) analysis tool.
    面向 foobar2000 口径的 DR 分析工具首个正式版。
  - Format coverage tests are still limited; Atmos (E‑AC‑3/AC‑3 in MP4/M4A) and DSD (DSF/DFF) paths have been verified.
    格式覆盖相关测试仍不充分；已针对全景声（MP4/M4A 内 E‑AC‑3/AC‑3）与 DSD（DSF/DFF）做专项验证。

- Added / 新增
  - FFmpeg auto‑routing and fallback for ac3/ec3/eac3/dts/dsf/dff; MP4/M4A with ac3/eac3 auto‑switches to FFmpeg (parallel API falls back to serial).
    为 ac3/ec3/eac3/dts/dsf/dff 启用 FFmpeg 自动路由；MP4/M4A 容器探测到 ac3/eac3 时自动切换（并行接口回退为串行）。
  - LFE identification via ffprobe JSON channel labels; optional `--exclude-lfe` for Official DR aggregation when reliable layout metadata exists.
    通过 ffprobe JSON 标签精确定位 LFE/LFE2；在存在可靠布局元数据时，支持 `--exclude-lfe` 从 Official DR 聚合中剔除 LFE。
  - DSD pipeline options: `--dsd-pcm-rate` (88200|176400|352800|384000, default 352800), `--dsd-gain-db` (default +6.0 dB), `--dsd-filter` (teac|studio|off; default teac).
    DSD 处理链：`--dsd-pcm-rate`（默认 352800）、`--dsd-gain-db`（默认 +6.0 dB）、`--dsd-filter`（teac|studio|off；默认 teac）。
  - `--show-rms-peak` flag to display/hide RMS/Peak diagnostics; now effective for mono/stereo/multichannel (default off).
    新增 `--show-rms-peak` 控制是否显示 RMS/Peak 诊断；现已覆盖单声道/立体声/多声道（默认关闭）。
  - Windows ffmpeg/ffprobe discovery prefers PATH before probing fixed locations.
    Windows 优先从 PATH 检测 ffmpeg/ffprobe，提升可用性。

- Fixed / 修复
  - Critical: FFmpeg fallback unified F32LE output but read path still treated data as S16/S32, causing multichannel DR≈0. Fixed by proper frame alignment (4‑byte per sample) and F32LE conversion.
    关键修复：FFmpeg 回退统一 F32LE 后，读取仍按 S16/S32 解析导致多声道 DR≈0；已改为 4 字节样本对齐并正确使用 F32LE 转换。
  - DSD report shows Bit Depth = 1 (processed as f32); bitrate suppressed where not meaningful.
    DSD 报告位深显示为 1（以 f32 处理）；在无意义处不再显示比特率。

- Behavior / 行为变化
  - FFmpeg fallback outputs F32LE for consistency; internal processing fully float‑based.
    FFmpeg 回退路径统一使用 F32LE；内部处理统一为浮点。
  - `--show-rms-peak` default off to reduce noise in reports.
    `--show-rms-peak` 默认关闭，减少报告噪音。
  - DSD reports show “native 1‑bit rate → processed rate (DSD downsampled)”, default 352.8 kHz (44.1k integer ratio); foobar2000 often shows 384 kHz (device/output resampling).
    DSD 报告显示“原生一位采样率 → 处理采样率（DSD 降采样）”；默认 352.8 kHz（44.1k 整数比）；foobar2000 常见显示 384 kHz（设备/输出链重采样）。

- Performance / 性能
  - Parallel decode (for safe‑parallel formats) improves throughput; multi‑file parallelism effective on SSD/NVMe.
    安全并行格式的并行解码显著提升吞吐；多文件并行在 SSD/NVMe 上效果明显。
  - Windows FFmpeg pipe throughput can be further tuned (planned larger read chunks) to reduce context switches.
    Windows FFmpeg 管道吞吐仍有优化空间（计划扩大读取块以减少上下文切换）。
  - Apple Silicon may show higher peak memory under large multi‑file workloads; monitor and adjust parallelism accordingly.
    Apple Silicon 在大批量多文件并行下可能出现内存峰值偏高；建议按需调整并发度。

- Known Issues / 已知问题
  - Small drift vs foobar2000 typically within ±0.02–0.05 dB; rare cases may approach ~0.1 dB (tail window, master differences).
    与 foobar2000 的典型偏差在 ±0.02–0.05 dB；少数情况接近 ~0.1 dB（尾窗纳入与否、母带差异）。
  - Windows DSF batch performance varies by environment (I/O, background scanning); ffmpeg null decode is fast—pipeline overhead under investigation.
    Windows 下 DSF 批量性能受环境影响（I/O、后台扫描）；ffmpeg 单文件解析很快，管道与流水线开销仍在分析优化。
  - Format coverage remains incomplete across container/codec variants and edge packet boundaries; samples welcome.
    不同容器/编解码变体与极端包边界的覆盖仍不充分；欢迎提供样本。

- Notes / 说明
  - Local‑only tool (no network I/O). Some upstream advisories via songbird/rustls/ring/pprof remain; acceptable for offline use.
    纯本地工具（无网络 I/O）。通过 songbird/rustls/ring/pprof 继承的安全通告仍存在；对离线使用可接受。
  - macOS builds are unsigned; use Security & Privacy or run `xattr -d com.apple.quarantine <binary>` if you trust the download.
    macOS 产物未签名；若信任来源，可在“安全性与隐私”放行，或执行 `xattr -d com.apple.quarantine <可执行文件>`。
  - Testing invitation: format coverage and cross‑platform validation; send samples/feedback to **ruuokk208@gmail.com**.
    测试邀请：协助扩充格式覆盖与跨平台验证；样本/反馈请发送至 **ruuokk208@gmail.com**。


## v0.1.0pre – Pre-Release / 预发布

- DR Precision / 精度说明
  - Typical drift vs foobar2000: ±0.02–0.05 dB
    相比 foobar2000，常见偏差约 ±0.02–0.05 dB
  - Rare outliers may reach ~0.1 dB (tail-window selection, master variations)
    少量曲目可能出现约 0.1 dB 的偏差（尾窗是否纳入计算、不同母带的首尾采样差异等）

- Format Coverage / 格式覆盖
  - Limited testing across container / codec combinations
    容器 / 编解码组合测试尚不充分
  - Some legacy logs and SDK assets removed
    已清理旧版日志和 SDK 资源

- Known Dependencies / 已知依赖风险
  - Upstream advisories via songbird / rustls / ring remain unresolved
    由于 songbird → rustls → ring 链路，继承的安全公告仍未修复（不影响本地离线使用）

- Platform Packages / 平台产物
  - Windows / macOS / Linux builds are published as zipped artifacts with the `-pre` suffix (e.g., `..._linux-x64-pre.zip`)
    Windows／macOS／Linux 可执行文件均以带 `-pre` 后缀的压缩包形式提供（如 `..._windows-x64.exe-pre.zip`）
  - macOS builds are unsigned; Gatekeeper may show “Apple can’t verify…” prompts—use Security & Privacy or `xattr -d com.apple.quarantine` if you trust the download
    macOS 产物未签名，可能触发“Apple 无法验证……”提示；若确认来源可信，可通过“安全性与隐私”或执行 `xattr -d com.apple.quarantine` 解除限制
- Linux package is untested on real hosts; treat as experimental
  Linux 产物尚未在真实环境验证，使用时请视为实验性质

- Testing Invitation / 测试邀请
  - Seeking help with container / codec format coverage and cross-platform validation
    欢迎协助扩充容器 / 编解码格式覆盖以及多平台验证
  - Audio sample feedback (attach or reference source files when possible) can be sent to **ruuokk208@gmail.com**
    音频样本反馈（如可附源文件）请发送至 **ruuokk208@gmail.com**

Please treat this tag as a pre-release; verify DR results with foobar2000 when they are critical.
此版本为预发布，重要曲目建议使用 foobar2000 交叉验证 DR 结果。
