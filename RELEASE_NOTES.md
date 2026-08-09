# Release Notes / 发布说明

## v0.3.0 – Stable MP4/M4A + ALAC / 稳定 MP4/M4A + ALAC

> Status: development release contract; no signed/notarized public artifact has
> been created. / 状态：开发线发行契约；尚未创建已签名、公证的公开制品。
>
> Concurrency note: 0.3.0 ships the bounded packet-level, decode-analysis and
> file-lane axes of accepted
> [ADR-0014](docs/adr/0014-deterministic-decode-analysis-pipeline.md), all under
> one application-owned worker and memory plan; window-level parallelism is not
> implemented. A report and its decoded PCM do not depend on how many workers a
> host grants, and there is no public thread, batch-size or queue control. This
> release publishes no throughput figure. The v0.2.0 exclusions below are
> historical release boundaries, not a permanent post-M6 ban. / 并发说明：0.3.0
> 交付已接受的 ADR-0014 中 packet 级、解码-分析与 file lane 三条轴，全部受同一
> application 自有 worker 与内存计划约束；窗口级并行未实现。报告与其解码 PCM 不依赖
> 宿主授予多少 worker，也不提供公开的线程、batch size 或队列控制。本版本不发布任何
> 吞吐数字。下方 v0.2.0 的排除项是历史发行边界，不是 post-M6 永久禁令。

- Renamed the CLI binary to `mdrmeter`; the product, crates, repository and
  GUI remain MacinMeter. The old name said "meter" without saying what it
  measures, and a command name is the one place that has to state it. Added
  `mdrmeter completions SHELL` for bash, zsh, fish, powershell and elvish,
  generated from the parser itself.
  CLI 二进制更名为 `mdrmeter`；产品、crate、仓库与 GUI 仍为 MacinMeter。旧名
  只说了 "meter"，没说测什么，而命令名恰恰是必须说清的地方。新增
  `mdrmeter completions SHELL`，覆盖 bash、zsh、fish、powershell 与 elvish，
  由解析器自身生成。
- Added a constrained stable in-process MP4/M4A + ALAC route: compatible
  version 0, 16/24-bit, 1–8 standard-layout channels, one audio-only track,
  non-fragmented ISO BMFF, and exact declared frame counts.
  新增受限的稳定进程内 MP4/M4A + ALAC 路径：compatible version 0、
  16/24-bit、1–8 个标准布局声道、单一纯音频 track、非 fragmented ISO BMFF，
  并要求准确声明总帧数。
- Added bounded first-party ISO BMFF validation and cross-checks for ALAC
  cookies, sample entries, edit lists, `mdhd`, `stts`, `stsz`, backend codec
  identity, sample rate, and final decoded frame count.
  新增有界第一方 ISO BMFF 首检，并交叉核对 ALAC cookie、sample entry、edit
  list、`mdhd`、`stts`、`stsz`、backend codec 身份、采样率和最终解码帧数。
- Added `.m4a` and `.mp4` to capability-driven discovery. AAC, video or extra
  tracks, multiple audio tracks, fragmentation, cropped edits, ALAC 20/32-bit,
  and nonstandard layouts remain unsupported.
  capability 驱动的目录发现新增 `.m4a` 与 `.mp4`；AAC、视频或额外 track、
  多音轨、fragmentation、裁剪 edit、ALAC 20/32-bit 和非标准布局仍不支持。
- Added the deterministic `native-alac-v1` WAV-twin corpus and expanded
  malformed-media coverage. FFmpeg 8.0.1 is a fixture-generation tool only;
  it is not a product runtime dependency.
  新增确定性的 `native-alac-v1` WAV 孪生语料并扩充 malformed-media 覆盖。
  FFmpeg 8.0.1 仅为语料生成工具，不是产品运行时依赖。
- Added public `mp4`/`alac` identities and upgraded the shared CLI/Tauri wire
  envelope to schema v4. The fixed analysis algorithm and numeric report
  fields are unchanged.
  新增公开 `mp4`/`alac` 身份，共享 CLI/Tauri wire envelope 升级为 schema v4；
  固定分析算法与数值报告字段不变。
