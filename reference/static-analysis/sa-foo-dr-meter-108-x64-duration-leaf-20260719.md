# SA-foo-dr-meter-108-x64-duration-leaf-20260719

## 身份与目的

- 事实类别：static-analysis
- target：
  [`TARGET-foo-dr-meter-1.0.8-x64-static-ff3556ad`](../targets/foo-dr-meter-1.0.8-x64-static.md)
- 输入 SHA-256：
  `ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489`
- image base：`0x180000000`
- 工具：IDA Professional 9.1 / Hex-Rays、LLVM objdump
- 分析日期：2026-07-19（Asia/Shanghai）

本记录把既有 report-renderer 记录中的 duration 数据流进一步窄化为一个可由
隔离 worker 安全调用的纯数值叶子。它只登记固定二进制的 ABI、调用点、导入和
清理契约，不保存目标二进制、IDA 数据库或反编译文本。

## Renderer 调用点

固定 renderer 的两处 duration 调用点分别位于：

- `0x18004080B..0x18004084D`
- `0x180041C66..0x180041CB8`

两处都执行同一数据流：

```text
frames_f64  = f64(track_result.decoded_frames_u64)
rate_f64    = f64(track_result.sample_rate_u32)
seconds_f64 = frames_f64 / rate_f64
format_timespan(out, seconds_f64, 0)
```

`decoded_frames` 来自 track result `+0x20`，`sample_rate` 来自 `+0x14`。对
`u64` 的 binary64 转换包含最高位路径；当前有效证据矩阵把 frame 数限制在
`2^53 - 1` 以内，避免把整数不可精确表示混入半秒舍入问题。

## `0x180038540` 的受控 ABI

`0x180038540` 不是裸 `llround(double)`。它是 duration/timespan 格式化叶子，
Microsoft x64 ABI 可登记为：

```cpp
void *format_timespan(
    void *out_0x30,
    double seconds,
    std::uint32_t fractional_digits
);
```

- `RCX`：调用方提供的 `0x30` byte 输出对象；
- `XMM1`：第二参数 `seconds`；
- `R8D`：小数位数；报告路径固定传入 `0`；
- `RAX`：返回同一个输出对象。

函数初始化输出对象，在非负有限路径上把
`seconds × 10^fractional_digits` 送入固定 UCRT math import 的 `llround`，
再把整数部分交给 `0x1800377C0` 分解为 week/day/hour/minute/second 文本。
当 `fractional_digits == 0` 时，不追加小数部分。

固定导入槽为：

| 导入 | IAT RVA |
| --- | ---: |
| `api-ms-win-crt-math-l1-1-0.dll!lround` | `0x49858` |
| `api-ms-win-crt-math-l1-1-0.dll!llround` | `0x49860` |
| `api-ms-win-crt-heap-l1-1-0.dll!free` | `0x497F0` |

analyzer core 的 peak/histogram 量化使用 `lround`；duration 叶子使用相邻但独立的
`llround`。因此既有 core observation 不能替代 duration 半秒观测。

## 输出与清理

输出文本指针位于对象 `+0x08`。对象持有的分配指针位于 `+0x18`；调用者在消费
文本后经目标自己的 `free` IAT 槽释放。隔离 worker 必须遵循同一规则，不能用
自身静态链接 CRT 的 `free` 释放目标分配。

安全的 direct-call 边界为：

1. 固定 target/runtime 身份并通过私有 staging 加载；
2. 验证 `0x38540` 位于可执行 section；
3. arm 固定目标全部 13 个 `shared.dll` 普通 IAT slot 的 fail-fast tripwire；
4. 固定浮点控制位，以 `0x30` byte、16-byte 对齐的零初始化对象调用叶子；
5. 要求返回值等于输出对象，文本为有界、NUL 结尾的 ASCII duration token；
6. 在 target unload 前经目标 `free` IAT 清理；
7. 恢复 IAT、卸载 target/runtime 并恢复浮点环境。

这里不调用完整 `0x18003F280..0x180042934` renderer。完整 renderer 依赖 track
table、宿主对象、metadata、异常清理和字符串组装上下文；跳入其中段或伪造这些
对象会扩大而不是收紧证据边界。

## 证据边界

本记录与
[`SA-foo-dr-meter-108-x64-report-renderer-20260718`](sa-foo-dr-meter-108-x64-report-renderer-20260718.md)
共同支持：

- renderer 确实把 binary64 `frames / sample_rate` 送入该叶子；
- 该叶子确实经固定 `llround` 导入完成整数秒舍入；
- minute/hour/day/week 文本由同一个固定叶子及其整数 formatter 产生。

后续
[`OBS-foo-dr-meter-108-x64-numeric-boundaries-v1-run1-20260719`](../observations/obs-foo-dr-meter-108-x64-numeric-boundaries-v1-run1-20260719/record.md)
已用 24 个 half/carry 向量直接执行该叶子，并与本静态数据流交叉为 E2。该动态
记录仍不会变成 foobar decoder、component lifecycle、完整 report byte parity
或 E3 证据。
