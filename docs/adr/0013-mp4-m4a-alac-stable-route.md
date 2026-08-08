# ADR-0013：稳定 MP4/M4A + ALAC 路由与 0.3.0 契约转换

- 状态：Accepted
- 实施状态：Done
- 日期：2026-07-30
- 完成日期：2026-07-30
- 决策范围：新增一条受限 ISO BMFF + ALAC 稳定 route、wire schema v4 与 0.3.0
- 前置决策：
  - [ADR-0003：M2 原生解码面与工程契约加固](0003-m2-native-decoder-contract-hardening.md)
  - [ADR-0012：WAVE_FORMAT_EXTENSIBLE 线性 PCM](0012-wave-format-extensible-linear-pcm.md)
- 后继能力修订：
  2026-08-03 扩大不可表示采样率 sentinel 的接受集，见下方 §2 与“后继修订”。
  2026-08-09 又修正非 ALAC 媒体的诊断优先级，见“后继修订”。格式矩阵与其余
  毕业证据不变。
- 后继并发修订：
  [ADR-0014](0014-deterministic-decode-analysis-pipeline.md) 保留本文的稳定 ALAC
  格式矩阵与严格错误契约，但允许在独立毕业后为该 route 增加有界、顺序提交的
  packet workers。本文的串行/并行非目标描述仍只限定 ADR-0013 的毕业切片。

## 背景

ADR-0003 将 AIFC、ALAC 与 ISO BMFF 保留为 planned，要求任何新 route 先形成内容
probe、不可变 metadata、几何、归一化、EOF、progress、route-specific malformed、
sticky terminal error 和 adapter 边界证据。WAVE_FORMAT_EXTENSIBLE 已在 ADR-0012
完成既有 codec 的封装扩展；下一步选择 ALAC，是因为它能继续使用当前进程内
Symphonia backend，同时覆盖常见的无损 `.m4a` 文件。

本决策把 ALAC 排在 AIFC 之前是有意的路线调整，不表示 AIFC 被否定。ALAC 同时
新增 container 与 codec 公开身份，必须显式升级 wire schema，不能偷偷扩张 v3
枚举集合。

## 决策

### 1. 稳定能力边界

新增 `(mp4, alac)` stable route，范围固定为：

- ALAC compatible version 0；
- 16-bit 或 24-bit；
- 1–8 声道，并采用对应声道数的标准布局；
- 一个 audio-only `soun` track、一个 `alac` sample entry；
- 非 fragmented ISO BMFF，至少一个非空 `mdat` 与恰好一个完整 `moov`；
- edit list 缺失，或只有一条 `media_time == 0`、速率 1.0、非零 duration 的
  identity mapping；
- ALAC frame length 固定为 4096，媒体声明非零精确总帧数。

`.m4a` 与 `.mp4` 都加入 capability-driven 默认发现。扩展名仍只是发现 hint；显式
文件按内容探测。AAC、视频、多轨或其他超出矩阵的 ISO BMFF 会作为
`UnsupportedFormat / Probe` 失败，不会被宣传为受支持能力。

### 2. 第一方 ISO BMFF 首检

在 Symphonia probe/decoder 创建前，第一方 `Read + Seek` parser 固定以下规则：

- 第一个顶层 box 必须是有效 `ftyp`；支持显式 32-bit 与 extended 64-bit size，
  size 为 0 的 box 不进入稳定矩阵；所有 box size、边界和位置运算均检查截断与
  溢出，不按媒体声明分配 payload-sized buffer；
- `moov` 与 `mdat` 顺序不限，允许普通 metadata、`free` 及未参与路由的普通 box；
- `moof`、`mvex` 或多个 track 均拒绝；track 必须含唯一 `mdia`，handler 必须为
  `soun`，sample description 必须只有一个 `alac` entry；
- 交叉核验 AudioSampleEntry、ALAC cookie、`mdhd`、`stts`、`stsz`：采样率、声道、
  packet 数、总帧数必须一致；`stts` 与 `stsz` 的 entry/count 运算必须检查溢出；
