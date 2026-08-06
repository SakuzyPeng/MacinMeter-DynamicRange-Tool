# ADR-0014：确定性有界并行与 packet 解码优先

- 状态：Accepted
- 实施状态：In progress（packet 级已为 ALAC 与资源几何落在 permit 内的 FLAC
  route 默认启用；文件级已有非默认测量实现但产品仍固定请求一个 lane；窗口级
  未实施；decode-analysis overlap 已有 `performance-probes` 非默认候选，尚未毕业）
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
| P0 | packet 级解码 | 单个 FLAC/ALAC 文件的解码延迟 | 已实施：ALAC 先毕业，FLAC 在解决全流 MD5 后毕业 |
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
- worker 数为 1、输入不足或最坏重排内存无法落在 reservation 内时，可以在开始
  解码前退化为串行；生产路径不得在并行错误后重跑串行并把失败隐藏成成功。

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

上述“产品级全流 verifier”已于 2026-08-03 实现并接管全部 FLAC route（见实施进度
“第 3 步之一”），FLAC packet workers 随后在同日毕业（“第 3 步之二”）。保住全流
签名的代价已被量化：它落在顺序侧，是 FLAC 顺序底线中较大的一半。

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
  前一次性完成；首个 caller 自有 lane 之外的 lane executor 先从总量扣除，剩余
  permit 才分给各 lane 的 packet pool，从而保证实际 lane thread 与 packet worker
  总数不超过 `total_workers`。`bounded()` 同时受产品上限和
  `available_parallelism()` 约束；
- `ApplicationJob` 持有该 allocation 并向 `codecs` 下发；`DecoderFactory` 只在
  收到的 permit 内解码，自身不创建 worker。该步生产 plan 仍恒为 serial、
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

### 第 2 步（2026-08-02，实现与正确性部分完成；2026-08-03 加固；未启用）

ALAC packet workers 已按 §2 的顺序提交拓扑实现，但该步生产 plan 仍恒为 serial，
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

M6 语料新增三条几何相同、各 2813 packet 的 240 秒 stereo 16-bit ALAC track：既有
伪随机信号压缩率 99.5%，会让 ALAC 退回 uncompressed escape 路径；整数三角波加
dither 的 tonal 信号压缩率 60.0%，落在无损音乐常见区间；varied 信号循环 8 个从
近静音到满幅噪声的难度变体，变体数与最大 worker 数相同，因此固定的
`index % workers` 派发会让每个 worker 在整条流上只拿同一种难度——这是该派发方式
能遇到的最坏不均衡。三者均为纯整数构造，由 ffmpeg 8.0.1 以 ADR-0013 固定的同一
bit-exact 形状编码，连续生成逐字节一致。

worker 数与 reorder permit 是 decode allocation 的维度而非另一个 binary，因此作为
case 参数与其余 15 个 case 在同一次 run 内固定 seed 完全交错（203 samples）；每个
case 各自复现 corpus 的 normalized `f64` oracle，verification 解码运行在与计时段
相同的 allocation 上。runner 复算 harness allocation 并核对 worker 实得值，可捕获
两份镜像之间的错位；但两者都只是 crate-private plan 的镜像。

clean source `3ef8ae3`、Apple M4 Pro / 12 逻辑核下的中位数：

| Workers | 伪随机 99.5% | tonal 60.0%（均衡） | varied 74.4%（最坏不均） |
| ---: | ---: | ---: | ---: |
| 1 | 395.1 ms | 350.5 ms | 380.5 ms |
| 2 | 1.93x | 1.93x | 1.88x |
| 4 | 3.30x | 3.63x | 3.68x |
| 8 | 5.95x | 5.93x | 6.26x |

最坏负载不均**没有**降低加速比。对每个变体单独测得的串行解码时间给出原因：压缩后
大小相差约 517 倍，解码时间只相差 3.7 倍，排除近静音变体后仅相差 1.43 倍——ALAC
解码成本主要由每 packet 固定 4096 frame 的输出几何决定，而非压缩数据量。据此由
最慢变体反推的 `8 × 89.2 / 113.7 = 6.28x` 与实测 6.26x 一致。该结论 route-specific，
依赖 ALAC 输出几何固定这一性质，不能外推到 FLAC 或其他 codec。

