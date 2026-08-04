# ADR-0014：packet pipeline 底线之上归因与 A/B

- 状态：Measured；两项候选已接受
- 日期：2026-08-04
- 方法：ADR-0007 source-bound 探针、同轮普通 decode 对照、完全交错 A/B
- 固定主机：Windows 11 build 26200 / x86_64 / Intel i7-11800H，16 logical CPU
- corpus：`m6-performance-baseline-v1`，19 个文件；manifest SHA-256
  `8a94f371357da05215fcf2487313622eb2ece7a66cf1b8c3b255d387aa5ad9eb`
- generator SHA-256：
  `028635f86180f7087803b4f460b67a73073bfda9424d9ea1413934f7d21fa8bc`
- 前置决策：[ADR-0014](../adr/0014-deterministic-decode-analysis-pipeline.md)

## 结论

ADR-0014 留下的“顺序底线之上还有什么”开放项已经在 pipeline component 层完成直接
测量与归因。原先只测到的顺序 demux/FLAC hash 不是唯一限制；其上至少有四类可观察
成本：

1. ALAC 打开阶段曾有 26–28 ms 的 ISO BMFF sample-size table 检查，旧底线没有把它
   算入；
2. 并发时各 decoder 的 aggregate active work 会膨胀，最明显的第一方原因是 PCM
   转换先填 `SampleBuffer<f64>`、再复制成 domain `Vec<f64>`；
3. 固定 `index % workers` 在 varied ALAC 上形成可测的 slot 不均衡和 demux dispatch
   back-pressure；
4. 调用线程的 result wait / ordered commit、FLAC hasher hand-off 与有界队列等待都在
   实际 critical path 周围发生。

其中两项可以在不改变拓扑、顺序或资源计划的前提下直接消除，现已接受：

- `stsz` 表由逐 4-byte seek/read 改成最大 64 KiB 的顺序分块检查；
- decoded PCM 直接写入最终 `Vec<f64>`，不再为同一 block 建立并复制第二份完整
  `f64` buffer。

第一项使 ALAC container inspection 从约 26–28 ms 降到 0.79–0.81 ms，8-worker
端到端改善 11.6–13.3%。第二项使 12 个深探针中的 PCM conversion aggregate time
下降 35–62%；24 个普通 decode case 的宽 A/B 中 22 个先得到改善，另外两个噪声点
经独立 21-sample 复核也分别改善 6.0% 与 1.7%。全部结果/PCM fingerprint 保持精确
一致，RSS 没有实质增长。

这关闭的是“未测量、未归因”，不是宣称理想线性扩展已经实现。Symphonia backend
active work 的并发膨胀已经定位到 decoder backend，但 cache、频率或共享资源中的哪一
项是其微架构主因，本记录没有继续拆分；varied track 的静态映射不均和队列交接也仍是
已测量但未改调度的后续候选。

## 原始记录

所有 JSON 保留 Windows runner 写出的原始字节；`.gitattributes` 对性能 JSON 使用
`-text`，下列 SHA-256 因此绑定实际归档文件，而不是换行规范化后的内容。

- 首层 open/drain 归因，clean source `75e52f89abcc1db3912e0f30624fe7f691095c87`，
  336 measured samples / 48 summaries：
  [`adr0014-packet-pipeline-attribution-v2-75e52f8-x86_64-pc-windows-msvc.json`](probes/adr0014-packet-pipeline-attribution-v2-75e52f8-x86_64-pc-windows-msvc.json)，
  SHA-256 `8b3bcfbaed789d953503814bd9fedb12349e7f056227085565c77d2cfb4e74e6`；
- source-owned 内部 phase 归因，clean source
  `cfba0d3349610cddf62949dbef11c725104ace86`，336 samples / 48 summaries：
  [`adr0014-packet-pipeline-internals-v1-cfba0d3-x86_64-pc-windows-msvc.json`](probes/adr0014-packet-pipeline-internals-v1-cfba0d3-x86_64-pc-windows-msvc.json)，
  SHA-256 `ebfb5e1001a49017d3644f6943b4a60eb786f4c2f8c4b7b53652500dda28bc98`；
- `stsz` broad A/B，baseline `cfba0d3`、candidate
  `b404adaaba446ecd31c081960e45c0d011b7b95f`，280 samples / 40 summaries：
  [`adr0014-alac-stsz-broad-ab-v1-b404ada-x86_64-pc-windows-msvc.json`](comparisons/adr0014-alac-stsz-broad-ab-v1-b404ada-x86_64-pc-windows-msvc.json)，
  SHA-256 `0c7dc6aaebe214f373a435a8ca0449ccaace36fbe4cd9cedcae86e8da6feb933`；
