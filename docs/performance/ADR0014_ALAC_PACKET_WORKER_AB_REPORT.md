# ADR-0014：ALAC packet worker 同轮 allocation 扫描

- 状态：Measured；packet workers 已于 ADR-0014 记录的启用决定后成为 ALAC route 默认
- 日期：2026-08-03
- 方法：ADR-0007 / 29-case ADR-0014 ALAC packet-worker sweep
- suite：runner-recorded id `m6-scalar-baseline-v1`；本次 29-case definition
  SHA-256 `ef282108ae465c5662d196d235a3d1456b23e2fe67b41696622153faae0d6aa6`
- corpus：`m6-performance-baseline-v1`（新增三条长 ALAC track）
- source：`3ef8ae38da5259f3f303cb0eb45daecc2bf92499`（clean）
- worker SHA-256：
  `7bcc0c9dc8046ab9fb18809f79fdd57e758d98bfcbdaf7b0c4525ffd066123a9`
- corpus generator SHA-256：
  `9265e317b8ae6cb013a828522dc26b8bd7ac7bc8f285c48dcdfcffe5e6a1e9cb`
- corpus manifest SHA-256：
  `0d2eef208812b1b369e22c8ec51d52190b77764dc7a3d915be09d9211ebe4919`
- canonical raw record：
  [`adr0014-alac-packet-worker-sweep-v4-3ef8ae3-aarch64-apple-darwin.json`](baselines/adr0014-alac-packet-worker-sweep-v4-3ef8ae3-aarch64-apple-darwin.json)
- raw record SHA-256：
  `1e7e348df729cf722dc172bf084428adb665917eb8c9bdf4db6ceb78054022ae`
- 前置记录：v1（单 track、19-case，source `3793c79`）、其 cross-check、
  v2（双 track、23-case，source `c6ca1ac`）与 v3（25-case，source `c1b25ea`）的
  raw record 均保留在 `baselines/`，v3 的 SHA-256 为
  `6f4c88462a69305f9fc9409b0dac63ab7be265403f7be41fe60fcac4fce2a26d`，其余为
  `4089d0ec1321ee73fc738df5c29205266470af39a29859e6e2b6b3c0d5bc1913`、
  `d3b90a1a8c7416870847831c067f7c3e195253abb88829ccebdbafc8ac47f3f8` 与
  `3fa7dd6fe0c8e631716454ba585c495e47e6187130afce980e386f0356d02ee2`
- 前置决策：
  [ADR-0014](../adr/0014-deterministic-decode-analysis-pipeline.md)

## 结论

ADR-0013 稳定 ALAC route 的有界 packet workers 在 240 秒长度输入上给出明确、
远超同轮 span 的解码加速。加速比在压缩率的两个极端之间基本一致，在为静态派发
构造的最坏负载不均下也没有下降；每条 track 的 1/2/4/8 worker 共享同一个结果
fingerprint；在单独扫描 permit 的 tonal track、8-worker allocation 上，最小、
plan 派生与产品上限也共享该 fingerprint。

本报告是一次**测量**。启用决定另行记录在 ADR-0014；本报告的数字是固定
source/binary/corpus/本机环境下的证据，不构成任何用户可见的性能承诺。

## 测量对象与协议

worker 数与 reorder permit 都是 decode allocation 的维度，不是另一个 binary，
因此它们是 case 参数而非 ADR-0007 的 `--variant`。十个 ALAC case 与其余 15 个
case 在同一次 run 内以固定 seed 完全交错（175 个 measured sample，warmup 独立
seed，`outliersRemoved = 0`）。

每个 case 各自被要求复现 corpus 的 normalized interleaved `f64` oracle，因此这个
扫描本身就是差分，而不是十组互不相干的计时。verification 解码运行在与计时段
**相同**的 allocation 上，不回退串行；完整 PCM hash 仍在计时区之外。

runner 另行复算 harness allocation，并核对 worker 实际获得的 `decodeWorkers` /
`decodeQueueCapacity` / `decodeMaxInFlightPcmBytes`；这会捕获 runner 与 worker 两份
镜像之间的配置错位。两者都只是 crate-private application plan 的镜像，并不自动
检测未来 plan 单独改变或另一台主机的 `available_parallelism` 收缩。本次固定主机有
12 个逻辑核，请求的 1/2/4/8 worker 与 source `c1b25ea` 的 dormant plan 数值一致；
经 `Application` 的真实派生仍是未满足的毕业门槛。

