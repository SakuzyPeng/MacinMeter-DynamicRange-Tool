# SA-foobar2000-2.25.10-x64-wave-metadata-20260718

## 身份与方法

- 事实类别：static-analysis
- runtime target：
  [`TARGET-foo-dr-meter-1.0.8-foobar2000-2.25.10-x64-win10-19045`](../targets/foo-dr-meter-1.0.8-foobar2000-2.25.10-x64.md)
- `foo_input_std.dll` SHA-256：
  `46a4b9c4515fae55add895e12d30602f73944959f0e0f7acf7122e6562b51651`
- `foo_dr_meter.dll` SHA-256：
  `ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489`
- 方法：Homebrew LLVM 21.1.8 的 `llvm-readobj`、`llvm-objdump`，并与 foobar2000 SDK
  2025-03-07 的 `file_info` 位深接口交叉检查
- 分析日期：2026-07-18（UTC+08:00）

本次只追踪报告 footer 的 bit-depth metadata，不把它扩写成 2.25.10 完整 decoder
分析。二进制从固定 runtime target 只读取得，临时文件、SDK 和反汇编输出不进入
仓库。

## 固定静态事实

当前 `foo_input_std.dll` 的 WAV metadata 路径在
`0x1800A1820..0x1800A18DE` 读取 `WAVEFORMATEX.wBitsPerSample` 并写入 source
bit-depth metadata；IEEE float 分支在 `0x1800A18F2..0x1800A1916` 另外写入
`floating-point` 属性。因而合法 float32 与 float64 WAV 的源 metadata 值应分别
是 32 与 64，不是特殊 sentinel。

SDK 交叉检查也明确区分：

- `bitspersample`：普通整数值；
- `bitspersample_extra=floating-point`：浮点标记；
- decoded bit depth 的有效范围：1 到 256。

固定 x64 插件在 `0x18004483D..0x1800448E3` 读取
`decoded_bitspersample`，失败时回退 `bitspersample`，并以
`(value - 1) <= 255` 过滤。`0x180044E3D..0x180044E42` 在分析后把结果写入
report metadata；核心 analyzer 初始化 `0x1800449EB..0x1800449F6` 不接收
bit depth。renderer 的 `0x18004017D..0x180040211` 只消费该 report metadata
来构建 footer 列表。

所以 bit depth 不参与 PCM sample、窗口、peak、RMS、DR 或 track 聚合。

## `32761` 的证据分类

固定 x64 observation 的原始 footer 是：

```text
Bits per sample:   8, 16, 24, 32, 32761, 32761
```

`32761` 超出 SDK 有效范围，与两个固定二进制的上述数据流均不相容，也没有已知
SDK sentinel 语义。它必须登记为本 target 的 host/plugin metadata-report 路径
异常，不能解释成 WAV 的真实位深，更不能进入解码或 DR 算法规格。

当前证据不能仅凭最终文本定位异常发生在 runtime metadata 对象、ABI 边界、列表
聚合还是其他外围状态。精确根因仍是 U；“它不是核心 DR 输入”则由静态数据流
直接确定。

## 处置边界

- 核心 DR conformance 忽略该 footer anomaly，但保留原始 token 和限制说明。
- 不追加音频样本：新的普通 WAV 不能区分上述外围根因。
- 只有将来要求整份报告 byte-for-byte 一致或 metadata parity 时，才应对
  2.25.10 runtime metadata/ABI 状态做专项动态跟踪或进一步静态追踪。
- 本记录没有分析 2.25.10 的全部 sample conversion；不得用它替代独立的完整
  decoder target。
