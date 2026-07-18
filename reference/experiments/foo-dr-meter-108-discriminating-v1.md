# EXP-foo-dr-meter-108-discriminating-v1

## 研究问题

这组输入用于区分 `foo_dr_meter` 1.0.8 与 M0 `ProvisionalV1` 中已经发现的
系统性规则差异。Fixture 只定义输入和待判别假设；插件导出的报告才可登记为
reference observation。

第一批覆盖：

- 一帧尾窗及 `W+1` 边界；
- 负 DR 的第一 peak 回退与零下限；
- RMS 在线性域或 dB 域量化；
- loudest 20% 边界 bin 的并列项处理；
- 量化 peak key 引入的到达顺序差异；
- 静音声道是否以 DR0 参与聚合；
- 三声道默认算术平均；
- 六声道官方值是否包含 LFE。

目标窗长使用静态分析已经确认的公式：

```text
sample_rate = 8000 Hz
W = floor(8000 × 3.0040816326530613) = 24032 frames
```

## 生成

```bash
python3 reference/tools/generate_foo_dr_meter_108_suite.py \
  --output /tmp/foo-dr-meter-108-discriminating-v1
```

生成器输出 `manifest.json`、`FILES.sha256`、三个报告组、对应 M3U8 和人工导出
说明。Manifest 记录实际 float32 PCM 的窗口 RMS/peak、RIFF data hash 和整文件
hash，但不记录参考插件的预期导出 DR；其中 `targetDrDb` 只是构造 PCM 的输入
参数，不是 golden。

## 执行

目标配置：

- Automatically save tags：off
- Add per-channel stats also for stereo album logs：on
- weight album DR by track lengths：off
- weight multichannel DR by channel loudness：off

按 `HOW_TO_EXPORT.txt` 分别导出 `01-core`、`02-degenerate` 和
`03-multichannel`。极短、静音输入与普通输入分开，避免一个外围失败使核心报告
失去可解释性。报告名包含架构、重复序号和组名，例如
`x86-run1-01-core.txt`。最初协议保守地建议每组独立运行三次；本次运行用于检验
预先从 x64 静态路径得到的 15 项判别预测，x86 单次结果已经 15/15 命中，因此
足以作为 candidate 的跨架构 E2 证据。重复运行只用于另行评估运行时确定性，不是
本 candidate 的阻塞条件。

当前已安装的 x86 1.0.8 用于黑盒观测；x64 1.0.8 使用同一 component 中的固定
二进制做静态分析。两种证据分别登记，不把 x64 黑盒重复运行设为 candidate
前置条件。

## Observation 入口条件

收回报告后，每次运行必须记录：

- Windows、foobar2000 和插件的版本、架构及 SHA-256；
- `manifest.json` 与原始报告 SHA-256；
- 上述全部配置值；
- 本地时间、时区、操作者步骤和重复次数；
- fixture ID 是否各出现一次，以及失败项的原始文本。

原始报告不做换行或时间戳清洗；规范化数据另存并引用原始 hash。

## 当前观测

- [`OBS-foo-dr-meter-108-x86-discriminating-v1-run1-20260718`](../observations/obs-foo-dr-meter-108-x86-discriminating-v1-run1-20260718/observation.json)
  是一次 x86 初步运行，包含三组原始 UTF-8/CRLF 报告。
- 该记录尚未建立重复运行确定性，也没有 x64 动态运行；这两项不是 candidate
  的阻塞条件，但该单次 observation 仍不作为 golden，也不宣称 conformance
  或版本间可外推性。
