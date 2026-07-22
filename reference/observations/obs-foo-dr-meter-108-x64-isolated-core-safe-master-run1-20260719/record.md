# OBS-foo-dr-meter-108-x64-isolated-core-safe-master-run1-20260719

## 结论

- 状态：`accepted_isolated_core_observation`
- 事实类别：固定 x64 target 的受控 analyzer-core 动态观测
- 日期：2026-07-19（Asia/Shanghai）
- target：`TARGET-foo-dr-meter-1.0.8-x64-static-ff3556ad`
- experiment：`foo-dr-meter-108-complete-v2` 的 39 项 `safe` 输入
- 兼容性：`none`
- foobar parity：`not_assessed`

本记录不启动 foobar2000。它把 manifest 绑定的 WAVE 样本确定性转换为有限、
交错 binary64 PCM，每项启动一个新的 Windows x64 worker，直接调用固定目标的
init/push/finish RVA，并保存 result、session、channel state 与浮点控制位的原始
bits。

固定 safe-master 的 39 项输入全部成功，没有 tagged input/worker error。产物
[`suite.json`](suite.json) 是 path-free、finite、key-sorted canonical JSON；
重新解析后仍满足 39 个成功 item、连续 manifest 顺序和每项 13 个
`shared.dll` IAT tripwire 的协议约束。

## 固定身份

| 对象 | Byte length | SHA-256 |
| --- | ---: | --- |
| complete-v2 manifest | — | `479e535a7196487fdb67a54f0c4de681f925920453e8092bac9eeb04eec4bbf8` |
| x64 target DLL | 424448 | `ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489` |
| x64 worker | 278016 | `0e09e6795a10f0d3e368ab5626cc2b0ab792edbc8bd9515baf3b12be6011b92f` |
| `shared.dll` | 142336 | `f860ee48f9e88a4da575c8114a82a11e3d25ceb9c8ce3405f646917cf07c7e4d` |
| `msvcp140.dll` | 579920 | `003da4807acdc912e67edba49be574daa5238bb7acff871d8666d16f8072ff89` |
| `vcruntime140.dll` | 109392 | `a8f950b4357ec12cfccddc9094cca56a3d5244b95e09ea6e9a746489f2d58736` |
| `vcruntime140_1.dll` | 49520 | `e4b533a94e02c574780e4b333fcf0889f65ed00d39e32c0fbbda2116f185873f` |
| suite | 161259 | `3cdb5132f7239ba1a500339e5138cb8d0713af952b9dfaff4ca206c112d34a61` |

- suite ID：
  `10e8d433a74dba864743692106d90f4f6a31ea6b742adde10149047e48b1697f`
- runtime profile：`fixed_foobar_2_25_10`
- process model：`one_worker_process_per_input`
- block size：512 frames

目标、worker 和 runtime 二进制不进入仓库；上表摘要与长度固定本次实际执行身份。
父进程在启动 worker 前检查这些身份，worker 又对 source bytes 和私有 staged
bytes 分别复核。

## 隔离边界

一次成功执行的边界是：

1. 父进程严格验证 manifest、fixture bytes、WAVE 几何与 data hash，再确定性转换
   为交错 `f64le`；整个 suite 只使用首次解析的一份 manifest 内存快照；
2. 新 worker 以拒绝写入/删除的 handle 锁定输入、target 和 runtime source，
   复核长度与 SHA-256；
3. worker 以 current-user + Local System 的 protected DACL 建立 128-bit 随机
   私有目录；拒绝 reparse 的目录 no-delete handle 持有到 unload，所有 staged
   file 也以 no-write/no-delete handle 持有，并在 load 前后复核 volume/file ID
   与 SHA-256；
4. worker 启用
   `LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32`；
5. 固定真实 `shared.dll` 完成目标 DLL 的 PE load lifecycle，随后 worker 验证
   实际 module path 与 staged bytes；
6. worker 验证固定 core/cleanup RVA 位于 executable section，再把目标对
   `shared.dll` 的全部 13 个普通 IAT slot 替换为同一个 fail-fast tripwire；
7. worker 固定并记录 x87 control word 与 MXCSR，执行 init、分块 push、finish，
   捕获 raw result/session/channel state，再在 tripwire 仍生效时调用 core
   cleanup；
8. worker 恢复原 IAT，卸载目标和四个 runtime module，验证它们不再加载，恢复
   原浮点控制位，最后输出一行严格 JSON。

父进程自己的 Windows staging 同样在启动前重哈希，并在完整子进程周期持有目录、
worker、PCM 与 request 的 no-write/no-delete、no-reparse handle；锁内又复核
worker、PCM hash/length 以及 request 的 canonical bytes。manifest 和 worker
response 的所有嵌套 JSON object 均拒绝重复 key；stdout 与 stderr 各自在读取期
限制为 1 MiB，超时或超限会终止直接 worker，而不是先无界收集到内存。

每个成功 item 都包含：

```json
{
  "loadLifecycle": "real_shared",
  "coreExecution": "fail_fast_iat_tripwire",
  "armedImportCount": 13
}
```

