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
