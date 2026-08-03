# ADR-0014：ALAC packet worker 同轮 worker-count 扫描

- 状态：Measured；packet workers 仍未默认启用
- 日期：2026-08-03
- 方法：ADR-0007 / 19-case ADR-0014 ALAC packet-worker sweep
- suite：runner-recorded id `m6-scalar-baseline-v1`；本次 19-case definition
  SHA-256 `a61d8484242e8e8634ed9acdec266913ac2b181265899658e90d91454c02bf73`
- corpus：`m6-performance-baseline-v1`（新增长 ALAC track）
- source：`3793c79cb2040ea4aa24dc553953cc6975df3947`（clean）
- worker SHA-256：
  `ca5b069cc0f386e00fb3f859b71b722dfc0217761e1c9f5f0f63cee48be07262`
- corpus manifest SHA-256：
  `e79de11eebe19c3e8beb334dabb965d44dc80edd291069a09697d28ec9209c31`
- canonical raw record：
  [`adr0014-alac-packet-worker-sweep-v1-3793c79-aarch64-apple-darwin.json`](baselines/adr0014-alac-packet-worker-sweep-v1-3793c79-aarch64-apple-darwin.json)
- raw record SHA-256：
  `4089d0ec1321ee73fc738df5c29205266470af39a29859e6e2b6b3c0d5bc1913`
- supporting cross-check raw record：
  [`adr0014-alac-packet-worker-crosscheck-v1-3793c79-aarch64-apple-darwin.json`](baselines/adr0014-alac-packet-worker-crosscheck-v1-3793c79-aarch64-apple-darwin.json)
- supporting raw record SHA-256：
  `d3b90a1a8c7416870847831c067f7c3e195253abb88829ccebdbafc8ac47f3f8`
- 前置决策：
  [ADR-0014](../adr/0014-deterministic-decode-analysis-pipeline.md)

## 结论

ADR-0013 稳定 ALAC route 的有界 packet workers 在 240 秒真实长度输入上给出
明确、远超同轮噪声的解码加速，且 1/2/4/8 worker 的结果 fingerprint 完全相同。

这是一次**测量**，不是启用决定。ADR-0014 的默认启用还缺若干门槛，见“未满足的
毕业门槛”。产品 plan 目前仍恒为 serial，本报告不构成任何用户可见的性能承诺。

## 测量对象与协议

worker 数是一个 decode allocation，不是另一个 binary，因此它是 case 参数而非
ADR-0007 的 `--variant`。四个 worker 数的 case 与其余 15 个 case 在同一次 run 内
以固定 seed 完全交错（133 个 measured sample，warmup 独立 seed，
`outliersRemoved = 0`）。

每个 case 各自被要求复现 corpus 的 normalized interleaved `f64` oracle，因此这个
扫描本身就是差分，而不是四组互不相干的计时。verification 解码运行在与计时段
**相同**的 allocation 上，不回退串行；完整 PCM hash 仍在计时区之外。

runner 另行复算 harness allocation，并核对 worker 实际获得的 `decodeWorkers` /
`decodeQueueCapacity` / `decodeMaxInFlightPcmBytes`；这会捕获 runner 与 worker 两份
镜像之间的配置错位。两者都只是 crate-private application plan 的镜像，并不自动
检测未来 plan 单独改变或另一台主机的 `available_parallelism` 收缩。本次固定主机有
12 个逻辑核，请求的 1/2/4/8 worker 与 source `3793c79` 的 dormant plan 数值一致；
经 `Application` 的真实派生仍是未满足的毕业门槛。

## 输入

`stereo-s16-alac-240s.m4a`：240 秒 / 48 kHz / 立体声 / 16-bit，2813 个 ALAC
packet。由 ffmpeg 8.0.1 以 ADR-0013 为 native-alac-v1 固定的同一 bit-exact 形状
编码（`+bitexact`、`-map_metadata -1`、`-compression_level 2`、显式 stereo
layout），已验证连续两次生成产出逐字节相同的文件与 manifest。

语料信号是既有 M6 的确定性伪随机整数流，几乎不可压缩（ALAC 输出为原始 PCM 的
99.5%，与既有 FLAC track 的 97.5% 同性质）。它施加了完整的 ALAC 解码路径成本，
但不代表真实音乐素材的压缩特性。

## 同轮结果

`decode` scope，每 case warmup 1 次 + measured 7 次，单次迭代解码整条 240 秒
track。加速比相对同一次 run 内的 1-worker case。

| Workers | Median (ms) | MAD (ms) | Min / Max (ms) | Speedup | Audio s/s | Median peak RSS (MiB) | RSS delta |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 395.4 | 1.77 | 392.6 / 399.2 | 1.00x | 607 | 3.0 | +0% |
| 2 | 206.7 | 0.86 | 204.5 / 207.6 | 1.91x | 1161 | 3.9 | +29% |
| 4 | 111.1 | 0.98 | 109.9 / 113.9 | 3.56x | 2161 | 4.7 | +58% |
| 8 | 69.1 | 3.36 | 63.1 / 72.4 | 5.72x | 3474 | 6.3 | +113% |

样本 min–max span / median 为 1.7% / 1.5% / 3.6% / 13.5%。8 worker 的 span
明显更高，同时本机在 run 期间保持较高负载（load average 起 9.68、止 9.64，
12 逻辑核）。

RSS 列是 7 个 measured sample 各自 process-tree peak RSS 的**中位数**；七次中的
实际最大值分别为 3.0 / 3.9 / 5.1 / 6.7 MiB。`RSS delta` 同样按中位数计算。

**四个 worker 数的 `resultFingerprintSha256` 完全相同**（唯一值
`40f68d10cbe50bcc...`），即解码 stream 几何、帧数、块数与完整 interleaved `f64`
SHA-256 均不随 worker 数变化。

## 稳定性交叉检查

同一 binary 与语料、不同 seed 的独立 4-case run：

| Workers | Median (ms) | Speedup | Min–max span / median |
| ---: | ---: | ---: | ---: |
| 1 | 437.8 | 1.00x | 23.1% |
| 2 | 235.8 | 1.86x | 24.5% |
| 4 | 125.1 | 3.50x | 24.8% |
| 8 | 73.8 | 5.93x | 19.2% |

该 supporting record 使用 seed `1300231`，suite definition SHA-256 为
`3db6a171237eefeb7ee0e7a83efd30cdb4df6f07892e1e625c718062881843f3`；source、worker、
corpus、toolchain 与主机身份均与 canonical record 相同，完整 raw samples 已随本报告
提交。绝对时间整体更慢、离散度更大，但 fingerprint 相同，且四个配置的相对收益与
canonical run 处于同一数量级。这是一次独立复现，不足以证明任意负载下的稳定性，
也不把 5.72x 解释为一般下界或上界。

## 正确性

- 1/2/4/8 worker 的 decode result fingerprint 与 PCM SHA-256 完全相同；
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
  本次记录了各 worker 数的 median peak RSS（3.0 → 6.3 MiB；七次最大值
  3.0 → 6.7 MiB），但未在最小队列 + 强制乱序下施加长流压力；
- **真实音乐素材的代表性**：语料信号几乎不可压缩，未覆盖典型音乐的 ALAC
  压缩特性对解码成本的影响；
- **application 层集成**：产品 plan 恒为 serial，本扫描通过 harness 的显式
  allocation 驱动 `codecs`，未经过 `Application` 的实际启用路径。

## 边界

本记录是固定 source / binary / corpus / 本机环境的证据。它不是跨机器可复现构建
声明、CI 阈值、任意音频或存储条件的吞吐保证，也不是文件级或窗口级并行的预授权。
