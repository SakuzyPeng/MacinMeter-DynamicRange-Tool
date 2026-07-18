# TARGET-foo-dr-meter-1.0.8-foobar2000-2.25.10-x64-win10-19045

## 定位

- 事实类别：reference target identity
- 状态：fixed-runtime-target
- 记录日期：2026-07-18（UTC+08:00）
- 用途：固定 `foo_dr_meter` 1.0.8 x64 端到端运行观测的目标身份

该 target 只约束下列固定二进制、宿主版本、系统和实验配置。它不把观测行为
外推到 x86、其他 foobar2000 版本、其他 input component 或 `foo_dr_meter`
1.0.3，也不表示 MacinMeter 已达到兼容。

## 二进制身份

| 对象 | 版本/角色 | 架构 | 字节数 | SHA-256 |
| --- | --- | --- | ---: | --- |
| `foo_dr_meter.dll` | 1.0.8 | x86-64 PE32+ | 424448 | `ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489` |
| `foobar2000.exe` | 2.25.10.0 | x86-64 PE32+ | 4789128 | `653cc120c146aaae9e6db9b6f19e5a1588407b8940bc1521f0ced739ff8924b0` |
| `foo_input_std.dll` | 本安装随附的 standard input component | x86-64 PE32+ | 2505616 | `46a4b9c4515fae55add895e12d30602f73944959f0e0f7acf7122e6562b51651` |

运行时检查同时确认进程为 x64、加载的插件模块来自 x64 component 目录，且已加载
模块的 SHA-256 与上表相同。报告头独立回显 `foobar2000 v2.25.10 / DR Meter
v1.0.8`。二进制本体及个人安装路径不进入仓库。

同一插件 DLL 的静态身份见
[`TARGET-foo-dr-meter-1.0.8-x64-static-ff3556ad`](foo-dr-meter-1.0.8-x64-static.md)。
该身份相同，因此插件内部静态路径可以与本次动态结果交叉印证。

foobar2000 2.0 的
[`WAV decoder 静态 target`](foobar2000-2.0-x64-wave-decoder.md)
不是本 target 的 input component：版本、长度和哈希均不同。除非另行分析
2.25.10 的固定 `foo_input_std.dll`，不得把 2.0 decoder 的逐指令结论直接登记成
本运行目标的静态事实。

## 宿主

- Windows 10 Pro，build 19045；
- x64 操作系统、x64 foobar2000 进程和 x64 插件；
- 时区：China Standard Time（UTC+08:00）；
- 精确区域设置未单独采集；原始报告自身为 ASCII，插件固定数字格式的静态证据
  单独保存在 static-analysis 记录中。

## 本次实验配置

| 配置 | 值 | 证据 |
| --- | --- | --- |
| Automatically save tags | off | 操作者按固定实验说明确认 |
| Add per-channel stats also for stereo album logs | on | 操作者确认；报告中的 stereo 分声道列同时印证 |
| Weight album DR by track lengths | off | 操作者按固定实验说明确认 |
| Weight multichannel DR by channel loudness | off | 操作者按固定实验说明确认 |

foobar2000 2.25.10 将配置保存在运行中的 SQLite 数据库；本次不关闭宿主、不复制
可能包含无关个人状态的数据库，也不把无法读取的数据库哈希冒充配置证明。

## 证据边界

- 插件来自仓库所有者提供的 component；本档案不保存私人授权原文。
- 本 target 的首次 observation 只覆盖 safe-master 39 项；三个隔离输入不属于
  该 observation。
- 设置中只有 stereo 分声道开关可由报告内容直接交叉检查，其余依赖操作者按协议
  确认。
- 报告 footer 中的 host metadata 只按原文登记；未解释字段不自动成为算法事实。
