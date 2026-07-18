# EXP-foo-dr-meter-108-complete-v2

## 状态与目的

- 状态：fixed-design
- 实验标识：`foo-dr-meter-108-complete-v2`
- 固定输入数：42 个 WAV
- safe master：39 个 WAV，一次导出
- isolated：3 个 WAV，各自单独运行
- 生成器 SHA-256：
  `f83fdcd0b88f2f414c53f8aa52a5b03f4fd4c8ee25024c4dce603df9a2179054`
- canonical manifest SHA-256：
  `479e535a7196487fdb67a54f0c4de681f925920453e8092bac9eeb04eec4bbf8`
- `FILES.sha256` 文件 SHA-256：
  `b5692b3165d82d7d189c38573a48fd0b5dd24750e82aa706d6c9eb45ce5d7595`

本实验一次性生成当前研究阶段所需的完整 WAV corpus。它同时承担两种不同用途：

1. 把已经由固定 DLL 静态分析确定的规则变成可重复的实现回归输入；
2. 集中观察静态分析无法从插件 DLL 本身回答的 foobar2000 host/decoder 边界。

第二项是本轮插件动态运行的唯一主要研究目的。已经静态确定的窗口、histogram、
peak、album 和报告内部数据流不要求操作者为了“再确认”逐项或重复导出。

## 与 v1 的关系

[`EXP-foo-dr-meter-108-discriminating-v1`](foo-dr-meter-108-discriminating-v1.md)
及其既有 observation 保持不变。v2：

- 不修改、重写或替换 v1 的 manifest、原始报告、哈希或 observation；
- 不把 v1 原始输出复制成 v2 observation；
- 可以重新生成语义相近的回归输入，但使用独立 experiment/corpus ID；
- 不把 v1 的 15/15 结果外推成 v2 golden。

v1 继续作为第一次固定 x86 黑盒观测的不可变证据；v2 是后续完整 corpus 和
host/decoder 研究协议。

## 2026-07-18 x64 safe-master 结果

固定
[`TARGET-foo-dr-meter-1.0.8-foobar2000-2.25.10-x64-win10-19045`](../targets/foo-dr-meter-1.0.8-foobar2000-2.25.10-x64.md)
已按本协议完成一次 39 项 safe-master 导出。原始报告、运行身份与规范化结果登记
在
[`OBS-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718`](../observations/obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/observation.json)：

- 39 个 fixture 各出现一次，顺序与 manifest 完全一致；
- 39 个 track DR 与 62 个每声道 DR token 均完整解析；
- `410_rms_half_f64_stereo` 和 `420_peak_half_f64_stereo` 动态区分了 x64
  binary64 PCM 路径，和固定 DLL 的静态精度结论形成 E2 交叉证据；
- 三个 isolated 输入没有在本 observation 中运行，重复一致性也未评估；
- footer 中两个 `32761` 位深 token 按原文保留，并已分类为不参与核心 DR 的
  host/plugin metadata-report 外围异常；精确根因仍未知。

构造模型与 observation 的独立逐字段比较保存在
[`foo-dr-meter-108-complete-v2-model-observation-comparison.json`](foo-dr-meter-108-complete-v2-model-observation-comparison.json)
（比较器：
[`compare_foo_dr_meter_model_to_observation.py`](../tools/compare_foo_dr_meter_model_to_observation.py)）。
两者 SHA-256 分别为
`91836cd709b2344f274cfbde74dd1967cc173131c2247e6d62556bc33e660718`
和 `caf555824cd31e3855096ac34422db909c637b0341e06a5372445c53f968de5f`。
结果为 track DR 39/39、channel DR 62/62、overall peak 39/39、overall RMS
39/39、channel RMS 62/62，差分数为 0。这里的 model 仍只是从已恢复规则构造的
诊断预测，不是 reference golden；事实源始终是固定 target 的原始 observation。

MacinMeter 的 reference-to-implementation 差分另见
[`CONF-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-20260718`](../conformance/conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-20260718/record.md)。
产品 PCM 主链改为 finite interleaved f64 后，公开可比较核心字段达到 track DR
39/39、channel DR 62/62。该有限结果不覆盖参考报告 overall peak/RMS、不可见
中间状态、isolated 输入或更广输入空间，因此 profile 继续保持
`FooDrMeter108CandidateV1 / Unverified`。

## Corpus 布局

生成器必须产生恰好 42 个 WAV：

