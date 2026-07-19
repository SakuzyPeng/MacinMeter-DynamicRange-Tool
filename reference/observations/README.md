# Observations

这里只保存参考目标的原始或最小规范化输出，不保存 MacinMeter 自身生成的结果。

当前观测：

- [`OBS-foo-dr-meter-108-x64-isolated-core-safe-master-run1-20260719`](obs-foo-dr-meter-108-x64-isolated-core-safe-master-run1-20260719/record.md)：
  固定 `foo_dr_meter` 1.0.8 x64 target `ff3556ad` 的 accepted 隔离
  analyzer-core 动态观测。它不启动 foobar2000；complete-v2 的 39 项 safe
  master 各使用一个全新 worker，直接调用 init/push/finish，39/39 均成功并保存
  result、session、channel state 与浮点控制位。真实、固定的 `shared.dll`
  被保留用于 load/unload lifecycle，core 执行期间全部 13 个目标普通 IAT 入口
  由 fail-fast tripwire 接管。该记录没有验证 foobar decode、registration、
  metadata、album 或 renderer，声明固定为 `compatibility: none`、
  `foobarParity: not_assessed`。
- [`OBS-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718`](obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/observation.json)：
  固定 foobar2000 2.25.10 x64 / `foo_dr_meter` 1.0.8 x64 对 complete-v2
  39 项 safe master 的单次原始导出；39 个 track 与 62 个声道值均已按 manifest
  完整规范化，39 个原始 duration token 与 footer 也按文本保留。后续
  conformance 可比较这些已存在字段，但不会回写本 observation。重复运行和三个
  isolated 输入未在该记录中采集；它们不是 ADR-0002 的 M1 阻塞项。
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

后续固定报告使用
[`reference observation import harness`](HARNESS.md)
建立或复核 observation 包。该流程离线重验 manifest、`FILES.sha256`、逐 fixture
内容哈希、报告哈希与 manifest 顺序，并阻止私人绝对路径进入产物；它不运行
foobar2000、候选模型或 MacinMeter。

固定 x64 analyzer core 使用
[`isolated core harness`](CORE_HARNESS.md)。该流程独立于 foobar process，以
manifest 或显式有限 interleaved binary64 PCM 驱动固定 DLL，并严格绑定
target、runtime、worker、block size 与输入身份。它是算法 core 观测工具，不是
foobar host 或兼容性测试替代品。host repeat、playlist/grouping、metadata 与
完整文本属于明确非目标，不因本 harness 未执行而成为缺失 core 证据。
