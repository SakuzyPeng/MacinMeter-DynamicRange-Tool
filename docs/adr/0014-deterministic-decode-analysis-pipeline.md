# ADR-0014：确定性有界并行与 packet 解码优先

- 状态：Accepted
- 实施状态：In progress（第 1–2 步已完成；三个并行轴均未启用，生产路径仍为串行）
- 日期：2026-08-02
- 决策范围：解除窗口级、packet 级与文件级并行的一刀切硬禁令；固定统一资源与
  确定性契约；把受限 route 的 packet 级解码定为首要优化方向
- 前置决策：
  - [ADR-0001：以 0.2.0 重建可信主干](0001-m0-0.2.0-trusted-trunk-rebuild.md)
  - [ADR-0003：M2 原生解码面与工程契约加固](0003-m2-native-decoder-contract-hardening.md)
  - [ADR-0004：M3 application 执行预算](0004-m3-application-execution-budget.md)
  - [ADR-0005：M4 固定 x64 数值声明与 decoder-independent 验收](0005-m4-bounded-x64-numeric-claim.md)
  - [ADR-0007：M6 可复现性能基线](0007-m6-reproducible-performance-baseline.md)
  - [ADR-0013：稳定 MP4/M4A + ALAC 路由](0013-mp4-m4a-alac-stable-route.md)

## 背景

### 串行基线已经完成了它的任务

M0 删除 0.1.x 未受信的并行路径，M2 固定严格 `PcmSource` 契约，M3 建立唯一的
application 执行域，M4/M6 又建立 bit-exact 差分与 source-bound 性能协议。此前
“不恢复并行”的约束用于先取得可信串行基线，不是永久否定并行本身。

现在可以解除窗口级、packet 级和文件级并行的 blanket ban，但不能把“允许立项”
误写成“已经实现”或“一次性开放所有轴”。当前产品仍串行；每条并行路径只有通过本
ADR 的逐轴毕业门槛后，才可以成为默认生产路径。

### 参考实现包含并行调度能力

[`SA-foo-dr-meter-108-x64-parallel-dispatch-20260802`](../../reference/static-analysis/sa-foo-dr-meter-108-x64-parallel-dispatch-20260802.md)
登记了固定 target `TARGET-foo-dr-meter-1.0.8-x64-static-ff3556ad` 内部的
fork-join 并行：

- 调度器位于 RVA `0xdf30`。它取 `_Thrd_hardware_concurrency()` 与工作量需求的
  较小值作为线程数；线程数 ≤ 1 时退化为当前线程直接调用；否则以挂起态创建
  N 个线程、全部 `SetThreadPriority(-15)`、`ResumeThread` 放行，最后以
  `WaitForMultipleObjects(bWaitAll = TRUE, INFINITE)` 收敛；
- 线程体位于 RVA `0xdaf0`，以 `AcquireSRWLockExclusive` / `Release` 保护共享
  状态并通过虚调用领取工作项；
- 从该调度器可达 analyzer 的三个固定入口 `0x8410` / `0x89f0` / `0x8df0`；反过来
  从这三个入口出发不可达调度器。

这补充了一处此前未登记的结构关系：既有隔离 core 记录直接调用三个入口，明确
绕过并行调度器；完整组件则包含一个能够在这些入口之上 fork-join 的外层调度器。
两条既有记录在
[`窄字段对照`](../../reference/conformance/conf-foo-dr-meter-108-x64-isolated-core-safe-master-report-run1-20260719/record.md)
中精确匹配。不过，既有 foobar observation 没有动态记录调度分支或实际线程数，
因此该对照只证明直接单线程 core 与完整宿主路径的公开字段一致；它与结果不变的
并行结构相容，但不单独证明那次 report 运行实际进入了 `n > 1` 分支，也不决定
MacinMeter 应采用哪一种并行粒度。

### 单文件压缩解码是更大的杠杆

一次指示性本机测量（i7-11800H / Windows 11，同一 DAW 母带的三种编码，
222.1 s / 48 kHz / 立体声，扣除 80 ms 进程启动后的中位数）给出：

