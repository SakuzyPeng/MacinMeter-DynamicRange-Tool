# 跨平台内存峰值差异调查（macOS vs Windows）

目的：记录“同一批任务在 macOS 上峰值内存远高于 Windows，但总耗时只快约 3 秒”的现象、成因假设、定位方法与后续可行的无损优化方向，便于后续按需推进。

---

## 背景与现象

- 数据集：约 107 个文件，总规模 ≈ 12 GB
- 路径样例（macOS）：`/Users/Sakuzy/Downloads/test/flac`
- 分支与模式：`foobar2000-plugin`（默认批处理模式）
- 典型默认并发配置（CLI 默认）
  - `--parallel-files 4`
  - `--parallel-threads 4`
  - `--parallel-batch 64`

实测快照（典型）：
- Windows（11th Gen Intel Core i7-11800H）
  - 峰值内存 ≈ 130 MB（133,392 KB）
  - 平均内存 ≈ 86–99 MB
  - 总时长 ≈ 16–17s（107 文件）
- macOS（Apple M4 Pro）
  - 峰值内存 ≈ 800 MB
  - 用时仅比 Windows 快 ≈ 3s

结论：macOS 峰值显著更高，但耗时优势有限；该差异需要解释，并评估是否有“零结果影响/低性能代价”的优化空间。

---

## 成因假设（由高到低）

1) 线程栈大小 × 并发乘法（主要原因）
- macOS 上 pthread 默认栈一般 ≈ 8 MB/线程；Windows 常见 ≈ 1 MB/线程（提交/保留策略也更节省）。
- 当前存在两层并发：
  - 文件级并行（`parallel_files`，默认 4）：`src/tools/parallel_processor.rs` 自建 rayon 线程池
  - 单文件内并行解码（`parallel_threads`，默认 4）：`src/audio/parallel_decoder.rs` 再自建 rayon 线程池
- 当 `4（文件） × 4（解码）` 并行时，总工作线程可达 ≈ 16–20+；仅线程栈保守估算：
  - macOS：≈ 20 × 8 MB ≈ 160 MB（不含 guard page/系统线程/缓存等）
  - Windows：≈ 20 × 1 MB ≈ 20 MB
- 再叠加解码库/缓冲/分配器保留页/文件 I/O 缓冲等，macOS 足迹显著高于 Windows 符合预期。

2) 分配器与内存度量差异
- 不同平台/分配器的桶增长因子、提交/保留策略、回收策略不同，峰值会有体系化差异。
- Activity Monitor 的 Memory Footprint 与 Windows Working Set 口径不同，macOS 往往更“保守”。

3) 生产者-消费者背压与突发（次要）
- mac 上解码更快，短时可能“生产快于消费”，导致通道 backlog 短时上升（有界但每批体积可观），放大峰值。
- 相关位置：有界通道 + 发送端重排序（`src/audio/parallel_decoder.rs` 的 `SequencedChannel` 与 `OrderedSender`）。

4) 处理层缓冲预分配（影响有限）
- 例如 `sample_buffer` 预分配与声道分离缓冲（`src/tools/processor.rs`）随窗口大小（3 秒 × 采样率 × 声道）分配；单文件数量级在 MB 级，总体不至于解释 10× 差距，但会叠加。

---

## 如何定位与验证（建议保留为“工具箱”）

A) 快速控参对比（无代码改动）
- 仅文件内并行：`--parallel-files 1 --parallel-threads 4`（降低总线程数）
- 仅文件级并行：`--parallel-files 4 --parallel-threads 1`（降低每文件线程数）
- 观察峰值是否近似按总线程数线性下降，以验证“线程栈”为主因。

B) 平台工具（macOS）
- `vmmap <pid>`：统计 `Stack`/`MALLOC_*` 区域总量，确认线程栈与堆区分布。
- Instruments Allocations：采样分配热点，识别是否存在异常 backlog/大对象。
- `leaks`/`heap`：进一步确认大块内存归属（分配栈轨迹）。

C) 进程内轻量诊断（Debug 构建）
- 记录关键缓冲最大 `capacity`：
  - `sample_buffer`、声道分离 `left/right_buffer`
  - 并行解码通道的在途批次数（粗略估计 backlog 峰值）
- 仅 Debug 打印；Release/生产不影响性能。

---

## 可行的“零结果影响”优化（默认暂缓，后续按需）

P0（优先，通常无性能损失）
- 为两个 rayon 线程池设置较小栈：
  - 文件级池（`src/tools/parallel_processor.rs`）`.stack_size(1 << 20)` 或 2MB
  - 文件内解码池（`src/audio/parallel_decoder.rs`）`.stack_size(1 << 20)` 或 2MB
- 预期：macOS 峰值显著下降；速度基本不变（调用栈浅、无递归）。

P1（并发预算，通常不降速）
- 当 `parallel_files > 1` 时自动下调单文件 `parallel_threads = max(1, num_cpus::get_physical() / parallel_files)`；
- 或策略互斥：多文件并发开启时禁用文件内并行解码（可配开关）。
- 预期：总线程数更可控，峰值随之下降，吞吐通常不降（减少调度抖动）。

P2（微调，收益有限）
- Windows 下将 `reserve` 替换为 `reserve_exact`（`#[cfg(windows)]`），抑制过度扩容；
- 调整通道容量乘数 `SEQUENCED_CHANNEL_CAPACITY_MULTIPLIER`（当前 4）按平台/并发自适应（需配实测）。

注：以上均不改变算法与结果；属于“基础设施层”参数/策略。

---

## 决策与状态（可追踪）

- [ ] P0：设置 rayon 线程栈（1–2MB）
- [ ] P1：并发预算策略（跨层协调）
- [ ] P2：平台化 `reserve_exact`/通道容量微调
- [ ] 诊断脚本与文档化（vmmap/Allocations 快速指引）

当前结论：问题原因已基本锁定（线程栈 × 并发乘法 + 分配器/度量差异）。优化项先记录，待后续合适时间点实施与验证。

---

## 附：定位指引备忘

- 复现实验需固定：构建模式（`--release`）、同一数据集、同一并发参数、一次预热 + 多次取中位数。
- 观测指标：
  - 峰值内存（Footprint/Working Set）
  - 总耗时/吞吐（MB/s）
  - 线程总数（`ps -M`/Instruments Threads）与栈总量（`vmmap`）
- 录入环境：CPU/OS/磁盘类型（SSD/HDD），以便后续横向对比。