## 输入

两条 track 几何完全相同（240 秒 / 48 kHz / 立体声 / 16-bit，2813 个 ALAC
packet），只在信号的可压缩性上不同。二者均由 ffmpeg 8.0.1 以 ADR-0013 为
native-alac-v1 固定的同一 bit-exact 形状编码（`+bitexact`、`-map_metadata -1`、
`-compression_level 2`、显式 stereo layout），已验证连续两次生成产出逐字节相同的
文件与 manifest。

| Track | 信号 | 压缩率 | 大小 |
| --- | --- | ---: | ---: |
| `stereo-s16-alac-240s.m4a` | 既有 M6 确定性伪随机整数流 | 99.5% | 43.7 MiB |
| `stereo-s16-alac-tonal-240s.m4a` | 整数三角波叠加加小幅 dither | 60.0% | 26.4 MiB |
| `stereo-s16-alac-varied-240s.m4a` | 8 个难度递增变体的循环 | 74.4% | 32.7 MiB |

伪随机 track 几乎不可压缩，会让 ALAC 退回 uncompressed escape 路径；tonal track
落在无损音乐常见的压缩区间，迫使 codec 真正执行预测与 rice 解码。两者都由单个
4096-frame block 重复而成，因此每个 packet 的成本相同，固定的 `index % workers`
派发天然完美均衡。

varied track 专门打破这一点：它循环 8 个难度变体，从近静音到满幅噪声。变体数与
最大 worker 数相同，因此静态派发会让每个 worker 在整条流上只拿到同一种难度，
这是该派发方式能遇到的最坏不均衡。三条 track 的信号均为纯整数构造，不使用浮点。

## 同轮结果

`decode` scope，每 case warmup 1 次 + measured 7 次，单次迭代解码整条 240 秒
track。加速比相对同一次 run、同一条 track 的 1-worker case。

### 三条 track 的 worker 扫描

| Workers | 伪随机 99.5% | tonal 60.0%（均衡） | varied 74.4%（最坏不均） |
| ---: | ---: | ---: | ---: |
| 1 | 395.1 ms | 350.5 ms | 380.5 ms |
| 2 | 204.6 ms（1.93x） | 181.6 ms（1.93x） | 202.1 ms（1.88x） |
| 4 | 119.7 ms（3.30x） | 96.6 ms（3.63x） | 103.5 ms（3.68x） |
| 8 | 66.4 ms（5.95x） | 59.1 ms（5.93x） | 60.8 ms（6.26x） |

样本 min–max span / median：伪随机 7.3 / 5.8 / 26.8 / 15.7%，tonal 8.7 / 6.9 /
10.0 / 9.8%，varied 16.5 / 22.1 / 18.6 / 7.9%。median peak RSS 由 2.9–3.0 MiB
升至 6.4–7.2 MiB，七次最大值 3.0–3.1 升至 6.8–7.3 MiB。每条 track 的四个 worker
数各自共享唯一的 `resultFingerprintSha256`（伪随机 `40f68d10cbe5...`、tonal
`d76ea3199589...`、varied `859bb3a07b4f...`）。

本次 run 的 load average 由 9.59 降至 7.90（12 逻辑核）；所有加速比均不超过对应
worker 数。此前一次在 load 由 10.5 升至 30.7 期间完成的 run 给出 4 worker 4.96x
等物理上不可能的值，已整体作废，未纳入本记录。

### 负载不均没有降低加速比，以及原因

预期是 varied track 会明显变慢。它没有：8 worker 上 6.26x，反而略高于完美均衡的
tonal（5.93x）。

对每个变体单独编码 60 秒并串行解码（指示性测量，5 次取中位数，不满足 ADR-0007）
给出原因：

| 变体 | 压缩后 | 串行解码 | | 变体 | 压缩后 | 串行解码 |
| ---: | ---: | ---: | --- | ---: | ---: | ---: |
| 0 | 0.02 MiB | 30.5 ms | | 4 | 9.83 MiB | 110.4 ms |
| 1 | 6.04 MiB | 79.5 ms | | 5 | 10.22 MiB | 109.0 ms |
| 2 | 9.23 MiB | 81.4 ms | | 6 | 10.24 MiB | 113.7 ms |
| 3 | 9.47 MiB | 98.9 ms | | 7 | 10.34 MiB | 90.0 ms |