tonal track 的 8-worker reorder permit 另行扫描：最小合法容量（等于 worker 数、
把每个 inbox 压到零容量 rendezvous）77.7 ms，plan 派生的 32 为 59.1 ms，产品上限
64 为 60.2 ms，说明 plan 派生值已在该维度收益拐点之后。peak RSS 中位数由
2.9–3.0 MiB 升至 6.4–7.2 MiB。完整身份、span、环境与限制见
[`ADR0014_ALAC_PACKET_WORKER_AB_REPORT.md`](../performance/ADR0014_ALAC_PACKET_WORKER_AB_REPORT.md)。

ADR 共同门槛要求的正确性全矩阵由独立的、不计时的 harness
（`examples/adr0014_allocation_matrix.rs`）在三条 track 上各运行 12 个单元：
worker 数 1/2/4/8，各配最小合法容量、plan 派生容量与固定产品上限 64。每条 track 的
decoded `f64`、`AnalysisResult` raw bits 与 wire-visible report 三项指纹各自唯一。
schema v2 拒绝非 ALAC 输入，并核对 content probe 后实际选择的 engine 与 worker 数，
所以静默串行回退不能使矩阵假通过；wire report 使用 basename 作为规范化 display
path，使同一输入的不同路径写法保持相同指纹。harness 直接输出排序、四空格缩进的
canonical JSON，因此记录可由文档命令逐字节重建。
`AnalysisResult` 指纹遍历其 exhaustive view 并按 IEEE-754 位模式累积，不比较渲染
后的十进制文本。矩阵的检测能力经两次注入验证：破坏顺序提交会被 commit buffer 的
既有契约检查在产生任何 PCM 之前拦下；只在 packet worker 路径翻转单个 sample 的
1 ULP 时，矩阵报告四个互不相同的 decoded-`f64` 指纹。

reorder 的滞留界另由直接针对 commit buffer 的压力测试固定：在最紧 permit 与最深
完成顺序下，1,000 与 100,000 个 packet 的滞留高水位完全相同；模拟 in-flight
permit 泄漏会使长流在 index 12289 处失败而短流仍通过，因此该结论由长度对比承载，
而非单一长度的观察。

非串行 plan 经 `Application` 的真实路径也已验证：`ConcurrencyPlan` 的生产派生在
固定宿主上限下构造 budget，通过 `Application::analyze_file` 分析 ALAC、WAV、FLAC
与 AIFF fixture，wire report 与产品 serial plan 逐字节相同；同一测试逐次核对实际
选择的 engine 与 worker 数，只有已毕业的 ALAC route 才切换到 packet workers，
其余 route 保持串行且恒为一个 worker。把 route 判定改成恒 false 会使该断言失败。
`ExecutionBudget` 的非串行构造仍是 `#[cfg(test)]`，公开 API 无法构造。

真实录音的代表性由一次本机交叉检查补充：个人音频库中 314 个 ALAC 里产品接受
309 个（98.4%，22.7 小时，43–760 秒，44.1 kHz 立体声）。按时长等间隔抽取的 40 个
文件上运行完整 allocation 矩阵，合计 480 次独立解码与分析、474,975,816 帧，每个
文件的 12 个单元三项指纹各自唯一，40 个文件给出 40 个不同指纹。同 run 交错的
指示性计时显示真实录音的 8-worker 加速比（4.44–6.22x）与合成 control（同 run
5.06x）同量级。该语料是私人的、不可再生的，按 ADR-0007 立场不进入仓库，因此它是
补充观察而非可复现证据。

同一次检查还记录了两类与 packet workers 无关的 ADR-0013 边界：三个 96 kHz 文件
把 16.16 sample entry 速率写为定点 `1.0` 而非当时唯一登记的零 sentinel；两个文件
的解码帧数比声明少恰好一个 4096-frame packet。当时二者均按既有契约拒绝。前者已由
[ADR-0013 的 2026-08-03 能力修订](0013-mp4-m4a-alac-stable-route.md)把 sentinel
接受集扩为 `{0, 1}` 后受理；后者经核实是文件损坏，维持 sticky 拒绝。

