# M4：固定 x64 数值声明收口报告

> 状态：`DONE`
>
> 日期：2026-07-20
>
> 决策：[ADR-0005](adr/0005-m4-bounded-x64-numeric-claim.md)
>
> 证据矩阵：[M4 x64 数值声明证据矩阵](M4_X64_NUMERIC_CLAIM_MATRIX.md)
>
> 当前记录：
> [direct-PCM Candidate conformance](../reference/conformance/conf-foo-dr-meter-108-x64-candidate-v1-direct-pcm-run1-20260720/record.md)

## 最终结论

M4 在有界范围内收口：

- 固定参考目标为 `foo_dr_meter 1.0.8 x64`，SHA-256
  `ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489`；
- 固定输入为 complete-v2 的 39 项 safe-master；
- 固定实现为 commit
  `76d0f2eab5cdfce9de6a9d76ab971c333eab8e71` 构建的
  `FooDrMeter108CandidateV1`；
- 直接 finite interleaved `f64` conformance 在 4096 与 997
  frames/block 两组完整运行中均为零差分；
- 纳入范围没有“结果有偏但原因不明”的残差；
- 产品仍明确标记 `FooDrMeter108CandidateV1 / Unverified`。

这里的“收口”表示固定目标、固定字段和固定 corpus 上的可审计一致性，不表示任意
输入、完整 foobar/component 或用户可见文本兼容。

## 已证明的数值范围

### per-track analyzer

以下规则已经形成“参考证据 → Candidate 规格 → 当前实现 → 本地回归”的完整链：

- binary64 PCM 入口和窗口系数；
- 任意非空尾窗与精确整窗行为；
- RMS/power 累计与 overall RMS；
- centi-dB histogram 量化及 `[-100, 0]` clamp；
- loud 20%、整组 boundary-bin 纳入和 bin-center 重建；
- peak key、严格排名、arrival-order tie 与 primary fallback；
- 负 DR 回退、静音数值 DR0、全声道/LFE 默认聚合；
- public binary32 窄化点；
- track/channel report RMS、primary peak 与 DR 字段。

正式 direct-PCM 记录的精确结果：

| 字段 | 结果 |
| --- | ---: |
| track DR raw bits | 39/39 |
| channel DR raw bits | 62/62 |
| channel RMS raw bits | 62/62 |
| channel primary peak raw bits | 62/62 |
| track DR token | 39/39 |
| channel DR token | 62/62 |
| overall peak token | 39/39 |
| overall RMS token | 39/39 |
| channel RMS token | 62/62 |
| duration token | 39/39 |
| difference count | 0 |

两种 block size 的完整公开结果 projection hash 相同，说明该 profile 的声明不依赖
调用层 chunk 切分。

### renderer 与 album 的纯数值边界

- duration 的 half-second、minute/hour/day/week carry 已由固定 x64 numeric
  observation 24/24 和产品单元测试共同覆盖；
- histogram endpoint 动态记录为 6/6，产品 clamp 回归覆盖相同边界；
- DR/dbFS 数值 token 的 half-away、near-zero centi 修正和 `-inf` 处理已有
  renderer 单元与 CLI 黑盒门禁；
- `AlbumAggregator` 的 public-f32 unweighted mean、duration weighting、最终
  f32 窄化、整数显示和数值 DR0 纳入已有独立产品 contract tests。

album 完整算术公式仍按其真实 E1 静态证据标注；M4 没有把纯数值 API 扩张为
foobar grouping、footer 或 metadata conformance。

## decoder 分层结论

39 项 safe-master 中有 5 项使用 `WAVE_FORMAT_EXTENSIBLE`。当前产品 decoder
按 ADR-0003 将它们结构化拒绝；其余 34 项 classic WAV 可以经过稳定文件路径。

direct-PCM 记录证明五项多声道输入的 Candidate 数值结果同样精确匹配。因此：

- 先前 34/39 文件 replay 不是算法 residual；
- 不需要、也不允许为了 M4 测试便利扩大稳定 codec 矩阵；
- reference profile conformance 已独立于 decoder 能力；
- 未来若毕业 extensible WAV，仍必须单独满足 ADR-0003 的 codec 合同。

## 路线图第 6.5 节出口审计

| 标准 | 结论 |
| --- | --- |
| 固定 corpus 的同语义最终字段完全一致 | `PASS`：两次 39 项 direct suite 均零差分 |
| window/RMS/histogram/peak/aggregate 规则可追溯 | `PASS`：Candidate §5 与证据矩阵逐项登记 |
| album/renderer 数值路径有产品边界测试 | `PASS`：album contract、duration/dbFS、histogram clamp |
| 没有系统性 residual 趋势 | `PASS`：正式 comparison `differenceCount = 0` |
| short/tail/tie/silence/multichannel 没有未解释例外 | `PASS`：fixed corpus、isolated core 与产品回归共同覆盖 |
| profile 独立于 decoder/chunk/可选路径 | `PASS`：direct f64、4096/997 block、默认唯一生产配置 |
| 已知奇怪规则被忠实保留 | `PASS`：一帧尾窗、whole-bin、arrival tie、DR0/LFE 纳入均有门禁 |

M4 没有发现需要修改 Candidate 算法的新反例。

## 明确限制

以下不属于本声明：

- product/foobar decoder normalization 与容器支持 parity；
- host lifecycle、registration、playlist、grouping、metadata 和 footer 来源；
- optional multichannel loudness weighting 的生产开关；
- channel label、模板、locale、换行、编码或完整文本 byte parity；
- x86 profile、1.0.3 或其他插件版本；
- NaN/Inf、异常资源范围、计数溢出或任意 CRT/libm 最后一位；
- 对未采样的任意 PCM 空间作穷尽数学证明。

square underflow 的固定控制流继续按 E1 限制公开；optional weighting 的参考事实
已动态记录，但由于默认生产 profile 不暴露该开关，不构成产品缺口。

## 状态决策

M4 不把 profile 改名为 `accepted`、`verified` 或 `compatible`。原因不是仍存在
已知算法偏差，而是当前标签刻意表达声明边界：证据只覆盖固定 x64 目标、固定字段
和有限判别 corpus，且 host/decoder/完整产品 parity 明确排除。

任何状态升级都必须另立决策，说明新名称究竟承诺哪些输入空间与产品行为。
