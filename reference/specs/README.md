# Versioned specifications

本目录保存从参考证据推导出的版本化算法规格。

规则：

- 每个规格声明状态：provisional、candidate 或 accepted；
- 每条关于参考实现的行为带 E1/E2/E3/H/U 证据等级和来源链接；
- provisional profile 可记录明确标识的工程不变量或临时实现契约；这类条目不冒充
  参考证据，其对应参考行为仍必须列为 U；
- 未观察到的行为必须写为未知，不能用当前实现补齐；
- breaking 语义变化创建新版本，不静默改写已用于 conformance 的版本；
- accepted 规格必须列出适用 target、反例、容差和未决问题。

当前规格：

- [`provisional-v1.md`](provisional-v1.md)：M0 的可信边界和参考算法未知项。
- [`foo-dr-meter-1.0.8-candidate-v1.md`](foo-dr-meter-1.0.8-candidate-v1.md)：
  基于 1.0.8 x64/x86 静态分析、x86 初步观测和 x64 complete-v2 safe-master
  观测，并由 accepted 39 项 isolated x64 core 记录补充 raw 动态状态的候选算法。
  当前 schema-v3 MacinMeter 在有限公开字段上达到 track DR
  39/39、channel DR 62/62、overall peak 39/39、overall RMS 39/39、channel
  RMS 62/62、duration 39/39。DR0 track 纳入 album 的窄规则达到 E2；精确
  album mean、窄化与 length weighting 公式为 E1 静态数据流，不要求 focused
  playlist 才能定义这些纯数值规则。
  已观测的短时 `m:ss` 与 channel ordinal `0..5, 9, 10` 标签规则为 E2，
  半秒/长时 duration 及未覆盖标签分支仍为 E1。规格保持
  `candidate / unverified`，不声明通用 parity。M1 已完成；host、playlist、
  metadata、完整 renderer 与 production 中间状态同构是非目标。
