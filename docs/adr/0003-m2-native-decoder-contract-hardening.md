# ADR-0003：M2 原生解码面与工程契约加固

- 状态：Accepted
- 实施状态：DOING
- 日期：2026-07-19
- 决策范围：M2
- 相关路线图：[架构整改与参考插件重新对齐路线图](../ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md)
- 前置决策：
  - [ADR-0001：以 0.2.0 重建可信主干](0001-m0-0.2.0-trusted-trunk-rebuild.md)
  - [ADR-0002：限定 M1 的参考数值契约](0002-m1-reference-numeric-scope.md)

## 背景

M0 已经建立 0.2.0 workspace、唯一 `AnalyzerSession`、严格同步
`PcmSource`、串行 application façade 以及 CLI/Tauri 共用的 wire DTO。M1 又把
`foo_dr_meter 1.0.8 x64` 的纳入范围收口为
`FooDrMeter108CandidateV1 / Unverified`，并固定了对应的数值证据、规格和本地
回归门禁。

旧 M2 描述仍混合了四类已经不再同阶段的事项：

1. “迁移剩余稳定 codec/backend”没有定义具体能力和毕业标准，而多 backend
   registry、Opus 与 FFmpeg 生命周期属于 M3；
2. “根据 reference 校正 Candidate”已经由 M1 完成，不应在 M2 常规重开；
3. SIMD 没有现存实现或 benchmark 依据，真正的性能工程属于 M6；
4. EdgeTrimmer 没有新的产品需求，旧实现又已有误删真实音频的证据。

直接照旧 M2 执行会为了填充里程碑而重新引入格式、优化和预处理复杂度。M2 因此
改为先扩张可信度和可验证边界，再决定是否扩张用户可见能力。

## 当前基线与已发现缺口

### 已成立的基线

- 所有第一方 Rust crate 使用 `#![forbid(unsafe_code)]`；
- 产品只有一个 safe scalar `AnalyzerSession`；
- 当前唯一 backend 是进程内 Symphonia；
- 当前稳定矩阵为 WAV integer/float PCM、FLAC、AIFF integer PCM；
- `PcmSource` 只有 `Data / Eof / Error`，EOF 与 terminal error 均为 sticky；
- 分析器已有窗口边界、随机 chunk、1/2/3/6/8/16 声道、异常数值、静音、
  极短输入和长流固定状态测试；
- ADR 建立时（commit `e6b41dc`）workspace 有 94 项 Rust 测试，没有
  benchmark、fuzz、SIMD 或并行分析路径。

### 必须先关闭的缺口

1. `PcmBlock::new(samples, channels)` 使用声道数验证 frame 对齐，却没有把构造
   时采用的声道几何保存在 block 中。application 随后只把裸 samples 交给分析器。当前
   Symphonia source 会自行核对格式，所以现有路径没有已知错误；但未来 source
   若用错误声道数构造一个“恰好仍能整除”的 block，application 无法拒绝静默
   重解释。
2. 支持格式文档声明 WAV 和 AIFF integer PCM 覆盖 8/16/24/32-bit；产品测试
   主要只覆盖两者的 16-bit。WAV float32/float64 与一个 mono 16-bit FLAC 已有
   测试，但尚未形成完整声明矩阵。
3. codec 测试按具体 fixture 分散编写，没有每条 route 都必须通过的共同
   `PcmSource` contract harness。
4. 多数合法 codec fixture 一次 `Data` 后即 EOF，没有系统覆盖多 block progress
   单调性、中途失败和所有 route 的 sticky 终态。
5. container parser、probe 与 decoder 尚无固定 mutation/crash corpus 或本地
   fuzz 入口。
6. analyzer 的随机 chunk 测试集中在一个 6 声道普通信号；复制声道测试无法发现
   lane 串音、索引错位和排列错误。现有 `PartialEq` 也不能区分所有浮点 raw-bit
   情况，例如 `+0.0/-0.0`。
7. 格式能力分别存在于 Rust 扩展名、container/codec enum、前端类型、GUI picker
   和双语文档中；增加 route 时存在重新漂移的风险。