压缩后大小相差约 517 倍，解码时间只相差 3.7 倍；排除近静音的变体 0 后仅相差
1.43 倍。ALAC 解码成本主要由输出几何决定——每个 packet 固定 4096 frame 的
`SampleBuffer` 填充、`f64` 转换与 `PcmBlock` 分配——而不是压缩数据量。

据此，8 worker 的总时间应由最慢的变体决定：`8 × 89.2 / 113.7 = 6.28x`，与实测
6.26x 一致。因此固定 `index % workers` 派发在该 route 上的不均衡代价是有界且可
预测的。

这个结论是 route-specific 的：它依赖 ALAC 每 packet 输出几何固定、解码成本方差
小这一性质，不能外推到 FLAC 或任何其他 codec。

### Reorder permit 维度（tonal track，8 worker）

application plan 对每个 worker 数只派生一个队列容量，因此该维度单独扫描。最小
合法容量等于 worker 数，会把每个 worker 的 inbox 压到零容量 rendezvous；最大值是
固定产品上限。只有队列上限变化，in-flight PCM permit 仍取 plan 的派生值。

| Reorder permit | Median (ms) | MAD (ms) | Span/median | 相对默认 | Median peak RSS (MiB) | Max peak RSS (MiB) |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 8（最小，rendezvous） | 77.7 | — | — | +31.5% | 5.7 | — |
| 32（plan 派生） | 59.1 | — | — | — | 6.7 | — |
| 64（产品上限） | 60.2 | — | — | +1.9% | 6.8 | — |

最小 permit 明显更慢且 RSS 更低，与它强制 rendezvous 派发一致；从 plan 派生的 32
放宽到产品上限 64 不再带来收益（本次 +1.9%，落在同轮 span 内），说明 plan 的派生
值已经在该维度的收益拐点之后。

**tonal track 的全部六个 case（四个 worker 数加两个额外 permit）共享同一个
`resultFingerprintSha256`**，即结果同时独立于 worker 数与 reorder permit。

`resultFingerprintSha256` 在每条 track 的 1/2/4/8 worker case 内唯一（伪随机
`40f68d10cbe50bcc...`、tonal `d76ea3199589d3ff...`）；tonal 的两个额外 permit case
也保持其 fingerprint。即已运行 case 的解码 stream 几何、帧数、块数与完整
interleaved `f64` SHA-256 均不随 allocation 变化。两条 track 之间 fingerprint
自然不同，因为信号不同。

8 worker 的 span 在两条 track 上都明显更高（10.4% / 12.6%），tonal 的 4 worker 一次
达到 20.9%；本机在 run 期间负载在 6.6 与 9.3 之间（12 逻辑核）。绝对计时受宿主
负载影响，中位数与 fingerprint 结论不受影响。

## 正确性

- 每条 track 内 1/2/4/8 worker 的 decode result fingerprint 与 PCM SHA-256 完全相同；
- tonal track 的最小 / plan 派生 / 最大 reorder permit 三种配置与上述四个 worker
  数共享同一 fingerprint；
- 三条 track 各自的 12 单元 allocation 矩阵（4 个 worker 数 × 最小 / plan 派生 /
  最大 reorder permit）在 decoded `f64`、`AnalysisResult` raw bits 与 wire-visible
  report 三项上各自唯一，见下节；
- reorder 的滞留高水位在最紧 permit 与最深完成顺序下，对 1,000 与 100,000 个
  packet 完全相同；模拟 in-flight permit 泄漏会使长流在 index 12289 处失败而短流
  仍通过，因此该结论由长度对比而非单一长度承载；
- 每个 case 独立匹配 corpus 的 normalized interleaved `f64` oracle；
- 每个 case 的 7 个 measured sample 之间 fingerprint 稳定；
- 产品侧另有 9 个 committed ALAC fixture 在 1/2/4/8 worker、最小与最大
  `queue_capacity`、确定性强制乱序以及损坏 packet 下的 raw-bit、错误与 progress
  等价测试，见 ADR-0014 第 2 步记录。

## Allocation 等价矩阵

ADR-0014 的共同门槛要求同一 corpus 在各 worker 数与最小 / 默认 / 最大队列容量下
具有相同的 decoded-`f64`、`AnalysisResult` raw bits 与 wire-visible report。该矩阵
由独立的正确性 harness 运行，不计时：

