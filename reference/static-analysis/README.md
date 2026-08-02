# Static analysis evidence

本目录保存对固定 reference target 的受控静态分析记录。只登记独立撰写的函数
身份、算法事实、证据范围和工具版本，不提交目标二进制、IDA 数据库、反编译文本
或私人授权材料。

当前记录：

- [`SA-foobar2000-2.0-x64-wave-decoder-20260718`](sa-foobar2000-2.0-x64-wave-decoder-20260718.md)：
  固定 foobar2000 2.0 x64 `foo_input_std` 的 WAV PCM 到
  `audio_chunk<double>` 转换。
- [`SA-foobar2000-2.25.10-x64-wave-metadata-20260718`](sa-foobar2000-2.25.10-x64-wave-metadata-20260718.md)：
  固定 x64 runtime target 中 `foo_input_std` 与插件的 WAV bit-depth metadata
  数据流；把 footer 的 `32761` 分类为根因未定的外围异常，并确认它不参与核心
  DR。
- [`SA-foo-dr-meter-108-x64-20260718`](sa-foo-dr-meter-108-x64-20260718.md)：
  `foo_dr_meter` 1.0.8 x64 核心分析路径。
- [`SA-foo-dr-meter-108-x64-parallel-dispatch-20260802`](sa-foo-dr-meter-108-x64-parallel-dispatch-20260802.md)：
  固定 1.0.8 x64 的 fork-join 并行调度器 `0xdf30`、线程体 `0xdaf0` 与线程相关
  导入的调用方；登记它们与 analyzer 三入口的固定调用图可达关系，并据此解释
  隔离 core 单线程路径与完整 foobar 宿主路径在公开字段上的逐位一致；既有宿主
  observation 未记录调度分支，因此不把它表述为已观测的多线程运行。不建立工作
  分割粒度、实际线程数或任何性能声明。
- [`SA-foo-dr-meter-108-x64-report-renderer-20260718`](sa-foo-dr-meter-108-x64-report-renderer-20260718.md)：
  固定 1.0.8 x64 duration 舍入与 minute/hour/day/week 格式、channel label
  mapper，以及插件 renderer 与宿主 footer metadata 的边界；后续固定数值
  边界 observation 已对 duration 叶子的半秒和四类 token 分支形成 E2 动态
  交叉，但没有执行完整 renderer。
- [`SA-foo-dr-meter-108-x64-duration-leaf-20260719`](sa-foo-dr-meter-108-x64-duration-leaf-20260719.md)：
  固定 renderer 所调用的 `0x180038540` duration/timespan 数值叶子 ABI、
  `llround`/`free` IAT、输出对象与安全 direct-call 清理边界。
- [`SA-foo-dr-meter-108-x64-dynamic-probe-plan-20260718`](sa-foo-dr-meter-108-x64-dynamic-probe-plan-20260718.md)：
  固定 1.0.8 x64 analyzer/session/channel/result 布局，以及可按 ASLR module
  base 加 RVA 执行的 core、album writer 与 renderer 动态探针计划。该 CDB/IDA
  方案保留为归档的外围专项工具；当前 M1 core 已由隔离 worker 直接执行。
- [`SA-foo-dr-meter-108-x86-cross-arch-20260718`](sa-foo-dr-meter-108-x86-cross-arch-20260718.md)：
  固定 1.0.8 x86 核心、x86/x64 精度差异、album 与报告数据流。

固定 x64 safe-master observation 已动态区分 binary64 架构精度，并与核心静态
路径形成 E2 交叉证据。随后 accepted
[`isolated-core observation`](../observations/obs-foo-dr-meter-108-x64-isolated-core-safe-master-run1-20260719/record.md)
又直接保存 session/channel/result raw state。具体规则是否达到 E3 仍须按实际
捕获字段逐项判定，不能把未执行的 album/renderer probe 一并升级。

固定
[`numeric-boundaries observation`](../observations/obs-foo-dr-meter-108-x64-numeric-boundaries-v1-run1-20260719/record.md)
随后以同一 target 直接执行 duration 叶子与 analyzer core，得到 duration
24/24、multichannel weighting track bits 8/8、channel 前提 8/8、pair
invariants 4/4 和 histogram clamp 6/6。它把实际覆盖的半秒/进位 token、
`C > 2` weighting 分支和 `[-100, 0] dB` histogram 端点提升为 E2；不覆盖
foobar、album length weighting、完整 renderer、无效输入或任意资源极限。
