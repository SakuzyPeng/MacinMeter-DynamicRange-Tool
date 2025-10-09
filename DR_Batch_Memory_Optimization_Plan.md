# DR 批量处理内存优化计划（可追踪）

**创建日期**: 2025-01-15  
**作者**: MacinMeter DR Tool  
**状态**: 进行中

---

## 背景与基线

- 平均处理速度: 206.40 MB/s
- 平均内存峰值: 57.83 MB
- 平均运行时间: 7.407 s
- 模式: 批量模式（默认并发；单文件流式分析；统一 DR 口径）

目标：在不显著牺牲吞吐（±2% 内）的前提下，将峰值内存压至 ≤ 50 MB；保持 0.0001 dB 一致性测试全通过。

---

## 内存热点评估（现状）

- 声道分离的临时缓冲（每窗口为 L/R 分配 Vec<f32>）：反复分配/释放，抬高峰值和分配抖动。
- 流式缓冲 `sample_buffer` 使用 `Vec` 并频繁 `drain(0..window_size)`：前段搬移增加 CPU 和暂时性容量膨胀。
- 并发文件数倍增 per-worker 占用（解码器缓存 + 流式缓冲 + 分离临时向量）。

---

## 优化路线（按性价比排序）

### 阶段A（优先）：复用声道分离缓冲（减少每窗口分配）

- 思路：为 `process_window_with_simd_separation` 增加预分配缓冲参数，或为 `ChannelSeparator` 新增“写入外部缓冲”API，循环内复用。
- 触点：
  - `src/tools/processor.rs` 窗口循环与通道分离调用
  - `src/processing/ChannelSeparator`（新增 `fill_into(left: &mut [f32], right: &mut [f32])` 或等价函数）
- 预期：每并发文件峰值降低 ≈ 1.0–1.2 MB；4 并发≈ 4–5 MB 峰值降低。
- 风险：低；局部改动，易验证。

待办清单：
- [ ] 设计缓冲复用 API（函数签名与安全边界）
- [ ] 在 processor 窗口循环中预分配并复用 L/R 缓冲
- [ ] 单测：与旧路径结果 bitwise 近似（0.0001 dB 容差内完全一致）

### 阶段B（优先）：替换/优化 sample_buffer（减少前段搬移）

- 方案1：使用 `VecDeque` 替代 `Vec`，用 `pop_front/drain` 取窗口；
- 方案2：保留 `Vec`，引入“起始偏移”+“周期性 compact（如容量 1/2 阈值）”。
- 触点：`src/tools/processor.rs` 窗口循环的 `extend` 与 `drain`。
- 预期：CPU 占用下降、峰值略降（减少 reallocation 过冲），整体更稳定。
- 风险：低-中；索引与窗口切片需小心。

待办清单：
- [ ] 选型：VecDeque vs 偏移 compact（基于基准测试）
- [ ] 实现并替换窗口抽取逻辑
- [ ] 单测：窗口边界/残留样本处理一致

### 阶段C（可选）：并发度按内存预算自适应

- 新增 CLI 参数：`--mem-budget-mb <INT>`（例如：48/64）。
- 用粗估 `per_worker ≈ 12–15 MB` 约束并发度：`effective_parallel_degree(requested, Some(budget / per_worker))`。
- 同时可在低预算下自动将 `parallel_batch_size` 从 64 降至 32。
- 触点：
  - `src/tools/cli.rs`（参数与默认）
  - `src/tools/utils.rs::effective_parallel_degree`（新增带预算变体或在 main 侧组合）
  - `src/main.rs`（最终并发度计算逻辑）
- 预期：低内存主机上峰值明确可控；吞吐按比例下降（可接受）。
- 风险：低；行为可配置。

待办清单：
- [ ] 新增 `--mem-budget-mb` 参数（不破坏现有默认）
- [ ] 在 main 侧按预算约束并发度（使用已存在的工具函数）
- [ ] （可选）预算低时自动降 `parallel_batch_size`

### 阶段D（可选）：对齐窗口/块大小（减少残留与切割）

- 提供“窗口友好”的 chunk hint 给解码器（若可行），或在读取端积累到窗口边界后再分析。
- 触点：`src/audio/universal_decoder.rs`（若支持）、`src/tools/processor.rs`（积累策略）。
- 预期：小幅降低残留占用与 CPU；收益中小。
- 风险：中；需解码器配合。

---

## 验收标准（统一）

- 一致性：WAV/MP3/AAC/OGG 四格式“批量 vs 单文件”一致性测试（容差 < 0.0001 dB）全部通过。
- 性能：平均吞吐 ≥ 200 MB/s（±2% 内）；运行时间不显著上升。
- 内存：峰值内存 ≤ 50 MB（默认 4 并发；综合格式场景）。
- 质量：`cargo fmt && cargo clippy -- -D warnings && cargo test` 全部通过。

---

## 度量与对比方法

- 构建：`cargo build --release`
- 吞吐/时间：运行 `./benchmark_10x.sh`（记录平均值，比较优化前后差异）
- 内存峰值（macOS）：`/usr/bin/time -l cargo run --release -- <输入>`（取 `peak memory`），或使用 `ps`, `vmmap` 采样对比。
- 记录位置：`Performance_Tracking.md` 与 `optimization_log.md`。

---

## 风险与回滚

- A/B 阶段属局部改动，出现回归可快速回滚对应函数签名与调用点。
- C/D 阶段为可选特性，默认关闭不影响现有行为。

---

## 实施计划（建议）

1) 阶段A（~0.5–1h）→ 阶段B（~0.5–1h）→ 基准与一致性验证（~0.5h）
2) （可选）阶段C（~0.5h）→ 阶段D（~0.5–1h）
3) 更新文档：`Performance_Tracking.md`、`docs/REFACTOR_PLAN_DR_OUTPUT_2025.md` 的阶段状态

---

## 任务清单（可勾选）

- [ ] A1 设计/实现 L/R 缓冲复用 API
- [ ] A2 在 processor 窗口循环复用缓冲
- [ ] A3 一致性与基准验证（A）
- [ ] B1 选型 VecDeque/偏移 compact
- [ ] B2 实现并替换窗口抽取逻辑
- [ ] B3 一致性与基准验证（B）
- [ ] C1 新增 `--mem-budget-mb` 参数并接入约束（可选）
- [ ] C2 低预算自动降 `parallel_batch_size`（可选）
- [ ] D1 对齐窗口/块大小（可选）
- [ ] 文档与追踪更新（结果、数据、回归）

---

（备注）本计划不改变默认行为；所有可选特性需显式开启或在低内存场景下触发。

