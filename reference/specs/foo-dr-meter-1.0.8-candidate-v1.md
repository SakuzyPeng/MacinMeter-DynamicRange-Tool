# foo_dr_meter 1.0.8 algorithm candidate v1

- 状态：candidate
- 规格标识：`foo-dr-meter-1.0.8-candidate-v1`
- 建立日期：2026-07-18
- 静态分析对象：`foo_dr_meter.dll` 1.0.8 x64 与 x86
- 黑盒观测对象：`foo_dr_meter.dll` 1.0.8 x86 / foobar2000 2.0 x86，以及
  `foo_dr_meter.dll` 1.0.8 x64 / foobar2000 2.25.10 x64
- 兼容性声明：无

## 1. 目的与边界

本规格以 1.0.8 x64 二进制为核心算法基准，使用固定 x86 二进制做跨架构静态
比较，并由 x86 初始实验、x64 complete-v2 safe-master、隔离 core 与专门数值
边界 observation 交叉验证实际覆盖的行为。它用于指导实现和 conformance 实验，
但不是：

- `foo_dr_meter` 1.0.3 或其他版本的规格；
- x86 与 x64 共用一个数值精度契约的声明；静态分析已确认两者不同；
- MacinMeter 已与任一参考目标数值兼容的声明；
- accepted 规格或任意 PCM 的穷尽 golden corpus。

本规格的核心伪代码只覆盖 foobar2000 已解码为有限、归一化、交错 PCM 后的分析
路径。容器探测、codec 解码、整数 PCM 归一化、报告 UI 和标签写入不属于核心
算法。

MacinMeter 0.2.0 的当前 codec/application 契约把解码结果统一为 finite
interleaved `f64`。固定 x64 目标的核心入口同样是 interleaved `f64`；固定 x86
目标接收 interleaved `f32`，且 sample square、peak logarithm 和已保存 peak key
仍有 binary32 运算或存储。当前生产实现以 binary64 样本分析，并以整数 centi-dB
保存 key、以 binary32 公开 channel/track DR。它修复了 float64 WAV 在分析前被
窄化的问题，但整数 key、运行库与有限 corpus 之外的边界仍不能由当前证据证明为
任意输入逐位等同固定 x64 目标。M1 不要求两种实现的内部表示同构。

默认 profile 固定以下会影响结果的插件设置：

| 设置 | 本规格值 |
| --- | --- |
| Weight multichannel DR by channel loudness | off |

album length weighting 的 off/on 数值分支在第 4.8 节作为辅助聚合规则单独说明；
自动保存、报告列与其他 UI 设置不属于本数值规格。

## 2. 证据来源

### 2.1 静态分析对象

本文使用独立登记的 x64 核心记录
[`SA-foo-dr-meter-108-x64-20260718`](../static-analysis/sa-foo-dr-meter-108-x64-20260718.md)：

| 属性 | 值 |
| --- | --- |
| 目标 | [`TARGET-foo-dr-meter-1.0.8-x64-static-ff3556ad`](../targets/foo-dr-meter-1.0.8-x64-static.md) |
| 文件 | `x64/foo_dr_meter.dll` |
| 版本 | 1.0.8 |
| 架构 | x86-64 PE32+ |
| 字节数 | 424448 |
| SHA-256 | `ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489` |
| 分析数据库 | IDA Professional 9.1；二进制和数据库不进入仓库 |

核心定位：

| 地址 | 本规格中的作用 |
| --- | --- |
| `0x180008410` | 初始化窗口、声道累计器、peak 状态和 10001-bin 直方图 |
| `0x1800089F0` | 消费交错 PCM、提交整窗或尾窗 |
| `0x180008DF0` | 选择 loud RMS、计算每声道 DR、聚合 track DR |
| `0x180044470` | foobar2000 解码循环与核心分析器编排 |

地址只对上述 SHA-256 的 x64 映像有效。

固定 x86 路径及跨架构精度比较登记在
[`SA-foo-dr-meter-108-x86-cross-arch-20260718`](../static-analysis/sa-foo-dr-meter-108-x86-cross-arch-20260718.md)：

| 属性 | 值 |
| --- | --- |
| 目标 | [`TARGET-foo-dr-meter-1.0.8-x86-static-6debd1d6`](../targets/foo-dr-meter-1.0.8-x86-static.md) |
| 文件 | `foo_dr_meter.dll` |
| 版本 | 1.0.8 |
| 架构 | x86 PE32 |
| 字节数 | 332288 |
| SHA-256 | `6debd1d665cec975853341fb4ae360d2187d2bb0c595eedde9e38b4b77301862` |
| 分析数据库 | IDA Professional 9.1；临时数据库、日志和二进制不进入仓库 |

固定 x64 report duration、channel label 和 footer renderer 边界另见
[`SA-foo-dr-meter-108-x64-report-renderer-20260718`](../static-analysis/sa-foo-dr-meter-108-x64-report-renderer-20260718.md)。
可隔离调用的 duration/timespan 叶子 ABI 与清理边界另见
[`SA-foo-dr-meter-108-x64-duration-leaf-20260719`](../static-analysis/sa-foo-dr-meter-108-x64-duration-leaf-20260719.md)。
它们与 x64 核心记录使用同一固定 DLL SHA-256，但仍属于同一类静态证据。

### 2.2 黑盒实验

#### 2.2.1 x86 初始判别

- 实验：
  [`EXP-foo-dr-meter-108-discriminating-v1`](../experiments/foo-dr-meter-108-discriminating-v1.md)
- 输入：
  [`foo-dr-meter-108-discriminating-v1.manifest.json`](../fixtures/foo-dr-meter-108-discriminating-v1.manifest.json)
- 目标：
  [`TARGET-foo-dr-meter-1.0.8-foobar2000-2.0-x86-win10-19045`](../targets/foo-dr-meter-1.0.8-foobar2000-2.0-x86.md)
