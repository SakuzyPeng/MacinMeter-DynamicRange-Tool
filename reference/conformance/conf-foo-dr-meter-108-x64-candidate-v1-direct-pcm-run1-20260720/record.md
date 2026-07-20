# CONF-foo-dr-meter-108-x64-candidate-v1-direct-pcm-run1-20260720

- 状态：Accepted
- 日期：2026-07-20
- 事实类别：decoder-independent reference-to-implementation numeric
  conformance
- 范围决策：
  [ADR-0005](../../../docs/adr/0005-m4-bounded-x64-numeric-claim.md)
- 规格：
  [`FooDrMeter108CandidateV1`](../../specs/foo-dr-meter-1.0.8-candidate-v1.md)
- 实现身份：commit
  `76d0f2eab5cdfce9de6a9d76ab971c333eab8e71`
- 生产声明：`FooDrMeter108CandidateV1 / Unverified`

## 目的

本记录回答一个窄问题：

> 当前 MacinMeter Candidate 在与固定 x64 analyzer core 相同的有限、交错
> binary64 PCM 上，是否产生相同的公开最终数值字段？

它不经过产品 decoder，也不要求 MacinMeter 与参考 DLL 使用相同的 histogram、
session layout 或其他内部状态。这样可以把算法 conformance 与
`WAVE_FORMAT_EXTENSIBLE` 是否属于稳定产品格式矩阵彻底分开。

## 固定身份

| 对象 | SHA-256 / 身份 |
| --- | --- |
| `foo_dr_meter.dll` 1.0.8 x64 | `ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489`；424448 bytes |
| runtime profile | `fixed_foobar_2_25_10` |
| complete-v2 manifest | `479e535a7196487fdb67a54f0c4de681f925920453e8092bac9eeb04eec4bbf8` |
| corpus generator | `f83fdcd0b88f2f414c53f8aa52a5b03f4fd4c8ee25024c4dce603df9a2179054` |
| isolated x64 core suite | `a511b9f46d6624d957bcd8afc7ff4e36525a06fd4772c35f7708ae4379e19d93` |
| normalized x64 report | `50205960b9850addb7f18bdb5f3c2c3c59897a5a2c5efc8e408870d5a3a2ffce` |
| Candidate worker source | `b93bd7bcbb6242c6386aace628cd90b97979956de76160b50e9b32a29569cf3d` |
| suite runner | `185779e210d0d6993bdc9420ec8c221cbe102a34c29b15007f4150ab3ac899c4` |
| final-field comparator | `07d9e3c41700d3b9642a228799a0a87105869c7229780c7871c95a198474d5ed` |
| release worker | `ae42263881d6a76f6bfc675fb9e52e1141a03a87dd0d91363616e14e9c4b669d`；424336 bytes |

worker 从上述 clean commit 以 workspace release profile 重建，不作为二进制提交。
suite 保存 worker hash、byte length、源码 commit、manifest、逐项 source/PCM
hash、block size 和 request identity。

## 执行边界

- manifest 的 39 项 safe 输入按固定顺序执行；
- reference-side adapter 验证容器、文件 hash、data hash、几何和 finite PCM，
  再按记录的 sample encoding 转成 interleaved `f64le`；
- 每项输入启动一个全新 Candidate worker 进程；
- worker 只构造 `StreamSpec` 并调用 `AnalyzerSession::new`、
  `push_interleaved`、`finish`；
- 分别以 4096 和 997 frames/block 执行完整 suite；
- comparator 只比较公开 binary32 bits 与固定 report token，容差为 0；
- product `DecoderFactory`、foobar decoder、Windows reference worker 和
  production/reference 中间状态均未执行或比较。

运行环境：

- macOS 27.0 arm64；
- `rustc 1.96.0 (ac68faa20 2026-05-25)`；
- `cargo 1.96.0 (30a34c682 2026-05-25)`；
- release profile：`opt-level = 3`、thin LTO、单 codegen unit、
  overflow checks enabled。

