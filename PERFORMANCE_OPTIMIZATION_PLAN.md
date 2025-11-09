# 性能优化计划（Windows & 多声道场景）

## 背景

- Foobar2000 `sub_180008570` 采用 **单次遍历 + SSE2** 完成窗口 RMS/峰值统计，复杂度与声道数线性，单文件耗时 <1s。
- 目前 MacinMeter 在同一 Windows 机器上处理 7.1 FLAC 需要 ~1s，主要瓶颈来自：
  1. 立体声仍走 `ChannelSeparator` → `Vec` 拷贝 → `process_samples`，多声道虽然跨步但仍是标量内循环。
  2. `window_rms_values` / `window_peaks` 等调试数组造成大量 push/alloc，声道数越多越明显。
  3. Rayon 并行对中等声道数（6~8ch）收益有限，反而引入调度开销。
  4. 缺少端到端性能监测，难以量化每项优化的收益。

以下计划按“先易后难”排序，优先清除最显著的 CPU/内存热点。

## 阶段 1：快速减负（预计 1~2 天）

1. **统一多声道单循环路径**
   - 目标：让 `channel_count >= 2` 都走 `calculate_dr_strided` 的单线程跨步循环，避免 `ProcessingCoordinator`/Rayon。
   - 影响文件：`src/core/dr_calculator.rs`, `src/processing/processing_coordinator.rs`.
   - 验收：7.1 样本在 Windows 上 CPU 利用率稳定，线程数不再爆炸。

2. **移除 release 默认的窗口调试缓冲**
   - 引入 feature flag（如 `diagnostic_window_buffers`）。Release 默认只保留直方图 + 必需峰值寄存器。
   - 影响文件：`src/core/histogram.rs`.
   - 验收：peak RSS 下降、push 次数显著减少；测试覆盖 debug 模式。

## 阶段 2：SIMD 内核（预计 3~5 天）

1. **向 `WindowRmsAnalyzer` 注入 SSE2/NEON 内核**
   - 在 `process_samples` 和 `process_samples_strided` 中按 4/8 样本块加载：
     - `_mm_and_ps` 清符号位
     - `_mm_mul_ps` 累积平方和
     - `_mm_max_ps` 维护峰值
   - 提供 `cfg(target_arch)` 分支，保持安全回退。
   - 影响文件：`src/core/histogram.rs`, `src/processing/simd_core.rs`.
   - 验收：Win x64 上 7.1 FLAC 耗时接近 foobar（≤1.0s）；CI 新增 SSE 单元测试。

2. **重用现有 4×4 转置基础**
   - 将 `SimdProcessor::transpose_4x4_block` 扩展为“批量馈送 Analyzer”模式，减少中间结构体。
   - 影响文件：`src/processing/simd_core.rs`, `src/core/dr_calculator.rs`.

## 阶段 3：系统化度量（并行）

1. **内置性能探针**
   - 在 `ProcessingCoordinator` / `DrCalculator` 输出（debug 模式）窗口处理速率、SIMD 命中率。
   - 影响文件：`src/processing/processing_coordinator.rs`, `src/core/dr_calculator.rs`.

2. **基准脚本**
   - 新增 `scripts/bench_win.ps1`，固定样本集（2ch、6ch、8ch）跑三次取中位数，记录 CSV。
   - 可与 foobar CLI 输出对比，量化差距。

## 阶段 4：可选 & 后续

- **SIMD 自适应阈值**：短文件直接走标量，避免启动 SIMD overhead。
- **多文件批处理管线**：结合 `rayon` 的 chunking，把多首歌并行，单首内部保持单线程。

## 风险与回滚

- SIMD 修改需严密测试（交错样本 + 尾窗 + 次峰），建议在 CI 添加 deterministic fixtures。
- Release/Debug feature flag 需保证默认行为与 foobar 匹配，避免用户结果突变。

## 进度更新（阶段一完成）

