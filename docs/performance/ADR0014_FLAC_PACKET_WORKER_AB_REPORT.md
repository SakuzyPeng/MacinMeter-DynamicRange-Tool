# ADR-0014：FLAC packet worker 同轮 worker 扫描与顺序底线归因

- 状态：Measured；FLAC packet workers 自 route 毕业提交起即为默认（见下述“启用
  时序”）
- 日期：2026-08-03
- 方法：ADR-0007 / 41-case baseline suite，FLAC 与 ALAC 的 worker 扫描在同一次
  run 内交错
- suite：runner-recorded id `m6-scalar-baseline-v1`；本次 41-case definition
  SHA-256 `3443af71e37e42e35432be231d9dc927de9cb10a4eae4f57e1f8b5f0e165516d`
- corpus：`m6-performance-baseline-v1`（新增三条长 FLAC track）
- source：`d43fca2c4ad9c4d7b5dcfa27be77de5991d120b8`（clean）
- worker SHA-256：
  `f69c7ff3de4862555a0e4687be9fa6e885218d0a235a57ffaf4f1206398fff33`
- runner SHA-256：
  `dcdd1bef3d0ea2c211c14e48bdf81a83a77369a80f3c88e555f869ab165e8e23`
- corpus generator SHA-256：
  `028635f86180f7087803b4f460b67a73073bfda9424d9ea1413934f7d21fa8bc`
- corpus manifest SHA-256：
  `8a94f371357da05215fcf2487313622eb2ece7a66cf1b8c3b255d387aa5ad9eb`
- canonical raw record：
  [`adr0014-flac-packet-worker-sweep-v1-d43fca2-x86_64-pc-windows-msvc.json`](baselines/adr0014-flac-packet-worker-sweep-v1-d43fca2-x86_64-pc-windows-msvc.json)
- raw record SHA-256：
  `0b3b23a718ec5d5dac9d49edbcde455582ba7bfa935e5f47257d066bf1ea0b75`
- 顺序底线探针记录：
  [`adr0014-sequential-floor-v1-d43fca2-x86_64-pc-windows-msvc.json`](probes/adr0014-sequential-floor-v1-d43fca2-x86_64-pc-windows-msvc.json)
- 探针记录 SHA-256：
  `be8d32be342e7054a8346de24c06f88e12286f0316d23e9f55c088b0233d3fcb`
- 前置决策：
  [ADR-0014](../adr/0014-deterministic-decode-analysis-pipeline.md)

## 结论

FLAC 的有界 packet workers 在 240 秒输入上给出明确加速：8 worker 上 16-bit
4.13x、24-bit 2.77–3.42x。每条 track 的 1/2/4/8 worker 共享同一个结果
fingerprint。

FLAC 的加速比明显低于同一次 run 内的 ALAC（4.07–4.31x），且随位深下降。原因由
一次独立的直接测量给出，而非从加速比反推：FLAC 的**顺序底线**（顺序 demux 加
产品自有的流签名哈希）占其串行解码时间的 22.5–28.9%，而 ALAC 只有 1.7–2.8%。
底线中哈希是较大的一半（64.3–95.5 ms），demux 较小（45.6–62.5 ms）。

本报告是一次**测量**。它不建立跨主机可比性，也不构成任何用户可见的性能承诺。

## 为什么记录在 Windows 主机上

本记录的固定主机是 Windows 11（26200）/ x86_64 / Intel i7-11800H，与既有 M6 与
ALAC 记录的 macOS arm64 主机不同。ADR-0007 不允许跨主机比较，因此本记录中的
FLAC 数字只与**同一次 run 内**的 ALAC 数字比较，不与 ALAC 报告中的 macOS 数字
比较。

选择另一台主机是因为 macOS 主机在测量窗口内持续负载 11–15（12 逻辑核）。在该
负载下完成过一次完整 run，但其 ALAC track 只有 4.90 / 5.07 / 4.56x，而同一 corpus、
同一 case 在干净 macOS run 中为 5.95 / 5.93 / 6.26x。ALAC track 在本 suite 中因此
同时充当**污染对照**：同输入同代码路径掉了约 20%，说明该 run 不可用，已整体
作废，未纳入本记录。本记录的 Windows run 全程 CPU 非空闲占比 12.3% → 6.4%。

跨平台运行本身暴露了三个与测量无关的缺陷，均已单独修复：`core.autocrlf` 会改写
把自身字节哈希进 corpus 的 generator 脚本、runner 的原生指标只支持
macOS/Linux、`os.getloadavg()` 是 POSIX 概念。

## 启用时序

ALAC 的默认启用是在共同门槛齐备后单独作出并记录的决定。FLAC 不是：产品的
`ExecutionBudget::product()` 在此之前已经是非串行的，因此 FLAC 一旦被加入 route
判定就立即成为默认，正式 A/B 在其后才产生。这与 ADR-0014“默认启用本身是一个独立
决定”的表述不一致，如实记录于此。