因此这不是用假 host 返回值“哄过”目标：core interval 中任何一次经过这 13 个
普通 IAT slot 的 `shared.dll` service 调用都会终止该 worker。真实
`shared.dll` 被保留用于目标 DLL 装载和卸载所需的窄生命周期；这不构成 foobar
component registration 或 host lifecycle 证据。该动态机制本身不拦截预先缓存
的函数指针、运行时 `GetProcAddress` 或经其他 module 中转的调用；这些路径只有
在另有静态或动态证据时才能排除。

## 负向边界检查

单独构建的 fail-fast `shared.dll` 精确导出目标所需的 13 个符号，并额外带一个
固定 marker。让目标直接装载它时，目标在 `LoadLibraryExW` 返回前即触发
fail-fast，Windows status 为 `0xc0000602`；所以该 shim 只作为 loader-lifecycle
负向探针，不是成功 runtime profile。

使用真实 runtime 完成装载、arm IAT 后，设置
`MACINMETER_CORE_TRIPWIRE_SELF_TEST=1` 主动调用第一个 patched slot，也在进入
core 前以 `0xc0000602` 终止。正常 39 项执行均在同一 13-slot 边界下完成。

最终 worker 的 PE import audit 只列出 `bcrypt.dll`、`ADVAPI32.dll` 与
`KERNEL32.dll`，没有在自身启动期预载 `shared.dll`、`MSVCP140.dll`、
`VCRUNTIME140.dll` 或 `VCRUNTIME140_1.dll`。负向 shim 的 13 个 host exports
均指向同一 fail-fast RVA，marker 使用独立 RVA。

## 运行与构建环境

| 项目 | 值 |
| --- | --- |
| OS | Windows 10.0.19045.6466 x64 |
| C++ compiler | clang-cl 22.1.8, target `x86_64-pc-windows-msvc` |
| assembler | Microsoft MASM x64 14.51.36246.0 |
| Windows SDK | 10.0.26100 |
| CMake / Ninja | 4.4.0 / 1.13.2 |
| CRT link | static `/MT` |

native JSON parser 的 CTest 为 1/1 通过。父进程、suite 和 comparator 所在的
reference Python tests 为 95/95 通过；父进程定向测试也在 Windows Python 3.9
实机通过 17/17。

## 分块与重复性旁证

在同一最终 worker/runtime/target 身份下，另行执行了两组不写入 suite 的受控
检查：

- `exact-window-control` 分别使用 block frames
  `1, 511, 512, 513, 1048576`；排除环境采样记录后的 canonical core result
  digest 均为
  `1250a95049c4fd93423045ba632c68230d6bb85cdf36b24879104feeba8dcb61`；
- `three-channel-arithmetic` 使用五个全新 worker 重复执行；同一结果投影的 digest
  均为
  `0e22c3d4d040aa41f2d0f3f34ad71f56cefc5fea0d84bfc0f5c00f5d41ba0bb4`。

这些检查只说明当前固定环境中的 isolated core 分块不变性与执行重复性，不替代
foobar runtime repeat run，也不推广到其他 OS、CRT、CPU 或 target。

## 与既有 foobar 报告的窄对照

独立 comparator 将本 suite 的 core result bits 按固定 renderer 数据流重建为四类
已导出字段，再与 2026-07-18 的固定 x64 foobar normalized report 比较：

| 字段 | 精确匹配 |
| --- | ---: |
| 整数 track DR | 39/39 |
| 每声道两位 DR token | 62/62 |
| 每声道 RMS dBFS token | 62/62 |
| overall peak dBFS token | 39/39 |

差分数为 0，数值容差为 0。固定身份、比较规则与未比较项见
[`CONF-foo-dr-meter-108-x64-isolated-core-safe-master-report-run1-20260719`](../../conformance/conf-foo-dr-meter-108-x64-isolated-core-safe-master-report-run1-20260719/record.md)。

这个交叉检查支持“本次 direct-core bits 能产生既有报告中的这四类字段”，不支持
“foobar decoder/host/renderer 已由 direct harness 重放”，也不构成完整 component
parity。

## 明确限制

- manifest WAVE 到 binary64 PCM 的转换属于 harness，不是 foobar decoder 观测；
- 没有执行 foobar component registration、配置、playlist、GUI 或宿主调度；
- 没有采集 metadata、album grouping/weighting、footer 或完整 report renderer；
- `host-decode-*` fixture 名称只描述输入编码，不能据此声称验证了宿主解码；
- IAT tripwire 只覆盖固定目标的 13 个普通 `shared.dll` import slot，不覆盖缓存、
  动态解析或其他 module 中转的潜在调用；
- worker 不是 OS sandbox；timeout/超限会终止直接 worker，但当前跨平台父进程不
  使用 Windows Job Object 清理潜在后代进程树；
- 固定 RVA、结构大小和 cleanup 顺序只适用于上表固定 target；
- 没有验证其他目标版本、x86、其他 Windows/UCRT/CPU 环境或未列出的输入；
- 没有把一次有限 corpus 的成功升级为 accepted algorithm specification、E3
  compatibility 或 `Verified` profile。

这些未进入边界的部分不是本 harness 的静默缺口：它们被明确排除，使当前记录只
陈述可由固定 PCM、固定二进制、fail-fast host-service 边界和 raw bits 直接支持
的 analyzer-core 事实。
