# ADR-0014：ALAC packet worker 同轮 worker-count 扫描

- 状态：Measured；packet workers 仍未默认启用
- 日期：2026-08-03
- 方法：ADR-0007 / 23-case ADR-0014 ALAC packet-worker sweep
- suite：runner-recorded id `m6-scalar-baseline-v1`；本次 23-case definition
  SHA-256 `fbfbb4c3a47a8505df2d9333dfc8ad00de43cde30784732f07a85b11ea5a9c38`
- corpus：`m6-performance-baseline-v1`（新增两条长 ALAC track）
- source：`c6ca1aca5a12d2c03bb68d3c30004b7fd4c0f957`（clean）
- worker SHA-256：
  `ca5b069cc0f386e00fb3f859b71b722dfc0217761e1c9f5f0f63cee48be07262`
- corpus generator SHA-256：
  `1cb3af0af7f3e98487812460ebc6054a963d76a9255059c435e565294f68d10f`
- corpus manifest SHA-256：
  `2a9c604cb2c37bb910aaa3781bb4dbd9131040c4163d59f398138d266dbd5b07`
- canonical raw record：
  [`adr0014-alac-packet-worker-sweep-v2-c6ca1ac-aarch64-apple-darwin.json`](baselines/adr0014-alac-packet-worker-sweep-v2-c6ca1ac-aarch64-apple-darwin.json)
- raw record SHA-256：
  `3fa7dd6fe0c8e631716454ba585c495e47e6187130afce980e386f0356d02ee2`
- 前置记录：本报告的 v1（单 track、19-case）扫描与其 cross-check raw record 仍
  保留在 `baselines/`，身份为 source `3793c79`、raw record
  `4089d0ec1321ee73fc738df5c29205266470af39a29859e6e2b6b3c0d5bc1913` 与
  `d3b90a1a8c7416870847831c067f7c3e195253abb88829ccebdbafc8ac47f3f8`
- 前置决策：
  [ADR-0014](../adr/0014-deterministic-decode-analysis-pipeline.md)

## 结论

ADR-0013 稳定 ALAC route 的有界 packet workers 在 240 秒长度输入上给出明确、
远超同轮 span 的解码加速，1/2/4/8 worker 的结果 fingerprint 在每条 track 内
完全相同，且加速比在压缩率的两个极端之间基本一致。

这是一次**测量**，不是启用决定。ADR-0014 的默认启用还缺若干门槛，见“未满足的
毕业门槛”。产品 plan 目前仍恒为 serial，本报告不构成任何用户可见的性能承诺。

## 测量对象与协议

worker 数是一个 decode allocation，不是另一个 binary，因此它是 case 参数而非
ADR-0007 的 `--variant`。八个 ALAC case 与其余 15 个 case 在同一次 run 内以固定
seed 完全交错（161 个 measured sample，warmup 独立 seed，`outliersRemoved = 0`）。

每个 case 各自被要求复现 corpus 的 normalized interleaved `f64` oracle，因此这个
扫描本身就是差分，而不是八组互不相干的计时。verification 解码运行在与计时段
**相同**的 allocation 上，不回退串行；完整 PCM hash 仍在计时区之外。

runner 另行复算 harness allocation，并核对 worker 实际获得的 `decodeWorkers` /
`decodeQueueCapacity` / `decodeMaxInFlightPcmBytes`；这会捕获 runner 与 worker 两份
镜像之间的配置错位。两者都只是 crate-private application plan 的镜像，并不自动
检测未来 plan 单独改变或另一台主机的 `available_parallelism` 收缩。本次固定主机有
12 个逻辑核，请求的 1/2/4/8 worker 与 source `c6ca1ac` 的 dormant plan 数值一致；
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
| 1 | 398.1 | 1.96 | 1.4% | 1.00x | 3.0 | 3.1 |
| 2 | 205.0 | 0.99 | 1.8% | 1.94x | 3.8 | 3.9 |
| 4 | 111.3 | 0.40 | 1.7% | 3.58x | 4.7 | 5.0 |
| 8 | 70.5 | 2.40 | 16.0% | 5.65x | 6.7 | 7.0 |