- direct PCM broad A/B，baseline `b404ada`、candidate
  `446c782588cb79add9864040be9158b169ea9c55`，504 samples / 72 summaries：
  [`adr0014-direct-pcm-broad-ab-v1-446c782-x86_64-pc-windows-msvc.json`](comparisons/adr0014-direct-pcm-broad-ab-v1-446c782-x86_64-pc-windows-msvc.json)，
  SHA-256 `b746dfc04199c65d33a94cfe591ec990d5e4a8b6b2b8d7199c4c906610fd0072`；
- varied ALAC 独立确认，仍为 `b404ada` / `446c782`，21 samples per case/variant，
  126 samples / 6 summaries：
  [`adr0014-direct-pcm-varied-confirm-v1-446c782-x86_64-pc-windows-msvc.json`](comparisons/adr0014-direct-pcm-varied-confirm-v1-446c782-x86_64-pc-windows-msvc.json)，
  SHA-256 `56f42fcc92875c512b6e575310514ea4188b9b34bd322e3868cf364d86a33838`。

后四份记录使用同一个 runner SHA-256
`b9a077f2922e43e4746b51cb7704871aab5a6fb7866cd755706d01cc85f0697c`；首层记录对应
runner SHA-256 `7221d9927201883ec1735c91da6487b6bcfb467f9e5570cb2b74ffcfe57c226e`。

## 探针边界与检测能力

`performance-probes` 是非默认 feature。默认 production build 不含 probe storage、
atomic 更新或逐 packet 计时；显式 probe build 才允许 `decode-pipeline` mode。计时点由
拥有实际工作的 source/thread 更新，而不是 runner 从进程外猜测：

- open：content identify、container inspection、backend probe 与 route setup；
- demux：packet read 与向固定 worker inbox 派发时的阻塞；
- 每个 decoder slot：inbox wait、backend decode、integrity conversion、PCM
  conversion、result send wait 与 thread lifetime；
- 调用线程：result wait、按序 commit、finish 与其余 drain 时间；
- FLAC hasher：receive wait、active digest、sender wait 与 lifetime；
- reorder：packet/byte 高水位与 stall 次数。

这些是会重叠的 wall intervals，不是可以横向相加成 elapsed 的 CPU partition。报告把
它们用于回答“时间在哪个 owner/phase 中发生”，不会把所有 worker time 误称为串行
时间。

每个深探针 case 另做一次不计时的完整 verification decode；完整 interleaved `f64`
SHA-256 在被观察 source 之外计算，避免 hash 本身反压 pipeline。runner 对 topology、
packet 总数、每个 slot、open/caller accounting、hasher packet 数、reorder permit 与
PCM oracle fail closed。`cfba0d3` 记录中深探针与同轮普通 decode 的中位数差落在
-3.54% 到 +3.06%，没有系统性 probe penalty。

## 归因结果

### 打开阶段此前漏掉了一段 ALAC 固定成本

首层记录把完整 decode 拆成 open 与 drain。三条 ALAC 的 open 中位数为
26.9–27.9 ms；它在 1-worker elapsed 中占约 3.2–3.6%，到 8-worker 已占
13.5–15.6%。FLAC open 只有约 0.3 ms。

旧 sequential-floor probe 只测“无解码 demux”和 FLAC 等量 hash，没有包含产品打开
时的 ISO BMFF validation。因此“2.8% 顺序底线对应 6.67x”从一开始就少算了这一段，
不是完整产品串行占比。

首层 phase mode 与普通 decode 的 24 组对照中 23 组落在 -1.9% 到 +2.5%；唯一
varied/w4 点为 +6.3%，后续更细的 source-owned probe 没有复现系统性开销。

### decoder aggregate work 会随并发膨胀

下表来自 `cfba0d3` 深探针。`Active` 是所有 decoder slot 的 backend + integrity +
PCM wall time 总和；8-worker 的 slot 同时运行，因此它不是 critical-path elapsed。
它的意义是直接显示“完成相同 packet 集合需要多少 aggregate active work”。FLAC 的
8 total permit 在该实现中为 7 decoder + 1 hasher。

| Track | Active w1 → w8 | 膨胀 | Backend 膨胀 | PCM 膨胀 | w8 slot 平均 / 最慢 |
| --- | ---: | ---: | ---: | ---: | ---: |
| ALAC 16-bit | 727.1 → 1108.4 ms | 1.52x | 1.42x | 2.07x | 138.5 / 150.5 ms |
| ALAC tonal | 713.1 → 1086.5 ms | 1.52x | 1.42x | 2.04x | 135.8 / 142.7 ms |
| ALAC varied | 678.8 → 910.1 ms | 1.34x | 1.29x | 1.59x | 113.8 / 142.9 ms |
| FLAC 16-bit | 332.1 → 444.8 ms | 1.34x | 1.18x | 1.77x | 63.5 / 65.5 ms |
| FLAC 24-bit | 333.9 → 486.3 ms | 1.46x | 1.14x | 1.83x | 69.5 / 76.1 ms |
| FLAC 24-bit tonal | 303.1 → 475.9 ms | 1.57x | 1.23x | 1.95x | 68.0 / 70.9 ms |

