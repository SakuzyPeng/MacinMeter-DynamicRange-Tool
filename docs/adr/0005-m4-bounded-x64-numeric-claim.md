# ADR-0005：M4 固定 x64 数值声明与 decoder-independent 验收

- 状态：Accepted
- 实施状态：DOING
- 日期：2026-07-20
- 决策范围：M4
- 相关路线图：[架构整改与参考插件重新对齐路线图](../ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md)
- 证据矩阵：[M4 x64 数值声明证据矩阵](../M4_X64_NUMERIC_CLAIM_MATRIX.md)
- 前置决策：
  - [ADR-0002：限定 M1 的参考数值契约](0002-m1-reference-numeric-scope.md)
  - [ADR-0003：M2 原生解码面与工程契约加固](0003-m2-native-decoder-contract-hardening.md)
  - [ADR-0004：M3 application 执行预算与串行准入](0004-m3-application-execution-budget.md)

## 背景

M1 已经固定 `foo_dr_meter 1.0.8 x64` 目标、静态数据流、39 项 safe-master、
隔离 analyzer-core 记录、duration/weighting/histogram 数值边界记录和有限
implementation comparison。M2/M3 又加固了解码与 application 边界，但没有改变
Candidate 的参考目标。

旧 M4 描述仍可能被误读为：

- 继续无上限地生成输入或采集 foobar 报告；
- 为了让历史文件 corpus 继续经过产品 decoder 而扩大稳定格式面；
- 把静态唯一确定但证据等级仍为 E1 的规则强行升级标签；
- 将有限字段完全匹配改写为任意输入、完整宿主或文本 parity；
- 在没有反例时继续修改已经匹配的分析核心。

这些做法都不能提高声明的准确性。M4 的目标是收口声明，而不是扩大产品面。

## 决策

### 1. 固定目标是 1.0.8 x64 数值路径

M4 唯一参考目标为：

- 文件：`x64/foo_dr_meter.dll`
- 版本：`1.0.8`
- 架构：x86-64 PE32+
- SHA-256：
  `ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489`
- 固定 runtime profile：`fixed_foobar_2_25_10`

MacinMeter 可以在其他操作系统和 CPU 架构运行，但 Candidate 所复现的是上述固定
x64 数值控制流。x86 1.0.8 只作为跨架构精度判别证据，不与 x64 合并为一个未限定
架构的契约；1.0.3 也不在本阶段外推。

### 2. M4 纳入的声明

默认 `FooDrMeter108CandidateV1` 纳入：

- 有限、交错 `f64` PCM 上的窗口、RMS、histogram、peak 排名/回退、loud 20%、
  静音和默认全声道 track 聚合；
- 公开 binary32 channel/track DR、channel overall RMS/primary peak，以及
  reference-shaped track RMS/peak；
- 非负有效 duration 的半值舍入与 minute/hour/day/week 数值 token；
- 显式 `AlbumAggregator` 中由静态数据流唯一确定的 unweighted、duration
  weighting、binary32 窄化和非负整数显示规则。

其中 album 公式继续保留其 E1 静态证据边界；“数值 DR0 不统一过滤”窄规则为 E2。
实现完整静态公式不等于建立完整 album subsystem conformance。

### 3. M4 明确不纳入

以下内容不是 M4 的未完成算法工作：

- foobar decoder 对容器/PCM 的归一化与产品 codec 支持矩阵；
- component 注册、宿主 lifecycle、playlist/grouping、metadata 和 footer 来源；
- channel label、模板、locale、换行、编码或完整文本 byte parity；
- 参考插件的可选多声道 loudness weighting 产品开关；该分支保留为已记录的参考
  事实，但当前唯一生产 profile 仍使用默认关闭设置；
- x86 数值 profile、1.0.3、无效或非有限 PCM、超出产品资源范围的计数溢出；
- 任意 CRT/libm 最后一位和任意 PCM 的穷尽证明。

这些边界必须出现在最终兼容性报告中，但不以增加产品选项、backend 或样本来
“补齐”。

### 4. conformance 必须与 decoder 分层

`foo-dr-meter-108-complete-v2` 的 39 项 safe-master 中有 5 项多声道文件使用
`WAVE_FORMAT_EXTENSIBLE`。M2 按 ADR-0003 明确把该容器变体保持为 unavailable。
在 commit `6b02167` 上重新生成相同 manifest 并运行 release CLI，结果是：

- 34 项 stable classic-WAV 路径成功；
- 5 项以 `UnsupportedFormat / Probe` 拒绝；
- 被拒绝项恰为 `three-channel-arithmetic`、`six-channel-lfe`、
  `eight-channel-report-map`、`aggregate-narrow-low` 和
  `aggregate-narrow-high`；
