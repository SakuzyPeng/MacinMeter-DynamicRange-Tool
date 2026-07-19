# SA-foo-dr-meter-108-x64-dynamic-probe-plan-20260718

## 身份与边界

- 事实类别：static-analysis / dynamic-probe-plan
- 目标：
  [`TARGET-foo-dr-meter-1.0.8-x64-static-ff3556ad`](../targets/foo-dr-meter-1.0.8-x64-static.md)
- 工具：IDA Professional 9.1 / Hex-Rays decompiler
- 计划日期：2026-07-18（UTC+08:00）
- PE image base：`0x180000000`
- 目标 SHA-256：
  `ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489`

本记录把固定 x64 二进制的静态布局恢复为可在 Windows 调试器中执行的探针清单。
其中的结构偏移、寄存器状态和 RVA 来自静态指令；**尚未执行这些动态探针**，因此
本文不产生 E3 证据。运行时地址一律按下式形成：

```text
runtime_address = loaded_foo_dr_meter_module_base + RVA
```

不得把 IDA preferred image base 下的绝对地址直接用于启用 ASLR 的进程。每次运行
前还必须在 Windows 侧重新校验实际加载 DLL 的 SHA-256；本地 IDB 身份不能替代
远端文件身份。动态记录必须把静态 target hash 写成
`expectedTargetSha256`，把运行侧明确提供的远端校验结果写成
`attestedLoadedModuleSha256`。二者相等是 arm 条件，但“attested”仍表示运行侧
声明，探针本身没有从远端文件系统重新计算 hash。

## Windows x64 调用边界

以下三个入口都使用 Microsoft x64 调用约定：

| 入口 | RVA | 参数 |
| --- | ---: | --- |
| analyzer initialization | `0x8410` | `RCX=session`、`EDX=sample_rate`、`R8D=channels`、`XMM3=opaque_f64` |
| stream consumption | `0x89F0` | `RCX=session`、`RDX=interleaved_f64`、`R8D=frames` |
| channel finalization | `0x8DF0` | `RCX=session`、`RDX=track_result`、`R8B=multichannel_weighting` |

`stream consumption` 的 frame 数不是 sample 数；PCM 指针指向
`frames × channels` 个 interleaved binary64 sample。finalization 首先以
`RDX=0, R8D=0` 内部调用 consumption，从而提交非空尾窗。

初始化的第四个参数只被保存到 session `+0x00`，没有进入本文复核的核心窗口、
histogram、peak 或 DR 结算路径。它的宿主语义仍未命名。

## Analyzer session 布局

宿主为 session 分配 `0x70` bytes。以下偏移只适用于固定 SHA：

| 偏移 | 宽度 | 静态身份 |
| ---: | ---: | --- |
| `+0x00` | 8 | 初始化第四参数；核心语义未定 |
| `+0x08` | 4 | sample rate |
| `+0x0C` | 4 | channel count |
| `+0x10` | 4 | window frames |
| `+0x14` | 4 | current window frames |
| `+0x18` | 8 | submitted window count |
| `+0x20/+0x28/+0x30` | 8 each | current accumulator begin/end/capacity |
| `+0x38/+0x40/+0x48` | 8 each | channel-state begin/end/capacity |
| `+0x50/+0x58/+0x60` | 8 each | histogram begin/end/capacity |
| `+0x68` | 8 | submitted frame count |

`+0x68` 只在一个窗口提交后增加该窗的实际 frame 数。流处理中仍未提交的尾部先只
体现在 `+0x14`；finalization 完成后 `+0x68` 才等于完整 decoded frame count。

### Current accumulator

`[session+0x20]` 指向 `channel_count` 个连续的 `0x10`-byte 元素：

| 元素偏移 | 类型 | 身份 |
| ---: | --- | --- |
| `+0x00` | `f64` | 当前窗 sum of squares |
| `+0x08` | `f64` | 当前窗 absolute peak |

### Per-channel state

