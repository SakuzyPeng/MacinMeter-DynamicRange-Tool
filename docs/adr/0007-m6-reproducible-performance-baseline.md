# ADR-0007：M6 可复现性能基线与比较协议

- 状态：Accepted
- 日期：2026-07-20
- 范围：0.2.0 标量生产主链的 benchmark、profile 与后续优化比较

## 背景

0.1.x 曾把包级/文件级并行、手写 SIMD、FFmpeg/DSD 与理论 CPU 能力混合成性能
叙述。M0 删除了这些路径，M2 只记录了可能的热点，M3 固定一个 active job 的
application 预算，M5 则清除了旧 benchmark 与伪加速结论。当前 0.2.0 主链安全、
串行、标量，但没有能够回答以下问题的可信数据：

- 时间主要消耗在 discovery、decode、analysis、report/render 的哪一层；
- 声道数和格式如何影响吞吐与常驻内存；
- 串行 batch 是否已经形成用户可感知瓶颈；
- 某项优化在结果完全相同的前提下是否仍有稳定收益。

只跑一次 CLI、只比较两个固定顺序的 wall time，或者从 CPU 指令集推导理论倍数，
都不能回答这些问题。

## 决策

### 1. 基线是专用工具，不进入产品 API

M6 使用 `crates/macinmeter/examples/m6_baseline_worker.rs` 驱动现有公开契约：

- direct finite interleaved `f64` 通过唯一 `AnalyzerSession`；
- decode 通过当前 `DecoderFactory` / `PcmSource`；
- file、batch 与 discovery 通过唯一 `Application`；
- JSON rendering 通过共享 `WireEnvelope`。

worker 是 release example，不新增生产 profile、backend、feature、scheduler 或公共
benchmark API。它与所有第一方 Rust 目标一样 `#![forbid(unsafe_code)]`。

### 2. 语料确定生成，但大媒体不提交

`scripts/generate-performance-corpus.py` 在
`target/performance-corpus/m6-performance-baseline-v1` 生成约 129 MB 的本地语料：

- 同一 60 秒、48 kHz、双声道 PCM 分别封装为 WAV s16、AIFF s16、FLAC s16 与
  WAV float64；
- 一条 30 秒、48 kHz、6 声道 WAV s24；
- 8 条各 15 秒的 WAV batch；
- 1024 个可发现文件与 256 个应忽略文件的目录树。

语料不含私人或版权音频。manifest 固定 generator bytes、容器 bytes、geometry、
归一化 interleaved f64 SHA-256 与 FLAC encoder identity。修改 generator 后旧
manifest 必须失败并重新生成，不能把不同数据静默视为同一基线。

大语料留在 ignored `target/`，不扩大源码仓库；正式记录保存 generator/corpus
manifest hash、worker binary hash 与原始样本。

### 3. 基线分层，不通过相减伪造 attribution

suite v1 有 15 个 case：

- analysis：2ch/600s、8ch/180s、64ch/30s；
- decode：WAV s16、AIFF s16、FLAC s16、WAV f64；
- application：上述四个同源双声道 route 与 6ch WAV；
- batch：当前 application 串行处理 8 条 WAV；
- discovery：递归扫描 1024 supported + 256 ignored；
- rendering：analysis 在计时区外，重复生成 pretty wire-schema-v3 JSON。

每个 scope 只陈述自身包含的工作。`application - decode`、`CLI - application` 等
差值不是独立计时，不得当作 analysis 或 rendering 的精确成本。

### 4. 每个样本同时记录工作时间、原生资源与完整身份

worker 使用 `std::time::Instant` 包围命名 workload；输入块/语料准备、结果
serialization 和 decode 的完整 PCM SHA-256 verification 不混入对应 workload
计时。runner 另行记录：

- macOS `/usr/bin/time -l` 或 Linux `/usr/bin/time -v` 的 user/system、max RSS
  与可用原生计数器；
- 使用 `ps` 周期采样、排除 measurement wrapper 后的 descendant process-tree
  RSS 总和；
- source commit/clean state、release worker SHA-256/size；
- Cargo release profile、rustc/Cargo/LLVM、target、OS、CPU/model、内存、电源
  来源和相关 `RUSTFLAGS`；
- corpus manifest、suite definition、seed、调度序号与全部原始样本。

正式 baseline 默认拒绝 dirty tree。`--allow-dirty` 只用于开发 harness，结果必须
醒目标记 `source.state = dirty`，不能进入性能结论。

### 5. 正确性先于计时

每次 worker 运行必须输出 schema-v1 JSON、有限数值、明确 work units 与
SHA-256 result fingerprint。runner 在形成 summary 前执行：

- 同一 case 的所有 measured sample fingerprint 完全一致；
- A/B 所有 variant 的同一 case fingerprint 完全一致；
- decode 的完整 interleaved f64 hash 等于 corpus oracle；
- geometry、audio seconds、sample/item 数与 manifest/arguments 完全一致；
- 携带相同归一化 PCM 的 WAV/AIFF/FLAC/float64 application case 具有相同
  `AnalysisResult` fingerprint；
- process-tree RSS 至少取得一个有效样本。

任何一项失败时整次 run 失败，不发布“更快但结果不同”的摘要。未来 SIMD 或其他
执行路径还必须补产品层 bit-exact/differential tests；benchmark fingerprint
不是其唯一正确性门禁。

### 6. 调度与统计不隐藏波动

默认每个 case/variant 先 warmup 1 次，再 measured 7 次。所有 measured
`case × variant × repetition` 使用固定 seed 完全交错洗牌；warmup 使用独立 seed。
结果保存 min、p10、median、p90、max 与 median absolute deviation，原始样本全部
保留，`outliersRemoved` 固定为 0。

