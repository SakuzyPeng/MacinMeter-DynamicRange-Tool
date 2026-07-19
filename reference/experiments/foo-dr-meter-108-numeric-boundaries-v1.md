# EXP-foo-dr-meter-108-numeric-boundaries-v1

## 状态与研究问题

- 状态：completed, accepted observation
- experiment ID：`foo-dr-meter-108-x64-numeric-boundaries-v1`
- target：
  `TARGET-foo-dr-meter-1.0.8-x64-static-ff3556ad`
- 执行器：
  [`run_foo_dr_meter_108_numeric_boundaries.py`](../tools/run_foo_dr_meter_108_numeric_boundaries.py)
- foobar2000：不启动
- 进程模型：每个向量一个新的 hardened x64 worker
- observation：
  [`OBS-foo-dr-meter-108-x64-numeric-boundaries-v1-run1-20260719`](../observations/obs-foo-dr-meter-108-x64-numeric-boundaries-v1-run1-20260719/record.md)

本实验一次完成三个仍可能影响 per-track 公开结果的高区分度问题：

1. duration 在精确半秒如何舍入，并在 minute/hour/day/week 进位时产生什么 token；
2. `Weight multichannel DR by channel loudness` 打开后实际采用哪条 track 聚合
   分支，是否只对多于两个声道生效；
3. RMS histogram 的 `-100 dB` 与 `0 dB` 两个端点是否真正 clamp 到第一个和
   最后一个 bin。

这些问题都在固定 DLL 内有明确静态入口，因此不再通过 foobar playlist、GUI 或
手工报告采集间接猜测。

## 一次性矩阵

执行器固定运行 38 个独立 worker：

| 家族 | 数量 | 内容 |
| --- | ---: | --- |
| duration leaf | 24 | `0.5s`/`1.5s`、44.1 kHz/48 kHz 半秒，以及 minute/hour/day/week 进位的下侧、精确半值和上侧 |
| multichannel weighting | 8 | 4 个 PCM scenario，各自 off/on 配对 |
| histogram clamp | 6 | 下端越界/精确/内侧与上端内侧/精确/越界 |

### Duration

duration 请求只携带 `decodedFrames`、`sampleRateHz` 和固定
`fractionalDigits = 0`。worker 在私有加载和 IAT tripwire 生效期间直接调用
[`固定 duration 叶子`](../static-analysis/sa-foo-dr-meter-108-x64-duration-leaf-20260719.md)。

核心判别包括：

```text
499/1000  -> 0:00
1/2       -> 0:01
501/1000  -> 0:01

22049/44100 -> 0:00
22050/44100 -> 0:01
22051/44100 -> 0:01

23999/48000 -> 0:00
24000/48000 -> 0:01
24001/48000 -> 0:01
```

同样的下侧/半值/上侧三元组覆盖 `59.5s`、`3599.5s`、`86399.5s` 与
`604799.5s`，使分钟、小时、日和周进位分支在同一轮完成。

### Multichannel weighting

PCM 使用 8 kHz、10 个完整窗口、交错 finite binary64。四个 scenario 为：

| scenario | 目的 | off | on 候选 |
| --- | --- | --- | --- |
| balanced 3ch | 证明开关进入加权分支 | DR10/20/30 算术均值 | 以各声道 overall RMS 加权 |
| RMS-source 3ch | 区分 overall RMS、loud-window RMS、RMS² 与不加权 | DR20 | 约 DR11.443 |
| gate 2ch | 验证 `channels > 2` 门槛 | DR20 | 必须与 off raw bits 相同 |
| partial-silence 3ch | 验证静音声道零权重且分母仍有限 | 约 DR13.333 | 约 DR11.818 |

每一对都预先声明：

- 实际公开 channel DR/RMS raw bits 必须先满足场景构造前提，不能只因最终 track
  bits 偶合就判定公式成立；
- `channelResults`、session、channel state 与 histogram raw observation 必须逐位
  相同；
- balanced、RMS-source 与 partial-silence 的 `trackDrBits` 必须改变；
- two-channel gate 的 `trackDrBits` 必须不变；
- 成功集不包含三声道全静音，因为该非默认分支的静态 `0/0` 不属于有限正向契约。

预期 raw bits 来自固定二进制既有 E1 静态公式和确定性 PCM 构造，只是预注册的
判别模型，不是第二份 reference observation。E2 结论来自该静态数据流与固定
target 隔离动态输出的交叉；Python 计算不会被算作独立的第三类证据。

### Histogram clamp

每项为 8 kHz、单声道、一个完整 binary64 窗口。输入 RMS 分别为
`-101/-100/-99/-1/0/+1 dB`。worker 在 finish 后、cleanup 前验证 histogram
vector 的 `begin/end/capacity`，只输出每声道紧凑摘要：

- total/nonzero bin count；
- bin `0`（`-100 dB`）计数；
- bin `10000`（`0 dB`）计数；
- 整个 10001-bin `u32le` slice 的 SHA-256。

`-101 dB` 必须进入 bin `0`，`+1 dB` 必须进入 bin `10000`；精确端点和相邻内侧
输入用于排除端点索引或闭区间方向错误。

## 证据与限制

成功 observation 可支持：

- 固定 x64 duration numeric leaf 对已列半秒和进位输入的 E2 结果；
- 固定 x64 analyzer core 的可选 multichannel weighting 分支及 `channels > 2`
  门槛；
- 固定 x64 analyzer core 的两个 histogram endpoint clamp。

它不支持：

- foobar decoder、playlist、component registration 或 GUI 行为；
- metadata、album grouping、完整 renderer 模板或报告 byte parity；
- 其他插件版本、x86、其他 OS/UCRT/CPU 的结果；
- MacinMeter compatibility/verified 标签。

所有生成 PCM 都由执行器确定性形成并由 request identity 绑定；仓库不提交个人
音频、目标二进制、私有授权原文或本机路径。

## Run 1 结果

2026-07-19 在固定 Windows x64 环境中一次完成全部 38 个独立 worker；没有启动
foobar2000：

| 判据 | 结果 |
| --- | ---: |
| duration | 24/24 |
| weighting track raw bits | 8/8 |
| weighting channel 前提 | 8/8 |
| weighting pair invariant | 4/4 |
| histogram clamp | 6/6 |

总判据 `allMatched = true`。固定 worker SHA-256 为
`9685bf13e69cce2f0920510b70e24c57cff4483b1c3296baada3f165704ca817`；
canonical suite SHA-256 为
`28416daabebfb0291305b80328a5b2003b10606830051c370f90c78070f2901b`。

实际 raw 结果、PCM identity、histogram slice digest、隔离边界和限制以对应
observation 为准。静态公式中的 expected bits 只作为预注册判别模型；结论来自
固定 target 的动态输出与既有静态数据流交叉，不把 Python 计算当作独立证据。
