# CONF-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-report-v3-20260718

## 结论

- 事实类别：reference-to-implementation report-metrics conformance
- 状态：`exported_report_metrics_match`
- 参考规格：`foo-dr-meter-1.0.8-candidate-v1`
- 参考观测：
  [`OBS-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718`](../../observations/obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/observation.json)
- 比较范围：39 个 safe-master track 的整数 DR、62 个每声道 DR、39 个
  overall primary peak、39 个 overall RMS 和 62 个每声道 overall RMS

Wire schema v3 把报告指标与 DR 诊断值分离后，同一固定 observation 与相同输入
得到：

| 字段 | 精确匹配 |
| --- | ---: |
| 整数 track DR | 39/39 |
| 每声道两位 DR token | 62/62 |
| overall primary peak token | 39/39 |
| overall RMS token | 39/39 |
| 每声道 overall RMS token | 62/62 |

全部比较使用导出 token 精确相等，差分数为 0，fixture 集合和实现输出顺序均与
reference 完全一致。这扩展了
[`schema v2 DR-only 记录`](../conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-20260718/record.md)
的公开可比范围，但不表示内部状态、任意输入、album-focused 行为或整份 footer
已经纳入；实现 profile 为 `FooDrMeter108CandidateV1`。

## 固定身份

### Reference

| 对象 | SHA-256 |
| --- | --- |
| 原始 x64 报告 | `e9afbde86ccb21cae56826803da5492e37135c8594a657130b3868b42956d11c` |
| 规范化报告 | `50205960b9850addb7f18bdb5f3c2c3c59897a5a2c5efc8e408870d5a3a2ffce` |
| complete-v2 manifest | `479e535a7196487fdb67a54f0c4de681f925920453e8092bac9eeb04eec4bbf8` |

### MacinMeter

实现建立在 commit `f6d44085216efe2db6a33fe9584de3955bf53e6a` 之后的 dirty
worktree；该 commit 是本轮 report
metrics 实现之前的固定检查点，不能单独重建这里的二进制。因此本记录以实际
release binary 与已保存 WireEnvelope 的哈希固定实现身份：

| 对象 | SHA-256 |
| --- | --- |
| release CLI binary | `ff249a3c3cdcd0f45f9bc91065f08259f2c690432f4677cb9216dc1692da399e` |
| schema v3 WireEnvelope | `4587f73881403b099cdfb41e7516ebfa62a4f776754e8e17ad1cf92d5705ad68` |
| comparison | `197a1f5119c2c8a02253e55984f01e3c8fd2a0d1b5fba7b1143b6f7b55c990d9` |
| schema v3 comparator | `f943a19dd0bf1cb8e4cd320e95e22e8041cdcc162c5f5535f6a8b2d0698958b4` |

Wire schema 为 3，tool version 为 0.2.0。实现输出保存在
[`implementation/schema3-wire.json`](implementation/schema3-wire.json)，
规范化比较保存在 [`comparison.json`](comparison.json)。

## 实现语义

新增数据层没有复用语义不同的 `loudWindowRms` 或 DR-selected peak：

- 每声道 overall RMS 是所有未量化窗口 RMS² 的等权二次均值，再公开窄化为
  binary32；
- 每声道 public primary peak 是 DR peak-key 在线排名第一项的 binary32
  公开值，不是 secondary 或 selected peak；
- track overall RMS 强制先在 binary32 中平方每声道公开 RMS，再提升到
  binary64 求和、取平均与开方；
- track primary peak 是所有公开 binary32 channel primary peak 的最大值；
- 零线性电平保留为数值 0，dBFS 使用显式 `null`，adapter 显示为 `-inf`；
- duration 保存实际 decoded frames 与实际 PCM sample rate，不保存格式化文本。

`FiniteF32`/`FiniteF64` 在构造和反序列化时拒绝非有限值；report 结算可能失败并
返回结构化 analysis error，不依赖 JSON serializer 把异常浮点静默变成 `null`。

## Album 边界

本轮同时增加显式 `AlbumAggregator` application API，但不把任意 batch 自动视为
album。它以公开 binary32 track DR 在 binary64 中累计 official mean；数值 DR0
无条件纳入；可选 duration weighting 使用 decoded frames/sample rate，零总时长
回退 official。

701/702/703 构造回归得到 official binary32 `0x412d2c5f`、显示 DR11；203+701
得到 `0x40a7d70a`、显示 DR5。对本记录 39 个产品 track DR 应用同一 official
公式得到 `0x413ae271`、显示 DR12，与 observation footer 相同。safe-master
footer 不能区分该公式与若干替代聚合，album 规则仍只有静态 E1；没有把这项结果
记作 album-focused conformance。

## 命令与运行结果

```bash
cargo build --locked --release -p macinmeter-cli
target/release/macinmeter batch <39 manifest-ordered paths> \
  --format json --output <schema3-wire.json>
python3 reference/tools/compare_macinmeter_report_metrics_to_foo_dr_meter.py \
  --reference <observation>/normalized/safe-master.json \
  --implementation-output <schema3-wire.json> \
  --implementation-binary target/release/macinmeter \
  --output <comparison.json>
```

CLI 退出状态为 0，39 项全部成功；stdout 为 0 bytes，进度和诊断只进入 stderr。
新 comparator 明确只接受 schema v3；旧 schema v2 产物继续由原 DR-only 工具
解释，两个证据边界互不覆盖。

## 未比较字段

- histogram、peak key、内部 binary64 DR 等不可导出的中间状态；
- reference `durationToken` 的文本格式；
- album-focused playlist、length-weighting 开关与三个 isolated 输入；
- footer 的采样率、声道、位深、bitrate、codec metadata；
- 更广输入空间、其他 foobar2000/plugin 版本及运行重复性。

因此该记录扩大了固定语料上的公开 report 字段 conformance，但不足以将候选规格
升级为 accepted、将兼容性改为 verified，或声称整份 reference parity。