39 项 safe-master 回归对照已在 clean commit `768670b` 上完成：track DR 39/39、
channel DR 62/62、overall peak 39/39、overall RMS 39/39、channel RMS 62/62、
duration 39/39、footer 可比较子集 4/4，差分数 0，fixture 集合与顺序完全一致。
corpus 由既有 generator 在本机重新生成并逐 case 校验，与提交的 manifest 具有相同的
全部 `dataSha256`/`fileSha256` 与 safe-master 顺序。见
[`CONF-…-macinmeter-030-adr0014-20260803`](../../reference/conformance/conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-030-adr0014-20260803/record.md)。
这 39 项输入全部是 WAV，不经过 ALAC route，因此该对照证明的是 ADR-0013/0014 的
改动没有波及既有 PCM 路径的数值，而不是 packet workers 本身与 reference 一致。

该扫描是一次测量，不是启用决定。共同门槛的证据现已齐备，但默认启用本身是一个
独立决定，尚未作出；在作出前 ALAC packet workers 仍不得默认启用。

### 第 3 步之一：产品自有的有序全流 FLAC 校验（2026-08-03）

§4 要求的“按原 packet 序更新的产品级全流 verifier”已落地，但只接管串行路径；
FLAC packet workers 仍未实现，也未启用。

设计基于两条已核对的 Symphonia 0.5.5 事实，而非推断：

- `symphonia-bundle-flac` 的 `validate` 是私有模块，`Validator` 无法复用；
- `FlacDecoder::decode_inner` 在把样本左移到 `i32` 满量程 **之前** 更新自身
  validator，因此调用方拿到的 buffer 已经左移 `32 - bits_per_sample`。

产品因此自行构造签名字节：按 `32 - bits_per_sample` 右移还原，逐帧交织、按声道
补齐到整字节、小端写出。还原对任何落在声明位深内的样本是精确的；落在外面意味着
frame header 与 STREAMINFO 的位深不一致，该流被拒绝，而不是按猜测宽度求哈希。
哈希函数直接用 Symphonia 自己的 `symphonia::core::checksum::Md5`，所以算法是共用
的，仅顺序与字节布局是第一方的。

字节构造是逐 packet 的，可以由 worker 承担；求哈希只能顺序进行，因此固定在唯一的
按序 commit 点。为使串行 oracle 与将来的并行路径不可能得出不同判定，FLAC 的
verification 在**所有** route 上都由产品承担，backend 的 `verify` 对该 route 关闭。
这不是 §4 禁止的“为吞吐关闭 verify”：全流检查没有减少，只是换了归属并变得有序。

证据：

- 单元向量覆盖 8/12/16/20/24/32 位与 1–3 声道，与一份独立写出的布局实现逐字节
  比较；位深越界样本被拒绝；同样两个 packet 交换顺序得到不同 digest；
- 与 backend 自身 `verify: true` 判定的差分：完好 fixture 双方通过，篡改
  STREAMINFO digest 双方失败；
- 尾部丢失场景：把流截到 frame 边界并改写声明样本数，使帧数检查被满足。此时清零
  签名的对照可以干净地读到 EOF，即产品里没有别的检查会发现丢失的音频，只有签名
  会——这正是该检查存在的理由；
- 真实素材：本机 308 个 FLAC（27.9 GiB，24-bit 为主，另有 16-bit；2/6/8 声道；
  44.1k–192k）全量分析，与改动前 `b43e6d1` 的结果逐项比较，308/308 的成败判定与
  完整报告完全一致。其中 45 个被拒绝的文件由 `flac -t` 独立确认确为 MD5 签名不
  匹配，两侧一致拒绝。该语料私人且不可再生，按 ADR-0007 立场不进入仓库。

已测得的代价（非 ADR-0007 正式记录）：在固定的 12 文件 24-bit 子集上交错重复运行
release CLI，本改动 10.0–10.4 s，`b43e6d1` 9.3 s，串行 FLAC 路径慢约 8%。归因实验
（保留字节构造、停用哈希）把成本分成约 2.0 s 的 MD5 与约 0.8 s 的字节构造差额，
后者即第一方构造相对 Symphonia 内建 validator 的实现差距。这组数字只用于判断改动
量级与规划第 2 步，不构成优化或回归结论。