- Added bounded packet-level decoding for the ALAC route and for FLAC streams
  whose geometry fits the granted reservation, overlapped decoding and analysis
  through a bounded depth-16 hand-off on a permit the route leaves unspent, and
  batch file lanes derived from the same plan. A hand-off whose worst-case PCM
  retention does not fit its lane grant remains serial. A single file still
  receives the whole decoder. Nothing here is a new public option, and a report
  never indicates which path produced it.
  为 ALAC route 以及几何能落入已授予 reservation 的 FLAC 流启用有界 packet 级
  解码，在 route 未花掉的 permit 上通过有界 depth-16 hand-off 重叠解码与分析，并按
  同一 plan 推导批量的 file lane；最坏 PCM 保留量装不进 lane 额度时 hand-off 保持
  串行。单个文件仍独占整个解码器。以上均不新增公开选项，报告也不指示由哪条路径产生。
- **Behavior change:** batch progress events now interleave across file lanes
  through the Rust progress sink, Tauri `analysis-event`, and CLI stderr. Every
  event still names the item it belongs to, and the item order inside the report
  is unchanged. Consumers must use the item index rather than treating event
  order as a completed serial prefix; the GUI aggregates per-item progress.
  **行为变更：** 批量进度事件现在通过 Rust progress sink、Tauri
  `analysis-event` 与 CLI stderr 跨 file lane 交错。每个事件仍标明所属条目，报告内
  的条目顺序不变；消费方必须读取 item index，不能再把事件顺序当作已完成的串行
  前缀，GUI 会聚合逐条目进度。
- The GUI release boundary remains unsigned/unnotarized Apple Silicon macOS
  11.0+, without Intel/universal, Windows, or Linux GUI artifacts.
  GUI 发行边界仍为未签名、未公证的 Apple Silicon macOS 11.0+，不增加
  Intel/universal、Windows 或 Linux GUI 制品。

## v0.2.0 – Trusted trunk rebuild / 可信主干重建

> Status: unsigned Apple Silicon release.
>
> 状态：未签名 Apple Silicon 版本。

### Architecture / 架构

- Replaced the root package with a virtual workspace separating domain,
  analysis, codecs, application, CLI, and Tauri adapters.
  将根 package 改为 virtual workspace，分离 domain、analysis、codecs、
  application、CLI 与 Tauri adapter。
- Replaced both legacy DR engines with one safe, streaming, consuming
  `AnalyzerSession`.
  删除两套 legacy DR 引擎，改为唯一、安全、流式且消费式结算的
  `AnalyzerSession`。
- All first-party Rust crates now forbid unsafe code; native CPU flags, manual
  SIMD, packet parallelism, EdgeTrimmer, and duplicate frontend pipelines were
  removed.
  所有第一方 Rust crate 禁止 unsafe；删除原生 CPU 强绑定、手写 SIMD、包级并行、
  EdgeTrimmer 和前端重复管线。

### Supported surface / 支持范围

- The 0.2.0 stable surface supports content-probed WAV integer/float PCM,
  FLAC, and AIFF integer PCM.
  0.2.0 稳定能力只支持按内容探测的 WAV 整数/浮点 PCM、FLAC 与 AIFF 整数 PCM。
- Supported decoders and the streaming analyzer share finite interleaved
  `f64` PCM, preserving float64 WAV values until analysis.
  受支持的解码器与流式分析器统一使用有限、交错的 `f64` PCM，float64 WAV
  在进入分析前不再提前窄化。
- AIFC, lossy codecs, Opus/Songbird, FFmpeg routes, DSD, preprocessing, and
  parallel execution are unavailable by design.
  AIFC、有损编码、Opus/Songbird、FFmpeg 路径、DSD、预处理和并行执行均有意
  暂不提供。