- 观测：
  [`OBS-foo-dr-meter-108-x86-discriminating-v1-run1-20260718`](../observations/obs-foo-dr-meter-108-x86-discriminating-v1-run1-20260718/observation.json)

该 observation 是一次 GUI 导出，状态仍为 `preliminary_single_pass`。其 15 个
track DR 依次为：

```text
101=12  102=12  103=19  104=2   105=2
110=39  111=12  120=13  121=12
201=0   202=0   203=0
301=6   302=20  303=15
```

这些结果逐项符合 x64 静态路径的候选预测，也符合 x86 静态控制流。它们提供了
很强的控制规则交叉印证，但 x86 与 x64 的 PCM/peak 精度不同，不能把结果外推为
所有浮点边界相同，也不把未被 fixture 区分的内部状态自动提升为 E2。

#### 2.2.2 x64 complete-v2 safe master

- 实验：
  [`EXP-foo-dr-meter-108-complete-v2`](../experiments/foo-dr-meter-108-complete-v2.md)
- 输入：
  [`foo-dr-meter-108-complete-v2.manifest.json`](../fixtures/foo-dr-meter-108-complete-v2.manifest.json)
- 目标：
  [`TARGET-foo-dr-meter-1.0.8-foobar2000-2.25.10-x64-win10-19045`](../targets/foo-dr-meter-1.0.8-foobar2000-2.25.10-x64.md)
- 观测：
  [`OBS-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718`](../observations/obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/observation.json)

该 observation 是一次 x64 GUI 原始导出。39 个 safe-master fixture 各出现一次且
顺序与 manifest 完全一致，包含 39 个整数 track DR、62 个每声道 DR、39 个
overall peak、39 个 overall RMS 和 62 个每声道 RMS token。构造模型的独立比较
在这些字段上分别得到 39/39、62/62、39/39、39/39、62/62，差分数为 0；比较产物
见
[`foo-dr-meter-108-complete-v2-model-observation-comparison.json`](../experiments/foo-dr-meter-108-complete-v2-model-observation-comparison.json)。
构造模型不是 golden，固定 target 的原始 observation 才是参考事实。

`410_rms_half_f64_stereo` 与 `420_peak_half_f64_stereo` 各自把两个
source-f64 声道放在相邻量化边界两侧；对应的 `411`、`421` 则把相同构造另存为
float32 WAV，使两个声道塌缩到同一输入值。x64 导出分别保留
`[8.01, 8.00]` 与 `[18.99, 19.00]` 的区分，而 float32 对照分别成为
`[8.01, 8.01]` 与 `[19.00, 19.00]`。该运行结果动态支持 binary64
PCM/RMS/peak 精度路径，并与固定 DLL 的指令级数据宽度形成两类独立证据，因此
“x64 与 x86 精度契约不同”在这些可观察边界上达到 E2。

本次没有采集三个 isolated 输入，也没有重复运行；导出报告仍不能直接观察
histogram、peak key 或内部 binary64 DR。因此这些限制不能因 safe-master 字段
完全相符而消失。

### 2.3 隔离 x64 core 动态记录

固定目标的 accepted 隔离执行见
[`OBS-foo-dr-meter-108-x64-isolated-core-safe-master-run1-20260719`](../observations/obs-foo-dr-meter-108-x64-isolated-core-safe-master-run1-20260719/record.md)。
它不启动 foobar2000，而是对同一 SHA-256 的 x64 analyzer core 逐项执行
init/push/finish，每个输入使用全新 worker，并保存 result bits、session
before/after、channel finish state 与浮点控制状态。

39 项 safe-master 全部成功。不同 block size 的结果投影摘要一致，五个 fresh
worker 的重复输入摘要也一致。core result 重建出的整数 track DR、每声道两位
DR、每声道 RMS 与 overall peak 分别和既有导出达到 39/39、62/62、62/62、
39/39 精确匹配。

真实 `shared.dll` 只保留在固定 DLL 的 load/unload lifecycle；core 调用期间，
目标的 13 个普通 `shared.dll` IAT 入口全部由 fail-fast tripwire 接管。该记录是
固定 analyzer core 的直接动态证据，不执行 foobar decoder、album writer 或
report renderer，也不建立 host parity。

### 2.4 x64 数值边界隔离记录

固定 x64 数值边界 observation 见
[`OBS-foo-dr-meter-108-x64-numeric-boundaries-v1-run1-20260719`](../observations/obs-foo-dr-meter-108-x64-numeric-boundaries-v1-run1-20260719/record.md)。
`suite.json` 的 SHA-256 为
`28416daabebfb0291305b80328a5b2003b10606830051c370f90c78070f2901b`；
固定 worker SHA-256 为
`9685bf13e69cce2f0920510b70e24c57cff4483b1c3296baada3f165704ca817`。
它不启动 foobar2000，每个向量使用全新 worker，并在固定 runtime、浮点环境和
13 个 `shared.dll` IAT fail-fast tripwire 下直接执行同一 target 的 duration
数值叶子或 analyzer core。

该记录的预注册关系全部满足：

| 家族 | 结果 | 动态覆盖 |
| --- | ---: | --- |
| duration | 24/24 | `0.5s`、`1.5s`、44.1/48 kHz 半秒，以及 minute/hour/day/week 进位的下侧、精确半值和上侧 |
| multichannel weighting track bits | 8/8 | 四个 off/on 场景，包括三声道整体 RMS 来源、双声道门槛和部分静音 |
| weighting channel 前提 | 8/8 | 每次公开 channel DR/RMS raw bits 均先满足构造前提 |
| weighting pair invariants | 4/4 | 同一 PCM 的 channel result、session、channel state 与 histogram 在开关两侧保持一致 |
| histogram clamp | 6/6 | `-101/-100/-99/-1/0/+1 dB` 对 bin `0`、内部 bin 和 bin `10000` 的落点 |