当时据此推算 8 worker 上限约 3.3×。该推算的**方向**成立（FLAC 的顺序侧确实比
ALAC 重得多），但推理过程不成立：它把“顺序哈希占比”当成可从整体耗时比例推得的
量，并预期解码越便宜的轨道越先触顶。第 3 步之二的直接测量给出相反的排序；正确的
量化形态见该节与其正式记录。此处保留原文以记录推算被推翻的经过。

### 第 3 步之二：FLAC packet workers（2026-08-03）

worker pool 原本就与 route 无关，只有名字带 ALAC，因此是泛化而非新写一套：一个
按具名 `ParallelRoute` 参数化的 `PacketWorkerPool`，使毕业仍然是一个显式动作，
而不是从扩展名或 codec descriptor 推导出来的结果。

FLAC 的测试围绕它与 ALAC 唯一的关键差异构建：签名是顺序相关的，因此在强制最坏
乱序下 digest 仍然匹配，就直接证明 verifier 是按 commit 序而非完成序喂入的；篡改
签名时 2/4/8 worker 与串行 oracle 给出逐字节相同的 digest 与错误。

corpus 与 suite 新增三条 240 秒 FLAC track。位深决定顺序哈希覆盖多少字节，可压缩性
决定它与多少解码工作竞争，因此用一组 24-bit 对照加一条 16-bit track 把两者分开，
而不是报告单一数字。

正式记录见
[`ADR0014_FLAC_PACKET_WORKER_AB_REPORT.md`](../performance/ADR0014_FLAC_PACKET_WORKER_AB_REPORT.md)。
其固定主机是 Windows x86_64，因为 macOS 主机在窗口内持续负载 11–15；同 suite 的
ALAC track 充当污染对照，在受污染的 run 中掉了约 20%，该 run 已整体作废。

三点结论进入本 ADR：

- FLAC 8 worker 加速比 16-bit 4.13x、24-bit 2.77–3.42x，同轮 ALAC 为 4.07–4.31x；
  每条 track 的 1/2/4/8 worker 共享同一 fingerprint；
- FLAC 的**顺序底线**（顺序 demux 加产品自有的流签名哈希）经独立探针直接测得，
  占其串行解码时间的 22.5–28.9%，ALAC 只有 1.7–2.8%。这是 §4 所预见的代价的量化
  形态：把全流校验保留下来是有价格的，价格在顺序侧；
- 由 1-worker 与 8-worker 比值反解 Amdahl 得到的“串行占比”是**上界**，不是顺序段的
  测量值——它把内存带宽、分配器争抢与通道交接一并计入。本记录中两条哈希工作量
  相同的 24-bit track，反推串行差 55.3 ms 而实测底线只差 14.6 ms。后续任何轴的
  归因都不得使用反推值代替直接测量。

启用时序上有一处与本 ADR 不一致，如实记录：ALAC 的默认启用是门槛齐备后单独作出的
决定，FLAC 则因产品预算此前已非串行，在被加入 route 判定的那一刻即成为默认，正式
A/B 在其后才产生。事后证据支持该默认，因此不回退，但顺序不是本 ADR 所设的顺序。

2026-08-04 的资源边界复核发现，最初的 route 判定没有把 FLAC 可变 block 几何纳入
启动条件。合法的 8 声道 / 24-bit / 65,535-frame block 在 plan 派生的 4 MiB/worker
permit 下，单个待提交 packet 会同时持有 4 MiB `f64` PCM 与约 1.5 MiB 签名字节；
自然调度可在积累若干较晚 packet 后触发 `ResourceExhausted`，从而使同一文件的成败
依赖完成顺序。这违反了本 ADR 的结果不随调度变化契约。

现有实现因此在创建 pool **之前**，从已解析 STREAMINFO 取最大 block size，并按
`max_block × channels × (8-byte f64 + signature width) × queue_capacity` 计算完整最坏
重排窗口。只有该值可表示且不超过 reservation 的 in-flight bytes 时，FLAC 才进入
packet workers；否则直接使用既有串行 oracle。该检查只向下收缩并发，不从媒体声明
扩大 application plan，也不是在并行失败后的重跑。单元回归固定 8 声道 / 24-bit /
65,535-frame 的拒绝和常规 4096-frame 的准入；原复现文件在 4/8-worker-shaped
reservation 下各连续 20 次成功，PCM 与结果指纹均与串行 oracle 相同。