`[session+0x38]` 指向 `channel_count` 个连续的 `0x28`-byte 元素：

| 元素偏移 | 类型 | 身份 |
| ---: | --- | --- |
| `+0x00` | `f64` | 已提交窗口的 RMS-square 总和 |
| `+0x08` | `f64` | primary peak amplitude |
| `+0x10` | `f64` | secondary peak amplitude |
| `+0x18` | `f64` | primary peak centi-dB key，缩放回 dB |
| `+0x20` | `f64` | secondary peak centi-dB key，缩放回 dB |

两个 key 初始化为 `-100000.0`，两个 amplitude 和 RMS-square 总和初始化为零。
key 的存储类型不是整数：它先由 `lround(2000 × log10(peak))` 得到 centi-dB
整数，再乘 `0.01` 保存为 binary64。

### Histogram

`[session+0x50]` 指向 channel-major 的连续 `u32` 数组：

```text
histogram[channel * 10001 + bin]
```

每声道 10001 个 bin。零 RMS 窗口增加 window count，但不写 histogram。

## Track result 布局

每个 track result 为 `0x58` bytes；album writer 也以该 stride 遍历：

| 偏移 | 宽度 | 静态身份 |
| ---: | ---: | --- |
| `+0x00` | 4 | public track DR `f32`；core finalizer 写入 |
| `+0x04` | 4 | album effective/weighted DR `f32`；album writer 后才有效 |
| `+0x08` | 4 | album unweighted DR `f32`；album writer 后才有效 |
| `+0x0C` | 4 | channel count；core finalizer 写入 |
| `+0x10` | 4 | host metadata field；核心 finalizer 不写，语义未定 |
| `+0x14` | 4 | sample rate；core finalizer 写入 |
| `+0x18` | 4 | host metadata field；核心 finalizer 不写，语义未定 |
| `+0x20` | 8 | decoded/submitted frames；core finalizer 写入 |
| `+0x28/+0x30` | 8 each | per-channel public DR array pointer/capacity |
| `+0x38/+0x40` | 8 each | per-channel public primary peak pointer/capacity |
| `+0x48/+0x50` | 8 each | per-channel public overall RMS pointer/capacity |

三个数组元素都是 `f32`。`+0x10` 与 `+0x18` 在宿主 orchestration 返回
finalizer 后写入；本计划不把它们猜成 bit depth、bitrate 或其他 metadata。
因此 `TRACK_PUBLISHED` 只能读取 `+0x00/+0x0C/+0x14/+0x20` 和三个已经写完的
channel 数组，不得把当时尚未由 album/host 写入的 `+0x04/+0x08/+0x10/+0x18`
登记为有效结果。

## 核心探针

表中的“断点时机”说明指令尚未执行还是已经执行。调试器应记录 thread ID；同一时刻
可能存在多个 analyzer session，不得用单个全局 `current_session` 关联事件。

