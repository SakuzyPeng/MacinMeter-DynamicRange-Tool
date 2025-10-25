# 测试审计计划（可追踪）

目的：系统性排查并改进测试集，识别并处理以下问题：
- 安慰剂测试：没有断言或只验证无关紧要事实、无法捕捉回归。
- 不合理测试：脆弱/非确定性（依赖时间、线程调度、外部环境）、越权（网络/全局状态污染）。
- 过慢测试：单测或用例组合显著拖慢反馈（默认阈值：单测 > 500ms、文件 > 5s；可按需调整）。
- 冗余/重复：语义重复或多处验证同一逻辑但无新增覆盖。

范围：`tests/` 目录的集成测试，以及 `src/**` 内的模块内 `#[test]` 单元测试。

度量与工具：
- 列表与分组：`cargo test -- --list`，统计所有测试条目（含 ignored）。
- 计时与排序：
  - 首选：`cargo nextest`（如允许）收集 per-test 时间分布；否则使用 `cargo test -Z unstable-options --report-time`（如可用）。
  - 兜底：多次运行 `cargo test`，通过日志标记粗略定位慢文件。
- 重复检测：按模块/路径聚类断言目标，人工甄别；对“参数化数据不同但断言完全重叠”的场景标记为可能重复。

执行策略：由“广到深、先快后慢”的顺序推进，逐文件建卡、逐条目更新状态；每项给出处置建议与验收标准。

状态枚举：
- pending：待审计
- investigating：审计中（标注发现）
- fix-ready：已提出修复建议（待实现/待验证）
- resolved：已修复、回归通过

判定标准速写：
- 安慰剂测试：
  - 无断言或只断言 `len >= 0`、`is_ok()` 而未验证值域/行为。
  - 大量 `println!`/日志而无核心判断。
- 不合理测试：
  - 依赖 wall-clock（`sleep`、时间阈值）、线程竞态、全局环境变量，却无隔离/随机种子固定。
  - 访问外部文件系统以外的资源（网络等）。
  - 对平台/CPU 指令集的假设未通过 `cfg` 守护。
- 过慢测试：
  - 单测 > 500ms（默认）；文件 > 5s；或 CI 合计 > 60s。
  - 大体量基准类测试未标记 `#[ignore]` 或未降采样。

——

## 任务总览（按目录）

- tests/（集成测试）
  - aiff_diagnostic.rs — pending
  - audio_format_tests.rs — pending
  - audio_test_fixtures.rs — pending（基准/治具，关注 I/O 体量）
  - boundary_tests.rs — pending（此前有“silence”波动史）
  - chunk_stats_tests.rs — pending
  - error_handling_tests.rs — pending
  - error_path_tests.rs — pending
  - memory_safety_tests.rs — pending
  - opus_decoder_tests.rs — pending
  - parallel_decoder_tests.rs — pending（历史上对并发/有界通道较敏感）
  - parallel_diagnostic_tests.rs — pending
  - simd_edge_case_tests.rs — pending（指令集/平台差异）
  - simd_performance_tests.rs — pending（若含性能断言→建议 ignore）
  - simd_unit_tests.rs — pending
  - tools_integration_tests.rs — pending（CLI/流程用例，关注慢）
  - universal_decoder_tests.rs — pending