```bash
cargo run --locked --release -p macinmeter \
  --example adr0014_allocation_matrix -- PATH > OUTPUT.json
```

每条 track 12 个单元：worker 数 1/2/4/8，各配最小合法容量（等于 worker 数）、plan
派生容量与固定产品上限 64。`workers = 1` 的三个单元都退化为串行引擎，这正是
“worker 数为 1 时在解码开始前退化”的验证；其余九个单元走 packet workers。harness
拒绝非 ALAC 输入，并逐单元核对 content probe 后实际选择的 engine 与 worker 数，因而
全串行回退不能产生成功记录。

`AnalysisResult` 的指纹是遍历其 exhaustive view、按 IEEE-754 位模式累积得到的，
不是渲染后的十进制文本，因此低于打印精度的差异不会被当作相等。
schema v2 在构造 wire report 前把 `SourceInfo.display_path` 规范化为记录中的 basename，
所以同一输入的相对、绝对或含 `..` 路径写法不再改变 wire fingerprint。harness 自身
输出排序且四空格缩进的 canonical JSON，重建命令可直接生成逐字节记录。

| Track | 单元数 | decoded `f64` | `AnalysisResult` raw bits | wire report | 记录 |
| --- | ---: | --- | --- | --- | --- |
| 伪随机 99.5% | 12 | `046c476b...` | `6c0c911b...` | `76be0435...` | [`…-stereo-s16-alac-240s.json`](equivalence/adr0014-allocation-matrix-stereo-s16-alac-240s.json) |
| tonal 60.0% | 12 | `d5b62bfc...` | `721bee5d...` | `91be17f9...` | [`…-stereo-s16-alac-tonal-240s.json`](equivalence/adr0014-allocation-matrix-stereo-s16-alac-tonal-240s.json) |
| varied 74.4% | 12 | `ac8394d4...` | `1e19f45e...` | `051a94fc...` | [`…-stereo-s16-alac-varied-240s.json`](equivalence/adr0014-allocation-matrix-stereo-s16-alac-varied-240s.json) |

三份记录的 SHA-256 分别为
`d483a78dfa4781be6039fe7a9e68e01daef9937ad58993d4a69867afdb4c46fd`、
`9d02a8b8939de529356889afe560a052f99890cdb2d0247dd808cd12cb3dfdf9` 与
`e4f606082d873447044918b1b8f8f1e2a609ecc5cbf6c78b8a2e819439e27bf9`；三条 track 各
11,520,000 帧。

该矩阵的检测能力经两次注入验证：把 reorder 的提交改为取任意 pending 项，会被
commit buffer 的既有契约检查在产生任何 PCM 之前拦下；只在 packet worker 路径对
单个 sample 翻转 1 ULP、不触发任何契约错误时，矩阵会报告四个互不相同的
decoded-`f64` fingerprint。两次注入均已回退。

## 真实录音交叉检查（本机一次性，语料不提交）

三条 corpus track 都是合成信号。为覆盖真实录音的立体声相关性、动态与预测复杂度，
在本机个人音频库上做了一次交叉检查。该语料是私人的、不可再生的，因此按 ADR-0007
的既定立场不进入仓库，也不是可复现基线；这里只保留聚合统计与结论。

库中 466 个 `.m4a` 里 314 个为 ALAC。产品 route 接受 **309 个（98.4%）**，
合计 22.7 小时，时长 43–760 秒，几何为 44.1 kHz 立体声，307 个 16-bit、2 个
24-bit。五个被拒的分为两类：

- 三个 96 kHz / 24-bit 文件写入 `sample_entry_rate = 1` 而非 ADR-0013 认可的
  零 sentinel，落入 `malformed_media / probe`；
- 两个文件的解码帧数比声明少**恰好 4096 帧**（正好一个 ALAC packet），落入
  sticky `decode_failed / decode`。

两类都是 ADR-0013 稳定矩阵的边界，与 packet workers 无关；本报告只登记观察，
不在此处改变该 route 的能力声明。

### 正确性

在按时长等间隔抽取的 40 个文件上运行完整 allocation 矩阵：每个文件 12 个单元，
合计 **480 次独立解码与分析、474,975,816 帧**（约 3 小时音频）。每个文件的 12 个
单元在 decoded `f64`、`AnalysisResult` raw bits 与 wire-visible report 三项上各自
唯一；40 个文件给出 40 个互不相同的指纹，说明该比较具有区分力而非恒等通过。

### 指示性性能