8. `ChannelCount` 可表达 65,535 声道，而分析器会为每个声道分配 10001 个
   `u64` histogram bin。仅 histogram 裸数据就会达到 5,243,324,280 bytes
   （约 5.24 GB / 4.88 GiB）；逐声道 `try_reserve` 不能可靠防止 overcommit
   或进程被 OOM killer 终止。
9. 若干公开 report/progress 类型仍含裸浮点字段并直接派生 `Deserialize`。
   production 路径会产生有限结果，但 domain 的“有效类型”边界尚未完全由类型或
   构造器强制。

## 决策

### 1. M2 的核心是契约加固，不以格式数量作为完成标准

M2 先完成当前稳定矩阵的证据闭合。即使最终没有增加新格式，只要现有能力通过
统一契约、异常输入和跨层验收，M2 仍可完成。

M2 可以评估或增加现有 Symphonia backend 已原生支持的 container/codec route，
但不能引入第二 backend、外部 decoder 或另一套 application 路径。

### 2. PCM block 必须保留可核对的 frame geometry

`PcmBlock` 至少保留其构造时使用的 `ChannelCount`。application 在把 block 送入
`AnalyzerSession` 前，必须将其与打开时固定的 `PcmStreamInfo.spec.channels`
核对；不一致返回结构化 decode error，不得按另一声道几何静默解释。

这项检查只证明 application decode 路径的 channel-count/frame geometry 一致，
不证明 lane 顺序、channel layout 或 source 身份。直接调用
`AnalyzerSession::push_interleaved` 的库用户仍负责遵守创建 session 时提供的
`StreamSpec`。

采样率和 layout 仍是 immutable stream-level contract。具体 backend 必须在
内部拒绝运行中发生的采样率或声道变化，并用 route 专属负例覆盖；`PcmBlock`
不重新携带动态 format object。

该改动不把动态 format object 引回 `ReadOutcome`，也不改变分析器公开的
`push_interleaved` API。

### 3. 分析会话必须有显式资源上限

M2 固定 `MAX_ANALYSIS_CHANNELS = 64`，作为稳定的产品分析能力，而不是从当前
histogram 表示偶然推导出的上限。以当前表示计算，64 声道的 histogram 裸数据为
5,120,512 bytes；实现还必须用 checked arithmetic 计算实际 session 状态。

限制放在三个明确边界：

- domain 公开并记录 `MAX_ANALYSIS_CHANNELS`，但 `ChannelCount` 仍可描述更大的
  源媒体几何；
- codec probe 对超过上限的源在创建 decoder 前返回
  `UnsupportedFormat / Probe`；
- `AnalyzerSession::new` 对直接 API 调用返回
  `ResourceExhausted / Analysis`。

application 发现 block/spec channel mismatch 时返回 `DecodeFailed / Decode`。
不能把 allocator 是否拒绝、操作系统 overcommit 或实际 OOM 当作输入验证。

该限制是工程资源契约，不是参考算法规则，不进入 Candidate 数值 descriptor。
M3 的 application 全局资源预算可以在此上进一步收紧，但不能替代单 session 的
本地安全上限。

### 4. 分层建立 codec contract 与 adapter integration

`macinmeter-codecs` 内建立只依赖 domain/codecs 的共同 `PcmSource` harness。
每条 stable route 使用合法 fixture 与该 route 能稳定构造的损坏 fixture 验证：

- 按内容探测；错误扩展名不改变 route；
- `stream_info` 在 source 生命周期内不可变化；
- 每个 `Data(PcmBlock)` 非空、有限、完整 frame 对齐，且 block channel count
  匹配 immutable stream info；
- progress 单调，decoded frames 等于累计 block frames；
- expected frames 与 decoded frames 分别保存，并在有声明时严格核对；
- 正常 EOF sticky；
- 该 route 可稳定构造的损坏或截断输入严格失败，不生成 partial successful
  report；
- 固定输入的 `f64` PCM 归一化满足明确 oracle。

