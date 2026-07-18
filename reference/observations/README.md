# Observations

本目录尚未加入参考观测。这里只保存参考目标的原始或最小规范化输出，不保存
MacinMeter 自身生成的结果。

每条 observation 至少包含：

- observation ID、target ID 和 experiment ID；
- 运行日期、时区、平台和配置；
- 输入 fixture ID 与 SHA-256；
- 原始输出或原始输出的受控转录；
- 转录/解析步骤及工具版本；
- 重复运行是否一致；
- 已知采集限制。

原始值与解释必须分开。算法解释写入 `specs/`，实现差分写入 `conformance/`。