| 事件 | RVA | 断点时机与稳定寄存器 | 应记录 |
| --- | ---: | --- | --- |
| `INIT_ENTRY` | `0x8410` | 函数入口 | `RCX/EDX/R8D/XMM3` |
| `PUSH_ENTRY` | `0x89F0` | decoder block 入口 | `RCX/RDX/R8D`；session header；当前 accumulator |
| `PEAK_RANKED` | `0x8CEA` | 本声道 peak 排名分支合流后 | `RBX=session`、`ESI=channel`、`XMM6=current_peak`；完整 `0x28` channel state；仅正 peak 时读取 `XMM0` key |
| `HIST_INC_PRE` | `0x8D52` | `inc histogram[...]` 执行前；只命中正 RMS 窗 | `RBX=session`、`ESI=channel`、`ECX=flattened_index`、`XMM8=window_rms`、写前 counter |
| `WINDOW_COMMIT_PRE_RESET` | `0x8D86` | 全声道、histogram、submitted frames 和 window count 已更新；`mov [RBX+0x14],0` 尚未执行 | session `+0x14/+0x18/+0x68` 与所有 channel state |
| `FINISH_ENTRY` | `0x8DF0` | 尾窗 flush 前 | `RCX=session`、`RDX=result`、`R8B=weighting`；session header |
| `LOUD_SELECTED` | `0x9026` | histogram 从响到静扫描结束后 | `R13=session`、`R12D=channel`、`R14=target`、`RSI=included`、`EBX=boundary_bin`、`XMM6=power_sum`、`XMM10=overall_rms` |
| `PEAK_SELECTED` | `0x904C` | secondary 不可用时已切到 primary；DR 除法前 | `XMM1=initial_selected_peak` 与 channel state |
| `NEGATIVE_FALLBACK` | `0x909A` | secondary/initial peak 得到负 DR 后，primary 重算前 | `XMM7=negative_candidate_dr`、`XMM6=loud_rms` 与 channel state |
| `CHANNEL_FINAL` | `0x90B8` | 有效、静音和无 peak 路径合流 | `XMM7=final_internal_dr`、`XMM10=overall_rms`、channel state |
| `CHANNEL_PUBLISHED` | `0x9116` | 三个 public `f32` channel 数组写入后 | `RBP=result`、`R12D=channel`、三个数组当前元素 |
| `TRACK_INTERNAL` | `0x9190` | channel aggregate 除法后、窄化前 | `R13=session`、`RBP=result`、`XMM9=track_dr_f64` |
| `TRACK_PUBLISHED` | `0x91F0` | core finalizer 的 public 字段写入后 | `R13=session`、`RBP=result`；仅当时有效的 track DR、channels、sample rate、frames 与三个完整 public 数组 |

### 解释与低扰动选择

- `PEAK_RANKED` 每个已提交窗口、每个声道命中一次；此时内存中的 primary/
  secondary amplitude 与 key 已经一致，不必在四个条件写点分别断下。若
  `XMM6=current_peak` 不是严格正值，控制流没有为本窗计算 candidate key，
  `XMM0` 是 stale register，不得记录为本窗 key。
- `HIST_INC_PRE` 只在真正写 histogram 时命中。`ECX - ESI × 10001` 是 bin；
  当前 counter 读数加一才是本次指令完成后的值。
- `WINDOW_COMMIT_PRE_RESET` 位于唯一 commit 路径上，适合验证
  `window_count`、本次待清零的 `current_frames` 与 submitted frames；断点继续
  后紧接着执行 current-frame reset。`0x8D8A` 不能用作 commit 断点，因为普通
  未提交 decoder slice 也会从 `0x8C10` 跳到该 loop-control 位置。
- `LOUD_SELECTED` 的 `RSI` 是纳入完整边界 bin 后的实际窗口数，可能大于 `R14`。
  `XMM6` 此时仍是加权 power sum，尚未除以 `RSI`。
- 是否发生 negative fallback 以 `NEGATIVE_FALLBACK` 是否命中为准。只比较最终
  DR 与 primary/secondary 数值会丢失这项控制流证据。
- `PEAK_SELECTED` 对全零/no-peak channel 不命中；`CHANNEL_FINAL` 仍会命中并给出
  零结果。
- finish 序言在 `0x8E1A` 执行 `mov R13,RCX`；到 `0x91F0` 前没有重写
  `R13`，且 `0x91DA/0x91E1/0x91E8` 仍以它读取 session，直到 `0x91F7`
  才恢复调用者的 `R13`。因此 `TRACK_PUBLISHED` 命中时可以用 `R13` 与
  initializer session 做硬绑定；`RBP` 同理从 `0x8E01` 的 `RDX` result 保持到
  该点。

默认运行建议启用上述断点。逐 sample 指令、`pow`/`log10` 导入和 allocator
断点都不应默认启用：它们的频率或跨模块噪声远高于本计划需要。

## Album writer 探针

album writer 固定在 `RVA 0xE540`。其 runner state 至少包含：