因此该 observation 与既有静态数据流形成独立交叉证据，但只覆盖固定 x64 target
和上述有限向量。duration 是直接调用报告所用的数值叶子，不是完整 renderer；
weighting 是 per-track 多声道选项，不是 album length weighting；成功集也刻意
不包含全静音三声道的零分母路径。它不建立 decoder、host、metadata、album、
完整文本或 MacinMeter compatibility 声明。

### 2.5 当前实现差分

固定 reference observation 与身份明确的 MacinMeter 产物之间的记录见
[`CONF-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-20260718`](../conformance/conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-20260718/record.md)。
该记录固定的是 wire schema v2 时代的 DR-only pre/post-f64 比较，作为历史
conformance artifact 保持不变。

修正前整数 track DR 已为 39/39，但每声道两位 DR token 为 60/62；两处差分均由
float64 WAV 在分析前窄化到 float32 造成。有效 PCM 主链改为 finite interleaved
f64 后，同一 observation 与同一批输入得到 track DR 39/39、channel DR 62/62，
差分数为 0。

wire schema v3 随后增加了与参考报告同语义的独立 channel/track report
metrics；固定产物与命令登记在
[`CONF-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-report-v3-20260718`](../conformance/conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-report-v3-20260718/record.md)。
对固定 39 项 safe master 的 schema-v3 实测继续采用公开 token 精确相等、零数值
容差，得到：

| 字段 | 结果 |
| --- | ---: |
| 整数 track DR | 39/39 |
| 每声道两位 DR | 62/62 |
| overall primary peak | 39/39 |
| overall RMS | 39/39 |
| 每声道 overall RMS | 62/62 |

这份首轮 follow-up 不改写 schema-v2 历史记录。随后从已提交源码重建的
[`clean-commit successor`](../conformance/conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-report-v3-clean-20260718/record.md)
保持上述五类字段完全匹配，并按第 4.7 节固定 renderer 规则得到 duration token
39/39。它还只对 footer 的 track count、sample-rate set、channel-count set 和
重建的 unweighted DR token 四项做有限比较：safe master 的 39 项中有 3 个数值
DR0 track；全部纳入时最终 token 是观察到的 `DR12`，若统一排除全部 DR0 则会
得到 `DR13`。该反事实支持“不做统一 numeric-DR0 filter”，但不能区分精确
public-f32 mean 与其他同样显示 `DR12` 的聚合，也不验证 length weighting。

两份 schema-v3 记录是有限实现复核，不是 accepted 规格、整份报告 parity 或
任意输入的 reference compatibility 声明。第 2.3 节已经补入 reference core raw
state，但 ADR-0002 不要求把 MacinMeter 的内部结构逐项映射到这些记录。
album/renderer 只保留纯数值规则；playlist/grouping、host footer metadata、
完整文本与更广输入空间不在声明范围。profile 继续保持
`FooDrMeter108CandidateV1 / Unverified`。

## 3. 数据与数值约定

下列伪代码使用：

- `W`：每个分析窗口的 frame 数；
- `C`：声道数；
- `N`：已经提交的窗口数，包括全零窗口；
- `lround`：C/C++ `lround` 语义，即有限半值远离零；
- `f32(x)`：按 IEEE-754 binary32 窄化；
- `log10`、`sqrt` 和 `pow`：目标 CRT 的浮点函数；
- `primary` / `secondary`：按量化 dB key 排名的两个 window peak 候选。

以下每声道状态描述固定 x64 目标：

```text
current_sum_squares: f64 = 0
current_peak:        f64 = 0
sum_window_rms2:     f64 = 0

primary.amplitude:   f64 = 0
primary.key_db:      f64 = -100000
secondary.amplitude: f64 = 0
secondary.key_db:    f64 = -100000

histogram[0..10000]: u32 = 0
```

会话状态还包含 `current_frames = 0`、`window_count = 0` 和
`consumed_frames = 0`。

### 3.1 架构精度契约

| 阶段 | x64 1.0.8 | x86 1.0.8 | 当前 MacinMeter candidate |
| --- | --- | --- | --- |
| PCM 入口 | binary64 | binary32 | binary64 |
| sample square | binary64 | binary32 后提升到 binary64 | binary64 |
| current peak | binary64 | binary32 | binary64 |
| peak logarithm | binary64 `log10` | `log10f` | binary64 `log10` |
| primary/secondary amplitude | binary64 | binary32 | binary64 |
| 已保存 peak key | binary64 centi-dB | binary32 centi-dB | integer centi-dB |
| RMS/histogram/DR | binary64 | binary64 | binary64 |
| 公开 channel/track DR | binary32 | binary32 | binary32 |

x86 单样本平方先以 binary32 计算，再提升到 binary64 累计。x86 新 peak key 在
binary64 中形成，但保存时窄化为 binary32，后续候选会和已窄化 key 比较；这与
x64 全 binary64 peak 路径不同。当前 MacinMeter 已消除 PCM 入口的 binary32
窄化，并在 complete-v2 可观察边界上与 x64 结果一致；但它保存 integer key，
且有限样本不能证明所有运行库和半值边界相同。因此本规格后续伪代码继续只表示
x64 核心控制与数值路径。当前 MacinMeter 仍是显式派生的
`CandidateV1 / Unverified`，不是 x64 或 x86 的隐藏兼容别名。

## 4. 候选核心算法

### 4.1 初始化

```text
COEFFICIENT_BITS = 0x4008085bf37612cf
COEFFICIENT      = 3.0040816326530613

W = trunc_toward_zero(sample_rate * COEFFICIENT)
```

对合法正采样率，`trunc_toward_zero` 等价于 `floor`。x64 路径在采样率、声道
数或 `W` 为零时抛出数据错误。

### 4.2 消费 PCM

固定 x64 目标由 foobar2000 以 interleaved `f64` PCM 交给核心路径。每次最多
消费到当前窗口边界：

