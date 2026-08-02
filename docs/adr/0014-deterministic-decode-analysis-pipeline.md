# ADR-0014：确定性解码-分析流水线

- 状态：Proposed
- 实施状态：Not started
- 日期：2026-08-02
- 决策范围：单文件分析路径内部的解码/分析线程分离；不改固定算法、不改 wire
  schema、不新增 route、不改发行边界
- 前置决策：
  - [ADR-0001：以 0.2.0 重建可信主干](0001-m0-0.2.0-trusted-trunk-rebuild.md)
  - [ADR-0004：M3 应用执行预算](0004-m3-application-execution-budget.md)
  - [ADR-0005：M4 固定 x64 数值声明与 decoder-independent 验收](0005-m4-bounded-x64-numeric-claim.md)
  - [ADR-0007：M6 可复现性能基线](0007-m6-reproducible-performance-baseline.md)

## 背景

### 参考实现确实并行

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

这解释了一处此前未被解释的观测：既有隔离 core 记录走的是三个入口的**单线程**
路径，而既有 foobar report 走的是**多线程**路径，两者在
[`窄字段对照`](../../reference/conformance/conf-foo-dr-meter-108-x64-isolated-core-safe-master-report-run1-20260719/record.md)
中精确匹配。参考实现的并行是结果不变的。

### 算法结构允许结果不变的并行

[`固定算法规格`](../../reference/specs/foo-dr-meter-1.0.8-candidate-v1.md) 的状态
更新中，浮点累加只发生在窗口内部：`current_sum_squares` 在窗口内逐样本累加、
窗口结束后归零，跨窗口只保留窗口级的 `sum_window_rms2`。因此窗口内顺序不变、
跨窗口按窗口索引序归约即可保持逐位一致；RMS histogram 是整数计数、peak 是
取最大值，合并均无精度损失。

### 瓶颈不在分析

一次本机同机测量（i7-11800H / Windows 11，同一 DAW 母带的三种编码，
222.1 s / 48 kHz / 立体声，扣除 80 ms 进程启动后的中位数）给出：

| 编码 | 端到端 | 其中解码 | 其中分析 |
| --- | --- | --- | --- |
| WAV 16-bit | 182 ms | 约 90 ms | 约 92 ms |
| FLAC 24-bit | 607 ms | 约 515 ms | 约 92 ms |
| ALAC 24-bit | 1052 ms | 约 960 ms | 约 92 ms |

分析时间由本机测得的单线程分析核心吞吐折算；解码时间为差值。**这些数字不满足
ADR-0007**（不是 same-run interleaved A/B，没有 result/PCM fingerprint 绑定，
单机单 CPU），只用于定位瓶颈量级，不构成优化声明，也不进入任何证据体系。

结论是：并行分析层最多只能消除约 92 ms。对 FLAC 是 15%、对 ALAC 是 9%，
对最常见的无损格式收益有限。

### ADR-0001 留下的恢复条件

ADR-0001 把包级并行判定为“尚不满足可信生产契约”，并写明“批处理先以确定性
串行语义为基准，并发只能在同一应用调度器下恢复”。这两个前置条件现已具备：
0.2.0/0.3.0 建立了确定性串行基准并保有逐 token conformance，M3 建立了唯一的
应用执行域。`CLAUDE.md` 的禁令针对的是“作为兼容助手重新引入”，不覆盖一条有本
ADR 支撑的新能力。

## 决策

### 1. 首个切片只做解码-分析流水线

单文件分析路径拆为两个线程：解码线程从 `PcmSource` 顺序读取 PCM 块并投入一个
有界 FIFO 队列；分析线程按同一顺序取出并交给 `AnalyzerSession`。

不做窗口级分析并行，不做 packet 级解码并行。选择流水线是因为它不改变解码输出
顺序，也不改变 `AnalyzerSession` 接收样本的顺序——分析侧看到的字节流与串行实现
完全相同，逐位一致由构造保证而非由归约设计保证。

理论收益上限是 `min(解码总时间, 分析总时间)`，即上表中约 92 ms。

### 2. 确定性契约

- 分析结果不得依赖线程数、队列容量、调度时序或宿主负载；
- 队列必须是有界 FIFO，容量固定且不由媒体声明决定；
- 关闭流水线（退化为单线程）必须产出逐位相同的结果，并作为可测试的配置存在；
- PCM 主链仍是有限交错 `f64`，不得在流水线边界改变精度或分块几何对结果的影响。

### 3. 与 M3 执行预算的关系

ADR-0004 写明“一个 active job 把并行 CPU/decoder/analyzer 数量收紧到 1”。本
决策不放宽该约束：仍然是一个 active job、一个 `PcmSource`、一个
`AnalyzerSession`。流水线只是把同一条串行流水拆到两个线程上执行，不增加并发的
decoder 或 analyzer 实例，也不改变 64 个排队 FIFO reservation 的语义。

批处理仍串行处理文件，不引入文件级并行。

### 4. 错误、取消与进度语义

- 解码错误仍是 sticky terminal，且必须在分析线程侧以与串行相同的顺序和分类
  暴露；不得因线程边界变成 EOF 或部分报告；
- 取消必须在两个线程上都及时生效，且不得产生部分写出的报告；
- 进度语义保持既有定义，不因流水线出现回退或跳变；
- 任一线程 panic 必须转为结构化错误，不得静默丢失或悬挂队列。

### 5. 验收门槛

- 39 项 safe-master 逐 token 对照保持 track DR 39/39、channel DR 62/62、
  overall peak 39/39、overall RMS 39/39、channel RMS 62/62、duration 39/39、
  差异数 0；
- 三套合法 corpus 的共享 `PcmSource` contract 与 ALAC/WAV 孪生逐位 PCM 等价
  全部保持；
- 新增确定性测试：同一输入在不同队列容量与线程配置下产出逐位相同的
  `AnalysisResult`；
- 性能声明必须按 ADR-0007 提供精确 result/PCM fingerprint 与同轮次交替 A/B；
  在此之前不得对外宣称任何加速比。

## 明确非目标

- packet 级或帧级解码并行、文件级并行；
- 窗口级分析并行（本 ADR 只论证其可行性，不实施）；
- SIMD、unsafe、第二 backend、外部解码进程；
- 修改固定分析算法、数值参数、报告字段或 wire schema；
- 把 elapsed time 或 RSS 变成普通测试/CI 阈值；
- 改变发行边界、平台矩阵或签名/公证状态。

## 后果

正面：解码与分析重叠可消除分析层的串行时间，且逐位一致由顺序不变构造保证，
是所有并行方案中风险最低的一档。它同时建立确定性并发的测试框架，为后续是否
推进 packet 级解码并行提供可复用的验收手段。

代价：单个 job 内部出现第二个线程，错误、取消与 panic 的传播路径变复杂；
需要新增确定性测试面。对 FLAC 与 ALAC 的端到端收益分别只有约 15% 与 9%，
真正的解码瓶颈仍未触及。

## 待补证据

静态分析发现已落成独立记录并与固定 target 绑定。转为 Accepted 前仍需补齐：

- 背景中的性能分解只是指示性测量，不满足 ADR-0007，也不进入证据体系。流水线
  落地后若要给出任何加速比，必须另行提供符合 ADR-0007 的 fingerprint 与同轮次
  交替 A/B；
- 确定性测试面尚未设计：需要明确以何种方式在测试中改变队列容量与线程配置，
  并证明 `AnalysisResult` 逐位不变；
- 取消与 panic 在跨线程边界的语义需要在实现前细化到可测试的断言，而不是留给
  实现自行决定。
