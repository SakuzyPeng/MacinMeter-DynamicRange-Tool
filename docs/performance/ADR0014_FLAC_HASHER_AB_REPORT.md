# ADR-0014：FLAC commit/analysis overlap 专用 hasher A/B

- 状态：Accepted；仅在有签名且总 allocation 为 8 permit 时启用
- 日期：2026-08-04
- 方法：ADR-0007 source-bound、同总 permit、双变体完全交错 A/B
- 固定主机：Windows 11 build 26200 / x86_64 / Intel i7-11800H，16 logical CPU
- baseline source：`7d5bd2fb100a454e38cba033a3b8755113ca5a40`
- baseline worker SHA-256：
  `b4a41fe84c732d807d6d7920c7f3abdbd7d62e24cf314ab066f8610cb0065abe`
- broad candidate source：`f23baf62b713a12892405a4e21151a34650de85b`
- broad candidate worker SHA-256：
  `01dc42e695e769374928c2a5bcffecf38b50926891268736b0b74329deb413da`
- gated candidate source：`7e7ad7f66074a35b6b1b9f63adf531007fa46a71`
- gated candidate worker SHA-256：
  `3d99c045672f107ae30be1f7c1247f231189574294a0472c8f3f7eb60c2387d1`
- selected-suite SHA-256：
  `c0be3bbeedc7a9b1ec8521e8453881f67dec720b02c82d527fccc9c3b33164f4`
- runner SHA-256：
  `f1c2339e47514fde05a8777733c48eff1f500f1401ebe55e849b4a990ca08588`
- corpus generator SHA-256：
  `028635f86180f7087803b4f460b67a73073bfda9424d9ea1413934f7d21fa8bc`
- corpus manifest SHA-256：
  `8a94f371357da05215fcf2487313622eb2ece7a66cf1b8c3b255d387aa5ad9eb`
- broad raw record：
  [`adr0014-flac-hasher-broad-ab-v1-f23baf6-x86_64-pc-windows-msvc.json`](comparisons/adr0014-flac-hasher-broad-ab-v1-f23baf6-x86_64-pc-windows-msvc.json)
- broad record SHA-256：
  `b8d825d3f221b9186ba7b1b93522a9380795d05d9cc93eb8cc5b942cc5edabe9`
- gated raw record：
  [`adr0014-flac-hasher-gated-ab-v1-7e7ad7f-x86_64-pc-windows-msvc.json`](comparisons/adr0014-flac-hasher-gated-ab-v1-7e7ad7f-x86_64-pc-windows-msvc.json)
- gated record SHA-256：
  `fa11d1ab51a8748e91b1436ad0af336be2ccb533e98c5a5b0ca48dfedb562d6d`
- 前置决策：[ADR-0014](../adr/0014-deterministic-decode-analysis-pipeline.md)

## 结论

专用 hasher 被接受，但选择面收紧为唯一有端到端正收益证据的 allocation：声明 FLAC
MD5 且取得总 permit `N == 8` 时使用 7 个 decoder permit 和 1 个 hasher permit。
`N == 2/4` 保留全部 decoder 和 inline verifier；`N == 1` 继续使用串行 oracle；未声明
签名的 FLAC 不创建 hasher。

收紧后的正式 run 中，四条完整 `Application` case 快 15.0–39.4%，median process-tree
peak RSS 同时下降 9.5–13.1%。2/4 permit 的直接 decode 相对 baseline 只在
-1.46% 到 +2.00% 之间，说明广义候选丢失一个 decoder 所造成的 13–51% 回退已经
消失。8 permit 的直接 decode 仍慢 1.7–10.6%，但完整应用路径把哈希与分析重叠后有
明确净收益，因此只毕业这一 allocation。

这是一项固定主机上的 route-selection 测量，不是跨主机吞吐承诺，也不建立公开线程
调节面。

## 候选与资源模型

原路径在唯一按序 commit 点把 FLAC 签名字节喂给 `FlacStreamVerifier`，完成 MD5 后
才把 PCM 返回给 analyzer。候选保持相同 verifier、字节布局、输入序与 EOF verdict，
只把已有签名字节 `Vec` 交给一条 source-owned hasher 线程。队列容量为 1，EOF 关闭
sender 后 join，再由同一 verifier 比较最终 digest。

线程与内存都来自原 application reservation：8 个总 permit 拆成 7 decoder + 1
hasher；hasher 的一个处理中和一个已排队的最坏签名 block 先从 in-flight bytes 扣除，
剩余 permit 必须仍容纳完整 reorder window。任何几何不可表示或超出 permit 都在创建
线程前确定性退化为串行，不在失败后重跑。

## 两阶段选择

第一次 run 测量广义候选：任何声明签名且 `N > 1` 的 FLAC 都拆出一个 hasher permit。
完整应用路径已经显示候选方向正确：