```text
for each input frame:
    for channel in 0..C:
        magnitude = abs_bitwise(sample[channel])
        state[channel].current_sum_squares += magnitude * magnitude
        state[channel].current_peak =
            max(state[channel].current_peak, magnitude)

    current_frames += 1

    if current_frames == W:
        submit_window(current_frames)
```

`abs_bitwise` 表示清除浮点符号位；对本规格限定的有限 PCM，它等价于 `abs`。
调用方 block 边界不创建分析窗口。

固定 x86 目标的相同循环消费 interleaved `f32`，并在 binary32 中完成绝对值、
sample square 和 current peak；其后才把 square 提升到 binary64 累计。因此本节
伪代码不是 x86 最后一位数值行为的定义。

### 4.3 提交一个窗口

```text
function submit_window(frames):
    for each channel state:
        rms2 = 2 * current_sum_squares / frames
        rms  = sqrt(rms2)
        sum_window_rms2 += rms2

        if current_peak > 0:
            peak_key_db = 0.01 * lround(2000 * log10(current_peak))

            if peak_key_db > primary.key_db:
                secondary = primary
                primary = {
                    amplitude: current_peak,
                    key_db: peak_key_db
                }
            else if peak_key_db > secondary.key_db:
                secondary = {
                    amplitude: current_peak,
                    key_db: peak_key_db
                }

        if rms != 0:
            rms_key_db = 0.01 * lround(2000 * log10(rms))
            rms_key_db = clamp(rms_key_db, -100, 0)
            bin = lround(100 * rms_key_db + 10000)
            bin = clamp(bin, 0, 10000)
            histogram[bin] += 1

        current_sum_squares = 0
        current_peak = 0

    consumed_frames += frames
    window_count += 1
    current_frames = 0
```

因此：

- window RMS 是 `sqrt(2 × sum_squares / frames)`；
- RMS histogram 的一个 bin 表示 `bin × 0.01 - 100` dB；
- RMS 高于 0 dB 或低于 -100 dB 会进入端点 bin；
- 全零窗口增加 `window_count`，但不增加任何 histogram bin；
- peak key 不使用 RMS 的 `[-100, 0]` clamp；
- peak 比较使用严格大于。与 primary 同 key 的后到 peak 可以填入尚低于该 key
  的 secondary；与 secondary 同 key 的后续 peak 不再替换它。

最后一条是 x64 binary64 key 的语义。x86 会把已保存 key 窄化为 binary32，
不得对未测试的半值和 tie 边界套用 x64 的逐位预测。

`OBS-foo-dr-meter-108-x64-numeric-boundaries-v1-run1-20260719` 在 finish 后、
histogram cleanup 前保存 10001 个 `u32le` bin 的摘要与 SHA-256。单声道单窗口
向量确认 `-101 dB` 与 `-100 dB` 落入 bin `0`，`+1 dB` 与 `0 dB` 落入 bin
`10000`，而 `-99 dB` 与 `-1 dB` 留在内部 bin。该动态覆盖支持上述有限、非零
RMS 输入的端点 clamp；它不外推 NaN/Inf、计数溢出、多窗口资源极限或所有
libm 最后一位边界。

### 4.4 EOF

```text
if current_frames > 0:
    submit_window(current_frames)
```

一帧尾窗也会提交，并仍按自己的实际 `frames` 归一化。输入恰好结束在完整窗口
边界时，`current_frames == 0`，不会提交虚拟零 peak 或额外 histogram 项。

### 4.5 每声道结算

```text
function finish_channel(state, N):
    if N > 0:
        channel_rms = sqrt(sum_window_rms2 / N)
    else:
        channel_rms = 0

    target = max(1, floor(N / 5))
    selected_count = 0
    selected_power = 0

    for bin from 10000 down to 0:
        count = histogram[bin]
        if count == 0:
            continue

        bin_db = bin * 0.01 - 100
        selected_count += count
        selected_power += 10^(bin_db / 10) * count

        if selected_count >= target:
            break

    dr = 0
    selected_peak =
        secondary.amplitude if secondary.amplitude > 0
        else primary.amplitude

    if selected_peak > 0 and selected_count > 0:
        loud_rms = sqrt(selected_power / selected_count)

        if loud_rms != 0:
            dr = -20 * log10(loud_rms / selected_peak)

            if dr < 0:
                dr = max(
                    -20 * log10(loud_rms / primary.amplitude),
                    0
                )

    return {
        internal_dr_f64: dr,
        internal_rms_f64: channel_rms,
        public: {
            channel_dr_f32: f32(dr),
            channel_rms_f32: f32(channel_rms),
            primary_peak_f32: f32(primary.amplitude)
        }
    }
```

循环先加入整个 bin，再判断是否达到 `target`，所以 loudest 20% 的边界 bin
若包含并列项，整个 bin 都参与二次均值。`selected_power` 使用量化 bin 中心重建，
不是使用原始 window RMS，也不是在线性域做 `0.0001` 截断。

如果非零 histogram 项少于 `target`，循环使用所有现有非零项；全静音声道没有
selected peak 和 histogram 项，沿上述路径得到数值 DR 0。

### 4.6 track 聚合

默认设置下：

```text
track_dr_f32 = f32(
    arithmetic_mean(channel.internal_dr_f64 for every channel)
)
```

所有声道都参与，包括数值 DR 0 的静音声道和布局中的 LFE；没有
`Silent`/`InsufficientData` 排除态。track 聚合读取尚未公开窄化的 binary64
channel DR；它不平均 `channel_dr_f32`。每声道公开窄化和 track 公开窄化是两个
独立步骤。

x64 二进制还包含一个可选分支，仅在 `C > 2` 且
`Weight multichannel DR by channel loudness = on` 时使用：

```text
track_dr_f32 = f32(
    sum(channel.internal_rms_f64 * channel.internal_dr_f64) /
    sum(channel.internal_rms_f64)
)
```