terminal error 的完整结构和值 sticky 属于具体 `PcmSource` 实现的状态机义务：
当前共享 `SymphoniaPcmSource` 使用确定性的 fault-injection test seam 验证一次，
不通过“打开后截断文件”等时序技巧为每个位深伪造 read-time error。未来新增
不同 `PcmSource` 实现时，必须独立通过同一 terminal-state harness；某条 route
本身若有稳定的 read-time corruption fixture，也同时纳入 route 专属回归。

route 专属测试只补充 container 或 codec 特性，不复制已由同一 source 实现证明的
终态状态机。

高层覆盖保持单向依赖：

- `macinmeter` application integration 使用真实 fixture 覆盖每类 stable route；
- application 内建立 crate-private、可接受 `OpenedAudio` 的编排 seam，production
  的 `DecoderFactory::open` 与 fake-source 单测都委托给它；
- CLI 对每类用户可见 route 至少有一条端到端路径；
- Tauri 继续测试“只调用 application”这一通用架构契约，并单独测试 capability
  查询/picker；不为每个位深复制一套 Tauri 分析测试。

codec crate 的共同 harness 不依赖 application、CLI 或 Tauri。

### 5. 补齐当前已声明的格式矩阵

在增加任何新 route 之前，至少补齐：

- WAV 8/16/24/32-bit integer PCM；
- WAV IEEE float32/float64 PCM；
- AIFF 8/16/24/32-bit integer PCM；
- FLAC 的合法、损坏、frame-count 与多 block/多声道代表样本；
- AIFF 与 FLAC 各至少一条 Rust API、CLI 和共享 report 端到端路径；
- 测试 fixture 的生成方式、几何、预期 PCM 和来源/许可记录。

位深存在于实现 feature 或枚举中不等于获得 stable 支持声明。

本矩阵把 WAV 支持面固定为 classic RIFF/WAVE format tag 1（linear PCM）与
format tag 3（IEEE float）。`WAVE_FORMAT_EXTENSIBLE` 的 valid/container bits、
channel mask 与 GUID 尚未形成独立 capability 证据，因此在 probe 阶段明确返回
`UnsupportedFormat`，不继承 Symphonia 的隐式支持。稳定 AIFF route 只接受可由
`u32` 精确表示的有限正整数 80-bit sample rate、恰好 18 bytes 的 COMM chunk，
以及零 SSND offset/block-size；其他合法变体也必须按新增 route 的准入条件单独
毕业。

### 6. 扩展 analyzer 的 bit-exact 工程不变量

M2 使用 test-only raw-bit projector 比较完整核心结果，不增加 production
intermediate snapshot API，也不复制第二套参考算法。

raw-bit 比较只用于同一 test binary 内不同执行方式之间的 metamorphic 对照；
不把本机 `log10/powf` 结果保存成跨 OS/libm 的通用 bitwise golden。已有固定
reference bit 结论仍只按各自证据范围使用。

确定性覆盖矩阵至少包含：

- 声道数：1/2/3/6/8/16；
- 长度：0、1、`W-1/W/W+1`、`2W-1/2W/2W+1`；
- chunk：整块、逐帧、跨窗口、固定 seed 伪随机、插入空 chunk；
- signal：静音、各声道不同信号、lane 单点扰动、声道排列、subnormal、
  signed zero、histogram/peak 边界和有限过满值；
- invalid input：错帧、NaN、正负 Inf、平方/累计溢出，覆盖首/中/末声道与窗口
  前后。

需要验证：

- 合法 frame-aligned chunk 切分不改变结果 raw bits；
- 单 lane 扰动不改变其他 lane；
- 声道排列后的逐声道结果可逆映射；
- 失败前不提交部分 mutation；错误后继续输入与 clean baseline raw bits 相同；
- 内存状态只随声道数增长，不随 stream 时长增长。

矩阵采用关键边界全交叉加其余轴的确定性成对组合，不要求所有维度形成不可维护的
完整笛卡尔积。

