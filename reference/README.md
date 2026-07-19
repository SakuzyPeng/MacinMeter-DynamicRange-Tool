# Reference research workspace

本目录保存 MacinMeter 对参考 DR 实现进行可重复研究所需的公开材料。这里的
文件用于建立证据链，不表示项目已经与参考插件兼容。

当前 M1 研究边界由
[`ADR-0002`](../docs/adr/0002-m1-reference-numeric-scope.md) 固定为
`foo_dr_meter 1.0.8 x64` 的 per-track analyzer 数值契约，并保留 album 聚合与
renderer 的纯数值规则。M1 证据基础已完成；`CandidateV1 / Unverified` 表示没有
声明任意输入或完整 foobar/component parity，不表示仍需补 host、playlist、
metadata、完整文本或 production 中间状态同构。

## 目录

| 目录 | 内容 |
| --- | --- |
| [`authorization/`](authorization/README.md) | 公开的最小授权来源与边界摘要 |
| [`targets/`](targets/README.md) | 参考二进制、宿主、平台、配置和哈希身份 |
| [`experiments/`](experiments/README.md) | 可重复实验定义和输入生成参数 |
| [`observations/`](observations/README.md) | 参考目标的原始输出和运行环境 |
| [`static-analysis/`](static-analysis/README.md) | 固定二进制的受控静态分析证据 |
| [`native/`](native/foo_dr_meter_108_core_worker/README.md) | 固定 x64 core worker 与 fail-fast host-service 边界 |
| [`tools/`](tools/run_foo_dr_meter_108_core.py) | observation、隔离 core suite 与 conformance 工具 |
| [`fixtures/`](fixtures/README.md) | 可公开或可重复生成的实验输入 |
| [`specs/`](specs/README.md) | 带版本和证据等级的算法规格 |
| [`conformance/`](conformance/README.md) | 参考观测与实现结果的差分摘要 |

当前生产候选规格是
[`specs/foo-dr-meter-1.0.8-candidate-v1.md`](specs/foo-dr-meter-1.0.8-candidate-v1.md)。
[`specs/provisional-v1.md`](specs/provisional-v1.md) 只保留为 M0 历史工程基线；
两者都不是 accepted 兼容声明。

当前一次性完整输入协议是
[`experiments/foo-dr-meter-108-complete-v2.md`](experiments/foo-dr-meter-108-complete-v2.md)；
它用于承载静态规则回归，也保存历史 host-edge 输入定义，不取代既有 v1
observation。固定
x64 runtime 对其中 39 项 safe master 的首次导出已登记为
[`OBS-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718`](observations/obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/observation.json)。
MacinMeter 改用 finite interleaved f64 主链后，对同一 observation 的公开可比
DR 字段达到整数 track DR 39/39、每声道两位 DR token 62/62；这份 schema-v2
历史差分见
[`CONF-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-20260718`](conformance/conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-20260718/record.md)。

schema v3 将 report metrics 与 DR diagnostics 分离后，最初的固定比较又得到
overall primary peak 39/39、overall RMS 39/39、channel overall RMS 62/62，
同时保持 track DR 39/39、channel DR 62/62；这份 dirty-worktree 身份的历史记录见
[`CONF-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-report-v3-20260718`](conformance/conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-report-v3-20260718/record.md)。

基于已提交源码重建的 clean-commit successor 保持上述五类字段完全匹配，并新增
reference duration token 39/39；见
[`CONF-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-report-v3-clean-20260718`](conformance/conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-report-v3-clean-20260718/record.md)。
它对 footer 只登记 track count、sample-rate set、channel-count set 和重建的
unweighted DR token 四项一致性，以及 DR0 纳入的反事实；不把最终整数 token
外推为精确 internal album mean、length weighting 或 host metadata parity。
旧产物不因 successor 落地被改写。有限 conformance 不会把 candidate 升级为
accepted，也不改变 `Unverified` 状态；album 完整数值公式为 E1 静态证据，但
其汇编数据流已经确定，不以 playlist 导出作为 M1 前置条件。