未来 A/B 必须在同一 run 中用 `--variant NAME=EXECUTABLE` 同机交错；不允许用两次
相隔较久、顺序固定的 run 直接宣称小幅收益。每个显式 variant 还必须提供
`--variant-source NAME=COMMIT`；source commit、worker hash 和结果 fingerprint
都是比较身份的一部分。

### 7. 基线没有跨机器阈值或用户性能承诺

正式记录是固定 source/binary/corpus/environment 的本机证据，不是：

- 跨 OS、SDK、toolchain 或机器的可复现构建声明；
- CI pass/fail 的 elapsed/RSS 阈值；
- 任意音频、存储设备或冷缓存的吞吐保证；
- 恢复文件级并行、SIMD、包级并行或外部 decoder 的预授权。

普通 pre-commit、workspace test 和手动 CI 不运行 benchmark。远端 CI 继续保持
手动且不消费该语料。

### 8. Sampling profile 使用独立的、可核对的捕获协议

M6 的首批 sampling profile 使用 macOS Xcode Time Profiler，不把 profiler
依赖带入 Rust workspace。`scripts/run-performance-profile.py`：

- 使用与基线相同的 release 优化、thin LTO 与单 codegen unit，只把
  `debug = 1`、`strip = false` 写入独立的 ignored build 目录以获得可解析符号；
- 在 worker 内用两个 `#[inline(never)]` 函数精确包住原有 analysis/decode
  `Instant` 计时区间；它们只建立采样栈边界，不新增产品 API 或算法分支；
- 默认对 stereo direct analysis、64-channel direct analysis 与 FLAC decode
  各做三次约 5 秒捕获，使用固定 1 ms Time Profiler sampling；
- 只纳入栈中含对应计时边界函数的 sample，因此进程启动、结果序列化及 decode
  的计时区外 PCM hash verification 不进入热点比例；
- 每次至少要求 1000 个有效 sample，且有效 sample weight / worker elapsed 必须
  落在 `0.85..1.15`，防止符号丢失或边界错误时仍生成结论；
- 继续验证 worker result fingerprint、work unit 与 FLAC decoded-f64 corpus
  oracle，三次捕获之间必须完全一致。

原始 `.trace` bundle 和 XML export 较大，保留在 ignored `target/`；正式 JSON
记录绑定每个 bundle/export 的 SHA-256 与大小，并保存每次捕获的完整折叠栈计数、
leaf/inclusive/source-line 聚合、profiler/Xcode 身份及所有 worker 输出。这样能
复核已提交的比例，也不会把平台专用二进制采样产物永久加入源码仓库。

Sampling 记录只用于同一 source/binary/host 上的函数归因。带 debug symbol 的
profile worker elapsed 不能替代第 4–7 节的 canonical scalar timing，更不能形成
跨机器或用户吞吐声明。

## M6 实施切片

1. 建立本 ADR、deterministic corpus、release worker、interleaved runner 与工具
   测试；
2. 从 clean harness commit 运行 suite v1，提交环境、原始样本摘要与明确限制；
3. 对基线中占主导的生产 scope 做 sampling profile，不先写优化；
4. 只有 profile 显示明确瓶颈时才提出一个优化 ADR/切片；
5. candidate 与 scalar 先通过完整差分门禁，再执行同 run 交错 A/B。

首个 candidate 已按该协议完成。`ab09c8b...` 对 1–4 声道保留原 validation
traversal，对 5–64 声道使用合并 finite check 的 frame-major transactional
shadow；与直接父提交做同轮交错后，stereo 中位差异 −0.04%，8ch elapsed
−4.45%，64ch elapsed −19.58%，三项跨 variant fingerprint 均一致。完整身份与
限制见
[`M6_VALIDATION_TRAVERSAL_AB_REPORT.md`](../performance/M6_VALIDATION_TRAVERSAL_AB_REPORT.md)。

## 初始基线出口条件

- 15 个 case 均达到足以多次 process-tree sampling 的工作量；
- 四个同源稳定 route 的 decoded f64 oracle 一致；
- clean source、worker、suite、corpus、toolchain 与 environment identity 完整；
- 7 次 measured sample 全部保留且 fingerprint 稳定；
- 本地标准 Rust/Python 门禁通过；
- 没有同时引入任何产品优化、并发轴或性能承诺。

## 后果

M6 可以先回答“现在慢在哪里”，再讨论“要不要优化”。自定义 runner 比单纯微基准
多维护一层协议，但它能覆盖 application、decoder、进程资源与未来同机 A/B，
同时不把 benchmark 依赖带入生产 crate。

基准本身会受到 OS cache、温度、电源和后台负载影响；因此记录分布和环境，而不把
单次最小值包装成事实。发现大幅波动时应先重跑或解释环境，不能删除不利样本。

## 未采用方案

### 只加入 Criterion 微基准

Criterion 适合进程内函数，但不能单独覆盖 codec 文件路径、Application、batch、
discovery、完整进程资源或未来不同 binary 的同机交错。若 sampling profile 指向
某个纯函数，后续仍可为该热点增补微基准。

### 用现有个人音乐库建立基线

它不可公开、不可稳定再生，也会把内容、格式和存储状态混在一起。个人样本可以用于
独立探索，不能成为仓库基线 oracle。

### 先恢复文件级并发再测

这会同时改变 scheduler、资源预算与性能，失去可信标量对照。文件级并发只能作为
未来 candidate 与本 ADR 固定的 scalar baseline 做差分和交错比较。