声道排列比较先逆映射 `channel_index` 与 layout position，再比较逐声道数值。
track aggregate 和 track report 都按原始声道顺序做浮点归约，因此不要求排列前后
的 track-level raw bits 相同；只断言规格实际保证的关系。

截至 2026-07-19，本切片已完成：

- 穷尽解构 `AnalysisResult` 的 test-only projector 将所有浮点转为 raw bits，
  并以 signed-zero 自测试证明它能发现普通 `PartialEq` 看不到的差异；
- 私有 `SessionBits` 穷尽记录分析状态，`StorageShape` 记录所有持久容器的
  长度和容量；二者都不进入 production API；
- 全交叉矩阵覆盖声明的声道数、窗口长度和 chunk 方案，并补齐 lane 独立 mono
  等价、局部扰动、可逆 PCM/layout 排列、错误后安全继续和定长存储；
- 定向数值测试补齐窗口长度表、`N=6` loud-count floor、稀疏 histogram、正常
  secondary peak、最终 `+0.0` clamp 和 public-f32 半值边界。

这些测试只证明同一 test binary 内不同执行方式的工程等价，不构成
foo_dr_meter DLL 的跨平台 bit parity 声明。

### 7. 收紧有限范围内的 domain 有效性边界

M2 只收紧下列 production output 边界，不以一次封闭整个 report graph 为目标：

- `DecodeProgress.fraction` 必须有限并位于 `[0, 1]`；
- `AnalysisResult.channels.len()` 必须等于 `stream.channels`，channel index 连续；
- 每个 channel outcome 的 frames 必须等于 `AnalysisResult.frames_seen`；
- report duration 的 decoded frames/sample rate 必须等于 result 的
  frames/stream sample rate；
- `AnalysisReport.pcm.spec` 必须等于 `AnalysisResult.stream`；
- 成功 report 的 diagnostics decoded frames 必须等于 analysis frames。

成功结果中的裸浮点优先改为透明 `FiniteF32/FiniteF64`；不需要作为输入的
report 类型可以删除 `Deserialize`。具体类型可以使用私有字段加有效构造器，
也可以在唯一构造边界集中验证，但上述关系必须无法从 production façade 绕过。

M2 继续允许 0.2.0 Rust API breaking change，不提供无效结构的兼容构造别名。
透明有限 wrapper 不改变 JSON 数值形状；任何字段、tag 或 enum 集合变化都必须
显式评估 wire schema 升级，不能静默破坏严格消费者。

这不要求把参考实现的内部状态复制进 domain，也不增加 production diagnostic
snapshot。

截至 2026-07-20，本切片已完成：

- `DecodeProgress`、`AnalysisResult` 和 `AnalysisReport` 封闭字段；
  progress 只由 decoded/expected/eof 派生有限且 clamp 到 `[0, 1]` 的 fraction，
  后两个结果根只能通过 `try_new` 建立，并只公开 getter 或只读
  `AnalysisResultView`；
- `AnalysisResult::try_new` 固定 channel 数量、连续 index、outcome frames 和
  duration frames/sample rate 关系；`AnalysisReport::try_new` 固定 PCM spec
  与 diagnostics decoded frames 关系。前者失败归入
  `AnalysisFailed / Analysis`，后者归入 `DecodeFailed / Decode`，application
  继续补充 path/backend 上下文；
- algorithm parameters、channel metrics、aggregate DR 和 progress fraction
  使用透明 `FiniteF32/FiniteF64`；不作为产品输入的 result/report、
  batch/event/wire 类型删除 `Deserialize`，request/profile 及独立 source/PCM
  metadata 等真实输入类型继续保留；
- JSON 字段、tag 和数字形状均未改变，wire schema 保持 v3。

这个切片不把 `AnalysisResultView` 当作可独立构造的有效性证明，也不新增
aggregate 派生值一致性、source metadata 等于实际 PCM、expected frames 等于
decoded frames 或 EOF/fraction 联动约束。它只封闭本节列出的成功结果关系，避免
把更宽的媒体事实误收紧为不成立的 product invariant。

### 8. 异常媒体采用“固定回归 corpus + 手动 fuzz”