| 角色 | 数量 | 执行方式 |
| --- | ---: | --- |
| safe master | 39 | 按 `playlists/00-safe-master.m3u8` 一次选中并导出一份报告 |
| `zero-frame` | 1 | isolated，单独运行并记录是否产生报告或外围错误 |
| `overfull-f32` | 1 | isolated，单独运行 |
| `overfull-f64` | 1 | isolated，单独运行 |

三个 isolated 输入不得进入 safe master。这样，零帧源或宿主对超满幅 float 的
特殊处理不会使 39 个普通输入的整批报告失效，也不会把外围失败误记成普通
track 结果。

逻辑分组固定为：

- `01-core`
- `02-degenerate`
- `03-multichannel`
- `04-numeric`
- `05-samplerates`
- `06-precision-report`
- `07-album`
- `08-host-decode`
- `99-isolated`

fixture 的完整 ID、文件名、PCM 格式、构造参数和所属分组以生成器写出的
`manifest.json` 为准；本文不预写尚未生成的文件哈希或参考 DR。

## 生成与验证

生成：

```bash
python3 reference/tools/generate_foo_dr_meter_108_complete_v2.py \
  --output /tmp/foo-dr-meter-108-complete-v2
```

重新验证已有目录：

```bash
python3 reference/tools/generate_foo_dr_meter_108_complete_v2.py \
  --verify /tmp/foo-dr-meter-108-complete-v2
```

输出契约：

- `manifest.json`
- `model-predictions.json`（构造模型诊断，不是 reference output）
- `generation-provenance.json`
- `FILES.sha256`
- `HOW_TO_EXPORT.txt`
- `RUN_METADATA_TEMPLATE.txt`
- `VERIFY.ps1`
- `playlists/00-safe-master.m3u8`
- 各逻辑分组对应的 M3U8
- 42 个确定性生成的 WAV

`--verify` 必须从文件内容重新检查数量、manifest、WAV 结构、playlist 成员和
哈希，不能只相信已有 `FILES.sha256`。fixture 缺失、多余、顺序变化、内容变化
或 isolated 混入 safe master 都必须失败。

Manifest 可以记录实际生成 PCM 的统计量、参数和内容哈希，但不得包含尚未观察到
的插件预期 DR。构造参数中的目标值也不是 reference golden。候选模型数值与
生成时判别断言只能进入独立的 `model-predictions.json`，且不得复制进
`observations/` 冒充参考结果。

## Safe master 执行

目标插件配置与 v1 保持一致：

- Automatically save tags：off
- Add per-channel stats also for stereo album logs：on
- Weight album DR by track lengths：off
- Weight multichannel DR by channel loudness：off

对一个固定 target/架构：

1. 先运行 `--verify`；
2. 只打开 `playlists/00-safe-master.m3u8`；
3. 确认其中恰好 39 项且顺序与 manifest 一致；
4. 一次执行测量并导出一份未经清洗的原始报告；
5. 不为了静态已解规则重复运行，也不拆成逐项人工采集。

三个 album focused playlist 只用于可选的静态回归诊断，不是本轮常规采集义务。

固定 x64 target 的首次 safe-master 运行已按上节登记。未来其他架构或 host
版本仍须建立独立 observation，不能与该 x64 报告合并成共同结果。

## Isolated 执行

三个 isolated 输入分别单独启动测量，不组成 album，不与 safe master 共选：

- `zero-frame`：观察 host 在第一次 decode 没有 audio chunk 时的最终外围行为；
- `overfull-f32`：观察 host 对有限、超出标称满幅的 float32 PCM 如何解码和传递；
- `overfull-f64`：观察 host 是否保留、窄化或拒绝 float64 PCM 的超满幅值。

它们分别位于 `99-zero-frame.m3u8`、`99-overfull-f32.m3u8` 和
`99-overfull-f64.m3u8`，每个 playlist 只含一项。每项只运行一次。若没有普通
报告，记录原始错误、无输出或 UI 行为，不伪造 track DR，也不把失败替换成静音
结果。

## 静态已解规则仍保留为回归输入

以下规则已经由
[`x64 静态记录`](../static-analysis/sa-foo-dr-meter-108-x64-20260718.md)
和
[`x86/cross-arch 静态记录`](../static-analysis/sa-foo-dr-meter-108-x86-cross-arch-20260718.md)
确定，不需要人工重复采集：

