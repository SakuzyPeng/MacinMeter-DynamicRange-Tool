# ADR-0004：M3 application 执行预算与串行准入

- 状态：Accepted
- 实施状态：DONE（2026-07-20，单 backend 需求评审收口）
- 日期：2026-07-20
- 决策范围：M3
- 相关路线图：[架构整改与参考插件重新对齐路线图](../ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md)
- 前置决策：
  - [ADR-0001：以 0.2.0 重建可信主干](0001-m0-0.2.0-trusted-trunk-rebuild.md)
  - [ADR-0003：M2 原生解码面与工程契约加固](0003-m2-native-decoder-contract-hardening.md)

## 背景

M0/M2 已经让单个 `BatchRunner` 按稳定输入顺序串行处理文件，并为每个
`AnalyzerSession` 固定 64 声道资源上限；这两项都不是进程级调度。

Tauri 的 `run_analysis`、`run_batch` 和 `discover_inputs` 各自调用
`spawn_blocking`。`JobRegistry` 只保存 `jobId -> CancellationToken`，不限制同时
进入 blocking pool 的任务数量。因此两个不同 jobId 可以同时创建 decoder 和
analyzer，多个 batch 也可以彼此并行。CLI 单次只执行一个命令，但此前没有与 GUI
共用可表达这一约束的 application 入口。

M3 路线图同时提到多 backend 与资源编排，但第二 backend 只能由明确产品需求
触发。不能为了进入 M3 就恢复 FFmpeg、Opus、Rayon/Tokio 调度或文件级并行。

## 决策

### 1. 建立显式共享的 `Application`

`macinmeter::Application` 是一个可克隆的执行域；其 clone 共享同一
`ExecutionCoordinator`。CLI 在命令生命周期内创建一个实例，Tauri 将一个实例放入
managed state。进程内需要共同受限的任务必须使用同一个实例，不使用隐藏的
process-global singleton。

`Analyzer`、`BatchRunner` 和受控字节发现逻辑继续作为 crate 内同步操作；
`Application` 成为唯一公开的文件分析/批处理 façade，CLI/Tauri 生产入口都由它
编排。明确需要自行喂入 PCM 的高级调用仍可使用 lower-level `AnalyzerSession`。
这样 library 用户可以明确决定哪些调用属于同一资源域，也不会让 lower layer
依赖 Tauri 或异步 runtime；公开 API 也不存在绕过 application budget 的第二条
文件执行路径。

### 2. 在提交 blocking work 前预留

Tauri 必须先调用：

```rust
Application::reserve(&CancellationToken) -> ApplicationJob
```

成功得到的 `ApplicationJob` 是单次使用的 reservation，并提供：

```rust
ApplicationJob::analyze_file(...)
ApplicationJob::run_batch(...)
ApplicationJob::discover_inputs(...)
```

reservation 在 `spawn_blocking` 之前创建，所以队列已满时不会继续向 runtime
提交隐藏的 blocking work。`Application` 同时提供同步 convenience methods，供
CLI 和普通 Rust 调用在当前线程完成“预留、等待、执行”。

### 3. M3 产品策略固定为有界 FIFO 串行

M3 的 `ExecutionBudget` 只开放串行策略：

- active job：固定为 1；
- queued job：产品默认最多 64；
- admitted job 总数：最多 65；
- reservation 按创建顺序使用单调 ticket 进入 FIFO。

这里的一个 job 是一次顶层 analyze、batch 或 discovery 请求。一个 batch 在持有
同一 active slot 时继续按 M0 契约串行处理其内部文件，不为每个 item 重复入队。

队列满时返回可恢复的
`ResourceExhausted / Validation`。这不是媒体失败，不产生 batch partial result。
队列顺序和上限不加入 wire report，也不改变 schema v3。

### 4. 取消和释放属于 reservation 状态机

reservation 只有 `Queued / Active` 两态：

- 入队前已取消：不占队列；
- 排队期间取消：返回 `Cancelled / Cancellation` 并从 FIFO 删除；
- active job 的取消继续由既有 `ExecutionControl` 在 discovery/decode 边界检查；
- queued 或 active reservation 的正常返回、错误返回、提前 drop 和 unwind 都由
  RAII 删除/释放；
- 一个 token 的取消不得改变其他 reservation 或 token。

当前同步实现使用 `Mutex + Condvar`，等待时至多每 25 ms 重新检查取消。application
crate 不为此引入 Tokio、Rayon 或新的 async trait。

### 5. 本切片是粗粒度资源边界，不冒充精确内存计量

一个 active job 把并行 CPU/decoder/analyzer 数量收紧到 1；64 声道
`AnalyzerSession` 上限继续约束单会话 histogram 状态。队列上限约束已接收任务数。

本实现不声称：

- 能测量 Symphonia 或操作系统的精确 byte allocation；
- 已经给 decoder 建立硬内存 sandbox；
- 已启用文件级并行；
- 已完成外部进程 supervisor；
- 已满足未来多 backend 的所有资源维度。

后续若出现实际第二 backend，必须先为其声明资源需求与运行时可用性，再扩展预算
模型；不能用一个空 registry 预先制造复杂度。

### 6. 不新增排队 wire 事件

排队期间不发出 `FileStarted`、`DiscoveryStarted` 或 decode progress。获得 active
slot 后才由既有操作发出事件，因此 adapter 不会把“已提交到线程池”误报成“已经
开始分析”。本切片不增加 `queued` wire variant，schema v3 保持不变。

