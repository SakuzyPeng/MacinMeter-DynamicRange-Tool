# TARGET-foo-dr-meter-1.0.8-x86-static-6debd1d6

## 定位

- 事实类别：reference target identity
- 状态：candidate-static-target
- 记录日期：2026-07-18（UTC+08:00）
- 用途：固定 `foo_dr_meter` 1.0.8 x86 静态分析与跨架构比较对象

## 二进制身份

| 属性 | 值 |
| --- | --- |
| 文件角色 | component 根目录中的 `foo_dr_meter.dll` |
| 版本 | 1.0.8 |
| 格式 | PE32 DLL |
| 架构 | x86 |
| 字节数 | 332288 |
| SHA-256 | `6debd1d665cec975853341fb4ae360d2187d2bb0c595eedde9e38b4b77301862` |

该 DLL 与
[`TARGET-foo-dr-meter-1.0.8-x64-static-ff3556ad`](foo-dr-meter-1.0.8-x64-static.md)
来自同一个双架构 component。component 为 356313 bytes，SHA-256 为
`6dde22df1e3fcae256cd59ab39fd98adf3d840ebc3053fe4a2f7cced2998ade6`，
其中只包含 x86 与 x64 两个 DLL。component 和二进制本体均不进入仓库。

同一 x86 DLL 也用于
[`TARGET-foo-dr-meter-1.0.8-foobar2000-2.0-x86-win10-19045`](foo-dr-meter-1.0.8-foobar2000-2.0-x86.md)
的黑盒观测；静态目标与运行目标分别登记，避免把 DLL 内部事实和 foobar2000
宿主行为混为一类证据。

## 使用边界

- 静态分析使用 IDA Professional 9.1，从上述固定原始 DLL 在临时目录建立数据库。
- 临时数据库、诊断日志、目标二进制和反编译文本不进入仓库。
- 该目标可以证明 x86 DLL 内部的数据宽度和控制流，不能单独证明 foobar2000
  decoder 如何归一化各种源格式。
- 与 x64 目标共享若干算法控制规则不表示两者数值精度契约相同。
- 目标状态为 candidate，不表示兼容性已经 verified 或 accepted。
