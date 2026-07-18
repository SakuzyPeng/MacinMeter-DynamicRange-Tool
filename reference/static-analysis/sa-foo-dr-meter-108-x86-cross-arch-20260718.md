# SA-foo-dr-meter-108-x86-cross-arch-20260718

## 身份与方法

- 事实类别：static-analysis
- x86 目标：
  [`TARGET-foo-dr-meter-1.0.8-x86-static-6debd1d6`](../targets/foo-dr-meter-1.0.8-x86-static.md)
- x64 对照：
  [`TARGET-foo-dr-meter-1.0.8-x64-static-ff3556ad`](../targets/foo-dr-meter-1.0.8-x64-static.md)
- 工具：IDA Professional 9.1 / Hex-Rays decompiler
- 分析日期：2026-07-18（UTC+08:00）
- x86 输入 SHA-256：
  `6debd1d665cec975853341fb4ae360d2187d2bb0c595eedde9e38b4b77301862`
- x64 输入 SHA-256：
  `ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489`

x86 数据库从固定原始 DLL 在临时目录建立；x64 结论在既有本地数据库中复核。
原始数据库、临时数据库、诊断日志、目标二进制和反编译文本均不进入仓库。本记录
只保存独立撰写的函数身份、指令级数据宽度和算法事实。

## x86 函数身份

| 地址范围 | 证据范围 |
| --- | --- |
| `0x100090C0..0x10009424` | 分析器初始化、窗长和累计器布局 |
| `0x10009430..0x10009758` | interleaved PCM 消费、整窗和尾窗提交 |
| `0x10009760..0x10009B5A` | loud RMS、peak 选择、channel/track DR 与公开结果 |
| `0x1000DA60..0x1000E040` | album 分组聚合与结果写回 |
| `0x10033080..0x10035ADF` | 文本报告、overall peak/RMS 和显示舍入 |
| `0x10036CF0..0x100374D8` | foobar2000 解码循环与分析器调用边界 |

地址只适用于本记录中的 x86 SHA-256。对应的 x64 核心身份见
[`SA-foo-dr-meter-108-x64-20260718`](sa-foo-dr-meter-108-x64-20260718.md)。
x64 的 album 聚合与报告路径分别位于
`0x18000E540..0x18000ED05` 和 `0x18003F280..0x180042934`。

## 共同控制规则

x86 与 x64 构建使用相同的窗长系数。x86 常量的原始八字节为
`cf1276f35b080840`，与 x64 登记的 binary64 bits
`0x4008085bf37612cf` 相同。两个构建还共享以下控制规则：

- 窗口跨 decoder block 连续累计；
- 任意非空 EOF 尾窗提交，精确整窗不增加虚拟窗口；
- RMS histogram 使用 centi-dB key、`[-100,0] dB` clamp 和 10001 bins；
- loud 目标为 `max(1,floor(window_count/5))`，边界 bin 整组纳入；
- peak 使用两级在线排名，DR 优先 secondary，负值时回退 primary 并限制到零；
- 默认 track 聚合包含静音 DR0 和 LFE；
- channel DR、channel overall RMS、primary peak 和 track DR 的公开存储为 binary32。

这些共同控制规则不构成逐位数值等价。固定 x86 黑盒报告对 15 个判别输入的
结果与 x64 控制规则预测一致，只能交叉印证被这些输入覆盖的行为。

## 架构特定精度

| 阶段 | x86 1.0.8 | x64 1.0.8 |
| --- | --- | --- |
| foobar2000 audio chunk | interleaved binary32 | interleaved binary64 |
| 单样本绝对值与 current peak | binary32 | binary64 |
| 单样本平方 | 先以 binary32 相乘，再提升到 binary64 | binary64 相乘 |
| sum of squares | binary64 累计 | binary64 累计 |
| window RMS 与 RMS histogram | binary64 | binary64 |
| peak logarithm | `log10f` 输入/结果 | `log10` binary64 |
| primary/secondary amplitude | binary32 | binary64 |
| 已保存 peak key | binary32 | binary64 |
| loud power、DR 和内部 track 聚合 | binary64 | binary64 |
| 公开 channel/track 数值 | binary32 | binary32 |

x86 peak key 先由 `log10f` 结果形成 centi-dB 值，再以 binary32 保存；后续候选
与已经窄化的 key 比较。x64 则保留 binary64 peak 和 key。因此即使两个窗口最终
落在同一整数 centi-dB key，所有浮点边界和到达顺序行为也不能跨架构外推。

这不是仍待更多样本证明的假设，而是两个固定二进制的数据宽度差异。有限 corpus
不能证明“所有浮点边界一致”；规格必须明确选择目标架构或分别定义数值契约。