| 编码 | 端到端 | 其中解码 | 其中分析 |
| --- | ---: | ---: | ---: |
| WAV 16-bit | 182 ms | 约 90 ms | 约 92 ms |
| FLAC 24-bit | 607 ms | 约 515 ms | 约 92 ms |
| ALAC 24-bit | 1052 ms | 约 960 ms | 约 92 ms |

这些数字不满足 ADR-0007：它们不是 same-run interleaved A/B，没有与
result/PCM fingerprint 绑定，也只来自一台机器。它们只用于判断优化量级，不是
性能声明。

已提交的 M6 sampling profile 也把固定 FLAC case 的主要成本定位在 Symphonia
decoder 内部，并记录完整性 validator 是其中不可删除的一部分。两份证据共同说明：
只把串行 decoder 与串行 analyzer 放到两个线程重叠，理论上最多消除约 92 ms；
它没有触及 FLAC/ALAC 的主要单文件瓶颈。首个生产切片因此从“解码—分析流水线”
调整为 packet 级解码并行。

### 不能复活 0.1.x generic parallel decoder

旧提交 `2eb49f4` 中的通用并行 decoder 使用 `Vec<f32>`、
`DecoderOptions::default()`，并把 packet `DecodeError` 或 worker error 变成空 samples
以维持序号连续；这会跳过坏包并可能生成部分成功结果，违反当前有限 `f64`、
`verify: true` 和 sticky terminal error 契约。该提交记录的局部变化只有
208 → 217 MB/s（+4.3%），同时内存 54 → 65 MB（+20%）；这些旧数字也不满足
ADR-0007。后续一次调度替换只记录 +0.4%，明确处于测量误差内。

本决策只接受在 0.2.0/0.3.0 可信主干上重新设计的、route-specific 的有界实现；
不移植、包装或恢复旧 generic parallel decoder。

## 决策

### 1. 解除三类并行的硬禁令，并固定优先级

| 优先级 | 并行轴 | 主要目标 | 当前决定 |
| --- | --- | --- | --- |
| P0 | packet 级解码 | 单个 FLAC/ALAC 文件的解码延迟 | 立即允许按 route 实施；首个切片为 ALAC，FLAC 必须先解决全流 MD5 |
| P1 | 文件级 | batch/album 总吞吐 | 允许在同一 `ApplicationJob` 内实施有界 file lanes |
| P2 | 窗口级分析 | analyzer 吞吐 | 允许实施，但在压缩解码瓶颈之后 |

这张表是架构授权与实施顺序，不是完成状态。每个轴可以独立毕业，也可以继续保持
串行；无需再为“是否原则上允许”另立 ADR，但任何超出本 ADR 的 public tuning、
第二 backend、外部进程或 codec 能力扩张仍需单独决策。

原草案提出的双线程 decode-analysis overlap 不再是首个切片。它以后可以作为共享
预算下的内部实现细节接受差分与 A/B，但不是 packet 并行的前置条件，也不能占用
独立、未计量的线程池。

### 2. packet 级生产拓扑固定为顺序提交

首选拓扑为：

```text
顺序 probe / demux / packet 编号
              ↓
      有界 compressed-packet 队列
              ↓
  N 个 route-specific decoder worker
              ↓
      indexed Result + 有界重排序
              ↓
  按 packet 序核验、提交 finite f64 PCM
              ↓
       每文件一个 AnalyzerSession
```

- probe、container validation、track 选择和 demux 保持顺序；第一切片不并行解析
  ISO BMFF box 或 sample table；
- packet 在进入 worker 前获得稳定、单调的序号和预期 frame 几何；worker 只能返回
  `Result<DecodedPacket, AnalysisError>`，不能用空 PCM 表示失败；
- decoder worker 只按已经毕业的具体 route 创建，不建立从扩展名或泛型 codec
  descriptor 猜测“packet 独立”的公共工厂；
- 乱序完成可以发生，但 finite `f64` PCM、frame count、完整性状态与错误只按输入
  packet 序提交；`PcmSource::read_block` 的公共 `Data / Eof / Error` 契约不变；
- worker 数为 1 或输入不足时可以在开始解码前退化为串行；生产路径不得在并行错误
  后重跑串行并把失败隐藏成成功。

### 3. ALAC 是第一个 packet 并行切片

