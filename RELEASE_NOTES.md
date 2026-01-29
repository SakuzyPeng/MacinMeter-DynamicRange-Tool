# Release Notes / 发布说明

## v0.1.2 (2026-01-29) – Documentation Refactor / 文档重构

- Documentation / 文档
  - Separated English and Chinese README: `README.md` (EN) + `README_CN.md` (CN).
    分离中英文 README：`README.md`（英文）+ `README_CN.md`（中文）。
  - Extracted detailed docs to `docs/`: `SUPPORTED_FORMATS.md`, `BENCHMARKS.md`, `LEGAL.md` with language variants.
    详细文档分离至 `docs/`：`SUPPORTED_FORMATS.md`、`BENCHMARKS.md`、`LEGAL.md`，均有中英文版本。
  - Streamlined batch report format using Markdown tables (comfy-table).
    批量报告改用 Markdown 表格格式（comfy-table）。
  - Added `--no-save` option to output results to console only.
    新增 `--no-save` 参数，仅输出到控制台不保存文件。
  - Added `*` (LFE excluded) and `†` (silent channels excluded) markers in batch reports.
    批量报告新增 `*`（LFE 已剔除）和 `†`（静音声道已剔除）标记。

- Added / 新增
  - Cross-platform benchmark tool `dr-bench` (Rust): replaces bash/PowerShell scripts.
    跨平台基准测试工具 `dr-bench`（Rust 实现）：替代原 bash/PowerShell 脚本。

- Cleanup / 清理
  - Removed obsolete scripts from root directory (moved to `scripts/`).
    移除根目录过时脚本（已移至 `scripts/`）。
  - Removed `PERFORMANCE_OPTIMIZATION_PLAN.md` (completed).
    移除 `PERFORMANCE_OPTIMIZATION_PLAN.md`（已完成）。

---

## v0.1.1 (2025-11-08) – LFE Detection Fix / LFE检测修复

- Added / 新增
  - Created `channel_layout.rs` module based on Apple CoreAudio AudioChannelLayoutTag specification.
    创建 `channel_layout.rs` 模块，基于 Apple CoreAudio AudioChannelLayoutTag 规范。
  - Support for multiple standard layouts: MPEG 5.1/6.1/7.1, EAC3, Dolby Atmos (5.1.2/5.1.4/7.1.2/7.1.4/9.1.6), DTS 7.1, and common formats (2.1/3.1).
    支持多种标准布局：MPEG 5.1/6.1/7.1、EAC3、Dolby Atmos (5.1.2/5.1.4/7.1.2/7.1.4/9.1.6)、DTS 7.1 以及常见格式（2.1/3.1）。
  - Three-tier LFE detection strategy: exact match → fuzzy match → conservative fallback.
    三级 LFE 检测策略：精确匹配 → 模糊匹配 → 保守回退。

- Fixed / 修复
  - **Critical**: Fixed LFE (Low Frequency Effects) channel misidentification in multi-channel audio files.
    **关键修复**：修复多声道音频文件中 LFE（低频效果）声道识别错误。
  - EAC3 raw stream (.ec3): LFE correctly identified at index 5 (L,C,R,Ls,Rs,**LFE**).
    EAC3 裸流（.ec3）：LFE 正确识别在索引 5（L,C,R,Ls,Rs,**LFE**）。
  - M4A/MP4 container: LFE correctly identified at index 3 (L,R,C,**LFE**,Ls,Rs).
    M4A/MP4 容器：LFE 正确识别在索引 3（L,R,C,**LFE**,Ls,Rs）。
  - Fixed ffprobe parsing bug: "7.1" was misidentified as duration (7.1 parses as f64); now uses numeric threshold (<20 is layout, ≥20 is duration).
    修复 ffprobe 解析 bug："7.1" 被误判为 duration（7.1 可解析为 f64）；现使用数值阈值判断（<20 为布局，≥20 为 duration）。
  - Fixed one bilingualization issue in processor.rs:710.
    修复 processor.rs:710 中的一个双语化问题。

- Behavior / 行为变化
  - **Container format (not codec) determines channel order**: The same EAC3 codec has different channel layouts in raw stream vs. M4A container.
    **容器格式（而非编码）决定声道顺序**：同一 EAC3 编码在裸流与 M4A 容器中具有不同的声道布局。
  - Enhanced `--exclude-lfe` accuracy: Now reliably detects LFE channels across different container formats with proper metadata.
    增强 `--exclude-lfe` 准确性：现可在不同容器格式中可靠检测 LFE 声道（需正确元数据）。