普通 workspace 测试保存小而确定的 malformed/mutation corpus。任何 fuzz
发现的 crash、panic、超时、过量分配或错误终态，最小化后进入该 corpus。

低层 WAV/AIFF parser 重构为可接受 `Read + Seek` 的实现，使 crate 内测试可直接
消费 bytes。若使用外部 fuzz runner，只开放非默认 feature 下的隐藏 dev
入口；默认产品 API 仍是基于 `Path` 的 `DecoderFactory`。

fuzz/sanitizer 是独立的本地或手动任务：

- 不进入 pre-commit；
- 不要求网络；
- 不自动触发远端 CI；
- 不以运行时长或“跑了若干小时未发现问题”替代回归样本；
- 优先覆盖 WAV/AIFF chunk parser、FLAC packet failure 与 `PcmSource` 状态机。

扩展 verifier 对固定 corpus 使用逐 case 子进程和 timeout；平台支持时同时施加
内存上限并记录限制方式。M2 的声明只覆盖已保存 corpus，不声称证明所有字节输入
永不 hang 或分配有界。

截至 2026-07-20，本切片已完成：

- 提交 `tests/fixtures/malformed-media-v1`：34 个确定性 case，覆盖 WAV/AIFF
  chunk 结构（截断、长度越界/下溢、非法字段、重复 chunk、固定 seed 的
  尺寸域 XOR）、FLAC 包失败（magic、STREAMINFO 截断、帧内字节翻转、中途截断、
  末字节翻转）与跨容器输入（未知内容、空文件）；manifest 记录每 case 的派生
  操作、SHA-256 与预期错误码/阶段；
- 第一方 WAV/AIFF parser 改为接受 `Read + Seek` 的字节接缝，crate 内测试可
  直接消费 in-memory bytes；非默认 `malformed-dev` feature 暴露隐藏
  `dev::probe_container_bytes` fuzz 入口，默认产品 API 不变；
- workspace 回归测试逐 case 校验字节身份、结构化失败、无 EOF/partial success
  与 decode 终态 sticky；`scripts/verify-malformed-corpus.py` 以逐 case 子进程
  + 30s timeout 执行，POSIX 上施加 `RLIMIT_AS`（默认 2 GiB），无该接口平台
  跳过并记录；`scripts/generate-malformed-media-v1.py --check` 审计提交字节与
  确定性再生成一致。

### 9. 新能力必须经过明确准入

新增 container/codec route 合并为 stable 前必须：

1. 仍使用当前进程内 Symphonia backend；
2. 有可靠 content signature/probe，不依赖扩展名路由；
3. 通过共同 `PcmSource` harness 和格式专属损坏测试；
4. 有合法、可再生成或来源清楚的 fixture；
5. 经 lossless 等价输入验证时，产生与既有 container 相同的核心结果；
6. 通过 application、CLI 和 Tauri 共享路径；
7. 同步 Rust capability、GUI picker/类型和中英文支持文档；
8. 明确评估新增 enum 对 wire schema 和严格消费者的影响；
9. 复核新增依赖/feature 的 license、advisory 与 `THIRD_PARTY_NOTICES`；
10. 不把“Symphonia 有 feature”直接等同于产品稳定支持。

M2 首选评估顺序为：

1. AIFC linear PCM；
2. MP4/M4A + ALAC；
3. MP3；
4. Ogg/Vorbis；
5. MP4/M4A + AAC。

这是调查顺序，不是支持承诺。可以在完成当前矩阵后一次评审一个原生能力批次，
但每条 route 仍独立满足毕业条件。

### 10. Candidate 在 M2 冻结

M2 不以继续逆向或主动修改 Candidate 为交付物。出现新的足够静态/动态证据、
最终可观察反例或实现转写缺陷时，按 ADR-0002 的证据规则处理。只有规格语义变化
才提升 profile version；单纯把实现修回既有规格不制造新的 profile 身份。

规格语义变化同步规格、边界测试和必要的 conformance 记录。实现修复至少同步
产品回归测试，并说明既有历史 observation/conformance 是否仍适用。

