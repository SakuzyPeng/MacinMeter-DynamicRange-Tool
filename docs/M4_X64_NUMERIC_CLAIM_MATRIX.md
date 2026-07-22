# M4 x64 数值声明证据矩阵

> 状态：`DONE`
>
> 决策：[ADR-0005](adr/0005-m4-bounded-x64-numeric-claim.md)
>
> 结论：[M4 固定 x64 数值声明收口报告](M4_X64_NUMERIC_ALIGNMENT_REPORT.md)
>
> 固定目标：`foo_dr_meter 1.0.8 x64`
> `ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489`

## 目的

本矩阵把 Candidate 规格中的规则映射到参考证据、当前实现、本地回归和 M4 处置。
它不是新的 observation，也不把实现测试冒充参考证据。

状态含义：

- `IN`：进入 M4 最终数值声明；
- `BOUNDARY`：与纳入规则相邻，但只在明确限制内声明；
- `OUT`：明确不属于 M4 产品数值声明；
- `GAP`：必须在 M4 收口前补齐。

## 固定证据身份

| 对象 | 身份/结果 | M4 用途 |
| --- | --- | --- |
| x64 target | SHA-256 `ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489` | 唯一参考二进制 |
| complete-v2 manifest | SHA-256 `479e535a7196487fdb67a54f0c4de681f925920453e8092bac9eeb04eec4bbf8`；42 cases，39 safe | 固定判别输入 |
| isolated-core suite | SHA-256 `3cdb5132f7239ba1a500339e5138cb8d0713af952b9dfaff4ca206c112d34a61`；39/39 success | decoder-independent reference core |
| numeric-boundary suite | SHA-256 `b5a99ff50eb78eeb2258fb15f5d75d8d92978743abb4dabe9639f3453bd570d3` | duration 24/24、weighting 8/8、pair 4/4、histogram 6/6 |
| clean schema-v3 comparison | SHA-256 `e2c6478f19fb9b3094bf056215c7472bb38eea585f6c9affe2ba1269a458dab0` | 六组公开字段历史零差分基线 |
| normalized x64 report | SHA-256 `50205960b9850addb7f18bdb5f3c2c3c59897a5a2c5efc8e408870d5a3a2ffce` | 公开 report token reference |
| direct Candidate suite，4096 frames/block | SHA-256 `60810a3a12100183e3dedad61f94d45ff4e5a07b515a0a3f7b91ecfe8d5ad712`；39/39 success | 当前 clean implementation 主记录 |
| direct comparison，4096 frames/block | SHA-256 `d35f392567499d0f80befe2eb03690c0f7a8a5d15773a7f3817b4b26dda3402e`；全部 final fields 精确匹配 | 当前主 conformance 结论 |
| direct suite，997 frames/block | SHA-256 `47911f270cd0cb1980ed320aee114420ec85aa1e407b8023824d9055ae0a79bb`；39/39 success | 非整齐 chunk 复核 |
| direct comparison，997 frames/block | SHA-256 `621e0ff98d70253dd674b44f23ffed164c60dae722178f82aae5dbcfb1aff775`；全部 final fields 精确匹配 | chunk-independent 复核 |

## 规则矩阵

