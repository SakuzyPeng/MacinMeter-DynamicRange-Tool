# TARGET-foobar2000-2.0-x64-wave-decoder-ea6e9c52-cf5b2a86

## 定位

- 事实类别：reference target identity
- 状态：fixed-static-target
- 记录日期：2026-07-18（UTC+08:00）
- 用途：固定 foobar2000 2.0 x64 标准 WAV decoder 的静态分析对象

该 target 固定宿主与其标准输入组件的共同身份，用于研究合法 WAV PCM 字节进入
`audio_chunk<double>` 前的转换。它不是可运行 target，也不包含
`foo_dr_meter`；若要登记插件端到端输出，仍需建立包含插件、操作系统与配置的独立
runtime target 和 observation。

## 二进制身份

| 对象 | 版本/角色 | 架构 | 字节数 | SHA-256 |
| --- | --- | --- | ---: | --- |
| `foobar2000.exe` | foobar2000 2.0 host | x86-64 PE32+ | 4329984 | `ea6e9c52465562f50695a1a783dbdc29ac2e57025e5adb209733756514cc730b` |
| `foo_input_std.dll` | foobar2000 2.0 standard input component | x86-64 PE32+ | 2236928 | `cf5b2a86dcb750afcfe6ba5860f0937c068dbc502d7ae35b5837425eb861205f` |

两份文件来自同一份隔离的 foobar2000 2.0 x64 runner。研究记录不保存取得位置、
个人安装路径或机器名称；二进制本体不进入仓库。

## 适用边界

- 当前静态结论只适用于上述固定 x64 `foo_input_std.dll`。
- 该 target 不证明 x86 standard input component 使用相同数据宽度或全部相同
  边界行为。
- WAV 解析之外的 FLAC、AIFF、压缩 WAVE、错误 UI 和插件算法不属于此 target
  的结论范围。
- 操作系统、locale、时区和用户配置不参与本次纯静态 decoder 结论；任何动态
  运行必须在新的 runtime target 中补齐这些属性。
- 目标二进制、IDA 数据库及反编译文本均不提交；公开记录只保留固定身份和独立
  撰写的证据摘要。