- 五项错误均为
  `WAVE_FORMAT_EXTENSIBLE is not in the stable native WAV matrix`。

这不是 Candidate 数值差分，也不能成为扩张 decoder 的理由。M4 的当前实现
conformance 必须新增 reference-side adapter：

1. 复用既有受控工具把 manifest fixture 转成有限、交错 `f64le` PCM；
2. 直接驱动公开 `AnalyzerSession`，不经过 `DecoderFactory`；
3. 每个输入使用独立 worker，固定 worker/输入/manifest 身份与 timeout；
4. 只比较公开最终数值字段，不要求 production/reference 中间状态同构；
5. 与保存的 x64 core bits 和 normalized report 做零容差比较。

历史 schema-v3 文件级 conformance 保持其原提交身份，不回写成当前 decoder
仍支持这 5 个容器输入。

### 5. 证据与产品回归分开

参考事实继续由固定 target、observation、static analysis 和 conformance artifact
承载。普通产品测试不得读取 observation 后把候选实现与自身输出比较。

M4 增加两类本地门禁：

- 证据契约测试：固定关键 JSON artifact 的 SHA-256、目标身份、记录范围、匹配计数
  和 `candidate / unverified` 声明；它只防止证据元数据静默漂移；
- 产品回归测试：直接固定 Candidate 的 chunk/window/multichannel/renderer/album
  结果；它保证实现忠实于已登记规则，但不提升参考证据等级。

Windows harness 和历史 observation 不进入日常测试。只有出现纳入范围、会改变
最终输出、且静态数据流无法排除替代解释的反例时，才追加动态采集。

### 6. 不使用宽容差或标签替代残差解释

固定 report token 和公开 raw bits 使用精确相等。不得以“误差小于某个 dB”掩盖
系统性边界差分。

M4 结束时每一项已知限制必须属于以下一种：

- 纳入范围且已有证据、实现和本地回归；
- 纳入范围但发现反例，已修复并留下差分记录；
- 有明确理由排除，并在最终声明中可见。

不能保留“结果有偏但原因不明”的第四类。

### 7. M4 不自动升级 profile 名称

完成 M4 只形成固定 x64 目标、固定数值字段和有限 corpus 上的有界一致性报告。
生产身份继续是：

```text
FooDrMeter108CandidateV1 / Unverified
```

`accepted`、`verified`、`compatible` 或新的 Reference profile 都需要独立决策；
本 ADR 不授权修改这些名称。

## 出口条件

M4 只有在以下条件全部满足时才能变为 `DONE`：

1. 路线图第 6.5 节七项标准逐项有证据矩阵结论；
2. 当前提交的 decoder-independent 39 项 Candidate suite 形成身份明确的记录；
3. 六组公开 report 字段与 reference token 继续零差分；
4. duration 24/24、weighting 8/8、pair invariants 4/4、histogram 6/6 的保存证据
   身份由本地测试固定；
5. album/renderer 纳入的纯数值边界具有产品测试；
6. 所有限制和非目标进入最终兼容性报告；
7. 没有纳入范围内未解释的系统性残差；
8. 输出仍明确标记 `CandidateV1 / Unverified`。

## 实施顺序

1. 建立本 ADR、证据矩阵和 evidence-contract test；
2. 实现只接收有限 interleaved `f64le` 的 Candidate conformance worker；
3. 建立 serial suite runner 和最终字段 comparator，不比较内部状态；
4. 对当前 clean commit 生成新的 39 项 decoder-independent conformance record；
5. 只补矩阵确认缺失的产品边界测试；
6. 发布 M4 最终兼容性报告并更新路线图状态。

## 后果

- M4 不会为了测试便利破坏 M2 codec 能力边界；
- reference profile 的验证入口与文件 decoder 明确解耦；
- 历史 file-level conformance 和新的 direct-PCM conformance 各自保留真实身份；
- 已逆向但未进入生产配置的 optional 行为不会悄悄扩大 profile；
- 后续算法重构拥有参考证据、直接 PCM conformance 和产品工程不变量三层门禁。

## 未采用方案

### 为五个 fixture 恢复 WAVE_FORMAT_EXTENSIBLE

拒绝。格式毕业必须满足 ADR-0003 的 codec 合同，不能作为 M4 测试夹具适配器。

### 只沿用旧提交的 39/39 文件级记录

拒绝。历史记录有效，但不能证明 M2/M3 后当前 Candidate 实现没有漂移。

### 比较 production/reference 全部中间状态

拒绝。白盒证据用于恢复规则；实现 conformance 比较已声明的最终语义。内部布局
同构既不必要，也会把实现锁死在参考二进制的数据结构上。

### 为提高 E1 标签继续采集无区分力报告

拒绝。静态数据流已经唯一确定且有产品边界测试时，改变证据类别本身不是产品交付。