| 范围 | 固定规则 | 参考证据 | 当前产品回归 | 状态与处置 |
| --- | --- | --- | --- | --- |
| algorithm parameters | 固定 1.0.8 x64、binary64 PCM 路径与版本化规则 | SA-x64、SA-cross、OBS-x64 precision fixtures | `records_fixed_algorithm_parameters`、`preserves_f64_pcm_without_narrowing_before_accumulation` | `IN` |
| window geometry | `floor(rate × coefficient)`；block 不定义窗口 | SA-x64/SA-cross、OBS-x86/x64、OBS-core block-size | window-length table、随机 chunk、完整 raw-bit chunk matrix | `IN` |
| EOF/tail | 任意非空尾窗含一帧；精确整窗无虚拟零 | 两份 SA、104/201、OBS-core | `every_nonempty_tail_is_submitted_and_no_virtual_window_is_added`、window boundary matrix | `IN` |
| RMS accumulation | `sqrt(2 × sum_squares / frames)`；overall channel RMS 为 window RMS² 等权均值 | 两份 SA、103/104/110/111 | fixture 110/111、overall RMS 与 tail overflow tests | `IN` |
| histogram | centi-dB 量化、`[-100,0]` clamp、10001 bins | SA、OBS-boundary 6/6 | `histogram_clamp_regression_covers_observed_boundary_vectors` | `IN` |
| loud selection | `max(1,floor(N/5))`；边界 bin 整组纳入；bin 中心重建 | SA、110/111、5/10-window observations | fixture 111、six-window floor、sparse histogram tests | `IN` |
| peak ranking | centi-dB key、严格 `>`、两个候选、tie 保留到达顺序 | SA、120/121、source-f64 peak fixture | 120/121、duplicate key、two-positive-peak tests | `IN` |
| peak fallback | secondary 优先；负 DR 用 primary 重算并 clamp `+0.0` | SA、105/202 | fixture 105、negative-primary regression | `IN` |
| silence | 静音产生数值 DR0 并进入默认 track mean | SA、203、301、album DR0 footer 反事实 | 201–203、301、CLI finite/null rendering | `IN` |
| square underflow | 有效窗口中 square 下溢为零时保留数值 `+0.0` | SA + 固定 MXCSR，E1 | `squared_underflow_remains_a_numeric_dr_zero_contribution` | `BOUNDARY`：按已登记控制流实现，不宣称专门动态覆盖 |
| default track aggregate | 内部 f64 channel DR 全声道算术均值，包含静音和 LFE | SA、301/302/303、8ch OBS | fixtures 301–303、lane/permutation/replication tests | `IN` |
| optional track weighting | `C>2` 时按 overall channel RMS 加权 | SA、OBS-boundary track 8/8 + pair 4/4 | 当前生产配置不暴露该开关 | `OUT`：保留参考事实，不扩张产品配置 |
| public narrowing | channel/track DR 与 channel report metrics 在固定点窄化到 f32 | SA、610/611、schema-v3 comparison | public-f32 rounding、report-square、aggregate-narrow tests | `IN` |
| report RMS/peak | public channel RMS 的 f32 平方聚合；public primary peak 最大值 | SA-cross、39/39 + 62/62 report comparison | track report tests、finite wrapper/result invariant tests | `IN` |
| DR/dbFS renderer | 非负 f32 DR `+0.5`；centi-dB near-zero 修正；`-inf` 显式表示 | SA-render、120/121、610/611、report observation | CLI renderer unit/black-box tests | `IN`：只声明数值 token，不声明完整文本 |
| duration | frames/rate 经 `llround`；minute/hour/day/week token | SA-render/SA-duration、OBS-boundary 24/24、report 39/39 | `duration_tokens_preserve_observed_half_second_and_carry_boundaries` | `IN` |
| album arithmetic | public-f32 track DR 的 f64 mean、可选 duration weighting、最终 f32、DR0 纳入 | album writer SA（完整公式 E1）；DR0 子规则 E2 | `album_contract` 的 unweighted/weighted/rounding/DR0/zero-duration tests | `IN`：只声明显式数值 API，不声明 grouping/footer |
| channel labels | ordinal 表与 fallback 文本 | SA-render；部分 ordinal E2、其余 E1 | 产品 human renderer 使用自身 `CH n` 文案 | `OUT`：文本 parity 非目标 |
| zero-frame host | core 与首次无 decode chunk 的宿主行为分层 | 两份 SA，E1 | 产品 codec/session 有自身结构化契约 | `OUT`：host/UI 行为非目标 |
| x86 precision | x86 使用不同 PCM/square/peak 精度 | SA-cross + x64 f64 discriminator，E2 | 产品不实现 x86 参考数值路径 | `OUT`：不得与 x64 合并 |
| invalid/extreme inputs | NaN/Inf、异常范围、计数溢出、全部 libm 最后一位 | U/H 或产品资源边界 | structured error、finite JSON、resource-limit tests | `OUT/BOUNDARY`：安全契约不冒充参考行为 |