- 已完成（Phase 1）
  - 多声道统一单循环路径：3+ 声道走 `calculate_dr_strided` 的单次遍历（跨步/4×4 转置），避免 Rayon 调度与多余拷贝。
  - 关闭 release 默认的窗口诊断缓冲：通过 `diagnostic_window_buffers` feature（默认关闭）屏蔽 `window_rms_values` 的 push/alloc，只保留直方图与必需的峰值寄存器。

- 影响范围
  - 代码：`src/core/dr_calculator.rs`（多声道单循环/4×4 转置）、`src/core/histogram.rs`（诊断缓冲 feature 宏与分支）、`Cargo.toml`（features）。
  - 行为：功能输出一致；release 下显著减少窗口级分配与 push 次数。

- 阶段一后最新 10× 测试（macOS 本机，2025‑11‑09）
  - 7.1 FLAC（215.43 MB）：
    - 原始秒数：0.759619, 0.387081, 0.391134, 0.391650, 0.399357, 0.383324, 0.387536, 0.396246, 0.394817, 0.422284
    - 统计：平均 0.4313 s｜中位 0.3932 s｜标准差 0.1099 s；吞吐均值 519.8 MB/s
  - 12ch WAV（1.65 GB，32‑bit float）：
    - 原始秒数：1.702392, 1.702867, 1.674672, 1.752811, 2.031035, 1.718499, 1.771171, 1.758129, 1.752152, 1.760626
    - 统计：平均 1.7624 s｜中位 1.7525 s｜标准差 0.0945 s；吞吐均值 963.8 MB/s

- 初步结论
  - macOS 下 7.1 场景与之前基线相当（中位 ~0.39s），12ch 场景轻微改善（~1.77s → ~1.76s）。
  - Windows 的主要差距仍在 12ch 浮点 WAV（我们 ~3s、foobar <1s），符合预期：阶段一清除了额外分配与线程调度开销，但核心热点仍在样本内标量循环。

## 当前性能基线（2025-11-09，本机 macOS）

- 命令与 10× 脚本：`./target/release/MacinMeter-DynamicRange-Tool-foo_dr <音频路径>`，使用文档上方的 Python 循环脚本重复 10 次。

### 基线 A：7.1 FLAC（215.43 MB）
- 文件：`audio/multiCH/7.1/海阔天空 ~DRV SURROUND AUDIO~.flac`
- 10 次耗时（秒）：`0.428466, 0.369061, 0.397246, 0.400853, 0.410435, 0.415073, 0.417205, 0.399411, 0.406666, 0.501098`
- 统计：平均 `0.4146 s`、中位 `0.4086 s`、标准差 `0.0325 s`；吞吐平均 `522.6 MB/s`、中位 `527.3 MB/s`、标准差 `37.0 MB/s`
- Windows 上 MacinMeter 处理同样 7.1 FLAC 约需 1 s，foob​​ar2000 依旧略快（<1 s）；此场景差距不大。

### 基线 B：12 声道 WAV（32-bit float, 1.65 GB）
- 文件：`audio/large audio/DRV CLASSIC｜杜比全景声 舒伯特降E大调钢琴三重奏 D.929 - Beaux Arts Trio 第四乐章.wav`
- 10 次耗时（秒）：`1.776259, 1.795054, 1.750479, 1.720640, 1.746741, 1.774959, 1.792677, 1.775451, 1.782105, 1.781246`
- 统计：平均 `1.7696 s`、中位 `1.7759 s`、标准差 `0.0221 s`；吞吐平均 `957.6 MB/s`、中位 `954.0 MB/s`、标准差 `12.1 MB/s`
- Windows 上 MacinMeter 约 3 s，而 foobar2000 <1 s。该 12 通道浮点 WAV 是主要的性能缺口；后续优化需以把 Windows 耗时从 ~3 s 压到 ~1 s 内为目标。

## 下一步

1. 开发单循环路径合并（阶段 1-1），提交 Profile 前后对比。
2. 并行推进调试缓冲 flag（阶段 1-2），为后续 SIMD 减少干扰。
3. SIMD 内核开发前先补充单元测试覆盖（窗口峰值、尾窗、虚拟0窗）。
