# Experiments

实验定义必须独立于当前 Rust 实现，并能生成或准确识别全部输入。

当前实验：

- [`EXP-foo-dr-meter-108-discriminating-v1`](foo-dr-meter-108-discriminating-v1.md)：
  已完成一次固定 x86 黑盒观测的 15 项初始判别实验。
- [`EXP-foo-dr-meter-108-complete-v2`](foo-dr-meter-108-complete-v2.md)：
  一次性生成 42 个 WAV 的完整回归与 host-edge corpus；39 项组成 safe master，
  3 项各自隔离。固定 x64 runtime 已完成一次 safe-master 导出；isolated 输入
  尚未采集。构造模型与 observation 的逐字段复核结果单独保存在
  [`foo-dr-meter-108-complete-v2-model-observation-comparison.json`](foo-dr-meter-108-complete-v2-model-observation-comparison.json)；
  它验证生成模型预测，但不是 reference golden。

实验记录至少包含：

- experiment ID 和研究问题；
- 目标 ID；
- 输入生成参数、随机种子和 fixture SHA-256；
- 宿主/插件配置；
- 执行步骤和重复次数；
- 预先声明的观察字段；
- 可区分的替代假设；
- 对应 observation ID。

建议优先覆盖窗口边界、极短输入、已知幅度、重复峰、尾窗和 1/2/3/6/8 声道。
