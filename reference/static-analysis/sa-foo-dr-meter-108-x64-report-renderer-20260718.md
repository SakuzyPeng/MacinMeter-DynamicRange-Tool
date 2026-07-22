# SA-foo-dr-meter-108-x64-report-renderer-20260718

## 身份与方法

- 事实类别：static-analysis
- 目标：
  [`TARGET-foo-dr-meter-1.0.8-x64-static-ff3556ad`](../targets/foo-dr-meter-1.0.8-x64-static.md)
- 工具：IDA Professional 9.1 / Hex-Rays decompiler
- 分析日期：2026-07-18（UTC+08:00）
- image base：`0x180000000`
- 输入 SHA-256：
  `ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489`

本记录沿固定 x64 数据库中的调用关系、数据宽度、分支和静态字符串复核报告
renderer。目标二进制、IDA 数据库、诊断日志和反编译文本均不进入仓库；这里只保存
独立撰写的函数身份、数据流结论与证据边界。

## 函数身份

| 地址或范围 | 受控身份 | 证据范围 |
| --- | --- | --- |
| `0x18003F280..0x180042934` | report renderer | track 行、duration、channel 列和 footer 的文本组装 |
| `0x180038540` | duration/timespan 格式化叶子 | binary64 秒数经 `llround` 舍入并形成 duration token |
| `0x1800377C0` | duration formatter | 分解并格式化 minute、hour、day 和 week |
| `0x1800452D0` | channel label mapper | channel ordinal 到固定标签或 fallback 文本 |

地址只适用于上述 SHA-256 和 image base。受控身份用于说明本记录中的作用，不是目标
原始符号名。

## Duration 数据流

对已有有效 track 结果，renderer 使用 decoded frame 数和实际 PCM sample rate：

```text
seconds_f64 = f64(decoded_frames) / f64(sample_rate)
seconds_i64 = llround(seconds_f64)
duration_text = format_duration(seconds_i64)
```

`0x18003F280..0x180042934` 中的报告路径先形成 binary64 秒数，经
`sub_180038540` 叶子内部的固定 `llround` 导入得到整数秒，再交给
`sub_1800377C0`。该叶子的 ABI、IAT 与清理契约进一步登记在
[`SA-foo-dr-meter-108-x64-duration-leaf-20260719`](sa-foo-dr-meter-108-x64-duration-leaf-20260719.md)。
在本记录限定的非负、有限 duration 上，半值远离零等价于半值向上；renderer
不是把 `decoded_frames / sample_rate` 预先做整数除法。

`sub_1800377C0` 按 60 秒、60 分、24 小时和 7 天逐级分解，并选择以下形状：

| 范围 | 文本形状 |
| --- | --- |
| 小于 1 小时 | `m:ss` |
| 小于 1 天 | `h:mm:ss` |
| 小于 1 周 | `Dd h:mm:ss` |
| 至少 1 周 | `Wwk Dd h:mm:ss` |

低位的秒、分使用两位零填充；hour、day 和 week 不补前导零。这里固定的是
renderer 对整数秒的分支和模板，不推断宿主如何产生 decoded frame 数或 sample
rate。

### 后续动态交叉

2026-07-19 的固定 x64 数值边界 observation
[`OBS-foo-dr-meter-108-x64-numeric-boundaries-v1-run1-20260719`](../observations/obs-foo-dr-meter-108-x64-numeric-boundaries-v1-run1-20260719/record.md)
直接调用上述 `0x180038540` 叶子。其 `suite.json` SHA-256 为
`b5a99ff50eb78eeb2258fb15f5d75d8d92978743abb4dabe9639f3453bd570d3`，
target SHA-256 与本记录相同，24 个预注册向量全部匹配：

- `0.5s`、`1.5s` 及 44.1/48 kHz 精确半秒的下侧、半值和上侧；
- `59.5s`、`3599.5s`、`86399.5s`、`604799.5s` 的下侧、半值和上侧；
- 由此实际得到 `m:ss`、`h:mm:ss`、`Dd h:mm:ss` 与
  `Wwk Dd h:mm:ss` 四类 token，并覆盖 minute/hour/day/week 进位。

该运行把固定静态调用数据流与固定数值叶子的动态输出交叉为 E2。它没有执行完整
report renderer；因此只支持非负 finite、`fractionalDigits = 0` 且实际覆盖范围
内的舍入与 token 分支，不证明 decoder 如何产生 frame/rate、负值/非有限输入、
超大整数转换、其他小数位或整份报告 byte parity。

## Channel label mapper

`sub_1800452D0` 对 ordinal `0..17` 使用固定标签：

