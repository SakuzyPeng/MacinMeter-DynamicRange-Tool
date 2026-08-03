# ADR-0014：ALAC packet worker 同轮 allocation 扫描

- 状态：Measured；packet workers 仍未默认启用
- 日期：2026-08-03
- 方法：ADR-0007 / 25-case ADR-0014 ALAC packet-worker sweep
- suite：runner-recorded id `m6-scalar-baseline-v1`；本次 25-case definition
  SHA-256 `9ce6ca2f4d677edece3c1b871df27a1869730cc63b2e6ef7d1302b1ff5a80ea4`
- corpus：`m6-performance-baseline-v1`（新增两条长 ALAC track）
- source：`c1b25eaa018fa5b3ce04198f10b3dd3f237a013e`（clean）
- worker SHA-256：
  `6717e39f8cc9fd651ebf4e006b912e491c23aafff0bfb9ae782dbd3c4888ae38`
- corpus generator SHA-256：
  `1cb3af0af7f3e98487812460ebc6054a963d76a9255059c435e565294f68d10f`
- corpus manifest SHA-256：
  `2a9c604cb2c37bb910aaa3781bb4dbd9131040c4163d59f398138d266dbd5b07`
- canonical raw record：
  [`adr0014-alac-packet-worker-sweep-v3-c1b25ea-aarch64-apple-darwin.json`](baselines/adr0014-alac-packet-worker-sweep-v3-c1b25ea-aarch64-apple-darwin.json)
- raw record SHA-256：
  `6f4c88462a69305f9fc9409b0dac63ab7be265403f7be41fe60fcac4fce2a26d`
- 前置记录：v1（单 track、19-case，source `3793c79`）、其 cross-check，以及
  v2（双 track、23-case，source `c6ca1ac`）的 raw record 均保留在 `baselines/`，
  SHA-256 分别为
  `4089d0ec1321ee73fc738df5c29205266470af39a29859e6e2b6b3c0d5bc1913`、
  `d3b90a1a8c7416870847831c067f7c3e195253abb88829ccebdbafc8ac47f3f8` 与
  `3fa7dd6fe0c8e631716454ba585c495e47e6187130afce980e386f0356d02ee2`
- 前置决策：
  [ADR-0014](../adr/0014-deterministic-decode-analysis-pipeline.md)

## 结论

ADR-0013 稳定 ALAC route 的有界 packet workers 在 240 秒长度输入上给出明确、
远超同轮 span 的解码加速。加速比在压缩率的两个极端之间基本一致，每条 track 的
1/2/4/8 worker 共享同一个结果 fingerprint；在单独扫描 permit 的 tonal track、
8-worker allocation 上，最小、plan 派生与产品上限也共享该 fingerprint。

这是一次**测量**，不是启用决定。ADR-0014 的默认启用还缺若干门槛，见“未满足的
毕业门槛”。产品 plan 目前仍恒为 serial，本报告不构成任何用户可见的性能承诺。

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

伪随机 track 几乎不可压缩，会让 ALAC 退回 uncompressed escape 路径；tonal track
落在无损音乐常见的压缩区间，迫使 codec 真正执行预测与 rice 解码。两条 track 一起
覆盖压缩率的两端。tonal 信号是纯整数构造，不使用浮点，因此完全可复现。

## 同轮结果

`decode` scope，每 case warmup 1 次 + measured 7 次，单次迭代解码整条 240 秒
track。加速比相对同一次 run、同一条 track 的 1-worker case。

### 伪随机 track（压缩率 99.5%）

| Workers | Median (ms) | MAD (ms) | Span/median | Speedup | Median peak RSS (MiB) | Max peak RSS (MiB) |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 397.8 | 0.53 | 1.3% | 1.00x | 3.0 | 3.1 |
| 2 | 206.6 | 1.60 | 5.0% | 1.93x | 3.9 | 3.9 |
| 4 | 110.2 | 0.21 | 2.3% | 3.61x | 4.7 | 5.0 |
| 8 | 65.4 | 1.79 | 10.4% | 6.08x | 6.6 | 6.8 |

### Tonal track（压缩率 60.0%）

| Workers | Median (ms) | MAD (ms) | Span/median | Speedup | Median peak RSS (MiB) | Max peak RSS (MiB) |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 354.8 | 2.30 | 2.3% | 1.00x | 3.0 | 3.1 |
| 2 | 180.9 | 0.76 | 1.6% | 1.96x | 3.8 | 3.9 |
| 4 | 97.1 | 0.53 | 20.9% | 3.65x | 4.7 | 5.1 |
| 8 | 61.3 | 2.26 | 12.6% | 5.79x | 6.6 | 7.2 |

两条 track 的加速比在每个 worker 数上相差不超过 0.29x（1.93/1.96、3.61/3.65、
6.08/5.79）。tonal track 的绝对时间整体更短，与其显著更小的压缩载荷一致。

### Reorder permit 维度（tonal track，8 worker）

application plan 对每个 worker 数只派生一个队列容量，因此该维度单独扫描。最小
合法容量等于 worker 数，会把每个 worker 的 inbox 压到零容量 rendezvous；最大值是
固定产品上限。只有队列上限变化，in-flight PCM permit 仍取 plan 的派生值。

