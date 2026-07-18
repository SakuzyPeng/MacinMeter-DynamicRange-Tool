# Provisional DR specification v1

- 状态：provisional
- 建立日期：2026-07-18
- 适用里程碑：M0 / 0.2.0
- 参考目标候选：foobar2000 DR Meter 1.0.3
- 兼容性声明：未建立

## 1. 目的

本规格冻结 M0 可以依赖的工程边界，并显式列出尚未由参考证据确认的算法行为。
它不是对参考插件内部算法的完整描述，也不把 0.1.x 实现提升为 golden truth。

关键字 MUST、MUST NOT、SHOULD 和 MAY 用于约束 0.2.0 可信主干。

## 2. 输入契约

- 分析器 MUST 消费带有非零采样率、非零声道数和明确样本格式的 PCM block。
- PCM block MUST 包含完整 frame；交错数据长度必须是声道数的整数倍。
- `PcmStreamInfo.spec.sample_rate` MUST 表示实际送入分析器的 PCM 采样率。
- 源容器采样率、原始 DSD 率、预期帧、已解码帧和已消费帧 MUST 分开记录。
- 同步 PCM 源的读取结果只能是 `Data`、`Eof` 或 `Error`；暂时无数据不得伪装
  成 `Eof`。若采用异步模型，`Pending` MUST 是独立状态。
- `Eof` MUST sticky；终态后不得再次产生数据。
- 解码错误、完整性校验失败和预期/实际 frame 数不符 MUST 产生结构化错误。
- M0 MUST 只接受 WAV linear integer/IEEE float PCM、FLAC 和 AIFF integer
  PCM；AIFC、外部解码器、FFmpeg、DSD 和其他 codec MUST 返回
  `unsupported_format`。
- M0 MUST 按内容探测。扩展名只能用于目录发现，不能参与 codec 判定。
- M0 遇到坏包 MUST 严格失败，不得跳包后生成部分成功结果。

以上属于工程不变量，不推断参考插件如何解码压缩格式。

## 3. 分析会话契约

- 生产代码 MUST 只有一套逐 block 状态更新和一次性 `finish()` 路径。
- 合法 chunk 切分 MUST NOT 改变同一 PCM 的结果。
- mono、stereo 和多声道 MUST 使用相同逐声道状态语义。
- `finish()` MUST 只调用一次；重复调用应被类型或状态检查拒绝。
- 无数据、数据不足、静音和计算失败 MUST 是可区分结果，不能都折叠为 DR 0。
- M0 MUST 使用 safe scalar 路径，不包含 SIMD 或 `unsafe` 实现。

### 3.1 `ProvisionalV1` 的固定参数

以下规则冻结的是 0.2.0 工程基线，不是参考插件真值：

1. 窗长为
   `floor(sample_rate × 3.0040816326530613)` frames。
2. 所有声道在同一个逐 frame 循环累计；调用者不得分离声道或拼装窗口。
3. 单窗口 RMS 为 `sqrt(2 × sum_squares / frames)`。
4. 结尾不足整窗时，至少 2 frames 才提交；0 或 1 frame 尾部不提交。
5. RMS 进入 10001-bin 直方图：乘 10000、向零截断，并 clamp 到
   `0..=10000`。为了保证“只有实际全零信号才是 Silent”和 JSON 数值有限，
   正 RMS 的最小 bin 为 1。
6. 从 bin 还原最响窗口 RMS 时，先精确累计选中 bin 的平方整数和，再计算
   `sqrt(mean(bin²)) / 10000`。
7. 最响窗口数量为 `max(1, floor(window_count × 0.2))`。
8. 每声道在线维护最大和第二大窗口 peak；相等 peak 保留为两个次序统计值。
9. 输入恰好结束在整窗边界时，peak 序列额外加入一个显式虚拟零值。
10. 优先选择第二 peak；第二 peak 缺失或不大于零时回退第一 peak。报告中缺失的
    第二 peak 使用显式 `null`，虚拟零 peak 则保留为数值 `0.0`。
