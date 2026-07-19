# OBS-foo-dr-meter-108-x64-numeric-boundaries-v1-run1-20260719

## 结论

- 状态：`accepted_isolated_numeric_observation`
- 事实类别：固定 x64 target 的受控 numeric-leaf / analyzer-core 动态观测
- 日期：2026-07-19（Asia/Shanghai）
- target：`TARGET-foo-dr-meter-1.0.8-x64-static-ff3556ad`
- experiment：`foo-dr-meter-108-x64-numeric-boundaries-v1`
- 兼容性：`none`
- foobar parity：`not_assessed`
- 重复次数：一次；38 个向量各使用一个全新 worker

本记录没有安装或启动 foobar2000。固定 x64 DLL 在私有 staging 中加载后，由同一
隔离 worker 的两个窄入口分别执行 duration numeric leaf 或
init/push/finish analyzer core。38 个预注册向量全部满足判据：

| 家族 | 通过 |
| --- | ---: |
| duration 下侧/精确半值/上侧 | 24/24 |
| weighting track raw bits | 8/8 |
| weighting channel 前提 raw bits | 8/8 |
| weighting off/on 配对不变量 | 4/4 |
| histogram endpoint | 6/6 |

完整、去路径化结果见 [`suite.json`](suite.json)。它是 finite、key-sorted
canonical JSON，`summary.allMatched` 为 `true`。

## 固定身份

| 对象 | Byte length | SHA-256 |
| --- | ---: | --- |
| x64 target DLL | 424448 | `ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489` |
| x64 worker | 316928 | `9685bf13e69cce2f0920510b70e24c57cff4483b1c3296baada3f165704ca817` |
| `worker.cpp` | 86901 | `8be451dca6e20149bbdea22cc26f13aa43a4a80b4ec9af429f2c6e91272f7956` |
| core parent | 75969 | `47f4b8469ffb73f3b5a256e1dd3595fb500bdcafd2654d7429fdd68be6371a29` |
| boundary runner | 27789 | `2acb81258bd3f062fe3031a2f23d0668582c91d2e467d71fd115b0c57be8a824` |
| `shared.dll` | 142336 | `f860ee48f9e88a4da575c8114a82a11e3d25ceb9c8ce3405f646917cf07c7e4d` |
| `msvcp140.dll` | 579920 | `003da4807acdc912e67edba49be574daa5238bb7acff871d8666d16f8072ff89` |
| `vcruntime140.dll` | 109392 | `a8f950b4357ec12cfccddc9094cca56a3d5244b95e09ea6e9a746489f2d58736` |
| `vcruntime140_1.dll` | 49520 | `e4b533a94e02c574780e4b333fcf0889f65ed00d39e32c0fbbda2116f185873f` |
| system `ucrtbase.dll` environment artifact | 1046080 | `3c60056371f82e4744185b6f2fa0c69042b1e78804685944132974dd13f3b6d9` |
| semantic manifest | — | `881ff4d52e279510943bbb126db9a0818483bc839593784c575d12cd4a6fd684` |
| suite | 214997 | `28416daabebfb0291305b80328a5b2003b10606830051c370f90c78070f2901b` |

runtime profile 为 `fixed_foobar_2_25_10`。父进程和 worker 都复核 target、
runtime、worker 与输入身份；每个 request ID 又绑定操作、几何、选项和输入
SHA-256。目标与 runtime 二进制不进入仓库。

## Duration 半秒与进位

worker 直接调用固定 RVA `0x38540`，传入 renderer 静态调用点同形的 binary64
`decoded_frames / sample_rate` 和 `fractional_digits = 0`。固定目标自己的
`llround` 与 `free` named import 均在调用前按 IAT RVA 验证，目标分配也经目标
自己的 heap import 释放。

最短边界的 raw 结果为：

```text
499/1000      -> 0:00
1/2           -> 0:01
501/1000      -> 0:01
1499/1000     -> 0:01
3/2           -> 0:02
1501/1000     -> 0:02

22049/44100   -> 0:00
22050/44100   -> 0:01
22051/44100   -> 0:01
23999/48000   -> 0:00
24000/48000   -> 0:01
24001/48000   -> 0:01
```

`59.5s`、`3599.5s`、`86399.5s` 和 `604799.5s` 的下侧、精确半值和上侧也全部
匹配预注册结果；精确半值依次得到：

```text
59.5s       -> 1:00
3599.5s     -> 1:00:00
86399.5s    -> 1d 0:00:00
604799.5s   -> 1wk 0d 0:00:00
```

这与固定 renderer 的两处 `frames/rate -> format_timespan(..., 0)` 静态数据流
交叉后，支持实际列出输入上的半值远离零舍入以及 minute/hour/day/week token。
它没有执行完整 report renderer，所以不支持完整报告 byte parity。

## Multichannel loudness weighting

四个确定性 PCM scenario 都先要求公开 channel DR/RMS raw bits 满足构造前提，
再比较 track raw bits；不能只因最终值偶合就判定公式成立：