- The packaged 0.2.0 desktop release targets Apple Silicon Macs on macOS 11.0
  or newer. It has no Developer ID signature or notarization; Intel/universal
  macOS and Windows/Linux GUI packages are outside this release.
  0.2.0 桌面发行包只面向 macOS 11.0 或更新系统上的 Apple Silicon Mac。它没有
  Developer ID 签名或 Apple 公证；Intel/universal macOS 与 Windows/Linux GUI
  包不属于本次发行。

### Interfaces / 接口

- Added explicit `macinmeter analyze` and `macinmeter batch` commands, stable
  exit codes, clean stdout/stderr separation, and atomic explicit output.
  新增显式 `macinmeter analyze` / `macinmeter batch` 命令、稳定退出码、
  stdout/stderr 分离与显式原子输出。
- CLI JSON and Tauri now share schema version 3 and the same application
  report, errors, cancellation, and progress model. Schema v3 separates
  channel/track report metrics from DR-state diagnostics, preserves exact
  decoded duration, and rejects non-finite public values through validated
  wrappers.
  CLI JSON 与 Tauri 共用 schema v3，以及同一 application report、错误、取消和
  进度模型。schema v3 将 channel/track report metrics 与 DR 状态诊断分开，保留
  精确 decoded duration，并通过受验证 wrapper 拒绝非有限公开数值。
- `AnalyzerSession::finish` is now fallible. DR diagnostics use
  `drSelectedPeak`, `drPrimaryPeak`, and nullable `drSecondaryPeak`; independent
  report fields expose public-f32 channel overall RMS/primary peak and
  reference-shaped track RMS/peak.
  `AnalyzerSession::finish` 现在可失败。DR 诊断使用 `drSelectedPeak`、
  `drPrimaryPeak` 与可空的 `drSecondaryPeak`；独立 report 字段公开
  public-f32 channel overall RMS/primary peak，以及按参考形状聚合的 track
  RMS/peak。
- Added an explicit `AlbumAggregator` library API. Batch execution is not an
  album operation: callers opt into unweighted public-f32 track arithmetic or
  exact-decoded-duration weighting, and numeric DR0 tracks remain included.
  新增显式 `AlbumAggregator` 库 API。批处理不会自动成为 album：调用方显式选择
  public-f32 track unweighted 算术平均或精确 decoded-duration weighting，数值
  DR0 track 仍会纳入。
- GUI jobs use caller-provided IDs and independent cancellation tokens.
  GUI job 使用调用者提供的 ID 与相互独立的取消 token。
- The desktop GUI supports whole-window file and directory drag-and-drop,
  bilingual Chinese/English presentation, result search and sorting, path
  hiding, and Markdown, JSON, PNG, and SVG export.
  桌面 GUI 支持整窗文件与目录拖放、中英文界面、结果搜索与排序、路径隐藏，以及
  Markdown、JSON、PNG 和 SVG 导出。
- File analysis, batch, and controlled discovery now share one public
  `Application` façade. The budget established in M3 runs one active top-level
  job and admits at most 64 additional FIFO reservations; queued cancellation
  and release are isolated. M3 closes with the single in-process Symphonia
  backend and does not claim a byte-accurate decoder memory sandbox.
  文件分析、批处理和受控发现现在共用唯一公开的 `Application` 门面。M3 建立的预算
  同时只运行一个顶层 job，最多接纳 64 个 FIFO 排队 reservation；排队取消与释放
  彼此隔离。M3 以唯一的进程内 Symphonia backend 收口，不声称具备逐字节精确的
  decoder 内存沙箱。

### Engineering / 工程

- Added analysis boundary/property tests, strict decoder contract tests, Rust
  API/CLI parity checks, CLI black-box tests, and independent GUI job tests.
  新增分析边界/属性测试、严格解码契约测试、Rust API/CLI 一致性、CLI 黑盒测试与
  GUI job 隔离测试。