| Ordinal | 标签 | Ordinal | 标签 | Ordinal | 标签 |
| ---: | --- | ---: | --- | ---: | --- |
| 0 | `FL` | 6 | `FCL` | 12 | `TFL` |
| 1 | `FR` | 7 | `FCR` | 13 | `TFC` |
| 2 | `FC` | 8 | `BC` | 14 | `TFR` |
| 3 | `LFE` | 9 | `SL` | 15 | `TBL` |
| 4 | `BL` | 10 | `SR` | 16 | `TBC` |
| 5 | `BR` | 11 | `TC` | 17 | `TBR` |

ordinal 大于等于 18 时进入数字化的 `Ch %u` fallback；名称无法形成时使用 `?`。
该函数只能证明 renderer 收到一个 ordinal 后如何命名，不能证明 foobar2000
如何从文件、decoder channel mask 或宿主 channel configuration 生成这个 ordinal。

## Footer 边界

report renderer 同时组装逐 track 行、track 数量、插件聚合得到的
“Official DR value”/“Weighted DR value”，以及 sample rate、channel、bit depth、
bitrate 和 codec 等 footer 列表。该调用图可以确认：

- DR、track count 和 duration 等插件结果在 renderer 内的消费与格式化边界；
- channel mapper 只负责名称，不生成宿主 channel mask；
- sample rate、bit depth、bitrate 和 codec 等值由上游 metadata 交给 renderer。

因此 DLL 静态分析不能反推出宿主如何解码 PCM、选择实际 sample rate、生成 channel
mask、计算 bitrate 或编码 codec 名称。固定 observation 中的 `32761` bit-depth
token 仍按
[`SA-foobar2000-2.25.10-x64-wave-metadata-20260718`](sa-foobar2000-2.25.10-x64-wave-metadata-20260718.md)
分类为外围 metadata-report 异常，不是核心 DR 或真实 PCM 位深。

## 证据等级

本节的 E2 动态来源包括固定
[`OBS-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718`](../observations/obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/observation.json)
及上述 numeric-boundaries observation。前者覆盖完整报告中实际出现的短时
duration 与 channel 列；后者只直接执行固定 duration 数值叶子。

| 规则 | 等级 | 依据与限制 |
| --- | --- | --- |
| duration 使用 `frames / sample_rate` 的 binary64 秒数并经 `llround` | E2 | 固定 renderer 数据流与 duration 叶子 ABI；numeric-boundaries 的 24/24 覆盖多个精确半秒与进位三元组。限于非负 finite、`fractionalDigits = 0` 和实际覆盖 frame 范围。 |
| `m:ss` 短时长格式 | E2 | 静态分支、固定 x64 safe-master 的 39 个报告 token，以及 numeric-boundaries 的短时/分钟向量相互印证。 |
| `h:mm:ss`、day 和 week 格式 | E2 | 固定分解与模板；numeric-boundaries 直接覆盖 hour/day/week 进位的下侧、精确半值和上侧。这里是数值叶子 token，不是完整 renderer 动态记录。 |
| 已观测 ordinal `0..5, 9, 10` 的 `FL, FR, FC, LFE, BL, BR, SL, SR` 标签 | E2 | 静态表与固定 x64 safe-master 的 1/2/3/6/8 声道报告列相互印证。 |
| ordinal `6..8, 11..17` 与 `>=18` 的 `Ch %u`/`?` fallback | E1 | 固定 mapper 的静态表和分支；现有 observation 未覆盖。 |
| renderer 消费 footer metadata | E1 | 固定 DLL 调用和格式化路径。 |
| 宿主生成 footer metadata 的规则 | U | 输入值来自宿主/decoder；本 DLL renderer 不能确定其来源或正确性。 |

同一固定二进制中的多处静态路径仍属于单类证据，不会因函数数量自动升级。E2 只适用
于被固定黑盒 observation 实际覆盖的文本分支。

## 限制

- 没有动态跟踪 renderer 中间对象；后续 direct-call observation 也只执行数值
  叶子，因此本记录不产生 E3 或完整 renderer parity 规则。
- 没有复核负 duration、非有限秒数、整数溢出或零 sample rate；它们不属于当前
  有效 PCM/report 契约。
- 本记录不声明完整报告 byte-for-byte parity，也不把模板、locale 或 CRLF 外推到
  其他版本、架构或宿主。
- 固定报告只能交叉印证实际出现的短时长和 channel ordinal；数值边界记录补充了
  duration 叶子的长时分支，但不能把它外推为完整报告执行。
- footer 的宿主 metadata、decoder 行为和最终 UI/错误呈现仍须使用各自固定 target
  研究，不能由 `foo_dr_meter.dll` renderer 补猜。