## 路线图第 6.5 节核对

| 标准 | 当前结论 | 剩余工作 |
| --- | --- | --- |
| 固定 corpus 同语义最终字段完全一致 | 历史 comparison 与当前两次 direct comparison 均零差分 | 无 |
| window/RMS/histogram/peak/aggregate 可追溯 | 版本化规格§5 已逐规则登记 E1/E2 | 无算法证据空白 |
| album/renderer 数值边界有产品测试 | album contract、duration/dbFS renderer、histogram clamp 已覆盖 | 无 |
| 残差无系统趋势 | 两次当前 comparison `differenceCount=0` | 无 |
| 极短/tail/tie/silence/multichannel 无未解释例外 | safe-master、OBS-core 与产品边界测试覆盖 | square-underflow 保持 E1 限制，不伪装动态事实 |
| 分析规则不依赖 decoder/chunk/优化路径 | direct f64 绕过 decoder；4096/997 block 的公开 projection 相同 | 无 |
| 忠实保留目标的奇怪规则 | 一帧尾窗、whole-bin、arrival-order tie、DR0/LFE 纳入均已实现 | 新反例出现时才重开 |

## 当前文件级 replay 结论

在 clean commit `6b02167` 上，使用仓库生成器重建 manifest SHA-256
`479e535a...` 后运行 release CLI：

- 34/39 项成功；
- 5/39 项返回 `UnsupportedFormat / Probe`；
- 五项都是 manifest 中带 channel mask 的多声道 `WAVE_FORMAT_EXTENSIBLE`：
  `three-channel-arithmetic`、`six-channel-lfe`、`eight-channel-report-map`、
  `aggregate-narrow-low`、`aggregate-narrow-high`；
- 当前错误与 ADR-0003 的稳定能力矩阵一致，不是数值 residual。

本段是可重复的 M4 架构审计，不是算法失败。当前正式
[direct-PCM conformance record](../reference/conformance/conf-foo-dr-meter-108-x64-candidate-v1-direct-pcm-run1-20260720/record.md)
已经把算法验证与该 decoder split 分开。

## M4 缺口处置

### GAP-M4-001：当前提交的 decoder-independent analysis suite

状态：`DONE`

reference-side
`candidate_v1_conformance_worker`、`run_macinmeter_candidate_v1_suite.py` 和
`compare_macinmeter_candidate_v1_suite.py` 已绑定 commit
`76d0f2eab5cdfce9de6a9d76ab971c333eab8e71` 形成正式记录：

- track DR bits 39/39；
- channel DR/RMS/primary-peak bits 各 62/62；
- track/channel DR、overall peak/RMS、channel RMS 和 duration report token
  全部精确匹配；
- `differenceCount = 0`，中间状态未比较，decoder 未使用。

4096 与 997 frames/block 两次运行的完整公开 projection SHA-256 都为
`afee42eebfde4646a7bc2c60cda9070b97a709a1c8d3e468b1baade365977969`。
不比较 histogram、session layout 或其他 production/reference 中间状态。

### GAP-M4-002：关键证据身份本地门禁

状态：`DONE`

`reference/tools/tests/test_m4_evidence_contract.py` 固定本矩阵引用的 artifact digest、
目标 SHA、数量、匹配摘要、声明边界和五项 decoder split。它不运行 Windows 或
重新计算参考输出。reference 工具测试同时覆盖 direct suite 的进程隔离、worker
身份、path-free 失败记录、精确 comparator 和 decoder-independent 约束。

### GAP-M4-003：最终兼容性报告

状态：`DONE`

[M4 固定 x64 数值声明收口报告](M4_X64_NUMERIC_ALIGNMENT_REPORT.md) 已发布
固定 target、字段、精确匹配、证据等级、限制和非目标。profile 只标识算法规则
修订，不作为结果兼容性结论。