- Recorded a fixed x64 1.0.8 safe-master observation and scoped conformance.
  The schema-v3 implementation matches 39/39 integer track DR, 62/62
  two-decimal channel DR, 39/39 overall peak, 39/39 overall RMS, and 62/62
  channel RMS tokens, plus 39/39 rendered duration tokens. A narrowly scoped
  footer check confirms the track/sample-rate/channel sets and DR token; it
  does not cover host metadata, precise album internals, or duration weighting.
  登记固定 x64 1.0.8 safe-master observation 与受限 conformance。schema-v3
  实现匹配整数 track DR 39/39、每声道两位 DR 62/62、overall peak 39/39、
  overall RMS 39/39、channel RMS 62/62 与渲染时长 39/39。严格限域的 footer
  检查确认 track/采样率/声道集合和 DR token；host metadata、精确 album 内部状态
  和 duration weighting 不在该记录范围内。
- M5 centralizes direct Rust dependency policy and package identity at the
  workspace root. GUI builds now check version mirrors without rewriting
  tracked files; explicit `npm run sync-version` is the only synchronization
  command.
  M5 将 Rust 直接依赖策略与 package identity 集中到根 workspace。GUI build
  只读核对版本镜像，不再改写 tracked files；只有显式
  `npm run sync-version` 才执行同步。
- GitHub Actions now provides bounded automatic Ubuntu 24.04, Windows Server
  2025 x64, and macOS 26 arm64 validation for pull requests and `main`. The
  macOS main gate runs ephemeral clean CLI/GUI staging; manual dispatch retains
  a verified unsigned Apple Silicon candidate for 14 days without publishing
  it. Hostile malformed media remains confined to
  the opt-in, per-process verifier with an enforceable memory limit by default.
  GitHub Actions 现为 pull request 与 `main` 提供有界的 Ubuntu 24.04、Windows
  Server 2025 x64 与 macOS 26 arm64 自动验证。macOS main 门禁执行临时 clean
  CLI/GUI staging；手动触发会保留 14 天已验证的未签名 Apple Silicon candidate，
  但不会发布。hostile malformed media 仍只由 opt-in 的逐进程 verifier 解码，
  且默认要求可执行的内存上限。
- Added explicit release staging. The distributed CLI archive is
  extracted and smoke-tested against the versioned JSON and decoder-route contract;
  release manifests and every artifact are covered by SHA-256. Current-host
  macOS DMGs receive image, mounted-bundle, and architecture checks, while
  remaining explicitly unsigned, unnotarized, and staging-only. The same
  contract runs ephemerally on macOS main CI. Manual CI uses the stricter
  unsigned-candidate manifest and retains it temporarily without creating a
  tag or Release.
  新增显式发行 staging：解包后的 CLI 会通过版本化 JSON 与解码路径 smoke，
  release manifest 与全部制品由 SHA-256 覆盖；当前 host 的 macOS DMG 会核对
  镜像、挂载 bundle 与 architecture，同时明确保持未签名、未公证、仅供 staging。
  同一契约会在 macOS main CI 中临时运行；手动 CI 使用更严格的 unsigned
  candidate manifest 并临时保留制品，但不会创建 tag 或 Release。

Earlier entries below describe historical 0.1.x releases and removed behavior;
they are not documentation for the 0.2.0 interface.

以下内容记录历史 0.1.x 版本及已删除行为，不是 0.2.0 接口文档。

---

## v0.1.3 (2026-02-07) – Boundary Risk Control & Security Fixes / 边界风险控制与安全修复

### Features / 新功能
- GUI / 图形界面
  - Added "Hide Boundary Risk" toggle button: control visibility of DR boundary warnings in UI and exports.
    新增"隐藏边界风险"按钮：控制边界风险警告在UI及导出中的显示。
  - Boundary risk setting persists across single/multi-file exports (MD, JSON, PNG).
    边界风险设置跨越单文件/多文件导出保持一致（MD、JSON、PNG）。

- CLI / 命令行
  - Added `--hide-boundary-risk` flag: suppress boundary risk warnings in text/JSON output.
    新增 `--hide-boundary-risk` 参数：隐藏文本/JSON 输出中的边界风险警告。
  - Behavior: warning is excluded from both compact and JSON reports when flag is set.
    行为：设置后警告从紧凑格式和 JSON 报告中排除。