- Known Issues / 已知问题
  - ~~LFE identification may be inaccurate on files without reliable layout metadata~~ – **Significantly improved** with new channel_layout module; fallback strategy provides conservative defaults for unknown layouts.
    ~~在缺少可靠声道布局元数据的文件上，LFE 识别可能不够精确~~ – 通过新 channel_layout 模块**显著改善**；回退策略为未知布局提供保守默认值。
  - Small drift vs foobar2000 typically within ±0.02–0.05 dB; rare cases may approach ~0.1 dB (tail window).
    与 foobar2000 的典型偏差在 ±0.02–0.05 dB；少数情况接近 ~0.1 dB（尾窗纳入与否）。
  - Format coverage remains incomplete across container/codec variants and edge packet boundaries; samples welcome.
    不同容器/编解码变体与极端包边界的覆盖仍不充分；欢迎提供样本。

- Testing / 测试验证
  - Verified LFE detection on 4 test files (5.1 m4a, 5.1/5.1.2/7.1 ec3): all passed
    在 4 个测试文件（5.1 m4a、5.1/5.1.2/7.1 ec3）上验证 LFE 检测：全部通过
  - All 377 unit tests passed, zero compiler warnings.
    所有 377 个单元测试通过，零编译警告。

---

## v0.1.0 (2025-11-06) – Release / 正式发布

- Overview / 概览
  - First public release of a foobar2000‑compatible Dynamic Range (DR) analysis tool.
    面向 foobar2000 口径的 DR 分析工具首个正式版。
  - Format coverage tests are still limited; Atmos (E‑AC‑3/AC‑3 in MP4/M4A) and DSD (DSF/DFF) paths have been verified.
    格式覆盖相关测试仍不充分；已针对全景声（MP4/M4A 内 E‑AC‑3/AC‑3）与 DSD（DSF/DFF）做少量验证。

- Added / 新增
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
  - Windows FFmpeg pipe throughput can be further tuned to reduce context switches.
    Windows FFmpeg 管道吞吐仍有优化空间）。

- Known Issues / 已知问题
  - LFE identification may be inaccurate on files without reliable layout metadata or with uncommon container label variants; verify when critical.
    在缺少可靠声道布局元数据或存在非常见容器标签变体的文件上，LFE 识别可能不够精确；关键场景请务必核对。
  - Small drift vs foobar2000 typically within ±0.02–0.05 dB; rare cases may approach ~0.1 dB (tail window).
    与 foobar2000 的典型偏差在 ±0.02–0.05 dB；少数情况接近 ~0.1 dB（尾窗纳入与否）。
  - Windows DSF batch performance varies by environment (I/O); ffmpeg null decode is fast—pipeline overhead under investigation.
    Windows 下 DSF 批量性能受环境影响（I/O）；ffmpeg 单文件解析很快，管道与流水线开销仍在分析优化。
  - Format coverage remains incomplete across container/codec variants and edge packet boundaries; samples welcome.
    不同容器/编解码变体与极端包边界的覆盖仍不充分；欢迎提供样本。

- Notes / 说明
  - Local‑only tool (no network I/O). Some upstream advisories via songbird/rustls/ring/pprof remain; acceptable for offline use.
    纯本地工具（无网络 I/O）。通过 songbird/rustls/ring/pprof 继承的安全通告仍存在；对离线使用可接受。


- Platform Packages / 平台产物
  - Windows / macOS / Linux builds are published as zipped artifacts
    Windows／macOS／Linux 可执行文件以压缩包形式提供
  - macOS builds are unsigned; Gatekeeper may show “Apple can’t verify…” prompts—use Security & Privacy or `xattr -d com.apple.quarantine` if you trust the download
    macOS 产物未签名，可能触发“Apple 无法验证……”提示；若确认来源可信，可通过“安全性与隐私”或执行 `xattr -d com.apple.quarantine` 解除限制
- Linux package is untested on real hosts; treat as experimental
  Linux 产物尚未在真实环境验证，使用时请视为实验性质

- Testing Invitation / 测试邀请
  - Seeking help with container / codec format coverage and cross-platform validation
    欢迎协助扩充容器 / 编解码格式覆盖以及多平台验证
  - Audio sample feedback (attach or reference source files when possible) can be sent to **ruuokk208@gmail.com**
    音频样本反馈（如可附源文件）请发送至 **ruuokk208@gmail.com**

This is v0.1.0 (first stable tag). It is still an early version and parts may be unstable; for critical work, please cross‑check DR with foobar2000 when in doubt.
本版本为 v0.1.0（首个稳定标签）。整体仍属早期版本，部分环节可能不够稳定；关键场景下如有疑虑，仍建议与 foobar2000 结果交叉验证。