- block/window、EOF tail 和精确整窗；
- 多采样率窗长、RMS clamp、histogram 与 loud boundary bin；
- peak 排名、secondary 选择和 negative fallback；
- 静音、LFE、内部 channel DR 到 track DR 的聚合；
- album official、length weighting、公开窄化和整数显示；
- report peak/RMS 与接近零格式化。

这些输入仍进入 v2，是为了：

- 验证生成器持续覆盖已知边界；
- 给 MacinMeter 建立确定性的 implementation regression；
- 防止后续重构把静态规则无意改回旧算法；
- 在一次 safe-master 报告中发现明显的端到端异常。

它们的存在不把静态事实变成新的黑盒实验义务。

## Architecture discriminators

x86 与 x64 必须分别解释：

- x86 插件消费 binary32 PCM，sample square、current peak、peak `log10f` 和已保存
  peak key 含 binary32 运算或存储；
- x64 插件消费 binary64 PCM，并保留 binary64 sample-square/peak/key 路径；
- 当前 MacinMeter 已把有效 PCM 主链改为 binary64，以 binary64 完成
  sample-square/peak/DR 路径，但 peak key 保存为整数 centi-dB，公开结果再窄化
  为 binary32；它与本轮可观察 token 一致，仍不是 fixed target 的逐位兼容声明。

因此 precision fixture 是 architecture discriminator，不设置 x86/x64 共同
golden。每个 observation 必须写明 target、架构和二进制 SHA-256；一个架构的
结果不能填入另一个架构的预期列。有限 corpus 也不得用于宣称所有浮点边界相同。

## Host/decoder 证据边界

固定 foobar2000 2.0 x64 `foo_input_std` 的进一步静态分析已经收口普通 WAV
到 `audio_chunk<double>` 的六条转换路径：

| WAV PCM | x64 decoder 输出 |
| --- | --- |
| U8 | `(u8 - 128) / 128` |
| S16 | `s16 / 32768` |
| packed S24 | `sign_extend_24(raw) / 8388608` |
| S32 | `s32 / 2147483648` |
| F32 | binary32 数值精确扩宽为 binary64 |
| F64 | binary64 sample 原样复制 |

证据、函数地址和 WAVEFORMATEXTENSIBLE `validBitsPerSample` 边界见
[`SA-foobar2000-2.0-x64-wave-decoder-20260718`](../static-analysis/sa-foobar2000-2.0-x64-wave-decoder-20260718.md)。

因此 safe corpus 中的普通 integer/float WAV 只承担端到端回归：确认固定输入经过
实际 host、decoder、chunk 与插件后没有外围异常。它们不再承担猜测上述除数、
符号解释或 float 扩宽公式的职责，也不得把实现侧预测写成 reference golden。

仍需从动态运行或其他固定 target 单独回答的边界包括：

- x86 standard input component 是否在所有边界上采用与该 x64 target 相同的转换；
- float64 在 x86 host/plugin API 边界发生的精度变化；
- x64 decoder 已不 clamp 的有限超满幅 float 在下游 host/plugin 中是保留、
  clamp 还是拒绝；
- 零帧源从 decode failure 到最终 UI/报告的映射；
- 宿主提供的 channel mask、bitrate 和 codec metadata 是否符合静态 renderer
  假定的输入。

若这些行为可通过对应固定宿主或组件继续静态确定，应登记新的静态证据，不覆盖
本 x64 decoder 记录，也不追加无区分力的人工采集。

## 明确排除的 GUI 输入

NaN、Inf 和损坏文件不属于这 42 个 WAV，也不得送入参考插件 GUI：

- NaN/Inf 不属于候选的有限 PCM 契约，并可能触发未定义或运行库特定的数学转换；
- 损坏容器属于 decoder/error-contract 测试，不应和合法 PCM 算法 corpus 混合；
- MacinMeter 对非有限样本和损坏输入的严格失败应由本地 codec/API 测试覆盖。

若未来研究这些外围行为，应建立独立 experiment，并明确风险、目标和隔离步骤。

## Observation 入口条件

收回结果后必须记录：

- Windows、foobar2000、插件版本与架构，以及对应 SHA-256；
- v2 `manifest.json` 与原始报告 SHA-256；
- `--verify` 结果和四个插件配置值；
- safe master 是否恰好包含 39 项且各出现一次；
- 三个 isolated 项各自的原始结果或失败表现；
- 本地时间、时区、操作者步骤和执行次数。

原始报告不得做换行、locale、时间戳或错误文本清洗。规范化比较必须另存，并引用
原始文件哈希。没有运行结果前，不建立 reference golden，也不填写预期 DR。
