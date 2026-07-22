# Conformance

2026-07-23 的 schema hygiene 迁移从保存的实现 JSON 中删除了旧项目状态字段；
PCM 与全部数值结果未变，受影响 artifact 的 SHA-256 和内部引用已同步更新。

M4 对现有记录的汇总审计见
[`M4 x64 数值声明证据矩阵`](../../docs/M4_X64_NUMERIC_CLAIM_MATRIX.md)。该矩阵
不是新的 conformance run。direct-PCM worker、serial runner 与 final-field
comparator 已建立；新的当前实现记录只有在工具源码提交、输入身份、worker 身份
和 comparison artifact 一起固定后才进入本目录。

当前记录：

- [`CONF-foo-dr-meter-108-x64-candidate-v1-direct-pcm-run1-20260720`](conf-foo-dr-meter-108-x64-candidate-v1-direct-pcm-run1-20260720/record.md)：
  从 clean commit 重建 Candidate worker，以 reference-side finite interleaved
  `f64` 直接驱动 `AnalyzerSession`。4096/997 frames-per-block 两次 39 项运行
  的 track raw bits 39/39、三组 channel raw bits 各 62/62、六组 report token
  均完全匹配，差分为 0；decoder 未使用，中间状态未比较。
- [`CONF-foo-dr-meter-108-x64-isolated-core-safe-master-report-run1-20260719`](conf-foo-dr-meter-108-x64-isolated-core-safe-master-report-run1-20260719/record.md)：
  将 39 项 accepted isolated-core raw result bits 与既有固定 x64 foobar
  normalized report 做窄字段对照。整数 track DR 39/39、channel DR 62/62、
  channel RMS 62/62、overall peak 39/39 精确匹配，差分为 0。它不运行
  foobar decoder、component registration、album 或 report renderer，声明仍为
  `compatibility: none`、`foobarParity: not_assessed`。
- [`CONF-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-report-v3-clean-20260718`](conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-report-v3-clean-20260718/record.md)：
  从已提交源码重建的 schema-v3 successor。它保持 track DR 39/39、channel DR
  62/62、overall primary peak 39/39、overall RMS 39/39、channel overall RMS
  62/62，并新增 duration token 39/39。footer 只比较 track count、sample-rate
  set、channel-count set、重建的 unweighted DR token 四项，并记录 DR0
  纳入/排除反事实；不把结果解释成精确 internal album mean、length weighting
  或 host metadata conformance。
- [`CONF-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-report-v3-20260718`](conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-report-v3-20260718/record.md)：
  schema v3 将 report metrics 与 DR diagnostics 分离后的首份扩展比较，固定
  dirty-worktree 二进制身份。五类 DR/report 字段全部精确匹配；该历史产物不由
  clean successor 回写。
- [`CONF-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-20260718`](conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-20260718/record.md)：
  schema v2 时代固定 x64 safe-master observation 与 MacinMeter 0.2.0
  candidate 的 DR-only pre/post-f64 历史差分。f64 修正后整数 track DR 为
  39/39，每声道两位 DR token 为 62/62；该记录和产物不由 schema v3 回写。

不得把 candidate 实现输出直接登记为“通过”；只有固定 reference observation
与身份明确的实现产物之间的比较，才可形成 conformance 记录。单个有限记录的
`match` 也不自动把规格升级为 accepted 或把 profile 改成 verified。

显式 `AlbumAggregator` 目前只落实静态恢复的完整 E1 公式。固定静态路径与
已导出 footer 的反事实共同支持“数值 DR0 track 不被排除”这一较窄子规则达到
E2；safe-master footer 仍不足以区分精确 internal mean、公开窄化点或 length
weighting 的替代实现，且 album-focused playlist 尚未导出。不得把该子规则的
升级扩张成完整 album conformance。按
[`ADR-0002`](../../docs/adr/0002-m1-reference-numeric-scope.md)，静态数据流已经
唯一确定的 album/renderer 纯数值公式可以由产品边界测试验收，不要求为了改变
证据类别而追加 playlist 导出。

每份差分摘要至少包含：

- conformance run ID；
- specification 版本和被测 commit/binary SHA-256；
- target、experiment、observation 和 fixture ID；
- 每个纳入范围的最终字段或纯数值规则的差分/边界断言；
- 容差及其证据来源；
- 已知系统性偏差和处置状态；
- 运行环境、执行命令和退出状态。

只有固定 reference observation 与实现输出的比较才能进入本目录。reference raw
intermediate state 可以作为逆向证据，但 production 内部状态、内存布局和逐检查点
相等不是 conformance 记录的必填项。