| scenario | off | on | 配对结果 |
| --- | --- | --- | --- |
| balanced 3ch | `41a00000` | `415a524a` | 仅 track 改变 |
| overall-RMS source 3ch | `41a00000` | `413714ce` | 仅 track 改变 |
| two-channel gate | `41a00000` | `41a00000` | track 不变 |
| partial-silence 3ch | `41555555` | `413d1746` | 仅 track 改变 |

每个 off/on 对的 channel results、session、channel state 与完整 histogram 摘要
逐项相同。结果支持固定目标在 `channels > 2 && option == on` 时使用内部
binary64 channel overall RMS 对内部 binary64 channel DR 加权；双声道仍走算术
均值，部分静音声道以零 RMS 获得零权重。三声道全静音会进入静态 `0/0` 路径，
不在本次有限正向契约内。

## Histogram clamp

六个单声道、单整窗输入分别以 `-101/-100/-99/-1/0/+1 dB` RMS 驱动 core。
finish 后、cleanup 前保存的是 10001-bin channel-major `u32le` slice 的摘要与
SHA-256：

| 输入 | bin 0 | bin 10000 | slice SHA-256 |
| ---: | ---: | ---: | --- |
| -101 dB | 1 | 0 | `cd3266d2d5b760fd0d911eaec808925ecfbecd23d8f9dfa1b287d2c7cfeb47fb` |
| -100 dB | 1 | 0 | `cd3266d2d5b760fd0d911eaec808925ecfbecd23d8f9dfa1b287d2c7cfeb47fb` |
| -99 dB | 0 | 0 | `999d5f6ac1bb73f39a1383aae4517e500887c28eed020fa683db9baba253bf23` |
| -1 dB | 0 | 0 | `e1c5e17e5de172381434008bd070b2f40ab8c3ab0a7dd7c3162625cf784d02d2` |
| 0 dB | 0 | 1 | `2655d0e59bc48cf76bac9a9e6672c3bdd656c6aec8fbfb65a5806c800e1549c9` |
| +1 dB | 0 | 1 | `2655d0e59bc48cf76bac9a9e6672c3bdd656c6aec8fbfb65a5806c800e1549c9` |

每项 `totalCount = 1` 且 `nonzeroBinCount = 1`。越过下端的输入与精确下端具有相同
slice；越过上端的输入与精确上端也具有相同 slice，而两个内侧输入落在不同内部
bin。这直接支持固定目标把量化 RMS key clamp 到 `[-100, 0] dB` 两端。

## 隔离边界与环境

每个向量都经过以下边界：

1. 父进程确定性生成 finite interleaved binary64 PCM，并绑定精确 SHA-256 与几何；
2. worker 锁定并复核 source bytes，在 protected-DACL 私有目录中重新 staging；
3. 目标以 DLL-load-directory 与 System32-only flags 加载，实际 module bytes
   再次复核；
4. 目标的 13 个普通 `shared.dll` IAT slot 在 numeric operation 全程由 fail-fast
   tripwire 接管；
5. worker 固定并记录 x87/MXCSR，执行一个请求，恢复 IAT、卸载目标和 runtime，
   再输出一行严格 JSON。

运行环境为 Windows 10.0.19045.6466 x64、Python 3.9.13、clang-cl 22.1.8
`x86_64-pc-windows-msvc`、MASM x64 14.51.36246、CMake 4.4.0 和 Ninja
1.13.2。CPU 为 `11th Gen Intel(R) Core(TM) i7-11800H @ 2.30GHz`
（`GenuineIntel`，x64）；系统 `ucrtbase.dll` file/product version 为
`10.0.19041.3636`。worker 为
`COFF-x86-64 / IMAGE_FILE_MACHINE_AMD64`，自身只直接导入 `bcrypt.dll`、
`ADVAPI32.dll` 与 `KERNEL32.dll`；native JSON CTest 为 1/1 通过。

固定 target 静态导入 `api-ms-win-crt-math-l1-1-0.dll` 与
`api-ms-win-crt-heap-l1-1-0.dll`；worker 在 duration 调用前验证实际解析后的
`llround`/`free` IAT slot 为非空可执行地址。suite 内嵌四个私有 runtime 身份，
上表另保存本次 Windows 环境的系统 UCRT artifact 身份；它仍不把这一轮结果外推
到其他 OS/UCRT/CPU。

## 证据边界

本 observation 与固定 x64 静态记录交叉后，把以下会改变 per-track 可见输出的
规则由单类静态证据补为 E2：

- duration 的 `frames/rate -> llround` 半秒规则；
- hour/day/week duration token；
- 可选 multichannel loudness weighting 及 `channels > 2` 门槛；
- RMS histogram 的 `[-100, 0] dB` endpoint clamp。

它不验证 foobar decoder、component registration、playlist、metadata、album
grouping、完整 renderer、其他插件版本、x86 或任意输入的通用等价。它也不是
MacinMeter 与目标的差分记录。候选规格因此继续保持
`CandidateV1 / Unverified`；这次收口消除的是已知、可能改变 per-track 输出的
未交叉规则，不是把有限 evidence 升格成兼容声明。