| Reorder permit | Median (ms) | MAD (ms) | Span/median | 相对默认 | Median peak RSS (MiB) | Max peak RSS (MiB) |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 8（最小，rendezvous） | 75.2 | 2.84 | 10.7% | +22.7% | 5.7 | 5.8 |
| 32（plan 派生） | 61.3 | 2.26 | 12.6% | — | 6.6 | 7.2 |
| 64（产品上限） | 57.4 | 0.75 | 13.7% | −6.4% | 6.5 | 6.8 |

最小 permit 明显更慢且 RSS 更低，与它强制 rendezvous 派发一致；从 32 放宽到 64
只再取得 6.4%，说明 plan 的派生值已接近该维度的收益拐点。

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
- 两条 track 各自的 12 单元 allocation 矩阵（4 个 worker 数 × 最小 / plan 派生 /
  最大 reorder permit）在 decoded `f64`、`AnalysisResult` raw bits 与 wire-visible
  report 三项上各自唯一，见下节；
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
cargo run --release -p macinmeter --example adr0014_allocation_matrix -- PATH
```

每条 track 12 个单元：worker 数 1/2/4/8，各配最小合法容量（等于 worker 数）、plan
派生容量与固定产品上限 64。`workers = 1` 的三个单元都退化为串行引擎，这正是
“worker 数为 1 时在解码开始前退化”的验证；其余九个单元走 packet workers。

`AnalysisResult` 的指纹是遍历其 exhaustive view、按 IEEE-754 位模式累积得到的，
不是渲染后的十进制文本，因此低于打印精度的差异不会被当作相等。

| Track | 单元数 | decoded `f64` | `AnalysisResult` raw bits | wire report | 记录 |
| --- | ---: | --- | --- | --- | --- |
| 伪随机 99.5% | 12 | `046c476b...` | `6c0c911b...` | `036a7eae...` | [`adr0014-allocation-matrix-stereo-s16-alac-240s.json`](equivalence/adr0014-allocation-matrix-stereo-s16-alac-240s.json) |
| tonal 60.0% | 12 | `d5b62bfc...` | `721bee5d...` | `237010c4...` | [`adr0014-allocation-matrix-stereo-s16-alac-tonal-240s.json`](equivalence/adr0014-allocation-matrix-stereo-s16-alac-tonal-240s.json) |

两份记录的 SHA-256 分别为
`7e1f7eb22c62683a8af4cb56bc445c5a392444fcebfe37fdb198b96c8cd806ad` 与
`b5f230dbb9e018991453cc0c8db02a0c2037f384b3320a7cc77d3601bff5372d`；两条 track 各
11,520,000 帧。

该矩阵的检测能力经两次注入验证：把 reorder 的提交改为取任意 pending 项，会被
commit buffer 的既有契约检查在产生任何 PCM 之前拦下；只在 packet worker 路径对
单个 sample 翻转 1 ULP、不触发任何契约错误时，矩阵会报告四个互不相同的
decoded-`f64` fingerprint。两次注入均已回退。

## 环境

Apple M4 Pro / Mac16,8，12 物理核 12 逻辑核，48 GiB，macOS 27.0（Darwin
27.0.0），AC 供电。release profile：`opt-level=3`、thin LTO、
`codegen-units=1`、`overflow-checks=true`、无额外 `RUSTFLAGS`。

计时为 worker 内 `std::time::Instant` 包围命名 workload；另记录
`/usr/bin/time -l` 原生计数器与排除 wrapper 的 descendant process-tree RSS。

## 未满足的毕业门槛

本报告不足以默认启用 packet workers。ADR-0014 仍要求：

- **39 项 safe-master 逐 token 对照**：需要固定 reference 环境，本次未运行；
- **小队列长流与最坏乱序的组合压力**：最小 permit 在 240 秒、2813 packet 的流上
  成功完成且未返回 capacity error，peak RSS（中位数 5.7 MiB、七次最大 5.8 MiB）
  低于更宽的 permit。这只是单一长度、自然完成顺序下的固定 case 观察；raw record
  不记录 reorder occupancy 高水位，不能单独证明内存不随媒体时长增长。确定性强制
  乱序 seam 又是 `#[cfg(test)]`、不在 release worker 中，所以“长流”与“强制最坏
  乱序”只各自被覆盖，二者的组合仍只有短 fixture 证据；
- **application 层集成**：产品 plan 恒为 serial，本扫描通过 harness 的显式
  allocation 驱动 `codecs`，未经过 `Application` 的实际启用路径。

两条 track 的对照缩小了此前“真实音乐素材代表性”的疑问：加速比在压缩率的两端
基本一致，因此该结论不依赖语料恰好落在 escape 路径。但两者都是合成信号，没有
覆盖真实录音的动态、立体声相关性与 packet 长度分布。

tonal track、8-worker allocation 的 reorder-permit 性能敏感性 A/B 已完成，三个容量
共享同一 decode fingerprint；ADR 共同门槛要求的全矩阵也已在两条 track 上各完成
12 个单元，见“Allocation 等价矩阵”。两者都通过 harness 的显式 allocation 驱动
`codecs`，因此仍不覆盖 `Application` 的启用路径。

## 边界

本记录是固定 source / binary / corpus / 本机环境的证据。它不是跨机器可复现构建
声明、CI 阈值、任意音频或存储条件的吞吐保证，也不是文件级或窗口级并行的预授权。