同次复核还把顺序底线探针的签名几何收紧为“FLAC 且实际声明
`VerificationCheck::Md5`”：未签名 FLAC 与没有流签名的 ALAC 都不再虚构一次空缓冲
哈希。历史原始记录的 500 ns 空哈希字节与 SHA-256 保持不变，只在正式报告中纠正其
实际对应的 `aeb7022` 实现和派生解释。

### 第 3 步之三：commit/analysis overlap 的专用 FLAC hasher（2026-08-04，已接受）

第 3 步之二把签名哈希留在调用线程的 commit 点，因此 `read_block()` 只有完成该包的
MD5 后才把 PCM 交给 analyzer；哈希与分析严格串行。新的 source-bound 候选不改
verifier、字节布局、输入顺序或最终判定，只改调度：唯一 commit sender 按输入序把
已有的签名字节 `Vec` 移交给一条 source-owned hasher 线程，队列容量固定为 1，EOF
关闭 sender、join，再由同一个 `FlacStreamVerifier` 比较一次最终 digest。

它不在 decoder pool 之外新增未计量线程。对声明签名且取得已实测总 permit `N == 8`
的 FLAC，一次性拆成 7 个 decoder permit 与 1 个 hasher permit；`N == 2/4` 的实测
direct-decode 代价不能支持向下外推，因此这些较小 allocation 保留全部 `N` 个 decoder
与 inline verifier，`N == 1` 继续使用串行 oracle。未声明签名的 FLAC 不创建 hasher，
全部 `N` 个 permit 仍用于 decoder。隐藏的 `DecodeExecution` correctness surface 分别
报告 total、decoder 和 hasher 数，application 测试固定 8 == 7 + 1 且 2/4 == 2/4 + 0，
因此不能把额外线程静默藏在旧的 worker 字段后面。

8-permit route 的内存也从同一个 reservation 向下切分。启动前按 STREAMINFO 最大
block 几何为 hasher 的“一个处理中 + 一个已排队”签名包预留字节，再把剩余 in-flight
bytes 交给完整 reorder window；两者任一不可表示或总和超出 permit 都在启动线程前
退化为串行。容量为 1 时，commit sender 当前持有的包是 inline 与 async 两条路径
共有的 head payload，异步调度只额外保留上述两个包。

候选的错误与终止面已经固定：hasher spawn failure 为结构化资源错误；panic/channel
disconnect 为 sticky internal error；packet-pool 构造在 hasher 启动后失败时两侧线程
全部 join；提前 drop/应用取消关闭队列并 join，但不对不完整流请求 digest verdict；
正常 EOF 仍保持“reorder 完整 → decoder pool verdict → 全流 MD5 → 声明帧数”的既有
错误优先级。单元覆盖 inline/async digest 等价、2/4/8 总 permit raw-bit 与报告等价、
强制乱序、篡改签名、spawn/panic、构造回滚、提前 drop、unsigned 对照与紧内存边界。

是否保留该候选由正式 A/B 决定，而不是从顺序底线推算。baseline suite 已加入三条
240 秒 FLAC 的完整 application 案例；同轮仍跑直接 decode worker sweep，以显式观察
少一个 decoder 的代价，并保留三条 ALAC worker sweep 作为宿主污染对照。首次广义
候选 run 在 8 permit Application 上快 15.3–36.6%，但 direct decode 在 2 permit 慢
46–51%、4 permit 慢 13–31%，据此把生产选择收紧到唯一具有端到端正收益证据的
8-permit allocation。固定 Windows 主机上的最终 gated run 保持总 permit 相同；四条
Application case 快 15.0–39.4%，2/4 permit direct decode 回到 -1.46% 至 +2.00% 的
同路径波动范围，8 permit direct decode 仍有 1.7–10.6% 的 decoder 交换代价。两轮
ALAC 污染对照均接近已知干净值，392 个最终样本的跨变体 fingerprint 全部一致，因而
接受上述仅限 8 permit 的生产选择。正式记录见
[`ADR0014_FLAC_HASHER_AB_REPORT.md`](../performance/ADR0014_FLAC_HASHER_AB_REPORT.md)。

### 第 3 步之四：顺序底线之上的 pipeline 归因（2026-08-04，已完成）