固定 x64 target `ff3556ad` 现另有
[`隔离 analyzer-core harness`](observations/CORE_HARNESS.md)。它不启动
foobar2000，而是让每个输入进入一个全新 Windows x64 worker，直接调用
init/push/finish。固定 complete-v2 safe-master 的 39 项输入已全部完成受控执行，
记录见
[`OBS-foo-dr-meter-108-x64-isolated-core-safe-master-run1-20260719`](observations/obs-foo-dr-meter-108-x64-isolated-core-safe-master-run1-20260719/record.md)。
真实、固定的 `shared.dll` 被保留用于 DLL load/unload lifecycle；core 执行期间，
目标的全部 13 个普通 `shared.dll` IAT 入口均由 fail-fast tripwire 接管。该边界
可直接观察 session、channel state 与 result，但没有执行 foobar decode、
registration、metadata、album 或 renderer。记录因此明确声明
`compatibility: none` 与
`foobarParity: not_assessed`，不能据此把 candidate 升级为 compatible。
这些 raw result bits 与既有固定 foobar report 的
[`窄字段对照`](conformance/conf-foo-dr-meter-108-x64-isolated-core-safe-master-report-run1-20260719/record.md)
得到 track DR 39/39、channel DR 62/62、channel RMS 62/62 和 overall peak
39/39 精确匹配；该对照不扩大上述执行边界。

最后几个可能改变 per-track 可见输出、但此前只有静态证据的数值分支，现由
[`38 项隔离 numeric-boundary observation`](observations/obs-foo-dr-meter-108-x64-numeric-boundaries-v1-run1-20260719/record.md)
一次收口：duration 24/24、multichannel weighting 的 track raw bits 8/8、
channel 前提 8/8、配对不变量 4/4、histogram endpoint 6/6。该运行仍不启动
foobar2000；duration 只执行固定 numeric leaf，因而不会把完整 renderer 或
component parity 纳入结论。

这组 accepted 隔离观测、静态记录、版本化规格和有界 conformance 共同构成 M1
主证据。此前保存的 foobar host、metadata、album writer 和 renderer 记录仍然
有效，但只在其各自声明的边界内作为历史或辅助数值证据，不是 M1 待办。

## 五类事实

所有新增数据必须标记为以下一种：

1. **工程不变量**：内存安全、终态、chunk 不变性等不依赖参考插件的契约；
2. **临时实现契约**：为保证 0.2.0 可复现而冻结、但不声称来自参考证据的选择；
3. **参考观测**：固定目标对固定输入产生的原始输出；
4. **算法规格**：由观测、静态分析或动态跟踪支持的行为说明；
5. **Legacy snapshot**：0.1.x 当前实现的输出，仅用于观察迁移差异。

Legacy snapshot 不得用作 correctness golden，当前实现的输出也不得被写回
`observations/` 冒充参考结果。

## 证据等级

| 等级 | 含义 |
| --- | --- |
| E3 | 黑盒实验、静态分析和动态跟踪相互印证 |
| E2 | 至少两类独立证据相互印证 |
| E1 | 单类证据支持，尚缺交叉验证 |
| H | 高置信假设，仍有可区分的替代解释 |
| U | 未知或证据冲突 |

每条关于参考行为的规格结论必须引用 observation/experiment 标识并注明等级。
纯工程不变量和明确标成“临时实现契约”的 M0 选择不伪造证据等级；它们是否符合
参考实现仍标为 U。关键参考行为在进入稳定 profile 前原则上不能停留在 H 或 U。

## 提交规则

- 记录目标版本、平台、配置、时区和二进制 SHA-256；只有实际证据边界包含宿主时
  才记录宿主版本，不为隔离 core 伪造 host 身份；
- 实验输入优先由文本参数或生成器确定，避免提交来源不明的大型媒体；
- 原始 observation 一旦用于规格应保持不可变，修正通过新记录完成；
- conformance 摘要必须同时指向 reference observation 和被测实现版本；
- 不提交私人授权原文、受限制二进制、账号信息、绝对个人路径或机器秘密；
- 大文件和不可公开 fixture 只提交生成说明、哈希和安全存放位置。

授权来源摘要见 [`authorization/README.md`](authorization/README.md)，公开边界见
[`docs/LEGAL_CN.md`](../docs/LEGAL_CN.md)。这些工程记录都不替代专业法律意见。
