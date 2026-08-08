# ADR-0014：file-lane 饱和宽度双主机记录

- 状态：Accepted；支持 P1 采用 plan 自行推导的饱和宽度
- 日期：2026-08-08
- 方法：ADR-0007 source-bound、单变体完全交错宽度 sweep
- source：`7ceb13d77d00bd6b5fbab5a3f1c5a6f268331620`，两台主机均为 clean
- corpus：`m6-performance-baseline-v1`
- corpus manifest SHA-256：
  `bf97e2521213d2a47d6a0315f3d873f34e9f2d7abae471ba20e44784175aa8d1`
- runner SHA-256：
  `fb3875e2981c4d47eeba206da19c83f76992ad5f9575a6e9cf2db4b312029fc8`
- macOS raw record：
  [`adr0014-lane-width-bound-v1-7ceb13d-aarch64-apple-darwin.json`](baselines/adr0014-lane-width-bound-v1-7ceb13d-aarch64-apple-darwin.json)
- macOS record SHA-256：
  `e22b99ca0243411095c5380d90ff74b5db8ad55147a3776f7004d6794aa501ad`
- Windows raw record：
  [`adr0014-lane-width-bound-v1-7ceb13d-x86_64-pc-windows-msvc.json`](baselines/adr0014-lane-width-bound-v1-7ceb13d-x86_64-pc-windows-msvc.json)
- Windows record SHA-256：
  `a691308bf35faf365ff18cd289228fc279436eb335af2f4915de36ba948f7ddb`
- 前置决策：[ADR-0014](../adr/0014-deterministic-decode-analysis-pipeline.md)

## 结论

P1 保持由 plan 自行推导的饱和宽度。固定 8-worker plan 推导出 3 条 lane；在
Apple M4 Pro 与 Intel i7-11800H 两台主机上，混合与 FLAC-only 组成的最优宽度都是
L3，WAV-only 的最优宽度都是 L8。三种组成的中位耗时总和与最差加速两条预先声明的
判据都在两台主机上选择 L3。

固定 L3 相对 WAV-only 自身的 L8 最优值慢 22.0% 与 38.4%；反过来固定 L8 相对
混合与 FLAC-only 自身的 L3 最优值慢 25.4%–31.7%。最大相对 regret 的 minimax
判据在 macOS 选择 L3、在 Windows 选择 L8，因此没有用它覆盖两条跨主机一致的判据。

这是两份各自绑定主机的本地记录，不把两台机器的绝对耗时互相比较，也不建立用户
吞吐承诺。

## 实际 allocation

每个样本都从 discovery 后的生产 `PlanAllocation` 记录请求 lane、实际 granted plan、
实际 lane 与每 lane decoder。两台主机的 15 个 case 都取得完整 8-worker plan，且
每个宽度逐字段一致：

| 请求宽度 | 实际 lane | 每 lane decoder | 解释 |
| ---: | ---: | ---: | --- |
| 1 | 1 | 8 | 单 lane 保留整个 decoder |
| 2 | 2 | 3 | 1 个 lane executor + 2×3 decoder |
| 3 | 3 | 2 | 2 个 lane executor + 3×2 decoder，恰好用尽 plan |
| 4 | 4 | 1 | 无法给每 lane 至少 2 decoder，整体退回串行 decoder |
| 8 | 8 | 1 | 每条 lane 使用串行 decoder |

runner 对这些字段 fail closed：宿主夹紧 worker 或 allocation 与 case 标签不一致时，
整次 run 在形成 summary 前失败。对应 Python 回归分别篡改请求宽度、granted plan、
实际 lane 与 decoder 宽度，四种篡改均被拒绝。

## 中位耗时

每台主机每个 case warmup 1 次、measured 9 次；15 个 case 完全交错，共 135 个正式
样本。下表为 `workerElapsedNs` 中位数，括号内为相对同组成 L1 的加速。

### Apple M4 Pro / macOS arm64

| 组成 | L1 | L2 | L3 | L4 | L8 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Mixed | 84.644 ms | 68.504 ms (1.236x) | **61.312 ms (1.381x)** | 96.317 ms (0.879x) | 80.759 ms (1.048x) |
| WAV-only | 77.295 ms | 42.138 ms (1.834x) | 31.918 ms (2.422x) | 31.987 ms (2.416x) | **26.170 ms (2.954x)** |
| FLAC-only | 92.085 ms | 83.385 ms (1.104x) | **61.584 ms (1.495x)** | 106.151 ms (0.867x) | 77.702 ms (1.185x) |

| 判据 | L1 | L2 | L3 | L4 | L8 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 三组成耗时总和 | 254.024 ms | 194.027 ms | **154.815 ms** | 234.455 ms | 184.631 ms |
| 最差组成加速 | 1.000x | 1.104x | **1.381x** | 0.867x | 1.048x |

### Intel i7-11800H / Windows x86_64

| 组成 | L1 | L2 | L3 | L4 | L8 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Mixed | 178.840 ms | 139.304 ms (1.284x) | **127.554 ms (1.402x)** | 186.609 ms (0.958x) | 162.299 ms (1.102x) |
| WAV-only | 173.195 ms | 95.498 ms (1.814x) | 75.493 ms (2.294x) | 68.956 ms (2.512x) | **54.530 ms (3.176x)** |
| FLAC-only | 171.743 ms | 137.728 ms (1.247x) | **115.665 ms (1.485x)** | 192.060 ms (0.894x) | 145.013 ms (1.184x) |

| 判据 | L1 | L2 | L3 | L4 | L8 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 三组成耗时总和 | 523.778 ms | 372.529 ms | **318.712 ms** | 447.625 ms | 361.843 ms |
| 最差组成加速 | 1.000x | 1.247x | **1.402x** | 0.894x | 1.102x |

## 正确性、资源与限制

- 同一组成跨全部宽度的 `resultFingerprintSha256` 唯一，每个 measured sample 都与
  该组成相同；work unit、文件数、frame/sample/audio-second 总量均匹配 corpus；
- 两台主机每种组成在全部宽度上都记录相同的实际 allocation，未发生 host clamp；
- median process-tree peak RSS 没有随 lane 数单调增长；该观察只排除本轮存在简单的
  lane-count 线性驻留关系，不是通用内存上限；
- L4/L8 的串行 per-lane decoder 是 allocation 断崖的真实执行结果，不是请求标签；
- 记录不覆盖其他 batch 组成、存储设备、宿主负载或超过固定产品 plan 的公开调参面。