第 3 步之二的正式记录只直接测量了顺序 demux 与 FLAC hash；它明确留下一个开放项：
ALAC 的 2.8% 实测底线换算为 6.67x Amdahl 上限，而同轮实际只有 4.07x，底线之上的
限制尚未测量。该项现由非默认 `performance-probes` feature 和 `decode-pipeline`
runner mode 完成 source-owned 归因。默认 production build 不含这些计时点。

固定 Windows 主机上的 48-case 内部记录同时保留普通 decode 对照，并测量 open、
demux/dispatch、每个 decoder slot 的 backend/integrity/PCM、result hand-off、调用线程
wait/commit、reorder 高水位与 FLAC hasher。深探针相对普通 decode 的中位数差为
-3.54% 到 +3.06%，没有系统性 probe penalty；完整 PCM SHA-256 在观察区外计算，
runner 对 topology、packet 数、permit 与 oracle fail closed。

归因得到四项：

- 旧底线漏掉 ALAC 26–28 ms 的 ISO BMFF 打开检查；到 w8 它已占完整 elapsed 的
  13.5–15.6%；
- 相同 packet 集合的 decoder aggregate active work 在 w8 膨胀 1.34–1.57x；其中
  backend 为 1.14–1.42x，PCM conversion 为 1.59–2.07x；
- varied ALAC 的最慢 slot 比平均多约 29 ms / 1.26x，demux dispatch wait 为
  65.3 ms，另外两条 ALAC 只有 5.4–6.1 ms；
- caller result wait、ordered commit 与 FLAC hasher hand-off 都有具名 owner 和直接
  计时；这些 interval 会与 worker 重叠，不再被误写成可相加的“串行比例”。

检查第一方实现后接受两项不改变拓扑的优化。`stsz` 从每 entry 一次 4-byte seek/read
改成最大 64 KiB 的顺序 chunk，使 container inspection 降到 0.79–0.81 ms，三条 ALAC
w8 端到端快 11.6–13.3%。PCM conversion 原先先填一个 `SampleBuffer<f64>`，再
`to_vec()` 复制成 domain buffer；现直接用相同 `IntoSample<f64>` 与交错顺序填最终
`Vec<f64>`。十种 backend sample format 与旧路径逐 sample 位模式一致，另有 source
f64 不收窄测试。

第二项的 504-sample broad A/B 中，PCM phase 下降 35–62%；FLAC 的 12 个普通 case
快 8.2–19.7%，ALAC w1 快 7.8–8.8%。宽轮 varied w4/w8 的两个
小负值小于 MAD，独立 21-sample 确认给出 6.0% / 1.7% 正收益。36/36 case 跨变体
fingerprint 相同，median process-tree peak RSS 变化范围为 -4.47% 到 +0.71%。同轮
ALAC 1→8 污染对照为 4.35–4.43x，没有低于既有干净值。

正式解释、全部 phase 表与五份原始记录见
[`ADR0014_PACKET_PIPELINE_ATTRIBUTION_REPORT.md`](../performance/ADR0014_PACKET_PIPELINE_ATTRIBUTION_REPORT.md)。
以前的“没有被测量，也没有被归因”据此关闭；backend 的微架构膨胀、varied 静态映射
不均与 hand-off 是**已经测量但尚未继续优化**的边界，不是仍然未知的顺序底线。

### decode-analysis overlap 候选（2026-08-06，非默认）

容量 1 的顺序 channel 候选已在 `Application` owning layer 实现：decode 保留调用
线程，单一 analysis thread 按块序推进同一个 `AnalyzerSession`。它只在非默认
`performance-probes` feature 中从 route 未使用的 worker permit 取得预算；普通
library、CLI 与 GUI build 的 overlap budget 恒为零，不改变产品默认路径。

准入不再用首块外推全流，而是消费 decoder 在 probe 时根据 route 元数据证明的单块
`f64` PCM 上界；无法证明上界时串行退化。channel 以显式 `Finish` 区分真正 EOF 与
decode failure/cancellation 的断开，后两者不 finalize 部分 analysis prefix；线程创建
失败、worker panic 与无终态断开均转为结构化错误并在返回前 join。确定性差分、运行中
取消、decode/finish 错误优先级、零帧 EOF 取消、可变块预算与 spawn failure 现有直接
测试。非默认 application worker sweep 现在把 requested/granted plan、实际 decode
engine/worker 细分、overlap 选择及末块几何写入原始样本并逐项校验；宿主无法完整授予
1/2/4/8 worker 时拒绝把较窄执行误记为较宽 case。WAV/AIFF 长语料各含 10,000 个完整
1,152-frame 解码块及一个真实的一帧末块。不过 ADR-0007 source-bound A/B、RSS 与完整
route/corpus 门禁仍未执行，因此该候选不得进入默认 production build。

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