本记录事后支持该默认：加速比明确、六条 track 的 fingerprint 均不随 worker 数变化，
因此不提出回退。但顺序不是 ADR 所设的顺序。

## 测量对象与协议

worker 数是 decode allocation 的维度而非另一个 binary，因此它是 case 参数而非
ADR-0007 的 `--variant`。12 个 FLAC case 与 14 个 ALAC case（含两个 permit case）及其余 15 个 case 在
同一次 run 内以固定 seed 完全交错（287 个 measured sample，warmup 独立 seed，
`outliersRemoved = 0`）。

每个 case 各自被要求复现 corpus 的 normalized interleaved `f64` oracle，因此这个
扫描本身就是差分。verification 解码运行在与计时段**相同**的 allocation 上；完整
PCM hash 在计时区之外。

原生指标由 Win32 `GetProcessTimes` 与 `GetProcessMemoryInfo` 取得，句柄贯穿子进程
生命周期；`maxResidentSetBytes` 在该平台上是 `PeakWorkingSetSize`，与 POSIX 主机的
同名字段不是同一个量。

## 输入

三条 FLAC track 长度、采样率与声道完全相同（240 秒 / 48 kHz / 立体声 / 2813 个
FLAC frame），由 libFLAC 命令行以 `--compression-level-5 --no-padding
--no-seektable` 编码。它们沿两条轴分开：位深决定签名哈希覆盖多少字节，可压缩性
决定解码需要多少工作。

| Track | 位深 | 信号 | 压缩率 | 大小 |
| --- | ---: | --- | ---: | ---: |
| `stereo-s24-flac-240s.flac` | 24 | 确定性伪随机整数流 | 90.2% | 59.4 MiB |
| `stereo-s24-flac-tonal-240s.flac` | 24 | 整数三角波叠加加 16-bit 量级 dither | 59.5% | 39.2 MiB |
| `stereo-s16-flac-240s.flac` | 16 | 与 ALAC 伪随机 track 相同的信号 | 97.5% | 42.8 MiB |

两条 24-bit track 构成一组**受控对照**：帧数、声道与位深完全相同，因此签名哈希
覆盖的字节数与每帧 demux 开销都相同，只有压缩体积与残差复杂度不同。

16-bit track 与 `stereo-s16-alac-240s.m4a` 由同一个信号编码，因此二者的解码
`f64` 应当逐位相同——本次 run 中它们确实共享同一个 `resultFingerprintSha256`
（`40f68d10cbe5…`），这是两条独立 route 之间的一次交叉核对。

## 同轮结果

`decode` scope，每 case warmup 1 次 + measured 7 次，单次迭代解码整条 track。
加速比相对同一次 run、同一条 track 的 1-worker case。

| Track | 1 worker | 2 | 4 | 8 |
| --- | ---: | ---: | ---: | ---: |
| FLAC 16-bit | 488.9 ms | 290.6（1.68x） | 178.6（2.74x） | 118.4（**4.13x**） |
| FLAC 24-bit tonal | 496.3 ms | 285.2（1.74x） | 197.9（2.51x） | 144.9（**3.42x**） |
| FLAC 24-bit 伪随机 | 556.5 ms | 319.6（1.74x） | 251.6（2.21x） | 200.9（**2.77x**） |
| ALAC 伪随机 | 821.2 ms | 499.7（1.64x） | 312.0（2.63x） | 201.8（4.07x） |
| ALAC tonal | 820.9 ms | 483.4（1.70x） | 308.8（2.66x） | 190.4（4.31x） |
| ALAC varied | 756.0 ms | 405.9（1.86x） | 241.6（3.13x） | 176.3（4.29x） |

median peak RSS 由 5.2–5.5 MiB 升至 7.6–8.8 MiB。每条 track 的四个 worker 数各自
共享唯一的 `resultFingerprintSha256`。ALAC 的 reorder permit 维度在本次 run 中
重复扫描：8 / plan 派生 / 64 为 219.1 / 190.4 / 189.3 ms，与 macOS 记录的结论方向
一致（最小 permit 强制 rendezvous 因而更慢，plan 派生已在收益拐点之后）。

## 顺序底线的直接测量

ADR-0014 的 packet pool 保持 demux 顺序，只并行解码；FLAC 另有一项产品自有的
流签名哈希，固定在唯一的按序 commit 点上。两者都无法进入 worker，因此共同构成
加速比的硬上限。

加速比本身说明不了这个上限由什么组成，所以它由一个独立探针直接测量：

```bash
cargo run --locked --release -p macinmeter-codecs \
  --example demux_cost_probe -- PATH
```

探针把一条 track 的全部 packet 抽取两遍，一遍丢弃、一遍解码，两遍的 demux 工作
完全相同，因此相减即得纯解码成本。签名哈希另行以同一个 `Md5` 实现在等量字节上
计时——MD5 吞吐与数据无关，因此零缓冲区测得的成本与真实签名字节相同。两个相位
在每一轮内交替，取 7 轮中位数。