ADR-0013 的稳定 ALAC route 已经在 decoder 创建前固定单一 track、非 fragmented
ISO BMFF、`stts`/`stsz`、精确 packet/frame 数和 4096-frame cookie。这给顺序 demux
与 packet 编号提供了比 generic container 更窄的输入边界。

Symphonia 0.5.5 的
[`AlacDecoder`](https://github.com/pdeljanov/Symphonia/blob/v0.5.5/symphonia-codec-alac/src/lib.rs#L605-L629)
将 `reset()` 实现为空操作，`finalize()` 返回默认结果。这使 ALAC 成为当前最强的
首批候选，但源码形状本身不是“任意 packet 可独立解码”的充分证明；实现仍必须以
多 packet、短尾、1–8 声道、损坏 packet 和注入乱序完成的 route-specific 测试，
证明 worker 数变化不改变 decoded-f64 raw bits、帧数、错误或报告。

现有短小 fixture 足以固定正确性边界，但不能建立真实长音频的调度收益。默认启用
前必须增加 source-bound 的长 ALAC corpus，并按 ADR-0007 比较 1/2/4/8 worker 与
不同队列容量。没有正式 A/B 前，不声明 ALAC 加速比。

### 4. FLAC packet 并行必须先保住全流 MD5

FLAC frame 是独立编码的 block；Symphonia 0.5.5 的
[`FlacDecoder`](https://github.com/pdeljanov/Symphonia/blob/v0.5.5/symphonia-bundle-flac/src/decoder.rs#L236-L266)
也明确不在 packet 之间保存解码状态。但当前产品以 `verify: true` 打开 decoder，
decoder 会按整条 PCM 流更新 validator，并在 `finalize()` 将结果与 STREAMINFO MD5
比较。FLAC 格式同时把该 MD5 定义为未编码音频数据的流级签名，见
[Xiph FLAC format overview](https://www.xiph.org/flac/documentation_format_overview.html)。

因此多个 worker-local validator 的 `finalize()` 不能等价替代当前全流校验。FLAC
只有在提供“按原 packet 序更新的产品级全流 verifier”或另一条同等、可复核的完整性
设计后才可毕业。以下做法明确禁止：

- 为了吞吐把 `verify` 改为 `false` 或忽略 verification failure；
- 只验证每个 worker 的子序列，然后声称等价于全流 MD5；
- 在 parallel path 后再做一次完整串行解码，并把它描述为有效加速；
- 因某个 packet 失败而跳包、补零或返回 partial report。

WAV/AIFF 的解码成本当前不是 packet 级 P0；它们仍可通过文件级 lanes 获得 batch
吞吐。未来新增或扩张 codec route 时，不得从扩展名、FLAC/ALAC 的结论或
Symphonia generic 支持推导 packet 独立性，必须逐 route 重新毕业。

### 5. 三个轴共用一个 application 资源计划

ADR-0004 的“一个 active 顶层 job、最多 64 个 FIFO 排队 reservation”保持不变。
本 ADR 只定向取代其中“一个 active job 内 CPU/decoder/analyzer 并发数固定为 1”
和“batch 内文件必须永久串行”的约束。

每个 active job 在开始执行前由 `Application` 形成一个有界内部资源计划：

- worker permit、decoder reservation、queue/reorder PCM 预算由同一个计划拥有；
- `Application` 只向 owning lower layer 下发不含 application 依赖的 reservation/
  执行参数；`codecs` 与 `analysis` 不反向依赖 application，也不能绕过 reservation
  自建未计量 worker；
- 数值上限必须是代码中的固定、可测试上限，可结合宿主并行度向下收缩，但不得由
  媒体声明、batch 长度或递归任务自行放大；
- file lane、packet worker、window worker 和 decode-analysis overlap 消耗同一组
  permits，不允许形成 `file lanes × packet workers × window workers` 的乘法并发；
- 所需 permit 在调度子任务前一次性分配，worker 不递归等待另一层 pool 的 permit，
  避免嵌套 pool 死锁；预算不足时向更低并发或串行退化；
- compressed packet、decoded `f64`、重排序项和 decoder/session reservation 均
  计入有界计划；队列容量不能随媒体时长、packet count 或声明的 frame count 增长；
- Tauri/CLI 不建立自己的 Rayon/Tokio pool，也不绕过 `Application`。

只有持有该内部计划的 production `Application` 路径可以激活并行。直接使用低层
`PcmSource` 或 `AnalyzerSession`、以及无法取得内部 permit 的路径保持串行；这避免
新增公共线程调参或 process-global scheduler。

这仍是保守的资源上界，不冒充对 Symphonia 或系统 allocator 的逐 byte sandbox。
每个实现切片必须把实际 worker hard cap、队列容量、单 reservation 估算和退化规则
写入测试或伴随设计记录，不能只调用 `available_parallelism()` 后无上限扩张。

### 6. 确定性、错误、进度与取消是共同硬契约

所有并行轴必须满足：

- 同一输入的 decoded-f64 raw-bit fingerprint、`AnalysisResult` raw bits、报告字段、
  错误分类与 batch item 顺序不依赖 worker 数、队列容量、调度时序或宿主负载；
- packet/window worker 的结果以输入序号提交。多个 packet 失败时，输入序最早的
  packet error 胜出；即使较晚错误先完成，也必须等待所有更早序号确定后再暴露；
- progress 只按已经连续提交的 decoded frames 前进，保持单调，不把“worker 已完成
  但前序尚未提交”计入公开进度；
- EOF 只能在所有前序结果提交、精确 frame count 与 route integrity/finalization
  通过后出现。错误仍为 sticky terminal，且永远不能变成 EOF 或 partial report；
- 取消会停止继续分发、唤醒队列两端、回收并 join 所有 worker，再通过 RAII 释放
  reservation；不得留下 detached thread、后台解码、悬挂 channel 或部分报告；
- worker panic、channel disconnect、索引重复/缺口和 reorder overflow 都转为结构化
  terminal error，并在返回前完成 join；不得恢复 poisoned state 后继续出报告；
- crate-private 串行路径长期保留为 differential oracle，但不是公共 profile、
  compatibility engine、用户线程开关或第二套 analyzer。

### 7. 文件级并行的边界

文件级并行只发生在一个已获准的 batch `ApplicationJob` 内：

- 输入发现与 item 编号保持稳定；file lane 可以乱序运行，最终 `BatchResult` 仍按
  输入索引排列，既有全部/部分失败语义不变；
- progress event 可以交错，但必须携带足以区分 item 的稳定索引/路径，单文件内部
  progress 仍满足上一节的连续提交规则；
- 每个文件使用同一固定 `AnalyzerSession` 实现的一次独立会话；这不是第二 analyzer
  engine 或 profile；
- lane 与其 packet/window worker 共用同一资源计划。若一个压缩文件取得更多 packet
  permits，调度器必须相应减少 file lanes，而不是为每个文件建立独立线程池；
- batch 取消必须停止并 join 全部 in-flight item；某个 item 的普通失败仍按既有
  batch 契约记录，不得取消或吞掉其他已准入 item，除非用户请求取消。

文件级并行面向 batch/album 总吞吐，不用来声称单文件更快。默认启用前必须用包含
WAV、FLAC、ALAC 与混合时长的 batch corpus 验证吞吐、公平性、稳定结果顺序和 RSS。

### 8. 窗口级并行的边界

窗口级并行保留在单一固定分析算法内部，不增加公共 analyzer 类型或 profile：

- 只按规格的固定完整窗口边界分配工作；每个窗口内部的 frame/sample 运算顺序不变，
  唯一不足整窗的尾部按现有规则处理；
- window summary 可以乱序计算，但必须按窗口索引提交。任何跨窗口浮点累加仍按原
  窗口序执行；不能因 histogram 为整数计数、peak 为 max 就顺带重排其他浮点归约；
- numeric validation 与 commit 的分离、chunk 不变性和原始输入序的错误优先级保持
  不变；并行 inspector 不得用 rollback 取代当前事务性验证；
- 不得为窗口 worker 复制整份媒体 PCM。待处理窗口、summary 与 shadow state 必须
  受同一内存计划约束。

现有量级显示 analyzer 不是 FLAC/ALAC 的首要瓶颈，因此窗口级为 P2。只有新的
source-bound profile 显示它在目标 workload 中重新成为主导时，才默认启用。

## 毕业门槛

共同门槛：

- 39 项 safe-master 逐 token 对照保持 track DR 39/39、channel DR 62/62、
  overall peak 39/39、overall RMS 39/39、channel RMS 62/62、duration 39/39、
  差异数 0；
- 同一 corpus 在串行与 1/2/4/8 worker、最小/默认/最大队列容量下具有完全相同的
  decoded-f64 fingerprint、`AnalysisResult` raw bits 与 wire-visible report；
- 当前三套合法 corpus 的共享 `PcmSource` contract、ALAC/WAV 孪生与 route-specific
  malformed matrix 全部保持；新增长音频、多 packet、短尾与多声道 corpus；
- 用确定性 delay/fault injection 强制反序完成，覆盖最早/中间/最后 packet 或窗口
  失败、多个并发失败、worker panic、channel disconnect、取消和 EOF 竞争；
- sticky EOF/error、最早输入序错误优先级、连续 progress、精确 frame count、无
  partial report、所有线程 join 与 reservation 释放均有独立测试；
- 通过小队列长流与最坏乱序压力测试证明 queue/reorder 内存不随媒体时长增长；记录
  各 worker 配置的 RSS，不把 RSS 变成普通 CI 的跨主机阈值；
- 性能裁决按 ADR-0007 在同一次 run 完成交错 A/B，绑定 source、binary、suite、
  corpus、toolchain、环境、raw samples 与精确 result/PCM fingerprints；至少比较
  1/2/4/8 worker，并记录吞吐、elapsed、RSS 和退化点；
- candidate 只有在收益稳定超出同轮噪声、资源代价可接受且正确性门禁全通过后才
  默认启用；否则保留串行生产路径。正式记录前不得发布加速比。

packet 级另加：

- 每个 route 独立证明 decoder 初始化、packet state、finalization、完整性与
  normalized-f64 顺序；ALAC 先毕业，FLAC 必须额外证明与当前全流 MD5 等价；
- 损坏 packet 永不跳过，worker 数变化不改变错误阶段、错误码或最早失败身份。

文件级另加：

- batch item 顺序、独立失败语义、交错 event 身份与整批取消在不同 lane 数下完全
  一致；混合 route 不产生嵌套超额并发。

窗口级另加：

- 所有窗口/随机 chunk/声道几何、极端有限数值和 window-boundary corpus 的 raw-bit
  结果一致；浮点归约与 numeric-error precedence 保持原序。

## 实施顺序

1. 在 `Application` 建立共用 worker/memory 计划与向下传递的 reservation；在
   `codecs` owning layer 内建立 packet indexed result/reorder、crate-private 串行
   oracle 和确定性 fault-injection seam，不改变层间依赖方向；
2. 只为 ADR-0013 稳定 ALAC route 实现 packet 级 worker，补长 ALAC corpus 与正式
   ADR-0007 A/B；
3. 设计并验证 FLAC 的有序全流 MD5，再决定是否启用 FLAC packet workers；
4. 在同一预算上评估并实现 batch file lanes；
5. 只有 profile 重新指向 analyzer 时，才实现窗口级并行。

每一步单独提交、验证和记录；后一步不是前一步毕业的捆绑条件。

## 实施进度

### 第 1 步（2026-08-02，已完成）

共用预算与顺序提交层已落地，且不改变任何生产行为：

- `domain` 新增 `DecodeReservation` 与固定上限 `MAX_DECODE_WORKERS = 8`、
  `MAX_DECODE_QUEUE_CAPACITY = 64`、`MAX_IN_FLIGHT_PCM_BYTES = 64 MiB`。它不含
  application 依赖；allocation 字段不可变，收到它的 lower layer 不能就地放大。
  跨 crate 构造入口因 Rust visibility 必须存在，但已从支持的 public docs 与顶层
  façade 隐藏；第一方 production 只在 application plan 中构造。serial
  reservation 的 in-flight 预算为 0 bytes，直接表达“串行路径不得让任何已解码
  block 等待更早序号”；
- `macinmeter` 新增 `ConcurrencyPlan`，每 worker 派生 4 个排队 packet 与 4 MiB
  in-flight PCM。`allocate()` 是唯一的 permit 发放点，在 job 进入 admission 队列
  前一次性完成，且以整除保证 `file_lanes × workers_per_lane ≤ total_workers`；
  `bounded()` 同时受产品上限和 `available_parallelism()` 约束；
- `ApplicationJob` 持有该 allocation 并向 `codecs` 下发；`DecoderFactory` 只在
  收到的 permit 内解码，自身不创建 worker。当前生产 plan 恒为 serial、
  file lanes 恒为 1；`ConcurrencyPlan`、`PlanAllocation` 与 job allocation accessor
  均保持 crate-private，低层跨 crate wiring 入口为 doc-hidden，不形成支持的公共
  worker/queue 调参面；
- `codecs` 新增 crate-private `PacketReorderBuffer`：packet 在 demux 时获得稳定
  单调序号，失败是一等 `PacketOutcome::Failed` 而非空 PCM，提交严格按输入序，
  最早失败序号胜出，committed failure 之后的迟到结果被丢弃而不是二次报错；
  重复序号、落后于提交点的序号、超出 queue/in-flight permit 以及 EOF 时残留的
  序号缺口都转为结构化 error。commit head 从 `accept` 直接返回、不占 reorder
  slot，因此满队列仍能接收并提交唯一可以解除阻塞的早期结果；
- 串行路径本身走这条提交层，因此顺序契约由生产覆盖而非只由并行代码覆盖；
  `open_test_source` 固定在 serial reservation 上，作为后续 route-specific
  worker 的 differential oracle。`fault::completion_orders` 提供确定性乱序注入，
  不依赖 wall-clock 竞争。

验证：仓库契约、fmt、严格 Clippy、workspace all-target tests（197 项，含新增
20 项）、release CLI build、两套 Python 测试与 Tauri frontend build 全部通过。
另以 136 个 fixture 逐个运行 release CLI `analyze --format json`，改动前后输出
逐字节相同（SHA-256 `2cba423b44bf6a96dea548d4e88fc486eb268974c6c27649cdf2985fba238e29`）。
本步不建立任何性能声明，也不启用任何并行轴。

## 明确非目标

- 接受 ADR 即宣称当前 0.3.0 已经并行，或一次提交同时打开三个轴；
- 恢复 0.1.x generic parallel decoder、`f32` PCM、坏包跳过或错误回退成功；
- 公共 `--threads`、batch size、queue size、profile 或兼容模式；
- 并行 container probe/demux、支持未毕业的 codec，或从扩展名猜测并行安全；
- 禁用 FLAC checksum、弱化 sticky terminal error、允许 partial report；
- SIMD、unsafe、第二 backend、外部解码进程或另一套 analyzer；
- 修改固定分析算法、数值参数、报告字段、wire schema、发行边界或平台矩阵；
- 把 elapsed time 或 RSS 变成普通 test/CI 的跨主机阈值。

## 后果

正面：项目不再被阶段性的“全串行”约束锁死，优化直接指向最明显的单文件压缩解码
瓶颈；packet、文件和窗口三个轴共享同一套确定性、错误、取消与资源规则，后续不会
靠多个互不知情的线程池叠加吞吐。

代价：packet 并行同时放大 decoder state、完整性、错误优先级、重排序、取消和内存
风险；尤其 FLAC 的流级 MD5 使“frame 可独立解码”不足以直接推出生产安全。实现与
测试成本明显高于简单流水线，且最终收益仍须由正式长音频 A/B 决定。

### 第 2 步（2026-08-02，实现与正确性部分完成；2026-08-03 加固；未启用）

ALAC packet workers 已按 §2 的顺序提交拓扑实现，但生产 plan 仍恒为 serial，
因此该路径目前只由测试的显式 reservation 驱动：

- `codecs` 新增 `decode_engine`，把解码语义收敛为单一 `decode_packet`：串行
  oracle 与 ALAC workers 共用同一份几何校验、错误分类与 `f64` 转换，两条路径
  不可能在这些语义上漂移；调度差异之外的一切保持一致；
- 拓扑为「顺序 demux + decoder slot 0 → 其余每 worker 独立 inbox → 有界 result
  通道 → 主线程按输入序提交」。派发是 `index % workers` 的固定函数，与哪个
  worker 恰好空闲无关，因此同一输入的 packet 到 worker 的映射完全可复现；demux
  线程同时承担 slot 0 的解码，N-worker reservation 恰好创建 N 条内部线程，而不是
  在 N 个 decoder worker 之外再建立一条未计量 coordinator；
- worker inbox 深度上限为 2，并按 reservation 向下收缩；最小合法的
  `queue_capacity == workers` 使用零容量 rendezvous，绝不因 lower layer 的额外
  假设 panic 或扩大许可。application 派生的 `workers × 4` 仍使用深度 2，result
  通道容量仍为 worker 数；
- worker 只在 `alac_info.is_some() && workers > 1` 时创建，绝不从扩展名或泛型
  codec descriptor 推导；worker 数为 1 时在解码开始前退化为串行；
- 每个 decoder 在调用线程预先创建，创建失败使 open 失败而非某个 worker 失败；
- ADR §4 的完整性约束在此路径上被主动执行：worker 只见到自身子集，所以其
  `finalize()` 不能代替流级签名。ALAC route 以“decoder 不报告任何流级 verdict”
  为前提毕业；一旦某个 worker 返回了 `verify_ok`，并行路径直接失败，而不是
  接受一次 per-subset 校验；
- 所有线程由 pool 拥有，`Drop` 先释放 receiver 唤醒 worker、再 join demux 与
  全部 worker；worker 或 demux panic 转为结构化 terminal error。若第 N 个 worker
  或 demux 本身创建失败，构造期清理路径会先断开 channel、join 已启动线程，再返回
  结构化 `resource_exhausted/decode`，不会因 `JoinHandle` drop 留下 detached thread。

正确性证据（9 个 ALAC fixture × 1/2/4/8 worker）：

- 解码 raw `f64` bits 与 `SourceInfo` 与串行 oracle 逐位一致；
- 强制乱序下同样逐位一致。实例级测试 seam 在 engine 边界持有 packet 0，直到一个
  较晚结果已经发布，并断言 reorder 确实发生滞留；它不使用 process-global 开关、
  sleep 或调度时序，因此默认并行 test harness 下也确定；
- `alac-corrupt-first-packet` 在自然与强制乱序、1/2/4/8 worker 下产生完全相同的
  错误码、阶段、消息与 sticky 重放，且 progress 保持 0 帧、不转为 EOF；
- 多 packet fixture 在 2/4/8 worker 下分别覆盖最小 `queue_capacity == workers` 与
  固定产品最大 `queue_capacity == 64`，两端均与串行 raw bits、metadata 完全一致；
- WAV/float64/FLAC/AIFF 在多 worker reservation 下证明零个 worker pool 被创建，
  PCM 仍逐位一致；
- progress 单调、EOF 只在精确帧数提交后出现；提前 drop 会 join 全部线程；
- route 选择与 queue-bound 等价矩阵以 thread-local pool 计数器确认用例确实运行在
  workers 上；把 route 判定改成恒 false 会使这些计数断言失败，而不会静默串行。

验证：仓库契约、fmt、严格 Clippy、workspace all-target tests（206 项，含新增
9 项）、release CLI build、两套 Python 测试与 Tauri frontend build 全部通过；
另在默认并行 test harness 下重复运行 codecs 100 次，全部通过；
136 个 fixture 的 release CLI 输出仍为 SHA-256
`2cba423b44bf6a96dea548d4e88fc486eb268974c6c27649cdf2985fba238e29`，与串行基线
逐字节相同。

### 第 2 步的长音频扫描（2026-08-03）

M6 语料新增两条几何相同、可压缩性相反的 240 秒 stereo 16-bit ALAC track
（各 2813 packets）：既有伪随机信号压缩率 99.5%，会让 ALAC 退回 uncompressed
escape 路径；新增的整数三角波加 dither 信号压缩率 60.0%，落在无损音乐常见区间，
迫使 codec 真正执行预测与 rice 解码。二者均由 ffmpeg 8.0.1 以 ADR-0013 固定的
同一 bit-exact 形状编码，连续生成逐字节一致，tonal 信号为纯整数构造。

worker 数是 decode allocation 而非另一个 binary，因此作为 case 参数与其余 15 个
case 在同一次 run 内固定 seed 完全交错（161 samples）；每个 case 各自复现 corpus
的 normalized `f64` oracle，verification 解码运行在与计时段相同的 allocation 上。
runner 复算 harness allocation 并核对 worker 实得值，可捕获两份镜像之间的错位；
但两者都只是 crate-private plan 的镜像，不自动检测 plan 单独改变或另一台主机的
`available_parallelism` 收缩。

application plan 对每个 worker 数只派生一个队列容量，因此 reorder permit 作为独立
维度在 8 worker 上另行扫描：最小合法容量等于 worker 数、把每个 inbox 压到零容量
rendezvous，最大值为固定产品上限；只有队列上限变化，in-flight PCM permit 仍取
plan 派生值。

clean source `c1b25ea`、Apple M4 Pro / 12 逻辑核下的中位数：

| Track | 1 worker | 2 | 4 | 8 |
| --- | ---: | ---: | ---: | ---: |
| 伪随机 99.5% | 397.8 ms | 1.93x | 3.61x | 6.08x |
| tonal 60.0% | 354.8 ms | 1.96x | 3.65x | 5.79x |

| Reorder permit（tonal，8 worker） | 中位数 | 相对 plan 派生 | median peak RSS |
| --- | ---: | ---: | ---: |
| 8（最小，rendezvous） | 75.2 ms | +22.7% | 5.7 MiB |
| 32（plan 派生） | 61.3 ms | — | 6.6 MiB |
| 64（产品上限） | 57.4 ms | −6.4% | 6.5 MiB |

两条 track 的加速比在每个 worker 数上相差不超过 0.29x，因此该结论不依赖语料恰好
落在 escape 路径。最小 permit 更慢且 RSS 更低，从 32 放宽到 64 只再取得 6.4%，
说明 plan 的派生值已接近该维度的收益拐点。每条 track 内所有 worker 数与所有
permit 共享同一 result fingerprint；peak RSS 中位数 3.0 → 6.6 MiB。完整身份、
span、环境与限制见
[`ADR0014_ALAC_PACKET_WORKER_AB_REPORT.md`](../performance/ADR0014_ALAC_PACKET_WORKER_AB_REPORT.md)。

最小 permit 在 240 秒、2813 packet 的流上完成且未触发任何 permit 耗尽，peak RSS
反而低于更宽的 permit，因此 reorder 内存不随媒体时长增长。但确定性强制乱序 seam
是 `#[cfg(test)]`、不在 release worker 中，所以“长流”与“强制最坏乱序”只各自被
覆盖，二者的组合仍只有短 fixture 证据。

该扫描是一次测量，不是启用决定。默认启用仍缺 39 项 safe-master 逐 token 对照、
长流与强制最坏乱序的组合覆盖，以及经 `Application` 的实际启用路径；因此 ALAC
packet workers 目前仍不得默认启用。

## 待补证据

本 ADR 已接受架构方向、ALAC packet-worker 实现和上述固定身份下的性能测量，但
尚未接受默认生产启用；剩余证据如下：

- ALAC 的长音频 source-bound corpus 与 1/2/4/8 worker 同轮扫描已完成；仍缺同一
  corpus 在最小/默认/最大队列下的 decoded-f64、`AnalysisResult` raw bits 与
  wire-visible report 全矩阵、队列容量性能 A/B、39 项 safe-master 逐 token 对照、
  真实音乐素材代表性，以及经 `Application` 的实际启用路径；
- ALAC packet 独立性已由当前产品 route 的 raw-bit、错误、强制乱序与最小/最大队列
  测试在 committed fixture 上证明；但这些 fixture 都很短，独立性在长音频上仍未
  验证；
- FLAC 的 ordered full-stream MD5 设计尚未形成；
- 文件级与窗口级仍只有准入契约，没有生产实现。

第 1 步已经明确并测试了 application 共用 worker/memory hard cap、reservation 数值
与退化规则；这些上限至今只在 serial 配置下被生产使用。多 worker 取值已有 committed
短 fixture 与 source-bound 长音频正式性能记录，但仍未完成上述正确性、资源和
`Application` 集成门槛。
