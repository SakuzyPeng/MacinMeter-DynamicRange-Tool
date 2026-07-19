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
- [`SA-foo-dr-meter-108-x64-report-renderer-20260718`](sa-foo-dr-meter-108-x64-report-renderer-20260718.md)：
  固定 1.0.8 x64 duration 舍入与 minute/hour/day/week 格式、channel label
  mapper，以及插件 renderer 与宿主 footer metadata 的边界。
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