## 验收

M3 必须固定：

- FIFO 顺序与同时最多一个 active job；
- queue capacity 在 blocking work 开始前生效；
- queued cancellation 不影响 active/其他 queued job；
- reservation drop 与 unwind 后 successor 可以继续；
- 两个 Tauri job 不会同时发出 `FileStarted`，取消排队 job 后 active job仍成功；
- CLI stdout/stderr、JSON、退出码和 Tauri report 形状不变；
- workspace fmt、严格 Clippy、测试和 GUI build/check 通过；
- 不触发或等待远端 CI。

## M3 收口评审（2026-07-20）

执行预算实现固定于 commit `89a0d07`（`feat: coordinate application jobs within
a serial budget`）。

### 实际 backend 与产品需求

收口时实际启用的 decoder backend 仍只有进程内 Symphonia。四条 stable route
是 wave/pcm_integer、wave/pcm_float、flac/flac 与 aiff/pcm_integer；capability
catalog 中的 AIFC、ALAC、MP3、Vorbis 和 AAC 仍是没有 discovery extension 的
`planned` 条目，不是可执行 backend。

M0 至 M3 没有记录要求恢复某一外部 decoder、特定未支持格式或独立 decoder
部署形态的产品需求。因此没有证据支持引入 `DecodePlan`、backend registry、
FFmpeg supervisor 或第二套错误/取消路径。条件性设计没有被实现，也不作为 M3
遗留欠账。

### 可执行的资源边界

M3 对当前产品实际能强制执行的资源维度作如下收口：

- 每个共享 `Application` 同时最多一个 active 顶层任务，最多 64 个 reservation
  排队；analyze、batch 和 discovery 共用同一 FIFO；
- 一个 batch 在持有 active slot 时逐文件串行，因而同一执行域中同时最多存在一个
  decoder 和一个 analyzer session；
- `AnalyzerSession` 的 64 声道上限继续在直接 session 分配前执行，媒体路径在
  decoder 创建前拒绝超过产品上限的声明；
- 队列容量限制的是轻量请求/reservation 数量，不把等待任务提前提交到 Tauri
  blocking pool。

进程内 Symphonia 和操作系统分配器不提供可由 application 精确强制的逐 decoder
byte quota。M3 因而只声明“并发驻留的 decoder/session 数量有界”，不声明恶意媒体
下的硬内存 sandbox。若未来产品要求把不受信任媒体放入硬内存限制，必须把隔离
进程或平台 sandbox 作为明确安全需求另立 ADR；不能把一个不能执行的数字放进
`ExecutionBudget` 后称为已隔离。

### 出口条件核对

| 出口条件 | 收口证据 | 结论 |
| --- | --- | --- |
| 所有实际 backend 使用相同 PCM 契约 | 当前只有 `SymphoniaPcmSource`；ADR-0003 的共同 contract matrix、route 专属损坏输入和 sticky terminal-state harness 已完成 | 满足 |
| 外部进程失败不成为 EOF | 产品没有外部 backend、进程启动或 supervisor 路径 | 当前不适用；未来引入时必须重新验收 |
| backend 与任务服从统一预算，取消/失败隔离 | `Application` 是唯一公开文件 façade；FIFO、queue full、queued cancellation、drop/unwind 和两个 Tauri job 的隔离均有本地测试 | 满足 |
| 支持格式反映真实能力 | 单一 Rust capability catalog 驱动 discovery、application query 与 GUI picker；stable snapshot 有产品测试 | 满足 |

因此 M3 在单 backend 产品面上完成。收口没有增加格式、backend、并行轴、wire
variant 或兼容性声明。

## 后续决策门

M3 收口后，只有新的实际产品需求才能重新打开 backend 设计：

1. 没有第二 backend 需求时，保持单一 Symphonia route 和当前 capability catalog；
2. 有明确格式/部署需求时，先写独立 backend ADR，定义 `DecodePlan`、运行时探测和
   同一 `PcmSource` contract；
3. 只有选择外部进程后，才实现启动、取消、timeout、stderr、退出状态和回收；
4. 文件级并发仍属于 M6 profiling 决策，不因预算类型已经存在而自动开放。

## 后果

- GUI 多 job 从非受控并行改为显式、有界、可取消的 FIFO；
- application 成为 CLI/Tauri 的共同执行入口；
- 文件级 `Analyzer`、`BatchRunner` 与独立 discovery 函数不再从 crate root
  公开；0.2.0 Rust API 统一由 `Application` 承担；
- 默认吞吐仍是一个顶层 job，不产生新的性能承诺；
- 最多 64 个 queued job 是产品资源契约，超过时调用方收到结构化可恢复错误；
- 第二 backend 与硬内存 sandbox 不是已完成 M3 的隐藏待办；未来若由明确需求
  触发，作为新的架构/安全决策重新定义和验收。

## 未采用方案

### 使用 process-global semaphore

拒绝。它会把不相关的 library 实例强制绑定到隐藏全局状态，也无法表达测试或嵌入方
需要的独立执行域。

### 只依赖 Tauri blocking pool

拒绝。runtime 的线程池策略不是产品资源契约，CLI/Rust API 也无法共用。

### 第二个 job 立即失败，不排队

拒绝。短暂的正常竞争不应被当成用户错误；有界 FIFO 同时保留确定性和背压。

### 在 M3 恢复文件级并行

拒绝。当前没有 M6 benchmark/profile 依据；资源预算先建立约束，不预先证明并发有
收益。
