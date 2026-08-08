# ADR-0014：decode-analysis hand-off 深度双主机记录

- 状态：Accepted；支持产品 hand-off depth 从 1 调整为 16
- 日期：2026-08-08
- 方法：ADR-0007 source-bound 深度 sweep 与同轮完全交错 A/B
- corpus：`m6-performance-baseline-v1`
- corpus manifest SHA-256：
  `bf97e2521213d2a47d6a0315f3d873f34e9f2d7abae471ba20e44784175aa8d1`
- 前置决策：[ADR-0014](../adr/0014-deterministic-decode-analysis-pipeline.md)

## 结论

产品 hand-off depth 保持 16。这个选择由双主机同轮 depth-one/depth-sixteen A/B
支持，不依赖“深度越大越快”的假设：先行 1/2/4/8/16/32/64 sweep 的原始中位数并不
单调。Windows 的主要收益在 16 前已经取得，16/32/64 的离散区间重叠；macOS 网格
更平、更嘈杂。32 与 64 没有建立额外的跨主机收益，却分别保留 33 与 65 个块，而
depth 16 保留 17 个。

同轮 A/B 中，长 WAV 在 macOS 快 1.04–1.05x、Windows 快 1.14–1.15x；长 AIFF
在 macOS 快 1.01–1.02x、Windows 快 1.08–1.09x。三 lane 纯 WAV batch 分别快
1.17x 与 1.10x。机制上不应变化的 14 个跨主机对照全部落在 1.33 倍合并 MAD 内。

这些都是各自绑定主机的本地记录，不把两台机器的绝对耗时互相比较，也不建立用户
吞吐承诺。

## 记录身份

深度 sweep 的两台主机都绑定 clean source
`0583a33b1383d7b7987bc7641eae268c4e539d4a`，每台 14 个 case，各 warmup 1 次、
measured 9 次，共 126 个正式样本，`outliersRemoved = 0`。runner SHA-256 为
`0dbea8958e703af6d4b08552c0f6a0dc93543798ac63585b3cbf2c87a0d4e3a5`。

| 主机 | suite SHA-256 | worker SHA-256 | raw record |
| --- | --- | --- | --- |
| Apple M4 Pro / macOS arm64 | `e859544eb2932a92833f50a8a80515529dda24bf82b68e0d9a2c14a414ba63c6` | `fda1cad51683f943b84fcc74b783a8b85c860c7c683fc026080b2a6d576f0763` | [`adr0014-overlap-depth-v1-0583a33-aarch64-apple-darwin.json`](baselines/adr0014-overlap-depth-v1-0583a33-aarch64-apple-darwin.json) (`50c1505106d29c537484e17c847488505e8a50df90b0e4a464aad062f956092f`) |
| Intel64 Family 6 Model 141 / Windows x86_64 | `90616601684b2b288a40da58f16c2e2d4fd5d879abc59641edb1c08acbeef461` | `972658bbfc180b34ed386da3df99a36019c75d4884309a5d733122a632b68cdc` | [`adr0014-overlap-depth-v1-0583a33-x86_64-pc-windows-msvc.json`](baselines/adr0014-overlap-depth-v1-0583a33-x86_64-pc-windows-msvc.json) (`834c3919227c68edf7f129067f7aaee1783c62622d7231f2434f42cb38669ced`) |

A/B harness 绑定 clean source
`5e017e4f1d3c322a62ab4e315a546e613e1aec34`；baseline binary 来自 `0583a33...`，
candidate binary 来自 `4ffc4cb9a5c05e0daae5b91fbab863b648419bd5`。每台 16 个 case、两个
variant，各 warmup 1 次、measured 9 次，共 288 个正式样本，`outliersRemoved = 0`。
runner SHA-256 为
`f8381022c96dcec246ea582a1a5bdaea369a0af6e31c2c01e8d7c9de4e93cb6e`。

| 主机 | suite SHA-256 | baseline / candidate worker SHA-256 | raw record |
| --- | --- | --- | --- |
| Apple M4 Pro / macOS arm64 | `d3e6d70697ffa8790f26c55070ebc26f044b8f8f41438a8095f080c66b38c40b` | `fda1cad51683f943b84fcc74b783a8b85c860c7c683fc026080b2a6d576f0763` / `f9fa588df91c2a619351f09fea0b648e895efab2138f7ed71e3c120fad7fd715` | [`adr0014-overlap-depth16-ab-v1-4ffc4cb-aarch64-apple-darwin.json`](comparisons/adr0014-overlap-depth16-ab-v1-4ffc4cb-aarch64-apple-darwin.json) (`eb282d21e0ff55c7d728b80ecd15348627c2ef8232c251a6aaf72b199987caea`) |
| Intel64 Family 6 Model 141 / Windows x86_64 | `8bb02e3d3cf092616a56636820d93af926b03552cde123ead8e5571575e233e6` | `972658bbfc180b34ed386da3df99a36019c75d4884309a5d733122a632b68cdc` / `2221e8fe1a94b76471eb47b897cd808fbaecbc70e76152db2a3193cc10e3a4a9` | [`adr0014-overlap-depth16-ab-v1-4ffc4cb-x86_64-pc-windows-msvc.json`](comparisons/adr0014-overlap-depth16-ab-v1-4ffc4cb-x86_64-pc-windows-msvc.json) (`c1b4db771b26b75e2975d665fbfb8b51464396b2154089b3d3a89bfb30af5999`) |

## 深度 sweep

下表为 `workerElapsedNs` 中位数。每个 case 分析两次 240 秒 track；requested 与
实际 applied depth 在每个样本中一致。