## 证据状态

本 ADR 已接受架构方向、ALAC 与 FLAC 两条 packet-worker route 的实现、下述固定
身份下的正确性与性能测量，以及在此基础上的默认启用。ALAC 的启用是门槛齐备后
单独作出的决定；FLAC 的启用时序与之不同，见“第 3 步之二”。

`ExecutionBudget::product()` 现在把 plan 取为固定上限与宿主并行度的较小值，
`Default` 使用它，因此 CLI 与 Tauri 的 `Application::new()` 默认启用 packet
workers。只有已毕业且资源几何可证明落在 permit 内的 route 会切换 engine——目前
是 ADR-0013 的 ALAC route，以及最坏重排窗口适配 reservation 的 FLAC；其余 route、
超出 permit 的 FLAC 与单 worker 宿主仍走串行引擎。`ExecutionBudget::serial()` 保持
完全串行，作为差分参照继续可达，不是产品默认的别名。文件级（P1）已有仅供
`performance-probes` 测量的实现，但产品仍固定请求一个 lane，尚未毕业或默认启用；
窗口级（P2）仍未实施。decode-analysis overlap 同样只存在于非默认
`performance-probes` build，尚未完成 ADR-0007 裁决，普通产品 build 保持串行。

启用后 136 个 fixture 的 release CLI 输出与启用前逐字节相同（SHA-256
`2cba423b44bf6a96dea548d4e88fc486eb268974c6c27649cdf2985fba238e29`），39 项
safe-master 的 WireEnvelope 也与已登记 conformance artifact 逐字节相同。

### packet 级（FLAC）共同门槛：已具备

- 产品自有的按序全流签名 verifier 接管全部 FLAC route，单元向量覆盖 8/12/16/20/
  24/32 位与 1–3 声道，位深越界样本被拒绝，与 backend 自身判定的差分在完好与
  篡改两侧一致；尾部丢失场景由“清零签名的对照可以干净读到 EOF”反证只有签名会
  发现；
- 真实素材 308 个 FLAC（27.9 GiB）在改动前后成败判定与完整报告 308/308 一致，
  45 个拒绝项由 `flac -t` 独立确认；
- 长音频 source-bound corpus 三条（24-bit 90.2% / 24-bit 59.5% / 16-bit 97.5%），
  1/2/4/8 worker 同轮交错扫描，每条 track 四个 worker 数共享同一 fingerprint；
- 16-bit FLAC track 与由同一信号编码的 ALAC track 共享 `resultFingerprintSha256`，
  构成两条独立 route 之间的交叉核对；
- 顺序底线由独立探针直接测量，而非从加速比反推；同轮 ALAC track 充当污染对照，
  据此作废了一次受负载污染的 run；
- 强制最坏乱序下签名仍然通过（签名顺序相关，因此这直接证明 verifier 按 commit
  序喂入），篡改签名时 2/4/8 worker 与串行 oracle 给出逐字节相同的 digest 与错误。
- FLAC route 在 pool 创建前把最大 block、声道、`f64` PCM、可选签名字节与完整
  reorder queue 一次性纳入 permit；常规多声道几何继续并行，超界或不可表示几何
  确定性串行退化，不再由完成顺序决定是否在运行期耗尽 permit。
- 声明签名且总 allocation 为 8 permit 时，route 从同一 plan 拆出 1 个有界 hasher
  permit，把 MD5 与分析重叠；2/4 permit 的广义候选因直接 decode 明确回退而被拒绝，
  最终 source-bound A/B 在完整 Application 上快 15.0–39.4%，且不改变任何结果或
  PCM fingerprint。

正式记录见
[`ADR0014_FLAC_PACKET_WORKER_AB_REPORT.md`](../performance/ADR0014_FLAC_PACKET_WORKER_AB_REPORT.md)
与
[`ADR0014_FLAC_HASHER_AB_REPORT.md`](../performance/ADR0014_FLAC_HASHER_AB_REPORT.md)。

