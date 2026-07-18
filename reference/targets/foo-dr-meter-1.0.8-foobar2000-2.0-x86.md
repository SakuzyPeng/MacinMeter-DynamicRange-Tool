# TARGET-foo-dr-meter-1.0.8-foobar2000-2.0-x86-win10-19045

## 定位

- 事实类别：reference target identity
- 状态：candidate-runtime-evidence
- 记录日期：2026-07-18（UTC+08:00）
- 用途：固定本次 `foo_dr_meter` 1.0.8 x86 黑盒观测的目标身份

该目标是当前 1.0.8 candidate 的黑盒运行证据，不把其行为外推到 1.0.3 或
其他版本，也不表示 candidate 已经 accepted。

同一插件 DLL 的独立静态身份见
[`TARGET-foo-dr-meter-1.0.8-x86-static-6debd1d6`](foo-dr-meter-1.0.8-x86-static.md)；
跨架构静态分析已经确认 x86 与 x64 共享主要控制规则但使用不同 PCM/peak
精度，因此本目标不能替代 x64 动态数值证据。

## 二进制身份

| 对象 | 版本 | 架构 | 字节数 | SHA-256 |
| --- | --- | --- | ---: | --- |
| `foo_dr_meter.dll` | 1.0.8 | x86 | 332288 | `6debd1d665cec975853341fb4ae360d2187d2bb0c595eedde9e38b4b77301862` |
| `foobar2000.exe` | 2.0.0.0 | x86 | 3444224 | `1486a3b192a539cb2ec97bc9d7fe39f9a3567430794529f0efb5591d619fc26e` |

版本由文件版本信息及导出报告头交叉记录；SHA-256 与长度在目标主机上读取。
二进制本体不进入仓库。

## 宿主

- Windows 10 Pro，build 19045；
- x64 操作系统，x86 foobar2000 进程与插件；
- 时区：China Standard Time（UTC+08:00）；
- 精确区域设置：本次未单独采集。

## 本次实验配置

| 配置 | 值 | 证据 |
| --- | --- | --- |
| Automatically save tags | off | 操作者按实验说明确认 |
| Add per-channel stats also for stereo album logs | on | 操作者确认；多声道报告中的 stereo 分声道列同时印证 |
| Weight album DR by track lengths | off | 操作者按实验说明确认 |
| Weight multichannel DR by channel loudness | off | 操作者按实验说明确认 |

其他未影响本次导出字段的配置不作推断。

## 证据边界

- 插件来自仓库所有者提供的 component；本档案不保存其私人授权原文，也不解释
  授权范围。
- 不记录个人安装路径、账号、主机别名或机器名。
- 本目标目前只有一次 GUI 黑盒导出；原始报告见对应 observation。