数值边界 observation 的四个 off/on 配对与这条静态公式逐位相符：三声道
balanced、整体 RMS 来源判别和部分静音场景在开关后只改变 track bits；双声道
场景保持相同 track bits，确认 `C > 2` 门槛。整体 RMS 来源场景还区分了
overall channel RMS、loud-window RMS、RMS² 与不加权候选。该分支仍不属于本
规格默认 profile；有限三声道输入不证明任意声道数或全静音零分母的成功行为。

### 4.7 报告派生与整数显示

x64 报告路径和结构相同的 x86 路径已经静态追踪到公开 binary32 数组：

```text
report_peak_linear =
    max(f32(channel.primary_peak) for every channel)

report_rms_linear =
    sqrt(
        sum(
            f64(f32(channel.overall_rms * channel.overall_rms))
            for every channel
        ) / channel_count
    )

report_peak_db = f32(20 * log10(f64(report_peak_linear)))
report_rms_db  = f32(20 * log10(report_rms_linear))
```

也就是说，report RMS 的每声道平方先以 binary32 完成，再提升到 binary64 求和；
report peak 取公开 primary peak binary32 的最大值。它们不同于 loud histogram
selection RMS 和实际 selected peak。

零 peak/RMS 显示为 `-inf dBFS`。dB 文本在
`-0.01 < value < 0.01` 时有显式 centi-dB `lround` 修正，随后格式化到两位
小数；修正后为零时显示 `0.00`，不保留 `-0.00`。renderer 显式使用 `C`
locale。默认 track DR 的正整数显示在 binary32 结果之后采用：

```text
display_dr = integer_cast(track_dr + 0.5)
```

这不是通用的有符号 half-away-from-zero；核心有效路径已经把 DR 保持在非负
范围。报告中的每声道 DR 保留 binary32 结果并格式化到两位小数。

固定 x64 renderer 的 duration 路径先计算：

```text
rounded_seconds = llround(f64(decoded_frames) / f64(sample_rate))
```

这里的 C `llround` 在半值处远离零。随后固定 PFC formatter 将非负整秒拆成
week、day、hour、minute 和 second：存在 week 时输出 `Nwk `；存在 day 或 week
时输出 `Nd `；存在 hour/day/week 时尾部为 `h:mm:ss`，否则为 `m:ss`。39 项
safe-master token 与这条路径全部一致；后续隔离 observation 又以 24 个向量覆盖
`0.5s`、`1.5s`、44.1/48 kHz 精确半秒及 minute/hour/day/week 进位的下侧、
精确半值和上侧，全部得到预注册 token。

这两类动态证据的边界不同：safe-master 来自完整报告但没有半秒判别输入；24 项
记录直接调用 renderer 所使用的固定数值叶子，而没有执行完整 renderer。结合
renderer 到该叶子的固定静态调用数据流，可把非负 finite、
`fractionalDigits = 0` 路径上已覆盖的 `llround` 与四类文本分支交叉定为 E2；
不能据此声明负时长、非有限值、零 sample rate、超大 frame 转换、其他小数位或
完整报告 byte parity。

#### 4.7.1 当前 schema-v3 实现映射

MacinMeter 现在把 report metrics 与 DR 计算诊断分开：

- 每声道 `report.overallRmsLinear` 和 `report.primaryPeakLinear` 都在参考公开
  窄化点保存为受验证的 finite binary32；零幅度的 dBFS 派生值使用 `null`；
- track overall RMS 先对每个 public-f32 channel RMS 在 binary32 中平方，再把
  各平方提升到 binary64 求和、除以声道数并开方；
- track primary peak 取每声道 public-f32 primary peak 的最大值；
- `DecodedDuration` 保存精确的 `decodedFrames` 与实际 PCM `sampleRate` 数对，
  不把舍入后的秒数当作事实；conformance adapter 只在比较 reference 文本时按
  上述固定 renderer 派生 token；
- DR 状态机诊断使用 `loudWindowRms`、`drSelectedPeak`、`drPrimaryPeak` 和
  可空的 `drSecondaryPeak`，不再用容易与 report primary peak 混淆的
  `selectedPeak` 字段。

公开 report/album 数值由经过验证的 `FiniteF32`/`FiniteF64` wrapper 承载；
`AnalyzerSession::finish` 为 consuming、fallible API，结算时的数值或资源失败
返回结构化错误，不生成包含 NaN/Inf 的成功结果。上述内容是当前实现状态，不改变
第 5 节各参考规则的证据等级。

### 4.8 默认 album 聚合

静音 track 在参考路径中是数值 DR 0，不是需要从 album 排除的离散状态。对于
同一 grouping 中参与聚合的结果项：

```text
official_album_dr = f32(
    sum(f64(track.dr_f32) for every track) / track_count
)

duration(track) =
    track.decoded_frames / track.sample_rate
    if track.sample_rate != 0
    else track.decoded_frames / 44100

weighted_album_dr = f32(
    sum(f64(track.dr_f32) * duration(track) for every track)
    / sum(duration(track) for every track)
)

effective_album_dr =
    weighted_album_dr
    if length_weighting_enabled and total_duration > 0
    else official_album_dr
```

默认设置关闭 length weighting，所以 effective 与 official 相同。报告的
“Official DR value” 使用 official binary32，“Weighted DR value” 使用
effective binary32；整数显示分别执行：

```text
integer_cast(album_dr_f32 + 0.5)
```

因此 album 输入是精确 binary32 track DR，而不是已经整数化的显示值；算术平均
和时长加权在 binary64 中累计，结果再独立窄化为 binary32。公式、DR0 纳入、
窄化点和显示舍入均已由静态数据流固定。

当前 application 门面通过显式 `AlbumAggregator::aggregate` 实现这条候选公式：