### packet 级（ALAC）共同门槛：已具备

- 长音频 source-bound corpus 三条（压缩率 99.5% / 60.0% / 74.4%，后者为静态派发
  的最坏负载不均），1/2/4/8 worker 同轮交错扫描，加速比在三条上一致，且不均衡
  代价由变体级解码成本测量解释并预测；
- tonal track 8-worker 的最小 / plan 派生 / 最大 reorder permit 性能 A/B；
- 三条 track 各 12 单元的 allocation 全矩阵（worker 数 × 三种 permit），decoded
  `f64`、`AnalysisResult` raw bits 与 wire-visible report 三项指纹各自唯一；矩阵
  拒绝非 ALAC 输入并核对实际选择的 engine 与 worker 数，且其检测能力经两次注入
  验证；
- reorder 滞留界由 1,000 与 100,000 packet 在最紧 permit 与最深完成顺序下的高
  水位对比固定，模拟 permit 泄漏只在长流上失败；
- 非串行 plan 经 `Application` 真实路径的 wire 等价，以及 engine 与 worker 数
  选择，由固定 8-worker 宿主上限的单元测试覆盖，不依赖测试机核数；
- 39 项 safe-master 回归对照在 clean commit `768670b` 上七类字段全部精确匹配、
  差分数 0，见
  [`CONF-…-macinmeter-030-adr0014-20260803`](../../reference/conformance/conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-030-adr0014-20260803/record.md)；
- 真实录音代表性由一次本机交叉检查补充：40 个真实 ALAC 上 480 次 allocation、
  4.75 亿帧，三项指纹逐文件唯一。

### packet pipeline 底线之上归因：已具备

- source-owned probe 已分别测量 open、demux/dispatch、decoder backend/integrity/
  PCM、caller/reorder 与 FLAC hasher；深探针有同轮普通 decode 对照并在观察区外核对
  完整 PCM oracle；
- ALAC 打开检查与重复 PCM allocation 两项已优化并通过 source-bound A/B；backend
  aggregate inflation、varied 静态映射不均、queue/hasher hand-off 已被直接定位而
  未被误算成顺序比例；
- 正式记录见
  [`ADR0014_PACKET_PIPELINE_ATTRIBUTION_REPORT.md`](../performance/ADR0014_PACKET_PIPELINE_ATTRIBUTION_REPORT.md)。

### 仍然成立的限制

- 该 39 项 corpus 全为合成 WAV（32 个 float32、3 个 float64，以及 u8/s16/s24/s32
  整数 PCM 各 1 个），不经过 ALAC route，因此它是既有 PCM 路径的回归基线，不是
  packet worker 与 reference 的对照；
- 真实录音交叉检查的语料是私人且不可再生的，按 ADR-0007 立场不进入仓库，因此它是
  补充观察而非可复现证据；
- 性能 harness 与 runner 各自镜像 crate-private 的 plan 派生。两者互相核对可以
  发现镜像间错位，但不自动检测 plan 单独改变或另一台宿主的
  `available_parallelism` 收缩；
- 确定性强制乱序 seam 是 `#[cfg(test)]`、不在 release worker 中，因此真实解码路径
  上的“长流 + 强制最坏乱序”组合仍未直接运行；该组合的界由 commit buffer 的直接
  压力测试承担；
- 三条 corpus track 都是合成信号，未覆盖真实录音的立体声相关性；该维度只由上述
  不可提交的交叉检查补充。

### 已归因、尚未继续优化

- decoder backend aggregate work 在 w8 比 w1 高 1.14–1.42x。成本已经定位到 backend
  owner，但现有 wall-time probe 不区分 CPU frequency、cache 与内部 memory traffic；
- varied ALAC 的固定 packet-to-slot mapping 产生约 29 ms 最慢-slot penalty 与
  65.3 ms dispatch wait。动态领取会改变确定性、失败和资源面，须作为独立候选毕业；
- caller/reorder 与容量 1 FLAC hasher hand-off 仍有重叠等待；后续只能用 owner 内直接
  测量选择候选，不能从 Amdahl 比值反推成单一“串行占比”。

### 尚未开始

- 文件级与窗口级并行的生产实现，二者目前只有准入契约。