| 主机 / track | d1 | d2 | d4 | d8 | d16 | d32 | d64 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| macOS / WAV | 125.906 ms | 122.729 ms | 125.279 ms | 125.700 ms | 125.268 ms | 125.424 ms | 121.365 ms |
| macOS / AIFF | 129.484 ms | 128.055 ms | 126.922 ms | 125.399 ms | 126.703 ms | 128.026 ms | 125.131 ms |
| Windows / WAV | 246.011 ms | 227.213 ms | 221.110 ms | 221.638 ms | 212.515 ms | 214.930 ms | 210.090 ms |
| Windows / AIFF | 248.887 ms | 228.557 ms | 231.318 ms | 227.095 ms | 226.064 ms | 225.809 ms | 226.421 ms |

median process-tree peak RSS 随保留深度的上界总体上升；下表保留决策附近与两个更深
端点，单位 MiB。

| 主机 / track | d1 | d16 | d32 | d64 |
| --- | ---: | ---: | ---: | ---: |
| macOS / WAV | 2.672 | 3.094 | 3.359 | 4.125 |
| macOS / AIFF | 2.688 | 3.109 | 3.359 | 4.141 |
| Windows / WAV | 5.582 | 5.766 | 6.039 | 6.688 |
| Windows / AIFF | 5.566 | 5.738 | 6.023 | 6.586 |

这些数值不支持旧表述中的单调曲线。它们只说明：Windows 的 d16 已进入与更深候选
重叠的区间；macOS 没有为 d32/d64 建立稳定的额外收益；更深候选仍确定增加允许保留
的块数与实际观测 RSS。

## depth 1 → 16 同轮 A/B

下表为 graduation target 的 `workerElapsedNs` 中位数。加速为 baseline/candidate；
“位移 / MAD”使用两 variant 的 median absolute deviation 之和作分母。

| 主机 / case | depth 1 | depth 16 | 加速 | 位移 / 合并 MAD |
| --- | ---: | ---: | ---: | ---: |
| macOS / WAV 240s w2 | 133.421 ms | 128.064 ms | 1.042x | 1.47x |
| macOS / WAV 240s w4 | 134.239 ms | 128.248 ms | 1.047x | 1.15x |
| macOS / WAV 240s w8 | 133.204 ms | 127.732 ms | 1.043x | 2.10x |
| macOS / AIFF 240s w2 | 128.704 ms | 127.138 ms | 1.012x | 0.35x |
| macOS / AIFF 240s w4 | 128.107 ms | 125.993 ms | 1.017x | 0.65x |
| macOS / AIFF 240s w8 | 129.503 ms | 126.965 ms | 1.020x | 0.90x |
| macOS / 8 WAV tracks, 3 lanes | 56.558 ms | 48.332 ms | 1.170x | 2.66x |
| Windows / WAV 240s w2 | 241.285 ms | 211.752 ms | 1.139x | 5.06x |
| Windows / WAV 240s w4 | 244.104 ms | 212.388 ms | 1.149x | 8.03x |
| Windows / WAV 240s w8 | 244.271 ms | 213.500 ms | 1.144x | 5.75x |
| Windows / AIFF 240s w2 | 243.381 ms | 225.045 ms | 1.081x | 5.49x |
| Windows / AIFF 240s w4 | 244.845 ms | 224.808 ms | 1.089x | 2.74x |
| Windows / AIFF 240s w8 | 245.061 ms | 227.471 ms | 1.077x | 1.74x |
| Windows / 8 WAV tracks, 3 lanes | 114.870 ms | 104.505 ms | 1.099x | 3.32x |

单 worker、ALAC/FLAC packet route 与 mixed batch 是机制对照：它们不启用这条
hand-off，或者主要时间不经过它。两台主机各 7 个对照的中位加速范围分别为
0.987–1.005x 与 0.991–1.026x，最大位移为 1.33 倍合并 MAD。

## 正确性、资源与退化边界

- 每个 A/B case 的 baseline/candidate summary 与全部 measured sample 只有一个
  `resultFingerprintSha256`；深度 sweep 的每条 track 跨全部深度同样只有一个结果
  fingerprint；
- runner 校验 requested/granted worker、实际 engine、overlap 选择、requested/applied
  depth、decoded block 数与末块几何，不能把 host clamp 或 serial fallback 标成目标 case；
- graduation target 的 median process-tree peak RSS 在 macOS 单文件最多增加
  0.438 MiB，在 Windows 最多增加 0.266 MiB；三 lane 纯 WAV batch 分别增加
  1.000 MiB 与 0.145 MiB；
- 记录只覆盖固定 corpus、suite、build、主机和电源环境，不外推到其他媒体组成或宿主。

更深 hand-off 仍受同一内存计划 fail closed。普通 8-worker plan 分成 3 条 file lane
时，每 lane 得到 2 个 decoder worker 与 8 MiB in-flight PCM。对 1,152-frame 最坏块
和稳定 64 声道上限：

```text
one block = 1,152 × 64 × sizeof(f64) = 589,824 bytes
depth 16 retention = (16 + 1) × 589,824 = 10,027,008 bytes
per-lane allowance = 2 × 4 MiB = 8,388,608 bytes
```

因此内部 decode-analysis overlap 必须保持串行，而不是超过 lane 的 grant；外层 3 条
file lane 不受影响。depth 1 的 1,179,648 bytes 在同一 grant 中能装入，但本记录没有
为 64 声道几何声明任何加速。生产回归测试固定了这条“旧深度可装入、产品深度拒绝并
串行”的资源边界。

探索性的多块合批候选没有形成符合 ADR-0007/0014 的已提交双主机原始记录，因此本报告
不引用它的未验证精确数字，也不把 cache 驻留解释提升为已证明原因。产品继续保持每条
hand-off 消息恰好一个 PCM block。