PCM 是唯一在全部六条 track 上都接近或超过 1.6x 的第一方 phase。代码检查随后找到
直接原因：Symphonia `SampleBuffer<f64>` 先分配并填满一个 boxed slice，调用点又以
`samples().to_vec()` 分配最终 domain buffer 并复制整块。并行 worker 同时进行两次
完整 buffer allocation/write，会放大 allocator 与 memory traffic。

Backend 本身仍有 1.14–1.42x aggregate inflation。这已经归因到 decoder backend
owner，而不是顺序底线；探针不区分 CPU frequency、cache 或其他共享硬件资源，所以
不进一步声称某个微架构原因已经被证明。

### 静态映射和 hand-off 是独立限制

varied ALAC 的 w8 slot 平均 active time 为 113.8 ms，最慢 slot 为 142.9 ms，差
29.2 ms / 1.26x；另外两条 ALAC 的最慢-slot penalty 只有约 6.9–11.9 ms。
同一 varied case 的 demux dispatch wait 为 65.3 ms，而另外两条 ALAC 只有
5.4–6.1 ms。这直接把此前只凭 workload 名称推测的“静态映射不均”变成了测量值。

其余 w8 hand-off 也有明确 owner：

| Track | Demux read | Dispatch wait | Caller result wait | Caller commit | Hasher active / send wait |
| --- | ---: | ---: | ---: | ---: | ---: |
| ALAC 16-bit | 36.5 ms | 6.1 ms | 134.6 ms | 0.1 ms | 无 |
| ALAC tonal | 24.7 ms | 5.4 ms | 121.5 ms | 0.1 ms | 无 |
| ALAC varied | 36.0 ms | 65.3 ms | 116.9 ms | 0.1 ms | 无 |
| FLAC 16-bit | 59.7 ms | 5.4 ms | 83.0 ms | 20.6 ms | 76.2 / 20.4 ms |
| FLAC 24-bit | 115.1 ms | 5.5 ms | 139.8 ms | 18.2 ms | 113.7 / 17.9 ms |
| FLAC 24-bit tonal | 65.3 ms | 5.6 ms | 35.1 ms | 77.5 ms | 110.3 / 77.2 ms |

`result wait` 大部分与 worker active work 重叠，不能再作为一段“串行比例”相加；但
这些值说明 critical caller 正在等哪个边界。FLAC tonal 的 commit/hasher sender wait
几乎相同，也直接显示容量 1 的有界 hash hand-off 在该 workload 上形成反压。

## 接受的优化一：分块检查 ALAC `stsz`

ALAC 的 2813-entry sample-size table 原先对每项执行一次 4-byte seek/read；合法性只
要求顺序检查非零值和保留准确的第一个失败索引，不需要随机访问。候选改为最多
64 KiB 的顺序 chunk，在 chunk 内仍按原索引解码，内存与 table 长度无关。

`cfba0d3` / `b404ada` 正式 A/B 的普通 decode 中位数如下；括号内为 candidate faster：

| Track | w1 | w2 | w4 | w8 |
| --- | ---: | ---: | ---: | ---: |
| ALAC 16-bit | 831.8 → 810.9 ms（2.5%） | 488.2 → 479.2（1.9%） | 309.0 → 288.9（6.5%） | 199.3 → 175.4（12.0%） |
| ALAC tonal | 790.3 → 778.1 ms（1.5%） | 467.4 → 445.5（4.7%） | 297.8 → 278.8（6.4%） | 185.7 → 164.2（11.6%） |
| ALAC varied | 766.3 → 748.4 ms（2.3%） | 404.6 → 383.9（5.1%） | 235.4 → 209.8（10.8%） | 179.7 → 155.7（13.3%） |

深探针中的 container inspection 为 25.99–27.50 ms → 0.79–0.81 ms；open 总计为
26.89–28.38 ms → 1.30–1.68 ms。FLAC negative control 的 w1/w8 为 +1.61% / -1.85%
（正数表示更快），没有 route 外的系统变化。所有跨变体 fingerprint 一致。

## 接受的优化二：直接构造最终 PCM `Vec<f64>`