| Track | 顺序 demux | 签名哈希 | 顺序底线 | 占 1-worker |
| --- | ---: | ---: | ---: | ---: |
| FLAC 24-bit 伪随机 | 62.5 ms | 95.5 ms | 158.1 ms | **28.4%** |
| FLAC 24-bit tonal | 48.1 ms | 95.3 ms | 143.5 ms | **28.9%** |
| FLAC 16-bit | 45.6 ms | 64.3 ms | 109.9 ms | **22.5%** |
| ALAC 伪随机 | 23.3 ms | 无 | 23.3 ms | 2.8% |
| ALAC tonal | 14.0 ms | 无 | 14.0 ms | 1.7% |
| ALAC varied | 18.4 ms | 无 | 18.4 ms | 2.4% |

三项由此固定：

1. **哈希是底线中较大的一半**，且严格随位深缩放：16-bit 覆盖 46.1 MB、24-bit
   覆盖 69.1 MB，哈希时间 64.3 与 95.3–95.5 ms，比值与字节数比值一致
   （本机 MD5 吞吐约 700 MB/s）。这解释了 FLAC 加速比随位深下降。
2. **FLAC 的顺序 demux 约为 ALAC 的两倍**：压缩体积相近（42.8 与 43.7 MiB）、
   packet 数相同（2813）时为 45.6 与 23.3 ms。FLAC demux 需解析帧头并对压缩
   字节计算 CRC-16，MP4 只做 sample table 查表。这一项真实但次要。
3. **ALAC 没有签名哈希**，其底线仅为 demux。探针对 ALAC 报告 `null` 而非零字节，
   因为“该格式没有流签名”与“该 track 缺少某个字段”不是同一个陈述。

## Amdahl 反推的“串行占比”是上界，不是测量值

用 1-worker 与 8-worker 的比值反解 Amdahl 得到的串行占比，对本记录的六条 track
分别是 27.0% / 19.1% / 13.4%（FLAC）与 12.2–13.8%（ALAC）。这些数**不是**顺序段的
测量值：它们把内存带宽、分配器争抢、通道交接等一切扩展不完美一并计入“串行”。

两条 24-bit track 的对照直接显示了差距：它们的哈希工作量完全相同，实测底线只差
14.6 ms，而 Amdahl 反推的串行差 55.3 ms。用反推值做归因会把这 55.3 ms 全部记到顺序
工作上，并据此得出“顺序成本随压缩体积走”的结论——该结论在本记录的直接测量下
不成立。

反过来，实测底线换算出的 Amdahl 上限对 ALAC 也是**高估**的：2.8% 的底线对应
6.67x，实测只有 4.07x；其 Amdahl 反推串行占比反而高达 13.8%，是实测底线的近五倍。
本次固定主机为 8 物理核 / 16 逻辑核，而 8-worker 配置下
demux 线程、8 个解码线程与 commit 线程同时活跃。超出底线的那部分限制没有被本
记录测量，不作归因。

因此本记录只主张：**顺序底线是加速比的上界，并且它解释了 FLAC 与 ALAC 之间以及
FLAC 各位深之间的差距**；不主张它是唯一的限制项。

## 一次被作废的归因尝试

曾尝试用数据而非代码隔离哈希：把 STREAMINFO 的 MD5 字段清零，使产品跳过校验，
再与原文件比较。该设计是错的，已作废。签名缺席会使 `backend_verification()`
返回 true，于是 Symphonia 自身的 validator 在**每个 worker 内**重新开启并并行
运行——这不是“去掉哈希”，而是“把哈希移回并行侧”。该实验在 16-bit track 上给出
“无签名反而慢 17.5 ms”的结果，正是这一混淆的表现；交错重跑后该值依然为负，说明
它不是取样噪声。任何数据层面的开关都无法隔离这一项，因此改用与产品路径无关的
等量字节计时。

## 正确性

- 每条 track 内 1/2/4/8 worker 的 decode result fingerprint 与 PCM SHA-256 完全相同；
- 16-bit FLAC track 与由同一信号编码的 ALAC track 共享 `resultFingerprintSha256`，
  即两条独立 route 产出逐位相同的 `f64`；
- 每个 case 独立匹配 corpus 的 normalized interleaved `f64` oracle；
- 每个 case 的 7 个 measured sample 之间 fingerprint 稳定；
- 产品侧另有 FLAC 的 raw-bit 等价、强制最坏乱序下签名仍通过、以及篡改签名时
  2/4/8 worker 与串行 oracle 给出逐字节相同 digest 与错误的测试，见 ADR-0014
  第 3 步记录。

## 范围

本记录不建立：

- 跨主机可比性——其中的 FLAC 数字只与同一次 run 内的 ALAC 数字比较；
- 顺序底线之外限制因素的归因；
- 任何用户可见的吞吐承诺，或把 elapsed time / RSS 变成 CI 阈值的依据；
- `Application` 路径的派生——本记录驱动的是 `codecs` 的显式 allocation。
