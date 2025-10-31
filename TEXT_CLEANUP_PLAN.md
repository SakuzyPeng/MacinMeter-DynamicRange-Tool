# Emoji & Messaging Cleanup Plan

> 目标：移除仓库内所有 emoji 符号，替换或澄清“阶段*”等模糊表述，并确保所有对用户可见的输出文案提供中英双语版本。

## 工作流
- [x] 逐文件检查是否包含 emoji、模糊阶段描述或仅单一语言的输出（源码首轮）。
- [ ] 修正后在本文件中更新对应条目标记为已完成。
- [ ] 修复完成后再次全局搜索确认无遗留（含插件/文档/测试输出）。

## 待处理文件

### 顶层文档
- [ ] `README.md`（多处 emoji、阶段描述模糊）
- [ ] `README.simplified.md`（需确认 emoji、输出描述是否合规）
- [ ] `SILENCE_FILTERING_PLAN.md`（含 “P0 阶段” 等表述）
- [ ] `PARALLEL_DECODER_OPTIMIZATION_PLAN.md`
- [ ] `DR_ALGORITHM_OPTIMIZATION_PLAN.md`
- [ ] `TEXT_CLEANUP_PLAN.md`（本文件最终也需复核）

### 源码（Rust）
- [x] `src/audio/format.rs`（错误提示双语）
- [x] `src/audio/mod.rs`
- [x] `src/audio/opus_decoder.rs`
- [x] `src/audio/parallel_decoder.rs`
- [x] `src/audio/stats.rs`
- [x] `src/audio/streaming.rs`
- [x] `src/audio/universal_decoder.rs`
- [x] `src/processing/channel_separator.rs`（调试输出符号）
- [x] `src/processing/sample_conversion.rs`（调试输出符号）
- [x] `src/processing/performance_metrics.rs`（性能报告文本）
- [x] `src/processing/dr_channel_state.rs`
- [x] `src/processing/edge_trimmer.rs`（模块文档标记 “P0阶段”）
- [x] `src/processing/mod.rs`
- [x] `src/tools/cli.rs`（CLI 帮助文本、emoji）
- [x] `src/tools/constants.rs`（阶段A/B/C/D 表述）
- [x] `src/tools/processor.rs`（日志与注释 Emoji，阶段说明）
- [x] `src/tools/formatter.rs`（输出文案需确认双语）



### 测试与脚本
- [ ] `tests/` 目录下输出断言是否包含 emoji（逐步检查，必要时记录具体文件）
- [ ] `scripts/` 下可执行脚本输出内容

## 已完成
- `src/audio/stats.rs`
- 典型改动：
  - 调试输出改为双语：`Processed packet #{count} … / 处理包#{count} …`，避免仅中文提示。
  - 包分布统计、变化系数等诊断信息改为中英并列描述。
- `src/processing/channel_separator.rs`
- 典型改动：
  - `debug_performance!` 改为 `SSE2 stereo separation… / SSE2立体声分离…` 双语日志。
- `src/processing/sample_conversion.rs`
- 典型改动：
  - NEON 调试信息改为 `Using NEON… / 使用NEON…` 双语格式。
- `src/processing/performance_metrics.rs`
- 典型改动：
  - 性能报告改成 `Performance report / 性能报告` 多段双语输出。
- `src/processing/dr_channel_state.rs`
- 典型改动：
  - 移除 emoji，并保留精准描述 `关键精度修复…`。
- `src/tools/cli.rs`
- 典型改动：
  - CLI 帮助信息双语化；输出文案如 `Results saved / 结果已保存`。
- `src/tools/constants.rs`
- `src/tools/processor.rs`
- 典型改动：
  - 阶段式注释改为明确策略描述；边缘裁切诊断全部双语。
- `src/audio/format.rs`
  - 错误提示双语，如 “Invalid sample rate / 采样率无效”。
- `src/audio/parallel_decoder.rs`
  - 将“优化#…”注释替换为具体策略说明，并引用计划文档。
- `src/audio/universal_decoder.rs`
  - 错误信息改为双语，并将 “Phase 3.2…” 描述改写为“首次进入 Flushing 时拉取剩余批次”等明确说明。
- `src/core/*`（dr_calculator.rs / histogram.rs / peak_selection.rs）
  - dr_calculator.rs：新增校验错误、调试输出、单元测试断言的双语信息。
  - histogram.rs：移除“Phase/步骤*”编号式注释，改成描述式场景；其它中文说明保持供内部参考。
- 阶段/优化编号示例：
  - 将“Phase 3.2优化”改写为“首次进入 Flushing 状态时集中拉取剩余批次，并使用 VecDeque 逐批弹出”。
  - 将“优化#10：使用 spawn_fifo + 移除 install 嵌套”改写为“调度策略：spawn_fifo + 无 install 嵌套，减少线程创建与同步成本”。