- 输入是调用方明确构造的 `AlbumTrackMetrics`，不会把任意 `BatchRunner` 结果
  自动解释为 album；
- 产品 API 将参考公式的候选重建命名为 Rust `unweighted_dr_db`（序列化为
  `unweightedDrDb`），不把 MacinMeter 输出称为官方结果；该值对 public-f32
  track DR（包含数值 DR0）提升到 binary64 后做算术平均，再窄化到 finite
  binary32；
- `DurationWeighted` 只在调用方显式请求时成为 effective 值，并使用每首 track
  的 exact `DecodedDuration`；总时长为零时回退产品的 unweighted 值；
- `SampleRate` 领域类型拒绝零值，因此参考实现的 `frames / 44100` 防御分支在
  产品有效输入契约之外；产品不会为复现不可达防御分支而引入无效采样率；
- API 同时保留 unweighted、可选 weighted、effective、track count 与总时长，
  所有公开浮点结果必须 finite。

这只是把完整 E1 静态公式落实为独立、可测试的产品 API。它定义 album 数值算术，
不验证 playlist、grouping、自动发现、footer 或其他 album subsystem 行为，也
不得把实现测试或 safe-master official footer 外推为完整 album conformance。

safe master 中的 3 个数值 DR0 track 提供了一个更窄的可区分反事实：按 schema-v3
公开 track DR 全部纳入时，binary64 mean 为约 `11.6802833`，最终显示 `DR12`；
统一过滤全部 DR0 后，36 项 mean 为约 `12.6536402`，会显示 `DR13`。reference
footer 是 `DR12`，且两种结果都离 `12.5` 边界足够远，不受已导出两位 channel
token 的量化不确定性影响。它与静态无条件聚合路径共同把“不做统一 numeric-DR0
filter”提升为 E2；它不能排除按其他隐藏状态选择性过滤的假想规则，也不能证明
精确 public-f32 mean、最终窄化点或 length-weighting 分支。

## 5. 逐规则证据等级

证据等级遵循 [`reference/README.md`](../README.md)。`SA-x64` 指 x64 核心
记录，`SA-render` 指固定 x64 report renderer 记录，`SA-duration` 指固定
duration 叶子 ABI 记录，`SA-cross` 指 x86/cross-arch 记录；`OBS-x86` 与
`OBS-x64` 分别指第 2.2.1、2.2.2 节的固定黑盒观测，`OBS-core` 指第 2.3 节的
隔离 core 动态记录，`OBS-boundary` 指第 2.4 节的数值边界记录。多份静态记录
仍属于同一类证据，不因目标或函数数量自动升级为 E2；同一构造模型与
observation 的比较也不构成第三类证据。`OBS-core` 和 `OBS-boundary` 只对各自
实际保存或直接执行的状态、数值叶子与有限向量构成动态证据，不能把未执行的
album、host 或完整 renderer 一并升级。

