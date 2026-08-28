# ADR-0017：批处理摘要列表与按需详情

- 状态：Accepted
- 实施状态：DONE（2026-08-28）
- 日期：2026-08-28
- 决策范围：批处理结果所有权、列表投影、详情生命周期与 adapter 物化边界
- 前置决策：
  - [ADR-0016：批处理结果分块交付](0016-chunked-batch-result-delivery.md)

## 背景

ADR-0016 去掉了 application 核心对完整 `BatchReport` 的整批驻留，但过渡期 Tauri
仍把每个完整 `BatchItem` 发送给 WebView，并在命令结束时再次返回完整 schema-4
envelope。数百或数千个文件会同时放大 Rust clone、IPC JSON、JavaScript 对象和 DOM
节点；仅把完整结果切成较小 chunk 不能解决最终全量传输与全量展开。

列表排序、搜索和进度展示只需要路径、成功/失败、track DR、时长与少量格式字段。
每声道结果、算法参数和完整诊断只在用户查看某一项或明确导出完整文档时需要。

## 决策

### 1. 完整结果仍只分析一次

`BatchItem::summary()` 从已经完成的 `BatchItem` 生成 `BatchItemSummary`。成功摘要的
`BatchTrackSummary` 包含列表所需的 track DR、peak/RMS、时长、采样率、声道数、
位深、container/codec、backend 与 warning 数量；失败摘要保留结构化错误。

摘要不是第二套分析结果，也不允许点击时重新读取或重新分析源文件。源文件可能已经
移动或改变，重复分析还会破坏一次 batch 的可复现身份。

### 2. 详情会话拥有一份完整结果

显式 `BatchDetailStore` 原子接收核心 `BatchChunk`：

- 以稳定发现 index 保存每个完整 `BatchItem` 的一个 `Arc`；
- 同时返回不含 per-channel/algorithm/diagnostics 图的 `BatchListChunk`；
- chunk 内或跨 chunk 的重复 index 会让整个新 chunk 失败，不产生部分插入；
- `finish(BatchCompletion)` 校验 index 连续性、总数、成功/失败计数与状态；
- `detail(index)` 只复制 `Arc`，供原生 UI 选择项直接读取；
- `report()` 只有在成功 completion 后可用，并仅为显式完整文档操作 clone 全部详情。

application 执行核心仍只保留未满的有界 chunk。`BatchDetailStore` 是调用方主动选择的
结果会话，因此为后续点击保留详情所需的 O(items) 空间由调用方承担。当前报告图在
典型双声道批次中足以支持数千项只保留一个 native copy；本切片不引入 library 文件
写入或临时数据库。若实测目标规模超出 native memory budget，再单独决定有界缓存与
adapter-owned spill，不把重新分析当作缓存淘汰策略。

### 3. Tauri 只向列表发送摘要

过渡期 Tauri adapter 做以下映射：

- 一个核心 chunk 对应一个 `batch_summaries` channel 消息；
- `run_batch` 成功只返回 result ID、tool version 与 `BatchCompletion`，不再返回完整
  batch envelope；
- `get_batch_item_detail(resultId, index)` 只 clone/序列化用户选择的一项；
- `get_batch_report(resultId)` 仅在 JSON、全部 Markdown、raw JSON 或整批图片导出时
  物化原有 schema-4 `WireEnvelope::Batch`；
- `release_batch_results` 在界面清空或开始下一批时释放会话；运行中的会话不能被该
  命令静默释放；
- 遗留会话最多保留 8 个，超过上限返回可恢复的 `ResourceExhausted`，防止失联窗口
  无界占用结果内存。

普通列表项只建立摘要卡片。成功项点击“详情”后才传输并建立声道表；收起不会再次
传输。同一项再次展开复用前端已经加载的 detail。显式整批图片导出会展开所有项，
因为该操作本身要求完整可视内容。

### 4. egui 不继承 Tauri result registry

result ID registry 只用于跨 WebView command 保存过渡期会话。未来 egui model 直接
拥有 `Arc<BatchDetailStore>`、轻量 row vector 和当前选择项的 `Arc<BatchItem>`：

```text
BatchChunk -> BatchDetailStore -> BatchListChunk -> virtualized egui rows
                       |
                       +-> selected index -> Arc<BatchItem> -> detail panel
```

egui 清空 tab/model 时由 Rust RAII 释放整批详情，不需要 IPC、JSON 或显式 registry
命令。列表必须使用虚拟化/可见行渲染，不能把 Tauri 的 DOM 分帧实现照搬过去。

## 兼容边界

- CLI 继续显式收集完整报告，human/JSON 输出、schema version 4、稳定顺序和退出码
  不变；
- 单文件 `WireEnvelope::Analysis` 不变；
- Tauri 导出的完整 JSON 仍由后端原始 schema-4 envelope 生成；
- Tauri 内部 `run_batch` response 与 channel message 是 runtime DTO，不属于稳定 wire
  schema，因此不保留原先“最终命令返回完整 batch envelope”的内部调用形状；
- 数值、错误分类、batch status 与 partial-success 语义不变。

取消、sink 失败或 task join 失败会从 registry 删除整个详情会话。此前可能已经显示的
摘要不能被当作完成结果；前端收到错误后清空它们。只有成功 `BatchCompletion` 会让
会话进入可导出状态。

## 验收

- 真实成功报告的摘要字段与详情逐字段来源一致，序列化摘要不含 `analysis` 或
  `diagnostics` 完整图；
- 同一详情在 store 内重复读取保持同一 `Arc`；
- 重复 index 的 chunk 原子失败；
- 1000 项会话保持稳定 index，可完成并按顺序恢复 schema-4 报告；
- Tauri 会话数量有硬上限并支持显式释放；
- 点击详情的 TypeScript 路径、frontend build、CLI/Tauri/workspace 与 repository
  contract 门禁通过。
