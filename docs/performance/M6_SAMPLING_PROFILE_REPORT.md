# M6：0.2.0 采样归因报告

- 状态：Sampling profile established；first candidate selected；optimization not started
- 日期：2026-07-21
- 方法：ADR-0007 / `m6-sampling-profile-v1`
- source：`7ad057b167ddac2b6c415958607d95051c77cfb3`（clean）
- canonical raw record：
  [`m6-sampling-v1-7ad057b167dd-aarch64-apple-darwin.json`](profiles/m6-sampling-v1-7ad057b167dd-aarch64-apple-darwin.json)
- raw record SHA-256：
  `e900e5fef6fb19e59ae079922694678f386656850532f26829d13dedc3099b58`
- 前置 timing baseline：
  [`M6_SCALAR_BASELINE_REPORT.md`](M6_SCALAR_BASELINE_REPORT.md)

## 结论

首批 sampling profile 已经把 M6 timing baseline 中的两个主要 scope 归因到具体
调用树：

1. analyzer 的首要可控成本不是 histogram finish、线程调度或 codec，而是每个
   `push_interleaved` 在真正累计前执行的两次完整只读遍历；
2. FLAC 的主要成本在 Symphonia FLAC decoder 内部，产品 adapter 的 f64
   materialization 与 `PcmBlock` 构造只占较小部分。

因此第一个优化 candidate 固定为：

- 把独立的 finite-input scan 合并进事务性 numeric-safety validation；
- 把当前按声道跨步读取 interleaved PCM 的 shadow validation 改为逐帧遍历；
- 保留 invalid chunk 不改变 session 的原子契约、每声道样本求和顺序、有限 `f64`
  边界、唯一 `AnalyzerSession`、安全标量代码和 bit-exact 最终结果。

这不是授权删除校验、改成单遍就地提交、增加 `unsafe`/SIMD、恢复并发或引入第二
backend。先做上述窄 candidate，再用完整差分门禁和 ADR-0007 同 run interleaved
A/B 判断它是否值得保留。

FLAC 暂不形成产品优化 candidate。禁用 checksum、fork Symphonia 或增加另一个
decoder 都会改变正确性/维护边界，而当前 profile 没有证明这种代价合理。

## 捕获身份与边界

| 字段 | 值 |
| --- | --- |
| worker SHA-256 | `888957c8a1e2d8b806ef254ef552d055d9eab65174f84599965fc72d0d3699e3` |
| worker build | release opt-level 3；thin LTO；codegen-units 1；debug 1；unstripped |
| profiler | Xcode Time Profiler / xctrace 16.0（27A5194q） |
| sampling | 1 ms；每 case 3 次独立 capture |
| machine | Apple M4 Pro / Mac16,8 / 12 CPU / 48 GiB |
| OS | macOS 27.0 build 26A5378n；Darwin arm64 27.0.0 |
| Rust | rustc 1.96.0；LLVM 22.1.2；Cargo 1.96.0 |
| corpus | `m6-performance-baseline-v1` |
| corpus manifest SHA-256 | `c985486a6317b927e95c5933f6b8e76eb5f2b6a8b1a0dd9c38f451fab27946b0` |

只统计 symbolicated stack 中含 noinline worker timing-scope anchor 的 sample。
进程启动、结果 serialization 和 FLAC 的计时区外完整 PCM hash verification 均
被排除。每次 capture 的 scoped sample weight / worker elapsed 都在
`0.9860..0.9911`，明显严于协议允许的 `0.85..1.15`：

| Case | 三次合并 scoped samples | worker elapsed median | coverage median |
| --- | ---: | ---: | ---: |
| analysis / stereo | 16,766 | 5.636 s | 0.9901 |
| analysis / 64ch | 14,387 | 4.857 s | 0.9875 |
| decode / FLAC s16 | 16,978 | 5.750 s | 0.9887 |

profile worker 为符号解析保留 debug 信息，因此这些 elapsed 不是新的 canonical
吞吐数据，也不能和 strip 后的 scalar baseline 直接比较；这里只使用栈内采样比例
做归因。

九个 `.trace` bundle 共 98,974,750 bytes，连同 XML export 保留在 ignored
`target/performance-profiles`。raw record 为每个 bundle、TOC 和 Time Profiler
XML 固定独立 SHA-256，并提交每次 capture 的全部折叠栈计数、leaf、inclusive 与
source-line 聚合，不只保存下面的摘录。

## 正确性门禁

每个 case 的三次 capture 都具有完全相同的 worker result fingerprint：

| Case | Result fingerprint |
| --- | --- |
| analysis / stereo | `246134380c25d3e69cd488344513fdbda05039e22416fb980314a91c3285ad47` |
| analysis / 64ch | `bd111ded4ddc723607fa0291fd97eabdac8a2fe329a87419f910bf45e7e7d7b6` |
| decode / FLAC s16 | `9285ac95fbc37f7c8710c245ce7a751150e9ea64bf05d748d35a1523965ac2a6` |

FLAC 三次完整 decoded interleaved-f64 oracle 都是：

```text
ad95eeb8a686dc31fe760fde845c5c0abf6b4d991700007eb024b82f56fcb9fa
```

它与 corpus manifest 和 scalar baseline 完全相同。work unit、geometry 与
worker details 也在三次 capture 之间保持一致。

## Analyzer 归因

下表是 inclusive subtree weight；同一列中的父子项会重叠，不能相加。只有
`validate_numeric_safety` 与入口 `Iterator::any` 两个彼此独立的 subtree 在随后
结论中相加。