### Fixed / 修复
- **Security**: Fixed RUSTSEC-2026-0007 (bytes integer overflow) by enforcing `bytes >= 1.11.1`.
  **安全**：通过强制 `bytes >= 1.11.1` 修复 RUSTSEC-2026-0007（bytes 整数溢出）。
- **Security**: Fixed RUSTSEC-2026-0009 (time DoS vulnerability) by enforcing `time >= 0.3.47`.
  **安全**：通过强制 `time >= 0.3.47` 修复 RUSTSEC-2026-0009（time 拒绝服务漏洞）。
- **Security**: Updated pprof from 0.13.0 to 0.14 (fixes unsound memory usage).
  **安全**：将 pprof 从 0.13.0 升级至 0.14（修复不安全的内存使用）。
- **Build**: Fixed Tauri GUI compilation errors when building with updated dependencies.
  **构建**：修复更新依赖后 Tauri GUI 编译错误。
- **CI/CD**: Fixed GitHub Actions `paths-filter` issue where tag push to same commit as main was ignored.
  **CI/CD**：修复 GitHub Actions paths-filter 在 tag 推送到与 main 同一 commit 时被忽略的问题。

### Changed / 变更
- All boundary risk diagnostics are now opt-in via GUI toggle or CLI flag (default: show warnings).
  所有边界风险诊断现通过 GUI 按钮或 CLI 参数可选（默认：显示警告）。

### Testing / 测试验证
- All 205 unit tests passed, zero compiler warnings.
  所有 205 个单元测试通过，零编译警告。
- Verified feature parity between GUI and CLI boundary risk hiding.
  验证了 GUI 和 CLI 边界风险隐藏功能的一致性。

---

## v0.1.2 (2026-01-29) – JSON Export & GUI i18n / JSON导出与GUI国际化

- CLI / 命令行
  - Added `--json` / `-j` option: output results in JSON format (mutually exclusive with `--compact`).
    新增 `--json` / `-j` 参数：以 JSON 格式输出结果（与 `--compact` 互斥）。
  - Added `--no-save` option to output results to console only.
    新增 `--no-save` 参数，仅输出到控制台不保存文件。
  - Exported `VERSION` constant from core library for unified version management.
    核心库导出 `VERSION` 常量，统一版本管理。

- GUI / 图形界面
  - Added i18next internationalization: Chinese/English language switching with localStorage persistence.
    新增 i18next 国际化：中英文切换，语言偏好保存至 localStorage。
  - Unified single/multi-file rendering logic (removed ~70 lines of redundant code).
    统一单文件/多文件渲染逻辑（减少约70行重复代码）。
  - Fixed language switch clearing analysis results.
    修复语言切换后分析结果被清空的问题。
  - Fixed single-file mode missing individual MD/PNG copy buttons.
    修复单文件模式缺少单独 MD/PNG 复制按钮的问题。
  - Improved CJK font support in image export (Japanese/Chinese characters).
    改善图片导出中的 CJK 字体支持（日文/中文字符）。
  - Added version sync script: auto-syncs version from main Cargo.toml on build.
    新增版本同步脚本：构建时自动从主 Cargo.toml 同步版本。

- Documentation / 文档
  - Separated English and Chinese README: `README.md` (EN) + `README_CN.md` (CN).
    分离中英文 README：`README.md`（英文）+ `README_CN.md`（中文）。
  - Extracted detailed docs to `docs/`: `SUPPORTED_FORMATS.md`, `BENCHMARKS.md`, `LEGAL.md` with language variants.
    详细文档分离至 `docs/`：`SUPPORTED_FORMATS.md`、`BENCHMARKS.md`、`LEGAL.md`，均有中英文版本。
  - Streamlined batch report format using Markdown tables (comfy-table).
    批量报告改用 Markdown 表格格式（comfy-table）。
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
