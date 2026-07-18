# Observations

这里只保存参考目标的原始或最小规范化输出，不保存 MacinMeter 自身生成的结果。

当前观测：

- [`OBS-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718`](obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/observation.json)：
  固定 foobar2000 2.25.10 x64 / `foo_dr_meter` 1.0.8 x64 对 complete-v2
  39 项 safe master 的单次原始导出；39 个 track 与 62 个声道值均已按 manifest
  完整规范化。重复运行和三个 isolated 输入未在该记录中采集。
- [`OBS-foo-dr-meter-108-x86-discriminating-v1-run1-20260718`](obs-foo-dr-meter-108-x86-discriminating-v1-run1-20260718/observation.json)：
  固定 foobar2000 2.0 x86 / `foo_dr_meter` 1.0.8 x86 的 15 项初步单次黑盒
  导出；已与固定 x64 静态路径交叉印证，但没有重复运行。

每条 observation 至少包含：

- observation ID、target ID 和 experiment ID；
- 运行日期、时区、平台和配置；
- 输入 fixture ID 与 SHA-256；
- 原始输出或原始输出的受控转录；
- 转录/解析步骤及工具版本；
- 重复运行是否一致；
- 已知采集限制。

原始值与解释必须分开。算法解释写入 `specs/`，实现差分写入 `conformance/`。