11. 可测声道的 DR 为 `-20 × log10(loud_rms / selected_peak)`。
12. `Silent` 只用于存在有效窗口且所有实际输入样本均精确为零的声道；
    没有可结算窗口，或非零信号只落在未提交尾部而无法形成有限测量时，使用
    `InsufficientData`。
13. 聚合只纳入 `Measured` 声道，并记录 `Silent`、`InsufficientData` 和
    `Lfe` 排除原因。即使没有可纳入声道，aggregate 对象也保留排除清单，并将
    `preciseDrDb` / `roundedDr` 显式设为 `null`。
14. 只有布局为 `known-no-lfe` 或带明确位置的 `known` 时才生成
    `without_lfe` 聚合；`unknown` 不猜测 LFE。
15. 每声道和 track 的整数 DR 都采用 half-away-from-zero 舍入。

## 4. M0 明确排除

- EdgeTrimmer MUST NOT 位于 M0 生产分析管线中；
- 单个 M0 batch 请求 MUST 串行处理文件，decoder MUST 不做包级并行；独立 API
  请求是否由宿主并发调用不改变各请求的结果或取消 token；
- 静音过滤、LFE 排除和其他产品增强 MUST NOT 被描述为参考算法的一部分；
- 0.1.x legacy 输出 MUST NOT 作为 reference conformance oracle；
- 当前 benchmark 数值 MUST NOT 用于证明算法正确性。

## 5. 结果模型

M0 报告 MUST 明确区分：

- source identity；
- 实际 PCM stream 信息；
- 每声道 measurement 或每声道失败；
- track aggregate；
- channel layout 的 `unknown / known-no-lfe / known` 三态；
- 被排除声道及原因；
- decode progress、预期/已解码 frame 和严格失败 diagnostics；
- 算法 profile、规格版本及兼容状态。

无法表示为有限 JSON 数值的量 MUST 使用显式 `null`/可选字段或离散状态，不能
把非有限浮点数偷偷序列化为与类型不符的值。

## 6. 尚未确认的参考算法行为

下表中的 U 表示当前没有足够证据。M0 可以实现 provisional 行为，但报告必须
明确标识 profile，且不得宣称参考兼容。

| 行为 | 当前等级 | 需要的证据 |
| --- | --- | --- |
| 窗口时长、样本数公式和取整 | U | 多采样率的 N-1/N/N+1 黑盒边界 |
| 不完整尾窗是否参与及如何归一化 | U | 多窗口尾部长度矩阵 |
| PCM 整数/浮点归一化 | U | 精确二进制幅度与多位深输入 |
| RMS 倍乘、量化和直方图还原 | U | 幅度跳变附近观测 + 静/动态分析 |
| 主峰、次峰、重复峰和回退 | U | 可控脉冲/重复峰实验 |
| “最响 20%”的数量与排序规则 | U | 已知强弱窗口组合 |
| 静音和极短输入 | U | 0/1/2 帧及窗口边界实验 |
| 最终整数 DR 舍入 | U | 正负半值和边界附近观测 |
| 多声道 track 聚合 | U | 1/2/3/4/6/8/16 声道矩阵 |
| LFE 处理 | U | 带可靠布局元数据的参考观测 |

## 7. 临时实现的变更规则

- Provisional 行为必须在代码和报告中携带 profile/spec 标识。
- 修改上述 U 项不构成参考兼容性回归，但必须更新本规格、测试说明和 schema
  影响评估。
- 一旦某项有证据，应记录 observation/experiment 链接并提高证据等级。
- Candidate/accepted 规格不得仅引用 MacinMeter 自身测试。

## 8. 进入 candidate 的条件

本规格升级为 candidate 前必须：

1. 固定参考 target 和宿主身份；
2. 建立可重复运行 harness；
3. 为第 6 节每个关键行为提供至少 E2 证据，或明确从目标 profile 排除；
4. 建立 final 与 intermediate 两层 conformance 数据；
5. 记录容差来源，不用宽容差掩盖系统性偏差；
6. 确认 CLI/Tauri/库使用同一 `AnalysisReport` 语义。
