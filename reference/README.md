# Reference research workspace

本目录保存 MacinMeter 对参考 DR 实现进行可重复研究所需的公开材料。这里的
文件用于建立证据链，不表示项目已经与参考插件兼容。

## 目录

| 目录 | 内容 |
| --- | --- |
| [`targets/`](targets/README.md) | 参考二进制、宿主、平台、配置和哈希身份 |
| [`experiments/`](experiments/README.md) | 可重复实验定义和输入生成参数 |
| [`observations/`](observations/README.md) | 参考目标的原始输出和运行环境 |
| [`static-analysis/`](static-analysis/README.md) | 固定二进制的受控静态分析证据 |
| [`fixtures/`](fixtures/README.md) | 可公开或可重复生成的实验输入 |
| [`specs/`](specs/README.md) | 带版本和证据等级的算法规格 |
| [`conformance/`](conformance/README.md) | 参考观测与实现结果的差分摘要 |

当前生产候选规格是
[`specs/foo-dr-meter-1.0.8-candidate-v1.md`](specs/foo-dr-meter-1.0.8-candidate-v1.md)。
[`specs/provisional-v1.md`](specs/provisional-v1.md) 只保留为 M0 历史工程基线；
两者都不是 accepted 兼容声明。

当前一次性完整输入协议是
[`experiments/foo-dr-meter-108-complete-v2.md`](experiments/foo-dr-meter-108-complete-v2.md)；
它用于承载静态规则回归和剩余 host-edge 观察，不取代既有 v1 observation。固定
x64 runtime 对其中 39 项 safe master 的首次导出已登记为
[`OBS-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718`](observations/obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/observation.json)。
MacinMeter 改用 finite interleaved f64 主链后，对同一 observation 的公开可比
核心字段达到整数 track DR 39/39、每声道两位 DR token 62/62；完整差分边界见
[`CONF-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-20260718`](conformance/conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-20260718/record.md)。
这份有限 conformance 记录不会把 candidate 升级为 accepted，也不改变
`Unverified` 状态。

## 五类事实

所有新增数据必须标记为以下一种：

1. **工程不变量**：内存安全、终态、chunk 不变性等不依赖参考插件的契约；
2. **临时实现契约**：为保证 0.2.0 可复现而冻结、但不声称来自参考证据的选择；
3. **参考观测**：固定目标对固定输入产生的原始输出；
4. **算法规格**：由观测、静态分析或动态跟踪支持的行为说明；
5. **Legacy snapshot**：0.1.x 当前实现的输出，仅用于观察迁移差异。

Legacy snapshot 不得用作 correctness golden，当前实现的输出也不得被写回
`observations/` 冒充参考结果。

## 证据等级

| 等级 | 含义 |
| --- | --- |
| E3 | 黑盒实验、静态分析和动态跟踪相互印证 |
| E2 | 至少两类独立证据相互印证 |
| E1 | 单类证据支持，尚缺交叉验证 |
| H | 高置信假设，仍有可区分的替代解释 |
| U | 未知或证据冲突 |

每条关于参考行为的规格结论必须引用 observation/experiment 标识并注明等级。
纯工程不变量和明确标成“临时实现契约”的 M0 选择不伪造证据等级；它们是否符合
参考实现仍标为 U。关键参考行为在进入稳定 profile 前原则上不能停留在 H 或 U。

## 提交规则

- 记录目标版本、宿主版本、平台、配置、时区和二进制 SHA-256；
- 实验输入优先由文本参数或生成器确定，避免提交来源不明的大型媒体；
- 原始 observation 一旦用于规格应保持不可变，修正通过新记录完成；
- conformance 摘要必须同时指向 reference observation 和被测实现版本；
- 不提交私人授权原文、受限制二进制、账号信息、绝对个人路径或机器秘密；
- 大文件和不可公开 fixture 只提交生成说明、哈希和安全存放位置。

授权与公开边界见 [`docs/LEGAL_CN.md`](../docs/LEGAL_CN.md)。该法律文档和本目录
中的工程记录都不替代专业法律意见。
