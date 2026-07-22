# CONF-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-report-v3-clean-20260718

## 结论

- 事实类别：reference-to-implementation report-metrics conformance
- 状态：`exported_report_metrics_match`
- 参考规格：`foo-dr-meter-1.0.8-candidate-v1`
- 参考观测：
  [`OBS-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718`](../../observations/obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/observation.json)
- 被测实现提交：`7112f8939a00b170e0ede364417958722ff61690`
- 实现 profile：`FooDrMeter108CandidateV1`

本记录是
[`首份 schema-v3 记录`](../conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-report-v3-20260718/record.md)
的 clean-commit successor。旧记录固定了 dirty-worktree 二进制，本记录则从上述
提交的 detached、无修改 worktree 重建 release CLI；旧记录和产物不被回写。

对同一批 39 个 manifest-ordered safe-master 输入，固定 reference observation 与
实现 WireEnvelope 得到：

| 字段 | 精确匹配 |
| --- | ---: |
| 整数 track DR | 39/39 |
| 每声道两位 DR token | 62/62 |
| overall primary peak token | 39/39 |
| overall RMS token | 39/39 |
| 每声道 overall RMS token | 62/62 |
| duration token | 39/39 |
| footer 可比较子集 | 4/4 |

逐 track/channel 比较的数值容差为 0，差分数为 0；fixture 集合与实现输出顺序
完全一致。有限结果不会把 candidate 规格升级为 accepted，也不会把兼容性改为
verified。

## 固定身份

### Reference

| 对象 | SHA-256 |
| --- | --- |
| 原始 x64 报告 | `e9afbde86ccb21cae56826803da5492e37135c8594a657130b3868b42956d11c` |
| 规范化报告 | `50205960b9850addb7f18bdb5f3c2c3c59897a5a2c5efc8e408870d5a3a2ffce` |
| complete-v2 manifest | `479e535a7196487fdb67a54f0c4de681f925920453e8092bac9eeb04eec4bbf8` |

参考运行身份、foobar2000 2.25.10 x64、插件 1.0.8 x64 及 Windows 环境由上述
observation 和 target 记录固定。本记录复用一次已登记的 reference 导出，没有把
实现侧重复运行冒充 reference runtime repeatability。

### MacinMeter

| 对象 | 身份 |
| --- | --- |
| source commit | `7112f8939a00b170e0ede364417958722ff61690` |
| build worktree | detached、clean |
| build command | `cargo build --locked --release -p macinmeter-cli` |
| build host | macOS 27.0 arm64 |
| Rust / Cargo | 1.96.0 / 1.96.0 |
| Wire schema / tool version | 3 / 0.2.0 |

| 对象 | SHA-256 |
| --- | --- |
| release CLI binary | `3a92d3671f6af2e4579d897d701929681c5d240f3643439f924fb59f7986c88e` |
| schema-v3 WireEnvelope | `4587f73881403b099cdfb41e7516ebfa62a4f776754e8e17ad1cf92d5705ad68` |
| comparison | `e2c6478f19fb9b3094bf056215c7472bb38eea585f6c9affe2ba1269a458dab0` |
| comparator | `60a4350938784df63b76b8df532a75b03244a839d42015800c4fc7d48869d2e1` |

实现输出保存在
[`implementation/schema3-wire.json`](implementation/schema3-wire.json)，规范化
差分保存在 [`comparison.json`](comparison.json)。保存的实现输出与随后一次独立
重跑逐 byte 相同；两次 CLI 均退出 0、stdout 为 0 bytes，重跑 stderr 为 6077
bytes。这个检查只说明同一实现、同一输入和同一本地环境的输出重复性。

## Duration

固定 x64 renderer 的受控静态记录见
[`SA-foo-dr-meter-108-x64-report-renderer-20260718`](../../static-analysis/sa-foo-dr-meter-108-x64-report-renderer-20260718.md)。
comparator 按该路径从结构化实现结果生成 token：

```text
seconds = f64(decoded_frames) / f64(actual_pcm_sample_rate)
whole_seconds = llround(seconds)
token = fixed minute/hour/day/week formatter(whole_seconds)
```

39 个 reference `durationToken` 均与实现生成 token 精确相同。现有 observation
全部只执行短时长 `m:ss` 分支，因此该分支由静态路径与黑盒导出共同达到 E2；
半秒舍入边界以及 hour/day/week 分支仍只有静态 E1，不能因 comparator 单元测试
覆盖了这些分支就冒充 reference 黑盒证据。

## Footer 与 DR0 反事实

comparison 只把语义可直接对应的 footer 子集与 39 份成功的实现 track report
比较：

| 字段 | Reference | Implementation | 结果 |
| --- | --- | --- | --- |
| track count | 39 | 39 | match |
| sample-rate set | 8000, 44100, 48000 | 8000, 44100, 48000 | match |
| channel-count set | 1, 2, 3, 6, 8 | 1, 2, 3, 6, 8 | match |
| unweighted DR token | DR12 | DR12 | match |

最后一项是 comparison-only reconstruction：按 manifest 顺序读取实现公开的
binary32 track DR，以 binary64 顺序求和与均值、窄化到 binary32，再对非负值
执行加 0.5 后截断。它没有调用产品 `AlbumAggregator`，也不把 MacinMeter 值命名
为 “Official”。

39 个公开 track DR 中有 3 个数值 DR0。纳入全部 39 个时重建 token 为 DR12，
与 reference footer 一致；仅排除这 3 个数值零后，36 个剩余 track 的反事实
token 为 DR13，与 reference 不一致。固定 album 静态数据流与这个可区分黑盒结果
共同支持“reference 没有统一排除数值 DR0 track”这一窄规则达到 E2。

该结果不能区分所有能产生 DR12 的内部聚合算法，不能验证精确内部均值、binary32
窄化时点、album grouping、length weighting 或其他 playlist。完整 album 公式
继续只有静态 E1。

## 命令

```bash
git worktree add --detach <clean-worktree> \
  7112f8939a00b170e0ede364417958722ff61690
cargo build --locked --release -p macinmeter-cli

<clean-worktree>/target/release/macinmeter batch \
  <39 manifest-ordered paths> \
  --format json \
  --output <schema3-wire.json>

python3 reference/tools/compare_macinmeter_report_metrics_to_foo_dr_meter.py \
  --reference <observation>/normalized/safe-master.json \
  --implementation-output <schema3-wire.json> \
  --implementation-binary <clean-worktree>/target/release/macinmeter \
  --output <comparison.json>
```

路径占位符不进入身份；实际二进制、WireEnvelope、reference normalization、
manifest、comparator 与 comparison 均由上节 SHA-256 固定。

## 未比较与出口条件

- histogram、window count、loud bin、peak key/ranking/fallback 和内部 binary64
  DR 等不可导出的中间状态；
- footer 的 bit depth、bitrate、codec 等宿主 metadata；
- 完整 channel-label/layout 文本和 byte-for-byte 报告格式；
- album-focused playlist、精确内部 album mean、grouping 与 length weighting；
- 三个 isolated host-edge 输入和更广音频空间；
- 第二次独立 x64 reference runtime 导出。

下一阶段最有信息量的工作是对固定 x64 目标动态记录窗口数、loud-bin 选择、
primary/secondary peak 排名与回退，以及 album writer 中间值。若 accepted policy
最终把 reference runtime repeatability 设为硬条件，还需对同一 target、同一
safe-master 采集独立 run2；静态逆向和实现重复运行都不能替代它。

因此本记录只证明固定 corpus 上已列出的公开字段及受限 footer 子集一致，不声称
任意输入、完整报告或端到端 reference parity。