| Subtree | Stereo | 64ch | 含义 |
| --- | ---: | ---: | --- |
| `validate_numeric_safety` | 28.95% | 54.44% | 提交前的事务性 shadow arithmetic |
| 入口 finite scan / `Iterator::any` | 10.53% | 14.76% | 独立完整遍历，仅检查有限值 |
| 实际累计 `ZipImpl::next` | 32.82% | 28.89% | 逐帧、逐声道 commit loop |
| `ChannelAccumulator::add_sample` | 12.51% | 1.24% | 上一项内部的累计主体，受 inlining 归因影响 |
| `window_rms_squared` | 6.05% | 9.18% | 主要位于 shadow validation 内 |

入口 finite scan 与 shadow validation 合计占 stereo 的 39.48%，在 64 声道达到
69.20%。三次独立 capture 的比例很稳定：

- stereo shadow：28.61%–29.33%；finite scan：10.38%–10.80%；
- 64ch shadow：53.72%–55.44%；finite scan：14.32%–15.07%。

64 声道 shadow 比例上升与当前源码的 channel-major
`skip(channel_index).step_by(channel_count)` 相符：它以 64 个 `f64`、即 512-byte
跨度反复读取 interleaved block。逐帧 shadow traversal 可以保持每个声道内部的
样本顺序和算术顺序，同时改善访问局部性。这一因果解释是结合调用栈与源码得出的
工程推断，profile 本身只直接证明比例。

histogram 初始化、window finish、最终聚合和 report construction 在这些长流
capture 中都没有形成 0.1% 以上的可见 subtree。它们不应成为首轮优化目标。

### 选定 candidate 的安全边界

首个 candidate 仍保持 validation 与 commit 两个阶段：

1. shadow 阶段逐帧读取，每个 sample 先执行现有 finite check，再按原顺序更新对应
   声道的轻量 shadow 数值；
2. 任一错误时直接丢弃 shadow，session 完全不变；
3. shadow 全部通过后才运行现有 commit loop；
4. 不复制 histogram、不改变 window/peak/rounding/profile 规则，不新增公开 API。

这样可以确定地删除独立 finite scan，并针对 profile 证明的 64-channel strided
shadow 成本改善局部性。进一步把 validation 与 commit 合成一次、为 histogram
设计 rollback，复杂度和风险都更高，不属于首个 candidate。

## FLAC 归因

FLAC 的主要 inclusive subtree 如下；decoder 内部各项同样存在父子重叠：

| Subtree | Inclusive weight |
| --- | ---: |
| `SymphoniaPcmSource::read_block` | 98.55% |
| Symphonia `FlacDecoder::decode` | 79.07% |
| `read_subframe` / `decode_linear` | 48.41% / 48.34% |
| LPC prediction | 31.31% |
| FLAC validator update | 28.39% |
| MD5 transform | 24.57% |
| residual / Rice decode | 16.91% / 14.58% |
| FLAC packet parser | 10.27% |
| `SampleBuffer<f64>::copy_interleaved_*` | 2.99% |
| product `PcmBlock::new` project leaf | 3.13% |
| probe/open project leaf | 0.10% |

三次 capture 中 `FlacDecoder::decode` 为 78.82%–79.35%，validator 为
27.93%–28.73%，`SampleBuffer` copy 为 2.81%–3.32%。这排除了单次采样偶然性。

结果说明 scalar baseline 里的 FLAC 差距主要来自真实压缩解码与格式校验，不是
content probe、文件打开或 MacinMeter 自建调度。MD5 validator 是 FLAC 完整性路径
的一部分；仅为了 synthetic corpus 的吞吐禁用它会削弱 M2 的严格解码契约。

本 corpus 只有一类确定生成的 FLAC 压缩形态，不能据此声称所有音乐内容都具有
同样的 LPC、Rice 或 checksum 占比。若未来真实输入暴露用户可感知的 FLAC 问题，
应先扩大 deterministic FLAC corpus，再评估 Symphonia 升级或上游改进；当前不
建立第二 backend。

## 没有授权的方向

- 没有文件级并发需求证据；profile 的热点都在单文件 CPU path；
- 没有恢复包级并行的理由；
- 没有先写 SIMD/unsafe 的理由；首个 analyzer candidate 是遍历与局部性问题；
- 没有为 FLAC 禁用 checksum、放宽坏包失败或绕过 `PcmBlock` 有限值契约的理由；
- 不需要额外 profile `application/wave-s16`：baseline 已把 WAV application
  与 direct analysis 的尺度闭合，本轮也未看到 progress/path 构造可能接近上述
  热点的证据。

## 下一步

下一切片只实现 analyzer validation-traversal candidate：

1. 先补直接验证 invalid chunk 原子性、非有限/平方溢出/累计溢出错误与任意 chunk
   边界的差分测试；
2. 合并 finite scan，改写 frame-major shadow validation；
3. 跑完整 workspace/reference/adapter 门禁，要求所有既有 result fingerprint
   不变；
4. 构建 scalar 与 candidate 两个 source-bound worker，在同一次
   `run-performance-baseline.py` 中只对三个 analysis case 做完全交错 A/B；
5. 收益若小于噪声或引入复杂度，则删除 candidate；只有稳定且 bit-exact 才保留。

在 A/B 之前不改 FLAC、不恢复并发、不加入 SIMD，也不发布新的性能数字。