`446c782` 保留 Symphonia 的十种 `IntoSample<f64>` 实现与同一 channel-major 到
frame-interleaved 循环，只把 destination 改成 domain 最终拥有、长度精确为
`frames × channels` 的一个 `Vec<f64>`。测试把 u8/u16/u24/u32、i8/i16/i24/i32、
f32/f64 全部与旧 `SampleBuffer<f64>::copy_interleaved_typed` 的逐 sample 位模式比较；
另用 `-0.0`、高精度 f64 与交错双声道单独固定“不先收窄 source f64”的契约。

宽 A/B 包含六条 track 的普通 1/2/4/8 decode 与 w1/w8 深探针，36 cases × 2 variants
× 7 samples = 504 measured samples。下表为 candidate faster；负数是候选较慢：

| Track | w1 | w2 | w4 | w8 |
| --- | ---: | ---: | ---: | ---: |
| ALAC 16-bit | 8.8% | 10.3% | 5.9% | 2.2% |
| ALAC tonal | 7.8% | 11.7% | 11.4% | 2.3% |
| ALAC varied | 8.7% | 8.6% | -1.4% | -2.3% |
| FLAC 16-bit | 10.6% | 19.7% | 17.9% | 18.0% |
| FLAC 24-bit | 8.2% | 12.5% | 12.8% | 16.3% |
| FLAC 24-bit tonal | 10.1% | 16.4% | 17.6% | 14.2% |

varied w4/w8 的 2.9/3.5 ms 表面差值都小于对应 MAD，且候选 CPU time 更低。独立
seed、21 samples per case/variant 的确认轮给出 w4 217.4 → 204.4 ms（快 6.0%）、
w8 163.2 → 160.4 ms（快 1.7%），因此宽轮的两个负值判为静态映射调度噪声，不是
可复现回归。确认轮的 pipeline w8 elapsed 差 1.2%、仍在 MAD 内，但 PCM aggregate
稳定从 202.9 降到 121.5 ms。

深探针对实现机制给出直接确认：

| Track | PCM reduction w1 / w8 | Pipeline elapsed faster w1 / w8 |
| --- | ---: | ---: |
| ALAC 16-bit | 46.4% / 46.9% | 8.4% / 1.2% |
| ALAC tonal | 45.9% / 44.6% | 8.0% / 2.0% |
| ALAC varied | 44.4% / 41.9% | 9.0% / 0.6% |
| FLAC 16-bit | 41.1% / 57.2% | 9.8% / 17.3% |
| FLAC 24-bit | 35.4% / 47.7% | 8.6% / 14.5% |
| FLAC 24-bit tonal | 39.5% / 62.3% | 11.0% / 14.4% |

24 个普通 case 的 median process-tree peak RSS 变化范围为 -4.47% 到 +0.71%，没有
为速度另付完整 PCM block 的驻留代价。36/36 case 的跨变体
`resultFingerprintSha256` 相同，decode case 还逐一匹配 corpus PCM oracle。

## 污染门禁

宽 A/B 的 Windows native CPU busy fraction 为 4.20% → 13.48%；变体与 case 由固定
seed 完全交错，因此起止点不同本身不偏向一个 binary。更强的门禁是同套件内三条
ALAC 1→8 sweep：

| Track | 已知干净值 | Baseline | Candidate |
| --- | ---: | ---: | ---: |
| ALAC 16-bit | 4.07x | 4.67x | 4.35x |
| ALAC tonal | 4.31x | 4.70x | 4.43x |
| ALAC varied | 4.29x | 4.86x | 4.33x |

没有一条 materially below 已知干净值，因此宽轮有效。独立确认轮的 CPU busy
fraction 为 10.21% → 6.08%，结果方向与宽轮一致。两轮 `outliersRemoved = 0`。

## 仍保留的、已经测量的限制

- decoder backend aggregate work 在 w8 仍比 w1 高 1.14–1.42x；这是 backend/shared
  hardware scaling 问题，不再是“未知串行底线”，但需要另一种 profile 才能区分
  frequency、cache 与 backend 内部 memory traffic；
- varied ALAC 的静态 mapping 最慢 slot 比平均多约 29 ms，dispatch wait 约 65 ms；
  改成动态领取会改变 packet-to-decoder mapping 与 fault/resource surface，须按
  ADR-0014 单独通过确定性、错误、取消、资源与 A/B 门槛，不能由本次归因顺带启用；
- FLAC 的顺序 demux、产品全流 MD5 与容量 1 hasher hand-off 仍是 route-specific
  限制；它们已经分别测量，且 MD5 不可删除；
- caller/result/reorder 等等待是重叠 interval。后续优化必须在 source owner 内继续
  测量，不能再用 Amdahl 反推值把所有非线性都写成“串行占比”。

文件级（P1）与窗口级（P2）并行仍未开始。本记录不授权新的 scheduler、动态派发、
SIMD、unsafe、第二 backend、放宽错误优先级或扩大 application worker/memory plan。