本切片发现并修复了一处既有规格的转写缺陷：有限非零 PCM 的平方下溢为零时，
Candidate 控制流保留数值 DR `+0.0` 并将该声道纳入 track mean。产品现在将有
有效窗口的该边界表达为 `Measured`，而不是 `InsufficientData`。这一变更未改写
任何历史 observation/conformance artifact，也不提升 profile version 或
compatibility status。

codec 归一化问题必须先与 analyzer 算法问题分离，不能用修改算法补偿 backend
差异。

### 11. 性能数据不是 M2 门禁

当前已知潜在热点包括：

- `push_interleaved` 的 finite scan、事务性 shadow pass 和实际提交 pass；
- 每声道 10001 个 `u64` histogram bin 的初始化与扫描；
- decoder 每 block 的 `SampleBuffer<f64>` 分配及 `to_vec`；
- no-op progress sink 仍会构造带路径的 progress event。

这些只作为 M6 profiling 候选，不构成现在重构的理由。M2：

- 不添加 SIMD；
- 不恢复包级或文件级并行；
- 不设置 elapsed、throughput 或 RSS 的测试阈值；
- 不因微基准更快而放松错误原子性；
- 只有准备引入第一条优化路径时，才在 M6 选择并加入微基准工具；
- 性能 A/B 必须先验证完整结果 fingerprint，再采用同机交错 AB/BA 和原始样本
  记录。

### 12. 能力目录最终只有一个 Rust 事实源

M2 在首次新增 route 前建立结构化 capability catalog，至少表达：

- container；
- codec；
- `planned / experimental / stable / unavailable` 状态；
- discovery extensions；
- backend；
- 关键限制。

实现契约固定为：

- `macinmeter-codecs` 拥有静态 catalog 和 stable discovery extensions；
- `macinmeter` 暴露只读 capability DTO/query；
- Tauri 提供 `get_capabilities` 命令；
- TypeScript 把 container/codec 标识视为可向前扩展的字符串，并从运行时返回的
  stable extensions 构造 picker，不生成另一份手写能力 union；
- CLI/batch discovery 使用同一 Rust catalog；
- `planned/experimental` 不进入默认 discovery 或用户稳定支持文档；
- 文档仍人工解释限制，但产品测试固定当前 stable catalog snapshot。

capability query 是独立 application API；若分析 report 的 container/codec enum
新增值会破坏 schema-v3 严格消费者，首次 route 毕业时显式升级 wire schema。

## 明确非目标

M2 不包含：

- 第二 decoder backend、`DecodePlan` 或 backend registry；
- FFmpeg、DSD、Songbird/Opus 或外部进程；
- 文件级/包级并行与全局资源预算；
- SIMD、性能承诺或发布 benchmark；
- EdgeTrimmer、静音过滤、重采样、增益或其他 preprocessing；
- foobar decoder、metadata、playlist、host 或完整文本 parity；
- 把 Candidate 改名为 `Reference`/`Verified`；
- 恢复 0.1.x universal decoder 或兼容 API。

若 EdgeTrimmer 或其他 preprocessing 出现明确需求，必须另立 ADR，放在分析器
之前的独立显式 stage，并在请求与报告中可观察；不得悄悄改变 Candidate PCM。

## 实施切片

按以下依赖顺序推进：

1. `fix: bind PCM blocks to their channel geometry`
   - `PcmBlock` 保存构造时的 channel count；
   - application 编排 seam 核对 block/spec；
   - fake source/错误 block 负例固定 `DecodeFailed / Decode`；
   - 保持现有 wire schema 不变。
2. `fix: enforce analyzer session resource limits`
   - domain 固定并公开 64 声道上限；
   - codec 在 decoder creation 前返回 `UnsupportedFormat / Probe`；
   - analyzer 超限稳定返回 `ResourceExhausted / Analysis`；
   - 分配前 checked 计算 per-session 状态；
   - 不依赖实际巨额分配测试。
3. `test: establish the shared PcmSource contract matrix`
   - 抽出共同 harness；
   - 先让现有 WAV/FLAC/AIFF route 全部通过。
