# SA-foo-dr-meter-108-x64-parallel-dispatch-20260802

## 身份与目的

- 事实类别：static-analysis
- target：
  [`TARGET-foo-dr-meter-1.0.8-x64-static-ff3556ad`](../targets/foo-dr-meter-1.0.8-x64-static.md)
- 输入 SHA-256：
  `ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489`
- image base：`0x180000000`
- 工具：IDA Professional 9.1 / Hex-Rays（idalib）
- 分析日期：2026-08-02（Asia/Shanghai）

本记录登记固定 target 内部的 fork-join 并行调度结构，以及它与既有 analyzer
三入口 `0x8410` / `0x89f0` / `0x8df0` 的调用可达关系。它不保存目标二进制、
IDA 数据库或反编译文本。

本记录的直接用途是解释一处此前未被解释的观测：既有隔离 core 记录与既有
foobar report 分别来自单线程与多线程两条路径，而窄字段对照精确匹配。

## 函数边界

| 角色 | RVA 范围 | 大小 |
| --- | --- | --- |
| 并行调度器 | `0xdf30..0xe53b` | 1547 bytes |
| 线程体 | `0xdaf0..0xde38` | 840 bytes |
| analyzer init | `0x8410..0x89e2` | 1490 bytes |
| analyzer push | `0x89f0..0x8de1` | 1009 bytes |
| analyzer finish | `0x8df0..0x91fb` | 1035 bytes |

## 并行相关导入

固定 target 的导入表中与线程/并发相关的项全部集中在调度器一个函数内：

| 导入 | IAT RVA | 调用方 |
| --- | --- | --- |
| `CreateThread` | `0x49288` | `0xdf30` |
| `ResumeThread` | `0x492e8` | `0xdf30` |
| `SetThreadPriority` | `0x49310` | `0xdf30` |
| `WaitForMultipleObjects` | `0x49318` | `0xdf30` |
| `_Thrd_hardware_concurrency` | `0x49388` | `0xdf30`、`0x1e258` |
| `CreateEventW` | `0x492d8` | `0xf5b0` |

`CreateEventW` 的调用方 `0xf5b0` 不在本记录的分析路径上，不作推断。

## 调度器 `0xdf30` 的结构

调度器实现固定的 fork-join 序列：

```text
n = min(工作量需求, _Thrd_hardware_concurrency())
存入上下文 +0x78
if n <= 1:
    直接在当前线程调用线程体并返回
else:
    重复 n - 1 次:
        CreateThread(lpStartAddress = 0xdaf0, lpParameter = 上下文,
                     dwCreationFlags = 4 /* CREATE_SUSPENDED */)
        SetThreadPriority(handle, -15)
        句柄写入上下文 +0x98 指向的数组
    对数组内每个句柄调用 ResumeThread
    WaitForMultipleObjects(n, 句柄数组, bWaitAll = TRUE, INFINITE)
```

线程以 `CREATE_SUSPENDED` 创建、统一 `ResumeThread` 放行，收敛点是单一的
`bWaitAll` 等待。线程优先级固定为 `-15`。`CreateThread` 失败时把线程数写回 1
并退化为当前线程执行。

线程体地址在调度器内出现两次，两次的引用类型不同，且与上述两条路径一一对应：

- `0xe380`：`dr_O`，取地址作为 `CreateThread` 的 `lpStartAddress`；
- `0xe4b8`：`fl_CN`，`n <= 1` 时的直接调用。

调度器自身在 `.text` 中没有直接调用引用。它的地址只出现在 `.rdata` 的
`0x4d768`、`0x4e228` 与 `.pdata` 的 `0x65978`，即经由虚表分派、并登记异常展开
数据。

## 线程体 `0xdaf0`

线程体以 `AcquireSRWLockExclusive` / `ReleaseSRWLockExclusive` 保护调度上下文
偏移 `+0x58` 处的共享状态，并通过上下文内的虚调用领取工作项，处理后经
`memcpy` 与目标 `free` 归并、释放。多个线程执行同一函数、从同一上下文领取不同
工作项，属于数据并行而非流水线并行。

## 与 analyzer 三入口的可达关系

以固定调用图前向遍历（`fl_CF` / `fl_CN`）得到：

| 起点 | 可达节点 | 是否可达 `0x8410`/`0x89f0`/`0x8df0` |
| --- | --- | --- |
| 三入口 | 25 | — |
| 调度器 `0xdf30` | 134 | 是（三个均可达） |
| 线程体 `0xdaf0` | 98 | 是（三个均可达） |

反向不成立：从三入口出发的 25 个节点内既不含调度器也不含线程体。

因此固定 analyzer 的三个入口是被并行层调用的下层计算，而不是并行层的入口。
按 RVA 直接调用三入口的隔离 worker 不会进入 `0xdf30`，其时序反映的是单线程
路径。

该否定结论限于固定调用图。静态可达性对虚调用与函数指针可能不完整，因此它
不单独构成“三入口在任何宿主下都不会并行”的断言；它与既有隔离 core 记录的
实际单线程行为相互印证，但不外推到未观测的宿主路径。

## 对既有证据的解释

[`CONF-foo-dr-meter-108-x64-isolated-core-safe-master-report-run1-20260719`](../conformance/conf-foo-dr-meter-108-x64-isolated-core-safe-master-report-run1-20260719/record.md)
把隔离 core 的 raw result bits 与既有固定 foobar report 对照，得到 track DR
39/39、channel DR 62/62、channel RMS 62/62、overall peak 39/39 精确匹配。

结合本记录：前者来自不经 `0xdf30` 的单线程路径，后者来自经 `0xdf30` 的多线程
路径，两者在这些公开字段上逐位一致。这支持“该实现的并行在上述字段上是结果
不变的”这一有界结论。

这与
[`固定算法规格`](../specs/foo-dr-meter-1.0.8-candidate-v1.md) 的状态结构一致：
浮点累加只发生在窗口内部（`current_sum_squares` 窗口内累加、窗口结束归零），
跨窗口只保留窗口级的 `sum_window_rms2`；histogram 为整数计数、peak 为取最大
值。窗口内顺序不变、跨窗口按固定顺序归约即可保持逐位一致。

## 证据边界

本记录只登记固定 target 的调度结构、导入调用方、函数边界与固定调用图可达性。
它不建立：

- 并行的工作分割粒度、负载均衡策略或实际线程数（`_Thrd_hardware_concurrency`
  的返回值与工作量需求均未在本记录中动态观测）；
- 任何吞吐、加速比或性能声明。本次伴随的计时属于指示性测量，不满足 ADR-0007，
  不进入证据体系；
- foobar decoder、component lifecycle、metadata、album 或 renderer 的行为；
- 对未列出字段（如内部中间状态）的结果不变性；
- 对其他版本、架构或其他 target hash 的任何结论。

本记录不改变既有 observation、conformance 或产品数值声明。
