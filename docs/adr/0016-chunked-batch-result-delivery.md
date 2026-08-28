# ADR-0016：批处理结果分块交付

- 状态：Accepted
- 实施状态：DONE（2026-08-28）
- 日期：2026-08-28
- 决策范围：application 批处理结果交付与 adapter 兼容边界
- 前置决策：
  - [ADR-0004：M3 application 执行预算与串行准入](0004-m3-application-execution-budget.md)
  - [ADR-0014：确定性的解码—分析流水线](0014-deterministic-decode-analysis-pipeline.md)

## 背景

`Application::run_batch` 原先在所有文件完成后返回一个完整 `BatchReport`。这会让
application 层把每个 `AnalysisReport` 一直保留到批次结束。文件达到数百或数千时，
结果驻留随 item 数量线性增长；同时 GUI 只能依赖额外的完成事件提前渲染，形成
“事件中一份、最终报告中再一份”的重复结果通道。

后续原生 egui adapter 需要在结果产生时持续消费，并允许界面只渲染当前可见行或
在用户选择条目后展示详情。先把 application 执行契约改为有界分块，可以去掉核心的
整批驻留；是否进一步把单个 item 拆成摘要与按需详情不属于本 ADR。

## 决策

### 1. 执行入口只返回终态，结果由同步 sink 接收

公开执行入口改为：

```rust
Application::run_batch(request, chunks, control)
    -> Result<BatchCompletion, AnalysisError>
```

`BatchChunkSink::emit` 接收拥有所有权的 `BatchChunk`。每个 chunk 非空，当前最多
包含 `BatchChunk::MAX_ITEMS == 8` 个完成项。sink 调用是同步的，因此慢消费者会向
有界 file lane 施加背压，不建立无界 application 结果队列。

file lane 可以并发完成，sink 也可能被不同 lane 并发调用。chunk 到达顺序和 chunk
内部顺序都不能当作发现顺序；每个 `BatchChunkItem` 的 `index` 是稳定的发现顺序
身份。消费者需要按 index 建立、更新或最终排序自己的视图。

### 2. 核心不构造完整 `BatchReport`

批处理核心只保留未满的尾 chunk、已发布数量与成功/失败计数。完整 chunk 的所有权
在 `emit` 时移交，核心不再保存一份完整 item 列表。完成全部交付后返回的
`BatchCompletion` 只包含 `BatchStatus` 与 `BatchSummary`。

需要单个完整文档的 adapter 可以显式使用 `BatchReportCollector`。collector 按稳定
index 校验并恢复 `BatchReport`，也明确让该 adapter 重新承担 O(items) 结果驻留。
`BatchReport` 继续作为 schema 4 wire/CLI 文档类型存在，但不再是 application
批处理执行的返回类型。

### 3. 只有一条完成结果路径

删除 `AnalysisEvent::BatchItemFinished`。`ProgressSink` 继续传递生命周期和 decode
进度；完整 item 只通过 `BatchChunkSink` 交付，避免 adapter 合并两条可能重复或
失配的结果流。

尾 chunk 成功交付后才发送 `AnalysisEvent::BatchFinished` 并返回
`BatchCompletion`。sink 返回错误时，批处理停止领取新工作、等待所有在途 lane
结束，再返回该错误；不发送 `BatchFinished`。取消也可能发生在部分 chunk 已经成功
交付之后，最终返回 `Cancelled`。因此消费者只有收到成功 completion 后，才能把已
接收分块视为完整批次。

### 4. 不保留旧 Rust 执行 API，保留用户可见契约

不提供旧的 `run_batch(request, control) -> BatchReport` 兼容入口，也不增加第二个
隐式收集方法。保留它会让 application 继续承诺整批驻留，并长期形成两套结果语义。

兼容范围固定为：

- 分析数值与错误分类不变；
- 由显式 collector 生成的最终 item 顺序不变；
- CLI JSON schema version 4、human 输出和退出码不变；
- 本 ADR 落地时过渡期 Tauri 仍返回 schema 4 `WireEnvelope::Batch`；后继
  ADR-0017 已把普通列表改成 `batch_summaries`，只在显式完整导出时物化该 envelope；
- 未来 egui adapter 直接消费 chunk，不继承 WebView/Tauri IPC 兼容层。

## 资源边界

本决策不增加 scheduler、线程池或并行轴。file lane、decoder worker、队列与内存
继续共用 ADR-0014 的单个 application-owned plan。核心结果驻留上界由一个未满
chunk 加上有界在途 sink 调用构成；adapter 主动复制、缓存或持久化结果所占资源由
adapter 自己负责。

本决策本身不实现“点击时才加载详情”；其后
[ADR-0017](0017-on-demand-batch-details.md) 已在不重新分析文件的前提下定义摘要投影、
详情所有权、会话生命周期与显式全量导出。核心 chunk 仍携带完整 `BatchItem`，由
调用方的详情 store 接管，而列表 consumer 只接收派生摘要。

## 验收

- 覆盖一个满 chunk 加尾 chunk，并断言每个 chunk 非空且不超过固定上限；
- 覆盖所有已毕业 file-lane 宽度下的结果身份与显式 collector 稳定排序；
- 覆盖 sink 失败停止批次且不发送 terminal event；
- 覆盖所有完成 item 均先于 `BatchFinished` 交付；
- CLI、Tauri、workspace 与 schema 4 契约测试保持通过。