| 规则 | 等级 | 依据与限制 |
| --- | --- | --- |
| `W = floor(sample_rate × 3.0040816326530613)` | E2 | SA-x64/SA-cross 的相同常量与截断；OBS-x86/OBS-x64 的边界与多采样率结果支持。公式对其他采样率不再需要另行导出才能定义。 |
| interleaved block 不直接定义窗口 | E1 | SA-x64/SA-cross 的跨调用 `current_frames` 状态；GUI 不控制 decoder block，运行报告不是合适的验证手段。 |
| window RMS 为 `sqrt(2 × sum_squares / frames)` | E2 | SA-x64/SA-cross；OBS-x86/OBS-x64 数值相符。x86 与 x64 的单样本平方精度按第 3.1 节分别适用。 |
| 任意非空尾窗（含一帧）提交 | E2 | 两份 SA 的 EOF 分支；OBS-x86/OBS-x64 的 104、201。 |
| 精确窗口边界不添加虚拟零 | E1 | 两份 SA；现有导出字段不能观察该隐藏状态，不要求重复导出。 |
| channel RMS 为所有 window RMS 的等权二次均值 | E2 | SA 的 `sum_window_rms2 / N`；OBS-x86/OBS-x64 的 103、104、110、111。 |
| RMS 先按 `0.01 dB` 量化再进入 histogram | E2 | 两份 SA；OBS-x86/OBS-x64 的 110 区分线性 `0.0001` bin。 |
| RMS key clamp 到 `[-100, 0] dB`、共 10001 bins | E2 | SA-x64/SA-cross 的常量、比较和数组大小；OBS-boundary 的 `-101/-100/-99/-1/0/+1 dB` 单窗口摘要确认两个端点和相邻内部 bin。E2 不覆盖非有限 RMS、计数溢出或资源极限。 |
| loud 目标数为 `max(1, floor(N/5))` | E2 | 两份 SA；OBS-x86/OBS-x64 的 5/10-window 输入相符。 |
| loud 边界 bin 整组纳入并按 bin 中心重建功率 | E2 | 两份 SA；OBS-x86/OBS-x64 的 111 区分精确目标数模型。 |
| peak 以 `0.01 dB` key、严格 `>` 两级排名 | E2 | 两份 SA 与两个 OBS 的 120/121；x64 precision fixture 还覆盖 source-f64 peak 半值，但有限输入不能证明全部 tie 边界。 |
| 优先 secondary，缺失时回退 primary | E2 | 两份 SA；两个 OBS 的单窗口 101/102/201 与重复 peak fixture 支持。 |
| secondary 产生负 DR 时以 primary 重算并 clamp 至 0 | E2 | 两份 SA；两个 OBS 的 105、202。 |
| 静音产生数值 DR 0 | E2 | 两份 SA 零 peak/histogram 路径；两个 OBS 的 203。 |
| 默认 track DR 是内部 binary64 channel DR 的全声道算术均值，包含静音和 LFE | E2 | 两份 SA；两个 OBS 的 301=6、302=20、303=15，OBS-x64 还覆盖 8 声道。公开 channel DR 不参与该平均。 |
| 可选多声道权重在 `C > 2` 时使用内部 binary64 overall channel RMS 与 DR | E2 | 两份 SA 的公式与门槛；OBS-boundary 的四个 off/on 配对达到 track bits 8/8、channel 前提 8/8、pair invariants 4/4，并区分 loud-window RMS、RMS² 与不加权候选。未覆盖全静音零分母和任意声道数。 |
| 默认 album 聚合不做统一的数值 DR0 track 过滤 | E2 | 两份 SA 的无条件聚合；OBS-x64 safe-master 含 3 个 DR0，全部纳入显示 DR12，统一排除则会显示 DR13。该反事实不证明其他 album 子规则。 |
| channel/track 公开结果窄化为 binary32 | E1 | 两份 SA 的明确存储路径；文本精度不足以独立确定所有窄化点。 |
| 正整数 DR 以 binary32 `+0.5` 后转换 | E2 | 两份 SA 的报告路径；两个 OBS 的 120/121 及 OBS-x64 的 610/611 位于可区分边界两侧。 |
| report RMS 使用公开 channel RMS 的 binary32 平方和二次均值 | E2 | SA-cross 登记完整数据流；两个 OBS 的 301–303 及 OBS-x64 的 39 个 overall RMS token 与预先构造的静态模型预测一致。 |
| report peak 为公开 channel primary peak 的最大值 | E2 | SA-cross 登记完整数据流；OBS-x64 的 39 个 overall peak token 与预先构造的静态模型预测一致，但有限输入不证明所有边界。 |
| duration 以 binary64 frames/rate 经 `llround` 得到整秒 | E2 | SA-render/SA-duration 的固定数据流与 ABI；OBS-boundary 在 `0.5s`、`1.5s`、44.1/48 kHz 半秒及四个进位边界的下侧/精确/上侧共 24/24。限于非负 finite、`fractionalDigits = 0` 和已覆盖 frame 范围。 |
| 短时 duration 使用 `m:ss` renderer | E2 | SA-render/SA-duration 的固定 formatter 分支；OBS-x64 的 39 个报告 token 与 OBS-boundary 的短时/分钟边界均相符。 |
| duration 的 hour/day/week renderer | E2 | SA-render/SA-duration 的固定分解与模板；OBS-boundary 直接覆盖 hour、day、week 进位的下侧、精确半值和上侧。它验证数值叶子 token，不等于完整 report renderer parity。 |
| 已观测 channel ordinal `0..5, 9, 10` 显示为 `FL, FR, FC, LFE, BL, BR, SL, SR` | E2 | SA-render 的固定表；OBS-x64 的 1/2/3/6/8 声道列覆盖这些 ordinal。该规则不证明宿主如何生成 ordinal。 |
| channel ordinal `6..8, 11..17` 与 `>=18` 的 `Ch %u`/`?` fallback | E1 | SA-render 的固定表与分支；现有 observation 未执行这些分支。 |
| official album DR 为 binary32 track DR 的 binary64 算术平均，再窄化为 binary32 | E1 | 两份架构的 album writer 静态数据流。 |
| album length weighting 使用 decoded duration，结果窄化为 binary32 | E1 | 两份架构的 album writer 静态数据流；设置在两个 OBS 中关闭。 |
| x86 与 x64 使用不同 PCM、sample-square 和 peak 精度 | E2 | SA-cross 的固定二进制指令级数据宽度；OBS-x64 的 source-f64/先窄化对照声道动态保留不同 token。证据只覆盖已导出边界，不外推所有最后一位。 |
| 核心零 frame 结算与 host 零帧源行为不同 | E1 | 两份 SA：核心产生 DR0；host 在首次 decode 无 chunk 时抛出 data error。 |

本表保留逐规则的保守 E1/E2 等级；第 2.3 节提供通用 safe-master core 动态
记录，第 2.4 节提供专门的有限边界判别。它们与静态规则交叉可形成 E2，但隔离
worker 的直接调用和摘要仍不是内部逐指令 trace，不能自动升级为 E3。M1 完成
也不依赖把其他静态已唯一确定的规则形式化升级到更高标签。

## 6. 已解决边界、限制与非目标

### 6.1 已由静态分析与固定观测收口

以下项目不再要求仅为确认 DLL 内部控制流而追加黑盒导出：

- x86/x64 是否共用数值精度：不共用，静态宽度与 x64 architecture
  discriminator 已交叉印证，差异见第 3.1 节；
- block/window、EOF、精确整窗、RMS clamp 和任意合法采样率的窗长；
- channel/track binary32 存储点；可选多声道 weighting 的 `C > 2` 门槛、
  overall RMS 权重来源和部分静音行为已有动态交叉，全静音零除零仍只作为静态
  无效边界保留；
- album official、length weighting、最终窄化与显示舍入的静态控制流；其中只有
  “不统一过滤 DR0”另有 footer 反事实并达到 E2；
- report peak/RMS、接近零修正、`C` locale 和固定文本；duration 的半秒舍入与
  minute/hour/day/week token 已由固定数值叶子动态交叉，但完整 renderer 仍未
  动态执行；
- 已观测 channel ordinal `0..5, 9, 10` 的公开标签达到 E2；`6..8, 11..17`
  与 `>=18` fallback 仍为 E1，宿主 channel mask 到 ordinal 的来源规则仍未知；
- 核心零 frame 结算与宿主首次 decode 无 chunk 的分层行为。

这些规则仍需要本地可重复 fixture 和实现侧测试。“不需要新增插件报告”不表示
“不需要回归覆盖”。channel mask 到标签的分支和静态表也可继续直接逆向登记，
无需通过生成音频来枚举。

### 6.2 当前实现已关闭的系统差分