4. `test: close the declared native PCM matrix`
   - 补齐 WAV/AIFF 位深、FLAC 代表样本和 application/CLI 端到端覆盖；
   - 建立 product fixture manifest。
5. `test: expand bit-exact analyzer invariants`
   - raw-bit projector；
   - chunk/channel/lane/transaction 矩阵；
   - 保留普通测试快速、确定、无网络。
6. `refactor: enforce valid domain result construction`
   - 只收紧本 ADR 列出的 progress/result/report 关系；
   - 用有限 wrapper、有效构造器或集中验证边界收紧；
   - 显式决定 Rust API 与 wire schema 影响。
7. `test: add malformed media regression corpus`
   - 固定 mutation seeds；
   - 建立 byte-oriented parser seam 和逐 case 子进程 timeout；
   - 提供显式本地扩展验证脚本并记录平台资源限制；
   - fuzz crash 最小化后回灌。
8. `refactor: centralize native codec capabilities`
   - 建立单一 Rust catalog；
   - discovery/application/Tauri 消费；
   - 评估 wire schema。
9. `feat: graduate evidence-backed native routes`
   - 当前矩阵闭合后再决定实际批次；
   - 每条 route 独立满足准入条件。

## 出口条件

M2 完成必须同时满足：

- application decode 路径的 block/spec channel geometry mismatch 无法静默进入
  analyzer；
- 超过 64 声道的媒体在 decoder creation 前返回
  `UnsupportedFormat / Probe`，直接 session API 返回
  `ResourceExhausted / Analysis`；
- 当前正式声明的 WAV/FLAC/AIFF route 全部通过共同 contract matrix，当前
  `PcmSource` 实现独立通过 sticky terminal-state harness；
- WAV/AIFF 已声明位深具有合法 fixture 与 PCM normalization oracle；
- analyzer 对声明矩阵中的合法 chunk 切分保持完整结果 raw-bit 一致；
- lane 隔离、声道映射和失败事务性具有确定测试；
- production façade 不能产生违反本 ADR 所列有限值/跨字段关系的成功结果；
- 固定 malformed corpus 在有 timeout 的独立 case 中不产生 panic、超时或
  partial success；资源限制按实际支持平台记录，不外推为所有输入证明；
- 任何新增 stable route 都通过 application、CLI 和 Tauri 共享边界；
- capability catalog、GUI 和双语文档不发生支持矩阵漂移；
- M1 派生出的普通产品 numeric boundary/regression tests 保持通过；历史
  observation/conformance artifact 不要求日常重跑，也不得被静默改写；
- 本地 fmt、严格 Clippy、workspace tests、TypeScript build 与 Tauri check 通过；
- 不触发或等待远端 CI。

## 后果

- M2 会先增加测试、fixture 和跨层校验，短期不保证新增格式；
- `PcmBlock` 领域对象获得解释自身 samples 所需的最小 frame geometry；
- codec route 的实现成本上升，但不会再以“能解出声音”替代完整终态契约；
- analyzer 的未来重构和 M6 优化将有 bit-exact 工程门禁；
- 性能机会被记录但不提前绑架正确性；
- M3 仍拥有多 backend、外部进程和统一资源预算；
- M6 仍拥有 benchmark、profiling、是否启用文件级并发、SIMD 与选择性优化。

## 未采用方案

### 直接开启 Symphonia `all`

拒绝。feature 可编译不证明 content probe、PCM normalization、终态、损坏输入和
adapter 集成均可靠。

### 先恢复 legacy universal decoder，再逐步修契约

拒绝。它会重新引入扩展名猜测、重复 backend 和未受控错误语义。

### M2 立即恢复 SIMD 或并行

拒绝。当前没有可复现 baseline，也没有第二执行路径需要产品承担。M2 只建立未来
优化必须通过的 bit-exact 门禁。

### 为兼容参考插件恢复 EdgeTrimmer

拒绝。EdgeTrimmer 不是参考 analyzer core 的组成部分，旧实现还存在删除真实音频
的已知反例。