| Application case | Baseline | Broad candidate | Candidate faster |
| --- | ---: | ---: | ---: |
| FLAC 16-bit / 60 s × 3 | 146.20 ms | 102.33 ms | **30.01%** |
| FLAC 16-bit / 240 s | 189.46 ms | 131.57 ms | **30.55%** |
| FLAC 24-bit 伪随机 / 240 s | 237.83 ms | 201.33 ms | **15.35%** |
| FLAC 24-bit tonal / 240 s | 237.99 ms | 150.83 ms | **36.62%** |

但同一 run 的直接 decode sweep 显示，较小 allocation 为 hasher 放弃一个 decoder 的
代价不可接受。下表为正数表示 candidate 更快、负数表示更慢：

| Track | N=2 | N=4 | N=8 |
| --- | ---: | ---: | ---: |
| FLAC 16-bit | -51.21% | -30.65% | -14.96% |
| FLAC 24-bit 伪随机 | -50.32% | -13.45% | -3.31% |
| FLAC 24-bit tonal | -45.97% | -16.21% | -1.95% |

因此没有从 8-permit Application 的收益向下外推。生产选择改为只在总 permit 为 8 时
拆分，随后从 clean `7e7ad7f` 对完整 28-case 选择重新运行同一协议。

## 收紧后的正式结果

每个 case warmup 1 次、measured 7 次；28 个 case、两个变体完全交错，共 392 个正式
样本。下表均为 `workerElapsedNs` 中位数，`Candidate faster` 为正时表示候选更快。

### 完整应用路径

| Application case | Baseline | Gated candidate | Candidate faster | Median peak RSS |
| --- | ---: | ---: | ---: | ---: |
| FLAC 16-bit / 60 s × 3 | 145.61 ms | 102.68 ms | **29.48%** | 9.05 → 8.04 MiB |
| FLAC 16-bit / 240 s | 190.84 ms | 131.42 ms | **31.14%** | 8.37 → 7.58 MiB |
| FLAC 24-bit 伪随机 / 240 s | 234.65 ms | 199.54 ms | **14.96%** | 7.24 → 6.29 MiB |
| FLAC 24-bit tonal / 240 s | 242.23 ms | 146.90 ms | **39.36%** | 7.87 → 6.99 MiB |

### 直接 decode 对照

| Track | N=1 | N=2 | N=4 | N=8 |
| --- | ---: | ---: | ---: | ---: |
| FLAC 16-bit | -1.60% | -1.46% | -0.53% | **-10.63%** |
| FLAC 24-bit 伪随机 | -1.41% | -1.14% | -0.86% | **-3.82%** |
| FLAC 24-bit tonal | +1.05% | +0.57% | +2.00% | **-1.70%** |

N=1/2/4 没有启用异步 hasher，表中的约 ±2% 给出本轮同代码路径的测量波动范围。只有
N=8 仍以一个 decoder permit 换取 hasher；直接 decode 没有足够的 analyzer 工作供其
重叠，因此该表如预期保留代价，而 `Application` 表测到实际产品关键路径的净收益。

## 污染门禁

两次 run 都把未改动的三条 ALAC worker sweep 留在同一个交错套件中。表内为
1-worker / 8-worker 的中位数加速比：

| ALAC track | 已知干净值 | Broad baseline / candidate | Gated baseline / candidate |
| --- | ---: | ---: | ---: |
| 16-bit 伪随机 | 4.07x | 4.223x / 4.111x | 4.259x / 4.186x |
| 16-bit tonal | 4.31x | 4.276x / 4.242x | 4.302x / 4.231x |
| 16-bit varied | 4.29x | 4.299x / 4.196x | 4.178x / 4.220x |

对照没有明显低于已知干净值，因此两轮均有效。Windows 原生 CPU busy fraction 在
broad run 为 6.26% → 4.74%，gated run 为 5.47% → 4.20%；两轮
`outliersRemoved = 0`。

## 正确性与终止面

- 两次 run 的每个 case、每个 measured sample 都通过跨变体
  `resultFingerprintSha256` 校验；decode case 还匹配相同的完整 interleaved `f64`
  SHA-256 与 corpus oracle；
- 2/4/8 总 permit 的报告与 raw-bit 差分、强制最坏乱序、inline/async digest 等价和
  unsigned FLAC 对照均由单元测试固定；
- hasher spawn failure、panic/disconnect、packet-pool 构造回滚、提前 drop/应用取消、
  篡改 MD5 与正常 EOF 的 join 和 sticky error 行为都有确定性覆盖；
- EOF 错误优先级仍为 reorder 完整、decoder pool verdict、全流 MD5、声明帧数，没有
  因线程拆分改变。

## 范围

本记录不建立：

- 2/4 permit、未签名 FLAC 或其他 codec 使用专用 hasher；
- 在 reservation 之外增加线程或内存；
- 文件级、窗口级并行或另一个 scheduler；
- 跨主机数字可比性、用户可见性能承诺或普通 CI 的 timing/RSS 阈值。
