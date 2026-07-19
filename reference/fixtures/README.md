# Reference fixtures

本目录尚未直接提交生成的 WAV fixture。优先提交可由短小参数精确生成的
PCM/WAV 输入；大型、私有或版权状态不清的音频只记录生成方式、属性和 SHA-256，
不直接提交。

`foo_dr_meter` 1.0.8 第一批判别输入由
[`../tools/generate_foo_dr_meter_108_suite.py`](../tools/generate_foo_dr_meter_108_suite.py)
生成；实验定义见
[`../experiments/foo-dr-meter-108-discriminating-v1.md`](../experiments/foo-dr-meter-108-discriminating-v1.md)。
首次观测实际使用的清单保存在
[`foo-dr-meter-108-discriminating-v1.manifest.json`](foo-dr-meter-108-discriminating-v1.manifest.json)；
生成的 WAV 与观测都不应被称为 golden。

后续完整 v2 corpus 由
[`../tools/generate_foo_dr_meter_108_complete_v2.py`](../tools/generate_foo_dr_meter_108_complete_v2.py)
一次生成并自校验；实验定义见
[`../experiments/foo-dr-meter-108-complete-v2.md`](../experiments/foo-dr-meter-108-complete-v2.md)。
它保留 v1 的 15 个 WAV 为 byte-identical 输入，并新增 27 项。生成目录中的
`manifest.json` 的受控快照保存在
[`foo-dr-meter-108-complete-v2.manifest.json`](foo-dr-meter-108-complete-v2.manifest.json)，
只记录输入事实；`model-predictions.json` 明确不是 observation 或 golden。v2
大型 WAV 同样不直接提交仓库。固定 x64 safe-master observation 已关联这份
manifest；参考结果仍只存在于 observation，而不写回 fixture。

每个 fixture 应记录：

- fixture ID；
- 生成器版本、命令、参数和随机种子；
- sample format、采样率、声道布局和帧数；
- 波形/幅度/分段的精确定义；
- 文件 SHA-256；
- 许可或可再生成性说明。

Fixture 本身不是 golden。只有关联到固定 target 的 observation 才能提供参考结果。

产品 codec 回归 corpus 位于
[`../../tests/fixtures/native-pcm-v1`](../../tests/fixtures/native-pcm-v1/README.md)。
它验证 MacinMeter 自身的 WAV/AIFF/FLAC 解码契约，不属于本 reference 证据目录，
也不是插件 observation 或 conformance golden。