## 结果

两次执行的结果相同：

| 字段 | 4096 frames/block | 997 frames/block |
| --- | ---: | ---: |
| 成功输入 | 39/39 | 39/39 |
| track DR raw bits | 39/39 | 39/39 |
| channel DR raw bits | 62/62 | 62/62 |
| channel overall RMS raw bits | 62/62 | 62/62 |
| channel primary peak raw bits | 62/62 | 62/62 |
| track DR token | 39/39 | 39/39 |
| channel DR token | 62/62 | 62/62 |
| overall peak token | 39/39 | 39/39 |
| overall RMS token | 39/39 | 39/39 |
| channel RMS token | 62/62 | 62/62 |
| duration token | 39/39 | 39/39 |
| 差分 | 0 | 0 |

两份 suite 的 `{inputId, coreBits, analysis}` canonical projection SHA-256
都为
`afee42eebfde4646a7bc2c60cda9070b97a709a1c8d3e468b1baade365977969`。
因此纳入 conformance 的字段在两次运行中都等于固定 reference；Candidate 自身的
完整公开 result projection 也跨两种分块精确相等。

产物：

- [4096-frame suite](suite-block-4096.json)：
  `93bfea94098035853b8630231d8e6c833a192cc2455093860f5dcb174ba7bec4`
- [4096-frame comparison](comparison-block-4096.json)：
  `cb2f6ea43f4c46d7cb6164f6124e720192c144012a1cecec0d4535dbc8b395fd`
- [997-frame suite](suite-block-997.json)：
  `1506b76b61452111fdaced4c2075eb6919d64bf52a06e2a3ed18742ac740af6c`
- [997-frame comparison](comparison-block-997.json)：
  `822ec149d28369c856ef4a01f9656ac8e9383746dc4feab8e177c23bb8356c1e`

## 重建命令

先从固定 commit 建立 clean worktree，再运行：

```bash
cargo build --locked --release \
  -p macinmeter-analysis \
  --example candidate_v1_conformance_worker

python3 reference/tools/generate_foo_dr_meter_108_complete_v2.py \
  --output <generated-corpus-root>

python3 reference/tools/run_macinmeter_candidate_v1_suite.py \
  --manifest reference/fixtures/foo-dr-meter-108-complete-v2.manifest.json \
  --corpus-root <generated-corpus-root> \
  --worker target/release/examples/candidate_v1_conformance_worker \
  --worker-sha256 ae42263881d6a76f6bfc675fb9e52e1141a03a87dd0d91363616e14e9c4b669d \
  --source-commit 76d0f2eab5cdfce9de6a9d76ab971c333eab8e71 \
  --block-frames 4096 \
  --output <suite-output>

python3 reference/tools/compare_macinmeter_candidate_v1_suite.py \
  --candidate-suite <suite-output> \
  --reference-core-suite \
    reference/observations/obs-foo-dr-meter-108-x64-isolated-core-safe-master-run1-20260719/suite.json \
  --normalized-report \
    reference/observations/obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/normalized/safe-master.json \
  --output <comparison-output>
```

第二次将 `--block-frames` 改为 `997`。两次 runner 和 comparator 均返回 0。

## 结论与限制

在固定 target、固定 finite binary64 PCM、固定 39 项 corpus 和上述公开字段范围
内，当前 Candidate 没有已知 residual，也没有 chunk-size residual。

本记录不声明：

- foobar 或产品 decoder parity；
- component lifecycle、metadata、playlist、album grouping 或 footer 来源；
- optional multichannel weighting 已进入生产 profile；
- channel label、locale、模板或完整文本 parity；
- x86、1.0.3、任意 PCM、无效输入或任意 CRT/libm 最后一位；
- `accepted`、`verified` 或一般意义上的插件兼容。

因此产品身份保持 `FooDrMeter108CandidateV1 / Unverified`。
