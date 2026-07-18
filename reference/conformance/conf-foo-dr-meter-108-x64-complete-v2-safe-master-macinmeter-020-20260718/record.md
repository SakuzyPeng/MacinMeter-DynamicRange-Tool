# CONF-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-20260718

## 结论

- 事实类别：reference-to-implementation conformance
- 状态：`exported_core_fields_match_after_f64_fix`
- 参考规格：`foo-dr-meter-1.0.8-candidate-v1`
- 参考观测：
  [`OBS-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718`](../../observations/obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/observation.json)
- 比较范围：39 个 safe-master track 的整数 DR，以及 62 个每声道两位小数 DR

基线实现的 track DR 为 39/39，但每声道 DR 只有 60/62。两处偏差都来自
float64 WAV 在分析前被窄化成 float32。PCM block、Symphonia 输出和
`AnalyzerSession` 入口提升为 f64 后，完全相同的参考 observation 与输入得到：

| 字段 | 基线 | f64 修正后 |
| --- | ---: | ---: |
| 整数 track DR | 39/39 | 39/39 |
| 每声道两位 DR token | 60/62 | 62/62 |
| 差分数 | 2 | 0 |

这表示当前有限 corpus 中语义相同且公开可比较的核心字段已经逐 token 对齐；它
不表示整份报告、全部中间状态、所有输入或其他目标版本已经兼容。profile 继续是
`FooDrMeter108CandidateV1 / Unverified`。

## 固定身份

### Reference

| 对象 | SHA-256 |
| --- | --- |
| 原始 x64 报告 | `e9afbde86ccb21cae56826803da5492e37135c8594a657130b3868b42956d11c` |
| 规范化报告 | `50205960b9850addb7f18bdb5f3c2c3c59897a5a2c5efc8e408870d5a3a2ffce` |
| v2 manifest | `479e535a7196487fdb67a54f0c4de681f925920453e8092bac9eeb04eec4bbf8` |

### MacinMeter

两次运行都在 `git HEAD
eff2140379b17d6ff418bd3761bc070900849bca` 之上的 dirty worktree 构建，因此
commit 不能单独重建二进制。本记录以实际二进制和已保存 WireEnvelope 的哈希为
实现身份，不把 HEAD 冒充完整 source identity。

| 运行 | CLI binary SHA-256 | WireEnvelope SHA-256 | comparison SHA-256 |
| --- | --- | --- | --- |
| pre-f64 | `8b1683fa6923d4f2b616e002f949403ae6eb730f99a05831f930dc425395dd6d` | `c8d31e871609edf7b9807eaa28deb4e0af0d8b643eec3e65ff3b2a0f60863f97` | `b006d89061e2726e7695ac1486039e226529369568136fa5cbc3006fbe82a60f` |
| post-f64 | `ec702c4f82803e7ed29634fe6fd08e8a1eeed5a64510c1ea4c7bf9548f157c90` | `96d6a9fa95edf9b57b7904fa0da7ffba362c9e96740b204709366765ba64ce0a` | `1190a7f3dd65035d9fc9b13219cff00012d9f4c08fd492f37016376b0a8b5ef6` |

`Cargo.lock` SHA-256 为
`5f966f1c0168690c6e928964e4f51a79f8a6a6341f47759e37884ab2ec8c64b7`。
构建环境为 Apple arm64、rustc/cargo 1.96.0；产品 wire schema 为 2，
tool version 为 0.2.0。

原始实现输出分别保存在
[`implementation/pre-f64-wire.json`](implementation/pre-f64-wire.json) 和
[`implementation/post-f64-wire.json`](implementation/post-f64-wire.json)；
规范化差分分别保存在
[`pre-f64-comparison.json`](pre-f64-comparison.json) 和
[`post-f64-comparison.json`](post-f64-comparison.json)。

## 精确差分与处置

比较策略不是数值容差，而是参考导出 token 的精确相等：

- track DR：整数 token 完全相同；
- channel DR：按报告两位小数格式化后 token 完全相同；
- item：按唯一 fixture stem 关联，目录发现顺序不属于算法结果。

基线两处差分：

| Fixture | 声道 | x64 reference | pre-f64 | 原因 |
| --- | ---: | ---: | ---: | --- |
| `410_rms_half_f64_stereo` | 1 | `8.00` | `8.01` | source-f64 RMS key 在 f32 窄化后塌缩 |
| `420_peak_half_f64_stereo` | 0 | `18.99` | `19.00` | source-f64 peak 半值在 f32 窄化后塌缩 |

处置是把有效 PCM 主链统一为 finite interleaved f64：

- `PcmBlock` 保存 `Vec<f64>`；
- Symphonia 复制到 `SampleBuffer<f64>`；
- `AnalyzerSession::push_interleaved` 接收 `&[f64]`；
- 分析入口在修改 session 前，以 f64 对整块做平方、平方和及 RMS 的有限性预检；
- 公开 channel/track DR 仍按参考路径窄化为 binary32。

修正后两项分别成为 `[8.01, 8.00]` 和 `[18.99, 19.00]`，整批结果为
39/39、62/62。

## 命令与运行结果

构建：

```bash
cargo build --locked --release -p macinmeter-cli
```

运行时将 manifest 中 `00-safe-master` 的 39 个路径按固定顺序显式传给：

```text
target/release/macinmeter batch <39 paths> --format json --output <wire.json>
```

post-f64 退出状态为 0，39 项全部成功；stdout 为 0 bytes，进度和诊断只进入
stderr。比较器：

```bash
python3 reference/tools/compare_macinmeter_to_foo_dr_meter.py \
  --reference <observation>/normalized/safe-master.json \
  --implementation-output <wire.json> \
  --implementation-binary target/release/macinmeter \
  --output <comparison.json>
```

比较器 SHA-256：
`77434845f29fad911ca22837975c78090b2b28a3f4fa658d1f23647b1d9b7236`。

## 未比较字段

- reference report 的 overall primary peak 与 overall channel RMS 没有同语义的
  MacinMeter wire 字段；
- MacinMeter `loudWindowRms` 不是 reference report 的 channel overall RMS；
- 导出报告不可观察 histogram、peak key 和内部 binary64 DR 等中间状态；
- footer、album focused playlist、三个 isolated 输入及错误 UI 不在本记录范围；
- footer 的 `32761` 位深异常单列为 host/plugin metadata 问题，不参与核心 DR
  conformance。

因此本记录满足“已有固定 reference observation 与固定实现输出之间的差分”，但
不足以把候选规格升级为 accepted 或把兼容性状态改成 verified。
