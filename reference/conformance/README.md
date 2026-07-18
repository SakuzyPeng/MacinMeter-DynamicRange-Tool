# Conformance

当前记录：

- [`CONF-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-20260718`](conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-20260718/record.md)：
  固定 x64 safe-master observation 与 MacinMeter 0.2.0 candidate 的公开核心
  字段差分。f64 修正后整数 track DR 为 39/39，每声道两位 DR token 为 62/62；
  报告 overall peak/RMS、内部状态、isolated 输入和更广输入空间不在比较范围。

不得把 candidate 实现输出直接登记为“通过”；只有固定 reference observation
与身份明确的实现产物之间的比较，才可形成 conformance 记录。单个有限记录的
`match` 也不自动把规格升级为 accepted 或把 profile 改成 verified。

每份差分摘要至少包含：

- conformance run ID；
- specification 版本和被测 commit/binary SHA-256；
- target、experiment、observation 和 fixture ID；
- 每个 final 字段与关键 intermediate state 的差分；
- 容差及其证据来源；
- 已知系统性偏差和处置状态；
- 运行环境、执行命令和退出状态。

只有固定 reference observation 与实现输出的比较才能进入本目录。