## Album 聚合

两个构建的 album 聚合控制流同构。对于同一 grouping 中参与聚合的结果项：

- official 值先把每个 binary32 track DR 提升到 binary64，做不加权算术平均，
  再把平均值窄化为 binary32；
- track-length weighting 开启时，时长为 decoded frames 除以 sample rate；
  sample rate 为零的防御分支使用 `frames / 44100`；
- weighted 值在 binary64 中累计
  `track_dr_f32 × duration` 和总时长，除法后窄化为 binary32；
- weighting 关闭或总时长为零时，effective 值回退到 official；
- 数值 DR0 不会被 album 聚合排除；
- 报告的 “Official DR value” 使用 official binary32，“Weighted DR value”
  使用 effective binary32；两者都在 `value + 0.5` 后截断为非负整数。

因此 official 的输入精度、length weighting 公式、最终窄化与显示舍入已经由
静态数据流确定，不需要再用导出样本在多个相容模型间猜测。

## 报告 peak、RMS 与格式化

固定 x64 报告路径、以及结构相同的 x86 路径，确定了：

- report peak 是所有 channel primary peak binary32 的最大值；
- report RMS 将每个 channel overall RMS binary32 先以 binary32 相乘，再提升到
  binary64 求和，除以声道数后开平方；
- peak/RMS 的 dB 换算使用 binary64 `20 × log10(linear)`，随后窄化为 binary32
  供文本格式化；
- 接近零的 dB 值有显式 centi-dB `lround` 修正；
- 数字格式化显式使用 `C` locale，报告模板和 CRLF 来自固定静态字符串；
- channel mask 到标签的分支和标签表位于同一 renderer，可继续静态登记，不需要
  为映射本身生成音频样本。

bitrate、codec 名称和实际 channel mask 的值来自宿主 metadata；DLL 内的格式化
规则不能反推出宿主如何生成这些值。

## 空流与非默认分支

- 如果已经构造核心分析器但没有推入 frame，结算路径产生零 RMS/peak 和数值
  DR0。
- foobar2000 orchestration 在构造核心分析器前要求第一次 decode call 返回 audio
  chunk；零帧源在该层走 data error，而不是产生普通报告。
- 可选的多声道 loudness weighting 对全静音输入形成零除零并得到非有限值。该
  设置在候选 profile 中关闭，MacinMeter 的公开结果继续要求有限数值。

最终错误对话框、日志措辞和 decoder 对损坏文件的外围行为仍属于宿主运行行为。

## 对当前 MacinMeter 候选的含义

MacinMeter 0.2.0 接受 interleaved binary32 PCM，将每个样本提升到 binary64 后
做平方、peak、对数和累计，并以整数 centi-dB 保存 peak key。因此它：

- 不等同 x64 目标，因为进入分析器前已经丢失 binary64 PCM 精度；
- 不等同 x86 目标，因为 x86 的样本平方、peak logarithm 和已保存 peak key
  仍有 binary32 运算或存储；
- 只能继续命名为 `FooDrMeter108CandidateV1 / Unverified`。

现有 x86 15/15 黑盒结果支持候选控制规则，但不能升级为任一架构的 bit-exact
或 reference parity 声明。

## 不再需要与仍需运行证据的边界

下列 DLL 内部事实已经可以由静态分析承担，不需要仅为“再确认”而导出报告：

- block/window、EOF、精确整窗、RMS clamp 和所有采样率的窗长控制；
- channel/track binary32 存储点和架构特定 peak 精度；
- album official、length weighting、DR0 纳入、最终窄化和显示舍入；
- report peak/RMS、`C` locale、固定文本和 channel-label 分支；
- 非默认多声道 weighting 的全静音除零路径。

实现仍应为这些规则生成本地、可重复的回归 fixture；“不需要黑盒”不表示“不需要
测试”。

只有在项目要声明 foobar2000 端到端行为时，以下边界才需要继续逆向固定宿主/
decoder，或在无法静态确定时采集运行证据：

- decoder 对 integer PCM、float PCM、超满幅和各容器的归一化；
- 零帧、损坏文件和 decoder error 到最终 UI/日志的映射；
- 宿主生成 channel mask、bitrate 和 codec metadata 的规则；
- 固定 Windows CRT/libm 在最后一位边界的行为，若目标是 bit-exact 而非明确容差。

`foo_dr_meter` 1.0.3 与 1.0.8 的比较也不应由 1.0.8 样本外推；必须先固定并哈希
1.0.3 原始二进制，再建立独立静态差分记录。