- `[state+0x28]`：`0x58`-byte track result 表的 base；
- `[state+0x10C]`：track-length weighting boolean。

grouping 还依赖宿主对象和 metadata string；这些对象的完整 C++ 类型尚未恢复。

| 事件 | RVA | 断点时机与稳定状态 | 应记录 |
| --- | ---: | --- | --- |
| `ALBUM_ENTRY` | `0xE540` | writer 入口 | `RCX=state`、track table pointer、weighting flag |
| `ALBUM_INCLUDED` | `0xE8CC` | 当前 record 已加入三个累计器且 group count 已增加 | `RSI=state`、`R15=record_byte_offset`、`RDI=group_count`、`XMM6=unweighted_sum`、`XMM7=weighted_numerator`、`XMM8=duration_sum` |
| `ALBUM_COMPUTED` | `0xE960` | unweighted/effective 已窄化，写回循环前 | `RBX=group_start_index`、`RDI=group_count`、`XMM6=unweighted_f64`、`XMM7=weighted_f64`、`XMM8=duration_sum`、`XMM0=effective_f32`、`XMM1=unweighted_f32` |
| `ALBUM_WRITTEN` | `0xEA1B` | group 所有 record 的 `+0x04/+0x08` 已写入 | track table 中 `[group_start, group_start+count)` |
| `ALBUM_EMPTY_PRE_WRITE` | `0xEA33` | 空 group 独有路径；`+0x04 = -1.0f` sentinel 写入前 | group index、record address、pending bits `0xbf800000` |

在 `ALBUM_INCLUDED`，`R15` 不是 pointer，而是从 table base 起算的 `0x58`-byte
对齐 byte offset：

```text
record = [RSI + 0x28] + R15
record_index = R15 / 0x58
```

当前 record 的 track DR、sample rate、frames 分别位于
`record+0x00/+0x14/+0x20`。只有 weighting flag 开启时 `XMM7/XMM8` 才是有意义
的 weighted numerator/duration；关闭时不得解释其中的暂存值。

`ALBUM_COMPUTED` 的 `XMM0/XMM1` 低 32 位是 binary32 值。若 weighting 关闭或总
时长为零，effective 回退到 unweighted；是否回退应同时根据 flag、duration 与
两条控制流事件判断，不能只因最终数值相同而推定。探针在 weighting 关闭时省略
`XMM7/XMM8`；开启但总时长为零时只登记 duration 和 unweighted fallback，不把
尚未除成 weighted mean 的 `XMM7` 标成 weighted result。

`0xEA3B` 是 empty/non-empty 两条路径共享的 cleanup join，不能用作 empty
sentinel 探针；非空路径也会从 `0xEA21` 跳到那里。`0xEA33` 才是 empty-only
写指令，断点位于 store 前，因此只记录目标地址和静态 immediate bits，不谎称
已经观察到写后内存。

## Renderer 闭环探针

report renderer 固定为 `RVA 0x3F280..0x42934`。下面两个读取点用于确认 album
writer 写入的同一 `0x58`-byte record 被 footer 消费：

| 事件 | RVA | 断点时机 | 应记录 |
| --- | ---: | --- | --- |
| `RENDER_UNWEIGHTED` | `0x4111E` | record `+0x08` 已载入 `XMM0`、加 `0.5f` 前 | `RAX+RCX=record`、`XMM0=unweighted_f32` |
| `RENDER_WEIGHTED` | `0x411B2` | record `+0x04` 已载入 `XMM1`、加 `0.5f` 前 | `RAX+RCX=record`、`XMM1=effective_f32` |

这两个点属于详细 album footer 路径，是否命中取决于报告设置和 official/weighted
显示分支。它们是 writer-to-renderer 的数据流闭环，不用于恢复宿主 bitrate、
codec、bit depth 或 channel mask。

## 建议记录格式

每条事件使用一行 JSON，至少包含：

