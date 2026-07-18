# SA-foobar2000-2.0-x64-wave-decoder-20260718

## 身份与方法

- 事实类别：static-analysis
- 目标：
  [`TARGET-foobar2000-2.0-x64-wave-decoder-ea6e9c52-cf5b2a86`](../targets/foobar2000-2.0-x64-wave-decoder.md)
- 工具：IDA Professional 9.1，headless autoanalysis 与 Hex-Rays
- 分析日期：2026-07-18（UTC+08:00）
- `foo_input_std.dll` SHA-256：
  `cf5b2a86dcb750afcfe6ba5860f0937c068dbc502d7ae35b5837425eb861205f`

分析数据库只在临时目录中建立。目标二进制、数据库和反编译文本均不进入仓库；
下文仅登记对固定映像独立复核后的函数身份、地址、公式与证据边界。

## WAV PCM 调用链

| 地址 | 本记录中的作用 |
| --- | --- |
| `0x18008D528..0x18008D82E` | 识别 PCM、IEEE float 与 WAVEFORMATEXTENSIBLE subformat，建立直读 decoder |
| `0x18008D4E0..0x18008D525` | 提取采样率、声道数、container bits 和 block alignment |
| `0x18008D830..0x18008D968` | 读取 frame-aligned data，并向整数或浮点转换器分派 |
| `0x180178F10..0x180179043` | little-endian U8/S16/S24/S32 到 binary64 PCM |
| `0x180178970..0x180178AD2` | packed little-endian signed 24-bit 转换 |
| `0x180179044..0x18017917C` | IEEE float32/float64 到 binary64 PCM |
| `0x1800EF8A0..0x1800EF9CC` | binary32 到 binary64 的逐样本精确扩宽 |

格式选择在 `0x18008D597..0x18008D64C` 完成：format tag `1` 进入 integer，
tag `3` 进入 IEEE float；tag `0xFFFE` 根据 PCM 或 IEEE float subformat GUID
进入同一两条路径。`0x18008D6CC..0x18008D6ED` 将整数 container bits 限定为
8/16/24/32，将 float bits 限定为 32/64。

`0x18008D8FB..0x18008D93E` 是最终分派点。两个转换器取得的目标 buffer 是
binary64 sample buffer，随后连同 frame 数、声道数和 channel mask 写入
`audio_chunk<double>`。

## 归一化公式

令 `u8` 为无符号八位码，`sN` 为按 two's-complement 解释的有符号 N 位码，
`f32`/`f64` 为对应 IEEE-754 字节的数值。合法 little-endian WAV 在该固定
decoder 中产生：

| WAV sample | `audio_chunk<double>` sample | 直接证据 |
| --- | --- | --- |
| U8 PCM | `(u8 - 128) × 2^-7` | `0x180178FC8..0x180178FF5`；`0x1801DDC88 = 2^-7` |
| S16 PCM | `s16 × 2^-15` | helper call `0x180178FAF..0x180178FC0`，gain `1.0`；同映像内联交叉证据见下文 |
| packed S24 PCM | `sign_extend_24(raw) × 2^-23` | `0x180178970..0x180178AD1`；`0x1801DDB20 = 2^-23` |
| S32 PCM | `s32 × 2^-31` | helper call `0x180178F86..0x180178F97`，gain `1.0`；同映像内联交叉证据见下文 |
| F32 PCM | `f64(f32)`，精确扩宽 | `0x1801790C9..0x1801790D2` 调用 `0x1800EF8A0`；后者使用 `cvtps2pd` |
| F64 PCM | 原 binary64 值 | `0x18017910A..0x180179118` 直接复制八字节 sample |

因此整数负满幅映射为 `-1.0`，正满幅分别映射为：

```text
U8:  1 - 2^-7
S16: 1 - 2^-15
S24: 1 - 2^-23
S32: 1 - 2^-31
```

float32 与 float64 路径不乘增益，也不在该转换层 clamp 到 `[-1, 1]`。
float32 先按 binary32 取值再精确扩宽；float64 sample bits 直接成为输出
binary64 sample。

## S16/S32 helper 的证据边界

WAV 的 S16 与 S32 fast path 分别调用具名导入：

```text
audio_math::convert_from_int16(short const*, size_t, double*, double)
audio_math::convert_from_int32(int const*, size_t, double*, double)
```

两处调用都从 `0x1801DDE00` 传入 gain `1.0`。helper 函数体属于导入的
`shared` 模块，不在本记录固定的 `foo_input_std.dll` 内，因此不能把其内部
指令冒充成本 DLL 的直接证据。

同一固定映像中的另一条 PCM-to-double 路径提供了独立交叉检查：

- `0x1800495B7..0x18004968E` 对 signed 16-bit 明确乘
  `0x1801DDB58 = 2^-15`；
- `0x1800496AF..0x18004977F` 对 signed 32-bit 明确乘
  `0x1801DDAF0 = 2^-31`。

结合具名 helper、gain `1.0` 和同映像等价内联实现，本记录将 `/2^15` 与
`/2^31` 作为高置信静态结论。若未来要求对这两项做逐指令闭环，应另行固定并分析
实际提供 `audio_math` exports 的 shared 宿主模块；当前记录不把该缺口提升为
不存在。

## WAVEFORMATEXTENSIBLE valid bits

`0x18008D4E0` 从 `WAVEFORMATEX.wBitsPerSample`，即 container bits，建立转换位宽
和 block alignment。extensible 分支会验证 subformat GUID、读取 channel mask，
但本转换路径不读取 `wValidBitsPerSample` 来选择额外除数或右移位数。

所以 extensible PCM 仍按 8/16/24/32-bit container 中的原始有符号码应用上表
公式。符合 WAVEFORMATEXTENSIBLE 约定、将较少 valid bits 左对齐到较宽 container
的输入会自然得到相应有效位宽的归一化；非标准右对齐或无效 valid-bits 声明不由
本记录补猜。

## 结论与限制

固定 x64 decoder 的六种普通 WAV PCM 转换已经静态收口。相同 PCM 的 safe
corpus 运行可以验证完整宿主与插件链路，但不再承担猜测上述除数、符号解释或
float 扩宽公式的职责。

本记录不声称：

- x86 `foo_input_std` 与 x64 在浮点数据宽度和边界上完全相同；
- 下游插件一定接受非有限或超满幅 float；
- zero-frame、损坏容器或错误 UI 的外围行为已经由这些转换函数确定；
- FLAC、AIFF、压缩 WAVE 或其他 decoder 使用同一入口；
- 任一 MacinMeter profile 已达到 reference parity。