真实文件与合成 tonal track 在同一次运行中完全交错，各 5 次重复取中位数，因此宿主
负载对两者影响相同。这是指示性测量，不满足 ADR-0007。

| 输入 | 1 worker | 2 | 4 | 8 |
| --- | ---: | ---: | ---: | ---: |
| 真实录音 1（325 s） | 910.0 ms | 2.09x | 3.70x | 6.22x |
| 真实录音 2（331 s） | 785.3 ms | 1.54x | 3.10x | 5.12x |
| 真实录音 3（362 s） | 824.9 ms | 1.51x | 3.05x | 4.67x |
| 真实录音 4（378 s） | 946.1 ms | 1.56x | 2.90x | 4.66x |
| 真实录音 5（423 s） | 947.3 ms | 1.58x | 3.04x | 4.44x |
| 合成 tonal control（240 s） | 425.0 ms | 1.83x | 3.22x | 5.06x |

同 run 的合成 control 在此环境下只取得 5.06x，而 clean run 中为 5.93x，说明本次
测量整体被宿主负载压低。因此结论只取相对关系：真实录音的 8-worker 加速比
（4.44–6.22x）与同 run control（5.06x）处于同一量级，其中一个高于 control，没有
系统性劣化。

按每秒音频计，真实录音的串行解码约 2.4 ms/s，合成 control 约 1.8 ms/s；真实素材
的预测负载更高，但这只改变绝对成本，不改变并行行为。

## 环境

Apple M4 Pro / Mac16,8，12 物理核 12 逻辑核，48 GiB，macOS 27.0（Darwin
27.0.0），AC 供电。release profile：`opt-level=3`、thin LTO、
`codegen-units=1`、`overflow-checks=true`、无额外 `RUSTFLAGS`。

计时为 worker 内 `std::time::Instant` 包围命名 workload；另记录
`/usr/bin/time -l` 原生计数器与排除 wrapper 的 descendant process-tree RSS。

## 未满足的毕业门槛

本报告不足以默认启用 packet workers。ADR-0014 仍要求：

- **39 项 safe-master 逐 token 对照**：需要固定 reference 环境，本次未运行；
- **端到端的长流强制乱序**：reorder 的滞留界已由直接针对 commit buffer 的
  1,000 / 100,000 packet 最坏顺序对比证明，且最小 permit 在 240 秒真实解码上
  完成而未耗尽任何 permit（peak RSS 中位数 5.7 MiB，低于更宽 permit）。但确定性
  强制乱序 seam 是 `#[cfg(test)]`、不在 release worker 中，所以真实解码路径上的
  “长流 + 强制最坏乱序”组合仍未直接运行；
- **application 层集成**：产品 plan 恒为 serial。本扫描与等价矩阵都通过 harness
  的显式 allocation 驱动 `codecs`。`Application` 的真实派生路径已另由单元测试
  覆盖——固定的 8-worker 测试宿主上限复用 `ConcurrencyPlan::bounded` 的生产派生
  逻辑，其 budget 经 `Application::analyze_file` 得到与 serial plan 逐字节相同的
  wire report，并逐次核对实际选择的 engine 与 worker 数——但性能测量本身仍未经过
  该路径，`ExecutionBudget` 的非串行构造也仍是 `#[cfg(test)]`。

三条 track 的对照大幅缩小了“真实音乐素材代表性”的疑问：加速比在压缩率的两端
基本一致，在为静态派发构造的最坏负载不均下也没有下降，且该结果由变体级解码成本
测量解释并预测。真实录音交叉检查进一步在 480 次 allocation 上确认了正确性，并在
同 run 交错条件下显示加速比与合成 control 同量级。该交叉检查的语料不可提交、
不可再生，因此它是补充观察而非可复现证据。

tonal track、8-worker allocation 的 reorder-permit 性能敏感性 A/B 已完成，三个容量
共享同一 decode fingerprint；ADR 共同门槛要求的全矩阵也已在两条 track 上各完成
12 个单元，见“Allocation 等价矩阵”。两者都通过 harness 的显式 allocation 驱动
`codecs`；`Application` 的真实 plan 派生另由单元测试覆盖，但尚未承载性能测量。

## 边界

本记录是固定 source / binary / corpus / 本机环境的证据。它不是跨机器可复现构建
声明、CI 阈值、任意音频或存储条件的吞吐保证，也不是文件级或窗口级并行的预授权。