```json
{
  "schemaVersion": 1,
  "expectedTargetSha256": "ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489",
  "attestedLoadedModuleSha256": "ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489",
  "runId": "<operator-assigned>",
  "fixtureId": "<path-free-fixture-id>",
  "expectedInputSha256": "<sha256>",
  "attestedInputSha256": "<same-sha256>",
  "processId": 0,
  "processEpoch": 1,
  "recordType": "event",
  "sequence": 1,
  "threadId": 0,
  "event": "WINDOW_COMMIT_PRE_RESET",
  "moduleBase": "0x...",
  "rva": "0x8d86",
  "session": "0x...",
  "fields": {}
}
```

要求：

- `sequence` 由单一 logger 单调增加；不要用 wall-clock timestamp 排序。
- pointer、module base、RVA 和 raw float bits 使用十六进制字符串，避免 JSON
  number 精度损失。
- `f32/f64` 同时记录 raw bits 和解析值；NaN/Inf 的解析值用明确字符串或
  `null`，不得输出非标准 JSON number。
- 以 `(runId, threadId, session pointer)` 关联 analyzer 事件；pointer 只在进程
  生命周期内有意义。
- observation 入库前移除远端用户名、绝对路径和机器名，只保留固定 target、
  fixture ID、hash 和必要事件。

### Fail-closed 与生命周期

模板执行后先安装 lifecycle hook，不会在身份未配置时悄悄采集。操作者必须在
target load 前调用：

```python
configure(
    "<run-id>",
    "<path-free-fixture-id>",
    "<expected-input-sha256>",
    "<attested-input-sha256>",
    "<attested-loaded-module-sha256>",
)
```

arm 过程要求：

- attested hash 与 expected hash 完全相等；
- 单次 run 登记一个 operator-attested、path-free fixture ID，expected/attested
  input SHA-256 必须相等；第一个事件必须是唯一 `INIT_ENTRY`，此后所有 core
  事件的 session 必须与该 initializer 相同；
- `FINISH_ENTRY` 必须恰好一次且 result 非空；它之后必须恰好出现一次
  `PUSH_ENTRY(session, pcm=0, frames=0)` 尾窗 flush。finish 前的 push 必须是
  非空 PCM，finish 后任何其他 data push 都使本 run 永久无效；
- `CHANNEL_PUBLISHED`、`TRACK_INTERNAL` 与 `TRACK_PUBLISHED` 的 result 必须与
  FINISH result 相同；TRACK 发布后任何 core 事件都使 run 永久无效，album/
  renderer 探针可继续，但标为 `unbound_peripheral_diagnostic`，不能单独当作已与
  当前 fixture result 绑定的证据；
- 所有计划地址均无既有 breakpoint；脚本不接管、不删除他人 breakpoint；
- loaded-module scan 必须完整枚举同 leaf 模块：零个表示尚未加载，恰好一个才可
  arm，两个及以上会永久 invalidate。首次 arm 后 module base 不可迁移；第二个
  base/load 只会 latch failure，不会删除旧断点并搬迁；
- 20 个 breakpoint 必须全部安装，并用 IDA `bpt_t/get_bpt` 逐个确认存在、
  enabled 且 type 为 `BPT_SOFT`，否则只回滚本脚本已经添加的地址；相同全集在每次
  dispatch 与 completion 清理前重新 live-verify，外部删除、disable 或改 type
  都会永久 invalidate；
- `_invalid_reasons` 非空或 capture 已 abort 后，所有 start/attach/library-load
  arm callback、`_install_breakpoints` 与 event capture 都 fail closed；不会重新
  应用 exception policy、重新 arm 或 request continue，直到失败 records 导出并
  reset；
- 所有 pointer、channel count、array capacity、histogram index、album offset 和
  group range 在读取前通过保守 bounds。