complete-v2 首次 conformance 暴露的两处系统差分都来自 source-f64 在分析前被
窄化到 binary32。当前主链已改为 finite interleaved f64，并在完全相同的 39 项
safe master 上把 track DR 从 39/39 保持为 39/39、channel DR 从 60/62 修正为
62/62。已保存的 pre/post 实现产物和精确 token 差分使这项处置可复核。

schema v3 又把独立 report metrics 加入产品模型；clean-commit safe-master
successor 得到 overall peak 39/39、overall RMS 39/39、channel RMS 62/62、
duration 39/39，同时保持 track DR 39/39、channel DR 62/62。这关闭了当前 corpus
中六组公开且同语义 token 的已知差分。

同一记录只对 footer 做部分一致性检查，并以 DR12/DR13 反事实关闭“统一过滤
numeric DR0”这一窄问题。这些结果不证明未导出中间状态、精确 internal album
mean、length weighting、host metadata、任意输入或整份报告 byte-for-byte
一致。按照
[`ADR-0002`](../../docs/adr/0002-m1-reference-numeric-scope.md)，前两项的
纯数值公式可由固定汇编数据流定义；host metadata、playlist/grouping 和完整文本
parity 则明确不属于 M1。

### 6.3 声明内的限制

以下限制约束本规格可以作出的数值声明：

| 行为 | 等级 | 说明 |
| --- | --- | --- |
| 1.0.3 与 1.0.8 是否相同 | U | 当前证据不得跨版本外推。 |
| 固定 Windows CRT/libm 的未覆盖最后一位边界 | H | 当前 x64 观测覆盖两个架构 precision fixture、24 个 duration 半秒/进位向量和 6 个 histogram 端点向量，但 bit-exact 目标仍须固定运行库；这些有限点不能证明所有 `log10`/`sqrt`/转换边界。 |
| album 聚合数值公式 | E1 | DR0 统一过滤反事实已单独达到 E2；public-f32 mean、最终窄化和 length weighting 由固定静态数据流唯一确定。focused playlist/grouping 不属于数值公式验收，batch 也不自动具有 album 语义。 |
| NaN、Inf、反常范围 PCM | U | 不属于有效 PCM 契约；目标异常数学行为不提升为产品契约。 |
| histogram/窗口计数极限与溢出 | U | 不属于候选有效资源范围；若研究应做静态类型/指令审计，而不是生成数百年音频。 |

1.0.3 比较应先固定并哈希对应原始 DLL，再建立独立静态差分记录；不应生成
1.0.8 样本来外推版本关系。

### 6.4 明确非目标

以下行为不是未知算法缺口，而是不属于
[`ADR-0002`](../../docs/adr/0002-m1-reference-numeric-scope.md) 的目标：

- foobar2000 decoder 对全部 PCM/容器的逐位归一化；
- 超满幅、零帧、损坏文件和 decoder error 的最终宿主 UI/日志；
- 独立 foobar host repeat、component registration、service 与 lifecycle；
- 宿主生成 channel ordinal/mask、bit depth、bitrate 和 codec metadata 的规则；
- playlist、album grouping、自动发现和 footer 语义；
- 整份报告的标签、模板、locale、换行、编码或 byte-for-byte parity。

已经保存的相关静态或黑盒记录继续作为历史证据，不因成为非目标而删除；它们也
不能反向扩大本规格的兼容性声明。

## 7. 使用规则

- 实现不得把本规格命名成 `Compatible`、`ReferenceExact` 或类似 profile。
- 按本规格修改算法时，必须保留来源标识和 `candidate / unverified` 状态。
- 当前 MacinMeter wire schema v3 将第 4.7 节 report metrics 放在独立
  `channel.report` 与 `analysis.report` 中；`loudWindowRms`、
  `drSelectedPeak`、`drPrimaryPeak`、`drSecondaryPeak` 仍只是 DR 计算诊断，
  不得和 report 字段互换。
- batch 不具有 album 语义；只有调用方显式构造 `AlbumTrackMetrics` 并调用
  `AlbumAggregator` 时才执行第 4.8 节候选聚合。
- conformance 必须分别记录参考 observation 与实现结果，不能把本文伪代码本身
  当作 golden。
- 修改量化、tail、peak tie、负值回退、静音或聚合规则时，应使用本实验 corpus
  做差分，并优先复核固定二进制静态数据流；只有 host/decoder 或运行库边界无法
  静态确定时才追加黑盒输入。
- x86 与 x64 必须分别标记目标精度；x64 complete-v2 已提供本轮 peak/RMS
  architecture boundary 证据，但不得外推到 x86 或所有未覆盖半值。
- 若未来 1.0.3 被固定为正式目标，应建立独立 target、observation 和规格；不得
  静默改名或覆盖本文件。

## 8. 进入 accepted 的最低条件

1. 固定项目实际要兼容的插件版本和架构；
2. 对窗口数、loud 边界 bin、peak 排名/回退和聚合输入等规则保存可审计的固定
   汇编数据流或隔离 core 证据，不要求 production 内部状态逐项差分；
3. 用一次生成的本地 corpus 覆盖多采样率、零 frame、RMS clamp、peak key 半值、
   album 公式和 x86/x64 精度判别；静态已确定的内部规则不要求重复人工导出；
4. 把现有 track DR 39/39、channel DR 62/62、overall peak 39/39、overall RMS
   39/39、channel RMS 62/62、duration 39/39 的有限 implementation comparison
   固定为验收基线，并保留 reference 数值边界 observation 的 duration 24/24、
   weighting 8/8 与 histogram 6/6；album 纯数值公式仍按其独立证据边界验收；
5. 明确只接受 x64、只接受 x86，还是分别提供两个数值 profile；不得再声明一个
   未限定架构的共同精度契约；
6. 将 host、decoder、playlist/grouping、metadata 来源和文本 byte parity 明确
   排除，不把未声明的外围行为误作缺失算法证据；
7. 保留所有系统性差异，不使用宽容差掩盖整数边界错误。