### Tonal track（压缩率 60.0%）

| Workers | Median (ms) | MAD (ms) | Span/median | Speedup | Median peak RSS (MiB) | Max peak RSS (MiB) |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 352.7 | 1.69 | 3.4% | 1.00x | 3.0 | 3.1 |
| 2 | 182.1 | 0.91 | 6.3% | 1.94x | 3.9 | 4.3 |
| 4 | 96.7 | 0.24 | 1.7% | 3.65x | 4.7 | 5.1 |
| 8 | 59.1 | 2.35 | 14.6% | 5.97x | 6.5 | 6.7 |

两条 track 的加速比在每个 worker 数上相差不超过 0.32x（1.94/1.94、3.58/3.65、
5.65/5.97）。tonal track 的绝对时间整体更短，与其显著更小的压缩载荷一致。

`resultFingerprintSha256` 在每条 track 内对四个 worker 数唯一
（伪随机 `40f68d10cbe50bcc...`、tonal `d76ea3199589d3ff...`），即解码 stream
几何、帧数、块数与完整 interleaved `f64` SHA-256 均不随 worker 数变化。两条
track 之间 fingerprint 自然不同，因为信号不同。

8 worker 的 span 在两条 track 上都明显更高（16.0% / 14.6%），同时本机在 run 期间
保持较高负载（load average 起 13.15、止 11.98，12 逻辑核）。

## 正确性

- 每条 track 内 1/2/4/8 worker 的 decode result fingerprint 与 PCM SHA-256 完全相同；
- 每个 case 独立匹配 corpus 的 normalized interleaved `f64` oracle；
- 每个 case 的 7 个 measured sample 之间 fingerprint 稳定；
- 产品侧另有 9 个 committed ALAC fixture 在 1/2/4/8 worker、最小与最大
  `queue_capacity`、确定性强制乱序以及损坏 packet 下的 raw-bit、错误与 progress
  等价测试，见 ADR-0014 第 2 步记录。

## 环境

Apple M4 Pro / Mac16,8，12 物理核 12 逻辑核，48 GiB，macOS 27.0（Darwin
27.0.0），AC 供电。release profile：`opt-level=3`、thin LTO、
`codegen-units=1`、`overflow-checks=true`、无额外 `RUSTFLAGS`。

计时为 worker 内 `std::time::Instant` 包围命名 workload；另记录
`/usr/bin/time -l` 原生计数器与排除 wrapper 的 descendant process-tree RSS。

## 未满足的毕业门槛

本报告不足以默认启用 packet workers。ADR-0014 仍要求：

- **队列容量维度的性能敏感性**：本次只覆盖 application plan 派生的默认容量。
  最小与最大 `queue_capacity` 目前只有正确性证据，没有 A/B；
- **39 项 safe-master 逐 token 对照**：需要固定 reference 环境，本次未运行；
- **小队列长流与最坏乱序压力测试**：需证明 queue/reorder 内存不随媒体时长增长。
  本次记录了两条 track 各 worker 数的 peak RSS（中位数 3.0 → 6.5/6.7 MiB，
  七次最大值 3.1 → 6.7/7.0 MiB），但未在最小队列 + 强制乱序下施加长流压力；
- **application 层集成**：产品 plan 恒为 serial，本扫描通过 harness 的显式
  allocation 驱动 `codecs`，未经过 `Application` 的实际启用路径。

两条 track 的对照缩小了此前“真实音乐素材代表性”的疑问：加速比在压缩率的两端
基本一致，因此该结论不依赖语料恰好落在 escape 路径。但两者都是合成信号，没有
覆盖真实录音的动态、立体声相关性与 packet 长度分布。

## 边界

本记录是固定 source / binary / corpus / 本机环境的证据。它不是跨机器可复现构建
声明、CI 阈值、任意音频或存储条件的吞吐保证，也不是文件级或窗口级并行的预授权。