每个状态都输出 `recordType=status` JSONL，包括 hooks installed、configured、
breakpoints armed、capture started、snapshot/continue failure 和显式
`capture_completed`。所有 machine record 同时进入最多 100000 条的内存列表；
达到上限即停住。`capture_status()` 可随时返回 active session/result、
INIT/data-PUSH/FINISH/tail-flush/TRACK 计数、永久 invalid reasons、arm block
reason、唯一模块 scan 以及最近一次 live breakpoint integrity 结果。只有暂停后
`mark_complete()` 成功清理断点并确认同一 session/result 上恰好一个
initializer、finish、zero-tail-flush 和 track result 后，
`records_jsonl()` 才允许导出 `internally_consistent` diagnostic。不能把最后一个
track result 自动等同于整个 run 完成。导出完成后必须显式调用
`reset_capture()`，确认旧 identity、计数器、records 和 completion 状态已经清空，
才允许配置下一份 fixture。

任一 snapshot、JSON 序列化或 continue-request 失败时，探针不会发出 continue，
并登记 `action=process_left_paused`，同时把 run 永久标为 invalid；手动继续不能
再把它 `mark_complete()`。correlation、breakpoint ownership、exception policy
或 cleanup 失败同样进入该 latch。失败 run 必须先显式 `abort_capture()` 清理瞬态
调试状态，再用 `failed_records_jsonl()` 导出失败证据；未导出的失败 records 不得
由 `reset_capture()` 清掉。detach、process exit 和 target unload 都清理
本脚本拥有的 breakpoint 与运行态关联；失败的 cleanup 会保留 ownership 并输出
机器可读失败状态。

固定宿主会正常抛出 Microsoft C++ EH `0xE06D7363`。模板在 attach/start 前保存
该 exception 的原 `exception_info`，临时设置为“不 break、不由 debugger
handle、silent”，并输出 policy status；uninstall、detach、process exit 和 target
unload 时恢复原值。异常策略无法保存、应用或恢复时 capture 不得开始或声明完成。

脚本使用进程内 singleton registry；重复执行会拒绝并保留既有 hook、breakpoint
ownership 和内存记录，避免双 hook 与 ownership 丢失。

仓库中的
[`probe_foo_dr_meter_108_x64_ida.py`](../tools/probe_foo_dr_meter_108_x64_ida.py)
是 IDA remote debugger 的 non-persistent/debugger-only 模板。它会安装瞬态
`BPT_SOFT`、临时修改并恢复 debugger exception table、读取 register/memory、
请求 continue，并向 IDA Output 输出 JSONL，属于瞬态侵入式调试控制；它不写
插件业务数据或 IDB。运行前仍须人工复核 module identity 和输出目录/采集
harness。

所有记录固定标记为
`evidenceClass=operator_attested_diagnostic`、
`consistencyLevel=internally_consistent`。本模板不会独占或锁定输入文件，不会在
远端重新计算 input hash，也不会触发 foobar2000 command；fixture ID、input hash
与“当前空闲宿主只处理这一输入”均来自操作者声明。因此单独运行本模板不能产生
可直接绑定到当前 fixture 的证据。只有外层 guarded runner 同时锁定并重新 hash
输入、绑定唯一 command trigger、验证宿主空闲/进程身份，并保存两层记录之间的
provenance 后，才可以在后续 evidence pipeline 中晋升。

## 仍不确定

- session `+0x00` 的宿主语义；
- result `+0x10/+0x18` 两个宿主补写字段的准确类型与 metadata 名称；
- album runner 的完整 C++ 类型、grouping object 和 metadata string 所有权；
- debugger 远端实际加载模块是否与静态 target 相同，必须由运行侧 hash 解决；
- 固定 CRT/libm 的边界结果在动态环境中的 raw bits；
- 本计划的断点频率对具体 foobar2000 build 的 wall-clock 扰动，首次运行应只用
  一个判别 fixture 测量后再批量执行。

以上未知不会妨碍捕获窗口、histogram、peak、DR 和 album 累计器本身，但会限制
对外围 metadata、grouping UI 与端到端 host 行为的声明。
