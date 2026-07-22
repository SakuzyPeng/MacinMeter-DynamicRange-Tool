# CONF-foo-dr-meter-108-x64-isolated-core-safe-master-report-run1-20260719

## 结论

- 事实类别：isolated-core-to-exported-report compared-field conformance
- 状态：`compared_fields_match`
- 固定 target：`foo_dr_meter 1.0.8 x64`
- corpus：`foo-dr-meter-108-complete-v2` 的 39 项 safe master
- 兼容性：`none`
- foobar parity：`not_assessed`

本记录只连接两份已经固定身份的观测：

1. 不启动 foobar2000、直接执行固定 x64 analyzer core 的
   [`isolated-core suite`](../../observations/obs-foo-dr-meter-108-x64-isolated-core-safe-master-run1-20260719/suite.json)；
2. 在固定 foobar2000 2.25.10 x64 + foo_dr_meter 1.0.8 x64 环境导出的
   [`safe-master observation`](../../observations/obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/observation.json)
   及其
   [`normalized report`](../../observations/obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/normalized/safe-master.json)。

比较器严格要求双方使用同一 manifest 摘要、同一组 39 个 fixture ID、连续且精确
相同的 manifest 顺序、39 个成功 core item，以及逐项一致的声道几何。它还验证
suite 无绝对或私有路径，验证输入/结果帧数、采样率、声道数与 binary64 PCM byte
length 自洽，并拒绝任何非有限或负的可比较 binary32 metric。

固定 renderer 数据流重建后的精确结果为：

| 字段 | 精确匹配 |
| --- | ---: |
| 整数 track DR | 39/39 |
| 每声道两位 DR token | 62/62 |
| 每声道 RMS dBFS token | 62/62 |
| overall peak dBFS token | 39/39 |

差分数为 0。这里的 “match” 只表示上表四类字段在这 39 个输入上相同；它不表示
完整组件、foobar 宿主或完整报告 parity。

## 固定身份

| 对象 | SHA-256 |
| --- | --- |
| complete-v2 manifest | `479e535a7196487fdb67a54f0c4de681f925920453e8092bac9eeb04eec4bbf8` |
| x64 target DLL | `ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489` |
| isolated-core suite | `3cdb5132f7239ba1a500339e5138cb8d0713af952b9dfaff4ca206c112d34a61` |
| isolated-core worker | `0e09e6795a10f0d3e368ab5626cc2b0ab792edbc8bd9515baf3b12be6011b92f` |
| exported raw report | `e9afbde86ccb21cae56826803da5492e37135c8594a657130b3868b42956d11c` |
| normalized safe-master report | `50205960b9850addb7f18bdb5f3c2c3c59897a5a2c5efc8e408870d5a3a2ffce` |
| comparator | `ecee37f882866bdb612f9bf7d20c43e75a2339ea255c5118943f283f868ff9ca` |
| comparison | `93dd4b0219c74e99ab32a499e9ec401489c02071060c759e774a77fe3d0cce86` |

isolated suite 固定 worker 为 278016 bytes，并按 `one_worker_process_per_input`
执行。目标 DLL 为 424448 bytes。suite 中每项都登记相同的固定 runtime
artifacts，core 调用期的 13 个 `shared.dll` import 均由 fail-fast IAT tripwire
保护；这说明本次已执行 core 路径没有调用这些宿主边界，不把完整 component
loader 或宿主服务纳入结论。

## 比较数据流

[`compare_foo_dr_meter_core_suite_to_report.py`](../../tools/compare_foo_dr_meter_core_suite_to_report.py)
只从 suite 的公开 result bits 重建报告中可直接对应的 token：

```text
track DR:
    trunc(f32(trackDrBits) + 0.5)

channel DR:
    fixed_two_decimal(f32(channel.drBits))

channel RMS:
    linear = f32(channel.rmsBits)
    linear == 0 ? "-inf" :
        fixed_two_decimal(f32(20 * log10(f64(linear))))

overall peak:
    linear = max(f32(channel.peakBits))
    linear == 0 ? "-inf" :
        fixed_two_decimal(f32(20 * log10(f64(linear))))
```

两位 dB renderer 使用固定 `C` locale 语义，并保留目标 renderer 在
`-0.01 < value < 0.01` 区间的显式 centi-dB 修正。比较采用 token 精确相等，
数值容差为 0。产物是 path-free、key-sorted、finite canonical JSON：
[`comparison.json`](comparison.json)。

## 命令

```bash
python3 reference/tools/compare_foo_dr_meter_core_suite_to_report.py \
  --core-suite \
    reference/observations/obs-foo-dr-meter-108-x64-isolated-core-safe-master-run1-20260719/suite.json \
  --normalized-report \
    reference/observations/obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/normalized/safe-master.json \
  --output \
    reference/conformance/conf-foo-dr-meter-108-x64-isolated-core-safe-master-report-run1-20260719/comparison.json

python3 -m unittest \
  reference/tools/tests/test_compare_foo_dr_meter_core_suite_to_report.py
```

命令中的仓库相对路径不写入 comparison；comparison 只保存输入内容 SHA-256 和
path-free 固定身份。

## 明确不在范围

- foobar WAVE decoder、source-to-PCM 行为及不同 source encoding 的宿主解码；
- component 注册、初始化、析构、异常恢复及其他 lifecycle；
- foobar service、metadata、playlist、GUI 与宿主调度；
- album grouping、length weighting、footer official DR 及其他聚合；
- report overall RMS、duration、footer、bit depth、bitrate、codec、声道标签、
  文本布局、编码与 byte-for-byte 输出；
- 未列出的输入、第二次运行、其他 target 版本或其他架构。

特别是，suite 中的 WAVE 到有限交错 binary64 PCM 转换属于 harness，不是 foobar
decoder 观测；isolated worker 也没有调用报告 renderer。本记录借助已静态固定的
renderer 数据流比较四类最终可见字段，不能据此把 decoder、host、lifecycle、
album 或 renderer 的其余字段标记为已验证。