- src/**（模块内单测，聚焦核心）
  - core/histogram.rs — pending（窗口/20%选择/虚拟零窗边界）
  - core/dr_calculator.rs — pending（策略/零阈值）
  - core/peak_selection.rs — pending（策略分支/削波阈值）
  - processing/simd_core.rs — pending（大量单测；慢用例需甄别）
  - processing/channel_separator.rs — pending（SIMD/余数路径）
  - processing/sample_conversion.rs — pending（多格式/通道组合）
  - processing/processing_coordinator.rs — pending（并发相关）
  - audio/universal_decoder.rs — pending（I/O/探测/缓存）
  - audio/parallel_decoder.rs — pending（有界通道/乱序）
  - tools/batch_state.rs、tools/cli.rs — pending（CLI/状态）
  - error.rs — pending（Display/From/分类，通常快速）

——

## 逐文件审计模板（复用此模板填充）

文件：`<path>`
- 状态：pending | investigating | fix-ready | resolved
- 规模：测试数约 N；ignored 数约 M；运行时长（粗略/CI）约 T
- 快速结论：
  - 安慰剂嫌疑：<列举测试函数/理由>
  - 不合理/脆弱：<列举测试函数/理由>
  - 过慢：<列举测试函数/理由>
  - 冗余/重复：<列举分组/说明>
- 建议与修复项（打勾追踪）：
  - [ ] 将性能/大体量用例标记 `#[ignore]`，只在手动或 profiling 下运行
  - [ ] 引入固定随机种子/去掉 sleep，改为条件等待或 deterministic stub
  - [ ] 降采样/缩小治具体量（秒级→百毫秒级）
  - [ ] 增加关键断言（值域/等价性/误差上界）替代“仅 is_ok”
  - [ ] 分平台 `#[cfg]` 守护（SSE2/NEON 差异）
  - [ ] 合并重复测试或参数化（table-driven）
- 验收标准：
  - 无安慰剂测试；慢用例均被 ignore 或降速；CI 全量测试 < 60s；平台稳定性通过两次复跑。

——

## 立即优先级（P0）

1) `tests/boundary_tests.rs`（历史“silence”波动）
- 检查是否仍使用严格零比较；确认已采用 DR_ZERO_EPS 或等效语义。
- 若包含长音频治具，转为短样本构造或将大用例标记 ignore。

2) `tests/simd_performance_tests.rs`
- 若包含性能断言/时间阈值：标记 `#[ignore]`，改为打印/记录，不进入常规 CI。
- 若仅验证“是否走 SIMD 分支”，优先以功能断言替代耗时断言。

3) `src/processing/simd_core.rs` 大量单测
- 甄别是否有“规模化”用例（1e5+ 样本）且未 ignore；如有 → 降采样或 ignore。
- 平台差异路径（SSE2/NEON）需 `#[cfg]` 守护或条件断言（避免在不支持平台跑 SIMD 分支）。

4) `tests/parallel_decoder_tests.rs`
- 检查是否存在对通道容量/背压/线程调度的时间假设；改为“事件驱动验证”（例如计数/对齐），避免 `sleep`。

——

## 度量与脚本（推荐）

- 列出所有测试：
  ```bash
  cargo test -- --list
  ```
- 单文件计时（粗略）：
  ```bash
  time cargo test --test boundary_tests
  ```
- 仅运行 ignored（慢用例）：
  ```bash
  cargo test -- --ignored
  ```
- 如允许可引入 nextest：
  ```bash
  cargo nextest run --profile ci --retries 1
  cargo nextest run --list --status
  ```

——

## 初始清单（待填充）

- [ ] tests/aiff_diagnostic.rs — pending
- [ ] tests/audio_format_tests.rs — pending
- [ ] tests/audio_test_fixtures.rs — pending
- [ ] tests/boundary_tests.rs — pending
- [ ] tests/chunk_stats_tests.rs — pending
- [ ] tests/error_handling_tests.rs — pending
- [ ] tests/error_path_tests.rs — pending
- [ ] tests/memory_safety_tests.rs — pending
- [ ] tests/opus_decoder_tests.rs — pending
- [ ] tests/parallel_decoder_tests.rs — pending
- [ ] tests/parallel_diagnostic_tests.rs — pending
- [ ] tests/simd_edge_case_tests.rs — pending
- [ ] tests/simd_performance_tests.rs — pending
- [ ] tests/simd_unit_tests.rs — pending
- [ ] tests/tools_integration_tests.rs — pending
- [ ] tests/universal_decoder_tests.rs — pending

- [ ] src/core/histogram.rs（模块内） — pending
- [ ] src/core/dr_calculator.rs（模块内） — pending
- [ ] src/core/peak_selection.rs（模块内） — pending
- [ ] src/processing/simd_core.rs（模块内） — pending
- [ ] src/processing/channel_separator.rs（模块内） — pending
- [ ] src/processing/sample_conversion.rs（模块内） — pending
- [ ] src/processing/processing_coordinator.rs（模块内） — pending
- [ ] src/audio/universal_decoder.rs（模块内） — pending
- [ ] src/audio/parallel_decoder.rs（模块内） — pending
- [ ] src/tools/batch_state.rs、src/tools/cli.rs（模块内） — pending
- [ ] src/error.rs（模块内） — pending

——

## 节奏与产出

- 每日推进 3–5 个文件的完整审计，随进度更新此文档的状态字段与发现清单。
- 对每个“过慢/不合理/安慰剂/重复”的条目，给出一条可落地的修复建议与验收标准。
- 阶段性里程碑：
  - M1：P0 文件完成（boundary/simd/perf/parallel）
  - M2：tests/ 全覆盖
  - M3：src/** 模块内单测全覆盖

