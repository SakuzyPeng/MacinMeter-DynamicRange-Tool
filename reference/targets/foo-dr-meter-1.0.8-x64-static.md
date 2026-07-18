# TARGET-foo-dr-meter-1.0.8-x64-static-ff3556ad

## 定位

- 事实类别：reference target identity
- 状态：candidate-static-target
- 记录日期：2026-07-18（UTC+08:00）
- 用途：固定当前 1.0.8 candidate spec 的 x64 静态分析对象

## 二进制身份

| 属性 | 值 |
| --- | --- |
| 文件角色 | component 中的 `x64/foo_dr_meter.dll` |
| 版本 | 1.0.8 |
| 格式 | PE32+ DLL |
| 架构 | x86-64 |
| 字节数 | 424448 |
| SHA-256 | `ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489` |

该 DLL 与本次 x86 黑盒目标来自同一个双架构 component。component 本身为
356313 bytes，SHA-256 为
`6dde22df1e3fcae256cd59ab39fd98adf3d840ebc3053fe4a2f7cced2998ade6`，
其中只包含 x86 与 x64 两个 DLL。component 和二进制本体均不进入仓库。

## 使用边界

- 该目标用于静态分析，不附带 foobar2000 运行宿主。
- 静态结论必须记录工具版本、函数位置和证据限制；不得提交反编译输出。
- 与 x86 黑盒结果逐项一致支持两个构建共享被覆盖的控制规则；后续
  [`x86/cross-arch 静态记录`](../static-analysis/sa-foo-dr-meter-108-x86-cross-arch-20260718.md)
  已确认 x86 使用 binary32 PCM/peak 路径，而 x64 使用 binary64，因此不支持
  所有浮点边界或数值精度相同的推断。
- 目标状态为 candidate，不表示兼容性已经 verified 或 accepted。