- cookie 只接受 24 或 48 bytes。24-byte cookie 按声道数使用 Symphonia 标准布局；
  48-byte cookie 只接受八个已登记标准 layout tag，且 tag 的声道数必须与 cookie
  一致；产品 `ChannelLayout` 仍报告 `Unknown`。

AudioSampleEntry 的采样率是 16.16 定点，其整数部分只有 16 bit，因此无法表示超过
`u16::MAX` 的速率——这是格式本身的限制，见
[Apple `sound_sample_description_version_1` 的 sample rate 定义](https://developer.apple.com/documentation/quicktime-file-format/sound_sample_description_version_1/sample_rate)。
写入方因此存入 sentinel，真实速率留在 ALAC cookie 中。接受集为 `{0, 1}` 两种已
观测拼写：字段为零（FFmpeg 的做法），以及定点值 `1.0`（`0x0001_0000`）。两者都
只在 cookie rate 确实大于 `u16::MAX` 时才接受，因此都不能掩盖真实的不一致；仍
必须与 `mdhd` 和 backend metadata 交叉核验，而 backend 读取同一字段，因此它报告
的是同一 sentinel 而非 cookie rate。

### 3. Backend 与运行时契约

workspace 只为既有 Symphonia 0.5.5 启用 `alac` 与 `isomp4` feature，不增加第二
backend、外部进程或新的一等依赖。解码前同时核验：

- backend codec 必须为 `CODEC_TYPE_ALAC`；
- backend sample rate 必须匹配第一方值（仅保留上述两种高采样率 sentinel，
  即字段为零或定点 `1.0`）；
- backend `n_frames` 必须等于第一方总帧数；
- backend `extra_data` 必须逐 byte 等于首检 ALAC cookie。

解码期间继续核验输出 sample rate 与声道数；EOF 前必须得到精确帧数。损坏 packet
或最终帧数不符进入 sticky `DecodeFailed / Decode`，不能变成 EOF 或 partial report。
解码保持串行，PCM 仍为有限交错 `f64`，`Application` 仍是唯一文件/批处理入口。

### 4. 固定错误分类

- 截断、box 越界或溢出、缺少/重复必需结构、矛盾字段、cookie/sample-table
  不一致：`MalformedMedia / Probe`；
- 非 ALAC/AAC、视频或额外 track、多音轨、fragmented MP4、裁剪 edit list、
  compatible version 非 0、20/32-bit、非标准布局、超过 8 声道、零帧媒体及明确
  超出稳定形状的 box：`UnsupportedFormat / Probe`；
- 数据包损坏或最终解码帧数不符：sticky `DecodeFailed / Decode`；
- 非 ISO BMFF 内容：`UnsupportedFormat / Probe`。

### 5. Corpus 与毕业证据

新增 `native-alac-v1`，固定 FFmpeg 8.0.1 仅用于再生成。普通构建/测试消费提交字节，
不要求 FFmpeg。每个 ALAC 文件都有携带逐位相同整数 PCM 的 WAV 孪生；矩阵覆盖
16/24-bit、1–8 声道、44.1/48/96 kHz、多 packet、短尾帧、`.m4a`/`.mp4`、
faststart/普通 atom 顺序、metadata，以及两种不可表示采样率 sentinel 拼写。
pinned encoder 只产生零拼写，因此 `1.0` 拼写由 generator 在编码后确定性改写
sample entry 的该字段得到；cookie 与 PCM 不变，所以它仍有逐位相同的 WAV 孪生。

manifest 记录生成器与 encoder 身份、归一化命令、文件 hash、box 顺序、cookie、
sample entry、`mdhd`、`stts`、`stsz` 字段、孪生关系、来源及 interleaved-`f64`
fingerprint。`malformed-media-v1` 增加长度、轨道、fragmentation、edit、AAC、
非 ALAC sample entry、20/32-bit、9 声道、24/48-byte cookie/layout、sample-table、
零帧和损坏 packet case。

真实 AAC 文件带 encoder-delay edit list，会先被 edit-list 规则拒绝，不会到达
sample-entry codec 判定。因此非 ALAC codec 拒绝由独立的 `alac-non-alac-sample-entry`
case 固定：它只把 outer sample entry 的 fourcc 改写为 `mp4a`，并断言具体消息与
`sample_entry=mp4a` details。

共享 `PcmSource` contract 对每个 ALAC fixture 验证 immutable metadata、有限逐位
PCM、progress、精确帧数、EOF 与 sticky 状态；ALAC/WAV 孪生还比较 PCM raw bits、
完整 `AnalysisResult` 与归一化共享 report。Application、CLI 与 Tauri 各自固定真实
ALAC、capability、发现及 schema 边界。

### 6. 20/32-bit 延后

首批不声明 ALAC 20/32-bit。FFmpeg 8.0.1 encoder 对高位深输入固定选择 24-bit，
无法为 20/32-bit 提供与当前矩阵同等级的可再生孪生证据。Apple flags 允许更广
能力不等于本产品已毕业；它们留待独立切片。

- [FFmpeg 8.0 ALAC encoder source](https://www.ffmpeg.org/doxygen/8.0/alacenc_8c_source.html)
- [Apple Audio Format Flags](https://developer.apple.com/documentation/coreaudiotypes/audio-format-flags)

### 7. Wire 与版本

公开 enum 新增 `ContainerFormat::Mp4` 与 `SourceCodec::Alac`，序列化为 `mp4`、
`alac`。这会扩张严格消费者可见的标识集合，因此 CLI、Tauri、release smoke 与当前
参考 comparator 同步升级到 wire schema v4；产品不提供 v3 输出开关。历史 v3
conformance bytes 与 0.2.0 文档不回写，当前 comparator 仍可读取历史 v3 记录。

route 通过毕业门禁后，workspace 与 GUI mirrors 同步到 0.3.0。GUI 发行边界继续是
未签名、未 notarize 的 Apple Silicon macOS 11.0+；不增加平台或发布动作。

## 明确非目标

- AAC、CAF/raw ALAC、fragmented MP4、视频 soundtrack、多音轨；
- ALAC 20/32-bit、compatible version 非 0、非标准布局；
- AIFC、第二 backend、FFmpeg runtime、重采样、裁剪或静音预处理；
- 并行 packet/file decode、SIMD、unsafe、性能基准或 release staging；
- 修改固定分析算法、报告数值参数或历史 0.2.0 证据。

## 后果

正面：常见无损 M4A/MP4 可以在同一离线进程内路径分析；支持面由第一方结构约束与
WAV 孪生证据固定，而不是继承 backend 的全部隐式能力。

代价：第一方 ISO BMFF parser 与 route-specific malformed 面显著增加；schema v4
是严格消费者需要显式接受的契约变化；20/32-bit 与更多 MP4 topology 仍是已记录
的后续工作。

## 后继修订

### 2026-08-03：不可表示采样率 sentinel 的第二种拼写

本文最初只接受字段为零这一种 sentinel，依据是 pinned FFmpeg 8.0.1 的行为。对本机
个人音频库的一次检查发现，314 个真实 ALAC 中有 3 个 96 kHz / 24-bit 文件把该字段
写为 `0x0001_0000`（定点 `1.0`）。这些文件本身完好：ffmpeg 无警告解出与声明一致的
全部帧数，而本产品因未登记该拼写将其判为 `MalformedMedia / Probe`。

因此把 sentinel 接受集扩为 `{0, 1}`，边界不变：仍只在 cookie rate 大于
`u16::MAX` 时适用。first-party parser 与 backend 交叉核验共用同一判定，因为两者
读取的是同一个 16.16 字段。该字段无法表示高采样率不是对某个写入方的观察，而是
格式定义本身，因此“需要 sentinel”这一前提不依赖具体 encoder。

证据：`native-alac-v1` 新增 `alac24-stereo-96000-rate-sentinel-one.m4a` 及其 WAV
孪生，与既有零拼写的 96 kHz fixture 并列；`malformed-media-v1` 新增
`alac-rate-sentinel-one-within-range.m4a`，把 48 kHz 轨道的该字段改写为 `1.0`，
固定它仍以 `sample_entry_rate=1; cookie_rate=48000` 判为
`MalformedMedia / Probe`。

同一次检查还记录了两个解码帧数比声明少恰好一个 4096-frame packet 的文件。ffmpeg
对它们解出完全相同的帧数并报 `invalid samples per frame: 0`，只是把截断结果静默
输出。本产品按既有契约判为 sticky `DecodeFailed / Decode`，不作改动。

### 2026-08-09：非 ALAC codec 先于 ALAC edit-list 规则报告

真实 IAMF/Opus 与 E-AC-3 文件同时携带非 identity edit list。第一方 parser 原先在
遍历 `trak` 时立即解释 `edts`，因此会先返回“trimmed edit list unsupported”，掩盖
该 track 根本不是 ALAC 的主要原因。现改为先完成 `mdia`/`stsd` 与 sample-entry
codec identity 检查，再对已经确认的 ALAC track 解释 edit list。非 ALAC 输入因此
稳定返回 `sample_entry=<fourcc>` 的 codec 错误；合法 ALAC 的 identity/cropped edit
规则、错误码和接受面均不改变。组合回归同时携带非 ALAC sample entry 与非 identity
edit，固定 codec identity 的优先级。

## 实施收口

2026-07-30 按本 ADR 完成实现与本地毕业：

- `(mp4, alac)` 已进入 stable catalog，`.m4a` / `.mp4` 进入默认发现；公开
  container/codec 标识、Rust/TypeScript wire、release smoke 和当前参考 comparator
  已同步到 schema v4，workspace 与 GUI mirrors 为 0.3.0；
- `native-alac-v1` 的 9 组 WAV/ALAC 孪生覆盖 1–8 声道、16/24-bit、44.1/48/96
  kHz、多 packet、短尾、两种扩展、两种 atom 顺序和 metadata；全部共享
  `PcmSource` contract 与 PCM raw-bit 等价测试通过；
- 64-bit `ftyp`、无 edit list、24/48-byte cookie、标准布局、backend codec/rate/
  frame/cookie mismatch、Application、CLI stdout/stderr/JSON/output-file、Tauri
  schema/capability/真实 ALAC 路径均有独立测试；
- malformed corpus 增加 ALAC box、cookie、布局、bit depth、9-channel、AAC、视频、
  多轨、fragmentation、裁剪 edit、零帧、sample-table 与损坏 packet case；安全的
  route case 在进程内固定分类，损坏 packet 固定 sticky decode error；
- fmt、严格 Clippy、workspace all-target tests、release CLI build、三套合法 corpus
  与 malformed 再生成检查、Python tests、repository contract 和 Tauri frontend
  build 全部通过；release CLI 另以真实 ALAC 验证 0.3.0/schema-v4 输出；
- 使用刷新到 1173 项的 RustSec advisory 数据扫描 491 个 locked crate：无
  vulnerability；将可修复的 `anyhow` 1.0.101 更新到 1.0.103。仍有 15 个 allowed
  informational warning，来自 Tauri 的 Linux GTK3/urlpattern 传递链（14 个
  unmaintained、一个旧 `glib::VariantStrIter` unsound 警告），不由新增 Symphonia
  ALAC/ISO-MP4 子包引入，也没有当前锁图内的直接替换；
- 当前 host 是 macOS，无法施加 Linux `RLIMIT_AS`，因此按既定安全规则没有运行
  hostile corpus 隔离验证；没有触发或等待远端 CI，也没有运行 performance、
  release staging、签名、公证或发布动作。
