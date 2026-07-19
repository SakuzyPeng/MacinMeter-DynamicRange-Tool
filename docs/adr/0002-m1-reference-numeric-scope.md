# ADR-0002：限定 M1 的参考数值契约

- 状态：Accepted
- 实施状态：DONE
- 日期：2026-07-19
- 决策范围：M1
- 固定目标：`foo_dr_meter 1.0.8 x64`
- 相关路线图：[架构整改与参考插件重新对齐路线图](../ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md)
- 候选规格：[foo_dr_meter 1.0.8 candidate v1](../../reference/specs/foo-dr-meter-1.0.8-candidate-v1.md)

## 背景

M1 已经完成固定 x64 analyzer core 的静态分析、隔离直接执行和 39 项
safe-master 记录。MacinMeter 对同一 corpus 的六组公开数值字段精确匹配；
隔离 core 重建的四组报告字段也与 foobar 导出精确匹配。

旧路线图仍把以下工作并列为参考对齐缺口：

- production 与参考实现的内部状态逐项差分；
- foobar decoder、组件注册、metadata 和完整 host lifecycle；
- album playlist/grouping 的端到端行为；
- report renderer 的完整文本与 byte-for-byte parity。

这些事项混合了算法数值契约、宿主集成和展示实现。它们不是同一个兼容性目标，
也不应共同阻塞 M1。

## 决策

M1 固定研究目标是 `foo_dr_meter 1.0.8 x64` 的可复现数值契约。

纳入 M1：

- per-track analyzer core 的窗口、RMS、histogram、peak、DR 与 track 聚合；
- report 使用的纯数值派生：overall peak/RMS、整数 DR、两位 dB、近零修正、
  负零规范化和 duration 舍入；
- album 的纯聚合算术：binary32 track DR 输入、binary64 累计、unweighted 与
  duration-weighted 公式、回退、最终 binary32 窄化及整数显示舍入。

不纳入 M1：

- foobar decoder、component registration、service、线程调度和 host lifecycle；
- playlist、album grouping、自动发现和 metadata 来源；
- channel mask、bit depth、bitrate、codec 名称等宿主生成规则；
- 报告措辞、列布局、模板、换行、编码和 byte-for-byte 文本 parity；
- 参考实现与 MacinMeter 的内部结构、内存布局或逐检查点状态相等。

`BatchRunner` 仍只产生独立 track report；只有调用方显式构造
`AlbumTrackMetrics` 并调用 `AlbumAggregator` 时才具有 album 数值语义。

## 证据与验证规则

固定二进制中可由汇编控制流、指令精度、数据宽度和调用关系唯一确定的数值规则，
可以直接进入规格。它们不需要为了提高证据类别而重复人工导出样本。E1 表示当前
只有一类证据，不等同于算法仍未知。

隔离 core 与参考导出用于固定目标身份和验证已执行路径；MacinMeter 的最终可观测
结果差分用于发现转写错误。只有汇编无法确定的外部运行库或宿主边界，才追加有
区分力的动态实验。

MacinMeter 不为 conformance 暴露 production intermediate snapshot，也不要求
histogram、累计顺序或内部 peak 容器与目标实现同构。出现新的最终结果反例时，
才针对该反例检查相应中间路径。

## M1 收口结论

M1 已满足：

1. 固定 x64 per-track core 的版本化规则均可追溯到静态分析、隔离执行或参考观测；
2. 当前判别 corpus 的同语义最终数值字段没有未解释差分；
3. album 与 renderer 的上述纯数值规则已写入规格，并已有产品实现与边界测试；
4. 兼容性声明只覆盖已固定的目标、数值字段和有效输入契约；
5. 所有排除项均作为非目标记录，不再冒充缺失证据。

这不证明任意音频上的数学等价，也不把未观察的 host、playlist 或文本行为描述成
兼容。产品 profile 继续保持 `CandidateV1 / Unverified`；M1 完成不等于建立更广
的 foobar/component 兼容承诺。

## 后果

- reference 研究不再为了不可观察的实现细节制造第二套 production 诊断 API；
- album 和 renderer 中真正影响数值结果的少量规则仍可被复用和测试；
- 无法由目标 DLL 决定的宿主行为保持在兼容性声明之外；
- 后续扩大 corpus 的目的只是在出现可区分边界或回归风险时验证实现，不是替代
  白盒逆向重新猜算法。
