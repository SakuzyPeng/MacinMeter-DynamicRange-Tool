# Conformance

当前记录：

- [`CONF-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-report-v3-20260718`](conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-report-v3-20260718/record.md)：
  schema v3 将 report metrics 与 DR diagnostics 分离后的扩展比较。整数 track
  DR 39/39、每声道两位 DR 62/62、overall primary peak 39/39、overall RMS
  39/39、每声道 overall RMS 62/62，全部为精确 token 匹配。未比较内部状态、
  reference duration 文本、footer、isolated 输入与 album-focused 行为。
- [`CONF-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-20260718`](conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-20260718/record.md)：
  schema v2 时代固定 x64 safe-master observation 与 MacinMeter 0.2.0
  candidate 的 DR-only pre/post-f64 历史差分。f64 修正后整数 track DR 为
  39/39，每声道两位 DR token 为 62/62；该记录和产物不由 schema v3 回写。

不得把 candidate 实现输出直接登记为“通过”；只有固定 reference observation
与身份明确的实现产物之间的比较，才可形成 conformance 记录。单个有限记录的
`match` 也不自动把规格升级为 accepted 或把 profile 改成 verified。

显式 `AlbumAggregator` 目前只落实静态恢复的 E1 公式。safe-master footer
不足以区分替代 album 公式，且 album-focused playlist 尚未导出，因此上述
schema-v3 记录没有把 album 计入 conformance。

每份差分摘要至少包含：

- conformance run ID；
- specification 版本和被测 commit/binary SHA-256；
- target、experiment、observation 和 fixture ID；
- 每个 final 字段与关键 intermediate state 的差分；
- 容差及其证据来源；
- 已知系统性偏差和处置状态；
- 运行环境、执行命令和退出状态。

只有固定 reference observation 与实现输出的比较才能进入本目录。
