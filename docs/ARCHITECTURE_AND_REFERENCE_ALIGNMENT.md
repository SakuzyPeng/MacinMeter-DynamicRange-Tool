# 架构整改与参考插件重新对齐路线图

> 状态：草案，作为后续整改、重构和逆向研究的主记录
>
> 建立日期：2026-07-17
>
> 当前版本：0.1.3
>
> 参考目标：foobar2000 DR Meter 1.0.3（`foo_dr_meter`）
>
> 相关授权与合规说明：[LEGAL_CN.md](LEGAL_CN.md)

## 1. 文档目的

本文档记录 2026 年 7 月项目全面架构审查后确认的问题、方向和实施顺序，覆盖：

- 当前实现中必须立即修复的正确性、安全性和产品接口问题；
- 核心分析、解码、应用层、CLI 和 GUI 的架构重构；
- 对参考插件重新进行黑盒实验、静态/动态逆向和算法对齐；
- 测试、证据、发布和兼容性声明的验收标准；
- 不影响主方向、但同样必须清理的工程债务。

本文档是路线图和事项总表，不是当前实现已经兼容参考插件的证明。

## 2. 当前结论

项目当前采用的几个大方向值得保留：

- 解码后统一为交错 `f32`；
- 使用流式处理避免保存整份 PCM；
- 使用在线窗口统计和直方图处理长音频；
- CLI 和 Tauri GUI 共用 Rust 分析库；
- 原生解码器为主、外部解码后端为补充。

但当前实现尚不能被视为可靠的全格式、全声道参考兼容实现，原因分为两类。

### 2.1 与参考算法无关的工程错误

这类问题可以直接判断为错误，不需要等待重新逆向：

- 解码器可能把暂时没有数据误报为永久 EOF；
- 安全 Rust API 可以因错误参数触发未定义行为；
- 同一信号因 chunk 切分或声道路径不同而得到不同结果；
- DSD 使用源采样率而非实际 PCM 输出采样率进行分析；
- EdgeTrimmer 可以删除真实音频；
- FFmpeg 非零退出可能被视为正常完成；
- JSON stdout 被横幅和进度污染；
- 批量部分失败仍返回成功退出码；
- 发行制品全局启用 AVX2，破坏 x86-64 基线兼容性。

这些问题必须先止血。

### 2.2 参考算法尚未证实导致的系统性偏差

当前结果与参考插件的差异不能继续笼统归为“浮点误差”或“约 ±0.02–0.05 dB”。尚需重新确认的行为包括：

- 窗口长度和尾窗处理；
- RMS 归一化及可能的倍乘；
- 直方图量化、截断和还原；
- 最响窗口的选取比例和取整；
- 主峰、次峰、重复峰和回退规则；
- 静音、极短音频及特殊边界输入；
- 多声道逐声道处理与最终聚合；
- 最终整数 DR 的舍入方式。

在重新对齐完成前，当前输出应视为 provisional implementation，不应作为参考算法的 golden truth。

## 3. 事实来源与术语

后续工作必须区分四类“真值”。

| 类型 | 定义 | 可以用于什么 | 不可以用于什么 |
| --- | --- | --- | --- |
| 工程不变量 | 不依赖参考算法的程序契约 | 内存安全、EOF、chunk 不变性、串并行一致性 | 决定插件的尾窗、峰值或舍入规则 |
| 参考观测 | 参考插件对固定输入的实际输出 | 建立黑盒 golden corpus、发现偏差 | 在没有证据时解释内部算法 |
| 算法规格 | 由实验、反汇编或动态跟踪支持的行为说明 | 实现 `Reference` profile 和中间状态测试 | 用单次结果或现有注释代替证据 |
| Legacy snapshot | 当前 0.1.x 实现的输出快照 | 观察重构改变了什么 | 证明结果正确或参考兼容 |

任何测试数据都必须标明属于哪一类。

## 4. 总体原则

1. 正确性和可解释性优先于性能。
2. 重新对齐的实现可以后期收口，但参考证据采集从现在开始。
3. 只允许一套生产 DR 状态机和一处结果构造逻辑。
4. 解码、算法、应用编排、CLI/GUI 和报告格式保持单向依赖。
5. 参考兼容模式忠实复现参考行为；产品增强不得悄悄修改参考结果。
6. 默认只启用已经证明可靠的路径；实验功能必须显式标记。
7. 大范围重构采用短期纵向切片，不维护长期失联的“大重写分支”。
8. 新旧实现只在差分验证期间短暂并存；切换生产路径后立即删除旧实现。
9. 0.1.x 阶段不为错误或不安全的公共 API 承担兼容包袱。
10. 所有“小问题”进入清理清单，不以“与主重构无关”为理由永久搁置。

## 5. 事项总表

状态约定：

- `TODO`：未开始；
- `DOING`：正在处理；
- `BLOCKED`：存在明确外部阻塞；
- `DONE`：实现、测试和验收均完成。

### 5.1 P0：立即止血

| ID | 状态 | 事项 | 当前风险 | 临时/最终方向 | 验收要点 |
| --- | --- | --- | --- | --- | --- |
| COR-001 | TODO | 修复并行解码 Pending/EOF 混淆 | 等待 100ms 无数据即返回 `None`，上层静默截断 | 修复前默认串行；同步接口阻塞到数据、真实 EOF 或错误 | 慢 worker 测试不截断；EOF sticky |
| COR-002 | TODO | 修复并行 worker 序列缺口和无限 drain | worker 初始化/发送失败可能不产生终态 | 每个序号必须产生 `Samples / Skipped / Error`；设置总体失败条件 | worker 创建失败、panic、channel 断开均能终止并报错 |
| ALG-001 | TODO | 统一窗口累计和最终结算 | 3+ 声道尾窗未结算，短音频被判静音 | 建立 `push_*()` + 单次 `finish()` | 1/2/3/6/8/16 声道和任意 chunk 切分一致 |
| SAFE-001 | TODO | 删除不安全的泛型样本转换 | `&[T]` 按独立格式标签重解释，可越界/错位读取 | 使用强类型重载或 sealed trait | 安全 API 无调用者不可见的 unsafe 前置条件 |
| SAFE-002 | TODO | 封闭 SIMD 安全包装 | Release 下只靠 `debug_assert` 保护裸指针 | Release 生效的长度/偏移检查；低层降为 `pub(crate)` 或 `unsafe` | 错误尺寸、零声道、越界声道返回错误且无 UB |
| META-001 | TODO | 修正 DSD 实际处理采样率 | 用源 DSD 率计算 PCM 窗口长度和时长 | `pcm_sample_rate` 进入分析器，源率只用于元数据 | DSD 转码后窗口时间尺度正确 |
| TRIM-001 | TODO | 暂停或修复 EdgeTrimmer | 可删除真实音频，Leading 余数回灌，缓冲可增长至 O(N) | 修复前禁用；最终作为独立前处理状态机重写 | 混合静音/短音频/尾静音不误删且空间有界 |
| CLI-001 | TODO | 恢复机器输出契约 | `--json` stdout 含横幅，目录 JSON 被忽略 | stdout 只放数据，诊断和进度走 stderr | 单文件 JSON 可被 `jq` 直接解析；批量有定义的 JSON/NDJSON |
| CLI-002 | TODO | 定义批量部分失败语义 | 部分文件失败仍退出 0，部分写入错误被吞掉 | 返回 `BatchOutcome` 和明确的 partial-failure 退出码 | 黑盒测试覆盖全部成功、部分失败、全部失败、写入失败 |
| BUILD-001 | TODO | 恢复发行制品 CPU 兼容性 | `target-cpu=native` 和全局 AVX2 可导致非法指令 | 发布便携 baseline；SIMD 运行时分派 | 老 x86-64 基线可启动；AVX2 如保留则作为独立制品 |

### 5.2 P1：核心重构与可靠性

| ID | 状态 | 事项 | 当前问题 | 目标 |
| --- | --- | --- | --- | --- |
| ARCH-001 | TODO | 消除模块反向依赖 | `core/audio` 依赖 `tools`，`core` 与 `processing` 相互绑定 | `domain -> analysis/codecs -> application -> adapters` 单向依赖 |
| ARCH-002 | TODO | 删除双 DR 引擎 | CLI 直接使用 `WindowRmsAnalyzer`，公共库另有 `DrCalculator` | 所有入口调用唯一 `AnalyzerSession` |
| ARCH-003 | TODO | 拆分配置和结果类型 | `AppConfig` 混合路径、算法、解码、并发、输出和 UI | `AnalysisConfig / DecodeOptions / BatchOptions / OutputOptions` |
| ARCH-004 | TODO | 具名化输出模型 | `AnalysisOutput` 是四元素元组 | `AnalysisReport`、`ChannelResult`、`DecodeDiagnostics` |
| CODEC-001 | TODO | 引入一次性 DecodePlan | 多处按扩展名重复路由，探测结果与实际 backend 可不一致 | 探测真实 codec 后生成 backend 和 capability 计划 |
| CODEC-002 | TODO | 建立后端能力分级 | `can_decode()` 只看扩展名，格式列表不反映 FFmpeg 可用性 | `NativeStable / External / Experimental / Unavailable` |
| FFMPEG-001 | TODO | 重建 FFmpeg supervisor | stderr 未消费、退出码未检查、kill/join 顺序可能挂死 | 统一管理 stderr、wait、cancel、kill、join 和结构化错误 |
| META-002 | TODO | 拆分格式和进度状态 | expected、decoded、consumed 共用/覆盖同一字段 | `SourceInfo / PcmStreamInfo / DecodeProgress / Diagnostics` |
| META-003 | TODO | 建立 LFE 三态模型 | 未知布局会按声道数猜测并影响正式聚合 | `Unknown / KnownNoLfe / Known(indices)`；未知不自动排除 |
| ERROR-001 | TODO | 结构化错误 | backend、阶段、路径、codec、退出码被压成字符串 | 保留 source、stage、backend、path、recoverability |
| CONC-001 | TODO | 建立全局资源预算 | 文件并发、包并发、codec 线程池相互嵌套 | application 层统一 CPU、内存和任务配额 |
| GUI-001 | TODO | 请求级取消与进度 | Tauri 使用进程级 `AtomicBool`，请求间互相干扰 | 每个任务拥有 ID、CancellationToken 和独立 progress |
| GUI-002 | TODO | 移除全局 FFmpeg 环境变量修改 | 运行中修改进程环境可能与分析线程竞争 | FFmpeg 路径作为不可变配置传给 DecoderFactory |
| SECURITY-001 | TODO | 收紧 Tauri 权限 | CSP 为空且开放通用文件写权限 | 设置 CSP，删除未使用的 fs plugin/permission |
| DEPS-001 | TODO | 收紧 Opus 依赖 | Songbird 引入与本地解码不相称的网络/TLS 依赖 | 使用更窄实现，或将 Opus 做成可选 feature |

### 5.3 P1：参考插件重新逆向与对齐

| ID | 状态 | 事项 | 交付物 |
| --- | --- | --- | --- |
| REF-001 | TODO | 固定参考目标身份 | 插件/宿主版本、平台、配置、二进制哈希 |
| REF-002 | TODO | 建立授权与来源档案 | 公开摘要链接、私有原始授权保存位置、授权范围记录 |
| REF-003 | TODO | 建立可重复的参考运行 harness | 固定输入、自动/半自动运行、结构化输出采集 |
| REF-004 | TODO | 建立合成 PCM 实验生成器 | 可精确控制采样率、长度、声道、幅度、脉冲和分段 |
| REF-005 | TODO | 完成黑盒行为矩阵 | 原始输入、参考输出、环境、实验参数、观察结论 |
| REF-006 | TODO | 开展静态与动态逆向 | 函数/状态映射、关键常量、数据结构、控制流证据 |
| REF-007 | TODO | 编写版本化算法规格 | 每条规则带证据等级、未决问题和反例 |
| REF-008 | TODO | 实现 Reference profile | 独立、可测试的参考兼容实现 |
| REF-009 | TODO | 建立参考 conformance suite | final 和中间状态双层对齐，无未解释系统偏差 |
| REF-010 | TODO | 修订兼容性声明 | 仅在验收通过后恢复明确的参考兼容承诺 |

### 5.4 P2：测试、发布、性能和维护

| ID | 状态 | 事项 | 目标 |
| --- | --- | --- | --- |
| TEST-001 | TODO | 建立工程不变量测试 | chunk、声道路径、scalar/SIMD、串并行差分 |
| TEST-002 | TODO | 建立参考 golden corpus | 只使用参考插件观测生成 correctness golden |
| TEST-003 | TODO | 建立 CLI 黑盒测试 | stdout/stderr、JSON、退出码、文件输出和参数冲突 |
| TEST-004 | TODO | 引入 Miri、sanitizer 和 fuzz | 覆盖 unsafe wrapper、decoder 状态机和异常输入 |
| TEST-005 | TODO | 处理 ignored/弱断言测试 | ignored 有责任人和原因；测试断言实际期望值 |
| CI-001 | TODO | 修复 path filter 和 target 构建 | 纳入 `.cargo/**`、scripts、`tauri-app/**`；使用真实 `--target` |
| CI-002 | TODO | 建立可复现构建 | 固定 MSRV/工具链，CI 使用 `--locked` |
| CI-003 | TODO | 修复 GUI release 链 | release 依赖 GUI build 并上传实际 GUI 制品 |
| CI-004 | TODO | 管理安全 advisory | 忽略项记录原因、负责人和到期日 |
| RELEASE-001 | TODO | 增加制品验证 | smoke test、checksum；后续评估签名、SBOM、provenance |
| PERF-001 | TODO | 删除伪性能指标 | 运行时只报告实测耗时和实际 kernel 计数 |
| PERF-002 | TODO | 重建 benchmark 方法 | 随机/交错 A/B、进程树监控、环境与二进制哈希 |
| DOC-001 | TODO | 修正过度兼容性声明 | 对齐完成前使用 provisional/尚在验证的表述 |
| DOC-002 | TODO | 清理陈旧文档和脚本 | Tauri 命令、Rust 版本、格式支持、路径和版本保持同步 |

## 6. 参考插件研究计划

### 6.1 授权与研究边界

现有法律文档记录：

- 原作者同意项目以 MIT 许可证开发；
- 原作者不反对研究 foobar2000 DR Meter；
- 原作者提供了 DR 测量技术规范文档。

用户进一步确认已取得原作者对逆向插件的许可。后续需要：

1. 保存原始授权材料及其日期、对象、版本和范围；
2. 公共仓库只提交适合公开的摘要，私人通信放在安全位置；
3. 分别记录“逆向研究”“独立实现发布”“名称及兼容性表述”的范围；
4. 对每个被研究的二进制保存哈希，避免不同版本结论混用。

本节只记录工程合规要求，不替代专业法律意见。

### 6.2 先黑盒测绘，再解释内部实现

重新逆向不从现有 Rust 代码或已有注释出发，而从可重复实验出发。

优先实验矩阵：

| 维度 | 取值/策略 | 主要问题 |
| --- | --- | --- |
| 采样率 | 8k、44.1k、48k、88.2k、96k、192k | 窗口长度公式和取整 |
| 样本长度 | 0、1、2、疑似窗口 `N-1/N/N+1`、多窗口边界 | 尾窗、极短音频和边界 |
| 波形 | 零、常量、脉冲、正弦、方波、白噪声、分段幅度 | RMS、Peak、静音和窗口选择 |
| 幅度 | 精确二进制值、整数 PCM 边界、量化跳变附近 | 归一化、量化和截断 |
| 峰值构造 | 唯一峰、重复最大峰、主峰+次峰、跨窗口峰 | Peak/次 Peak 规则 |
| 窗口分布 | 已知强弱窗口组合 | 最响 20% 数量和排序 |
| 声道 | 1/2/3/4/6/8/16，声道内容相同或不同 | 逐声道状态和最终聚合 |
| 位深/样本格式 | 16/24/32-bit PCM、float（若宿主支持） | 输入归一化和宿主解码影响 |

实验首先使用无压缩、可精确控制的 PCM/WAV，以隔离宿主 codec 解码和算法本身。压缩格式、DSD 和容器布局在算法行为确认后单独验证。

### 6.3 证据等级

算法规格中的每条结论必须标记：

| 等级 | 含义 |
| --- | --- |
| E3 | 黑盒实验、静态分析和动态跟踪相互印证 |
| E2 | 至少两类独立证据相互印证 |
| E1 | 单类证据支持，尚缺交叉验证 |
| H | 高置信假设，仍存在可区分的替代解释 |
| U | 未知或存在冲突证据 |

`Reference` profile 的关键行为原则上不得依赖 `H/U` 结论进入稳定发布；确需进入时必须有明确的风险说明和后续实验。

### 6.4 建议的研究产物

在具体工作开始时建立独立目录，建议结构如下：

```text
reference/
├── README.md
├── targets/          # 版本、平台、哈希和宿主配置
├── experiments/      # 实验定义和输入生成参数
├── observations/     # 参考插件原始输出与环境信息
├── fixtures/         # 可公开、可重复生成的测试输入
├── specs/            # 版本化算法规格和证据等级
└── conformance/      # 参考结果和差分摘要
```

私人授权原文不应因目录建议而直接提交到公开仓库。

### 6.5 对齐验收标准

完成参考对齐至少需要满足：

1. 所有边界 fixture 的最终整数 DR 与参考插件完全一致；
2. 每声道窗口数量、被选窗口数量和峰值选择一致；
3. 可观测的 RMS/Peak 中间值在事先定义并解释的数值容差内；
4. 残差不随采样率、时长、幅度或声道数呈系统趋势；
5. 极短音频、尾窗、重复峰、静音和多声道没有未解释例外；
6. 每个特殊行为都可追溯到实验、静态分析或动态跟踪证据；
7. Reference profile 不依赖解码 chunk 大小或内部优化路径；
8. 对参考插件本身的奇怪行为保持忠实复现，不在兼容模式中擅自修正。

“最终差值小于某个 dB”不能单独作为对齐完成的标准，因为多个内部错误可能相互抵消。

## 7. 目标架构

### 7.1 逻辑分层

```text
domain
├── AnalysisConfig / AnalysisProfile
├── SourceInfo / PcmStreamInfo
├── AnalysisReport / ChannelResult
├── DecodeDiagnostics / BatchOutcome
└── AudioError

analysis
└── AnalyzerSession
    ├── push_interleaved()
    ├── progress()
    └── finish()

codecs
├── DecodePlan / DecoderFactory
├── Symphonia adapter
├── Opus adapter
└── FFmpeg supervisor

application
├── analyze_file / analyze_many
├── progress / cancellation
└── global resource budget

adapters
├── CLI + Human/JSON/NDJSON formatter
├── Tauri commands and events
└── benchmark harness
```

`domain` 不依赖 CLI、Tauri、Symphonia 或 FFmpeg。`analysis` 不知道文件路径和输出格式。`adapters` 不直接构造算法内部状态。

### 7.2 分析 profile

初期建议至少区分：

```text
Provisional
ReferenceFooDrMeter103
```

名称可在参考目标最终固定后调整。

- `Provisional` 明确表示当前尚未完成参考对齐；
- `ReferenceFooDrMeter103` 只包含被证据支持的参考行为；
- Edge trim、静音过滤等作为显式 preprocessing pipeline，不伪装成另一套“官方”算法；
- 是否长期保留 `Provisional`，在 Reference profile 完成后再决定。

### 7.3 解码契约

同步 `AudioSource` 的读取结果必须只有：

```text
Data(chunk)
Eof
Error
```

若需要非阻塞模型，应显式增加 `Pending`，不能复用 `Eof`。

`PcmStreamInfo.sample_rate` 永远表示送入分析器的实际 PCM 率。源格式、原始 DSD 率、预期帧数、已解码帧数和已消费帧数分别存储。

### 7.4 并发策略

- 默认保留文件级并行；
- 包级并行在重新证明 codec 独立性、EOF 和错误传播前保持关闭；
- 有状态 codec 不按文件扩展名猜测并行安全性；
- application 层为文件任务、decoder worker 和外部进程分配统一预算；
- 若实测证明包级并行收益不足以抵消复杂度，允许直接删除。

## 8. 分阶段实施

逆向研究是一条从现在开始的并行轨道，不是所有重构完成后的最后一步。

### M0：止血版本

建议目标版本：0.1.4。

- 完成 `COR-001`、`ALG-001`、`SAFE-001/002`、`META-001`；
- 禁用尚未修复的 EdgeTrimmer 和默认包级并行；
- 修复 JSON、批量退出码和 portable release；
- 为每项缺陷先增加可复现测试；
- 修订 README 中把系统性偏差描述为普通误差的表述。

出口条件：

- 不再有已知静默截断和公开安全 API UB；
- 多声道短音频不再因路径差异得到全零结果；
- JSON 和退出码可用于自动化；
- 发行制品使用便携 CPU baseline。

### M1：事实与证据基础

- 建立工程不变量测试；
- 建立 reference 目录、目标档案、实验生成器和运行 harness；
- 保存第一批参考观测，不用当前实现反向生成 correctness golden；
- 建立算法规格模板和证据等级；
- 将当前输出仅保存为 legacy snapshot。

出口条件：

- 关键工程行为都有测试；
- 参考插件结果可重复采集；
- 每条算法结论可追溯到证据或明确标为未知。

### M2：唯一分析内核

- 引入 domain 类型和 `AnalyzerSession`；
- 用串行 PCM/WAV 建立第一条完整纵向路径；
- 差分验证新旧实现；
- CLI/Tauri 切换到新内核；
- 删除 `tools::processor` 与 `DrCalculator` 的重复结果构造；
- 重写 EdgeTrimmer 为独立 preprocessing stage。

出口条件：

- 仓库只有一套生产 DR 状态机；
- chunk、声道和优化路径不影响 provisional 结果；
- 旧引擎已删除。

### M3：解码与应用层

- 引入 `DecodePlan`、backend registry 和结构化 `StreamInfo`；
- 加固 Symphonia、Opus 和 FFmpeg 生命周期；
- 建立请求级取消和全局资源预算；
- CLI/Tauri 只依赖 application 层；
- 收紧 Opus、benchmark 和 GUI 依赖。

出口条件：

- 所有 backend 满足共同契约测试；
- 外部进程失败不会被视为正常 EOF；
- GUI 请求互不干扰；
- 支持格式列表反映运行时真实能力。

### M4：参考算法收口

- 根据 REF 轨道证据实现 `ReferenceFooDrMeter103`；
- 对齐窗口、RMS、量化、Peak、20%、舍入和多声道聚合；
- 建立 final + intermediate conformance suite；
- 处理所有系统性残差和未解释边界；
- 完成兼容性报告。

出口条件：

- 满足第 6.5 节全部标准；
- 不再存在“已知有偏但归因不明”的结果族；
- 参考 profile 可独立于 decoder、chunk 和 SIMD 路径验证。

### M5：产品与仓库收敛

- 视逻辑边界稳定情况拆为 Cargo workspace；
- 统一 CLI、GUI、版本、MSRV、lockfile 和 release；
- 修订用户文档、支持格式和兼容性声明；
- 清理遗留脚本、无效依赖和过期构建文件；
- 建立制品 smoke test 和 checksum。

### M6：重新性能工程

- 删除未实现 AVX/AVX2 的理论加速报告；
- 使用可复现 benchmark 重新 profile；
- 决定是否恢复包级并行；
- 只优化已确认瓶颈；
- 性能路径必须通过与标量参考路径的差分测试。

## 9. 建议 PR 顺序

1. `fix: stop silent truncation and make serial decoding default`
2. `fix: finalize multichannel windows consistently`
3. `fix: close unsafe public conversion and SIMD APIs`
4. `fix: correct DSD PCM rate and quarantine edge trimming`
5. `fix: restore JSON, exit-code and portable-release contracts`
6. `docs: qualify current compatibility claims and add reference research plan`
7. `test: add invariant, reference-observation and CLI contract suites`
8. `refactor: introduce domain models and AnalyzerSession`
9. `refactor: migrate production to the single analyzer and remove legacy engine`
10. `refactor: introduce DecodePlan, StreamInfo and FFmpeg supervisor`
11. `refactor: rebuild application, CLI and Tauri adapters`
12. `feat: implement and verify the reference plugin profile`
13. `chore: unify workspace, CI, versions and release artifacts`
14. `perf: re-profile and selectively restore proven optimizations`

每个 PR 应包含对应测试和验收说明。不要把全部步骤压成一个无法审查的大提交。

## 10. 小项清理清单

以下事项不决定总体架构，但同样必须处理：

- [ ] 设置准确的 `rust-version`，修正文档中的 Rust 1.80 表述；
- [ ] 统一根 crate、Tauri Cargo、Tauri package.json 和 lockfile 版本；
- [ ] CI 和本地构建使用 `--locked`；
- [ ] 处理 `panic = "abort"` 与 `catch_unwind` 的矛盾；
- [ ] 删除非测试代码中的无保护 `expect`；
- [ ] 默认不提升到系统最高线程优先级，改为显式 opt-in；
- [ ] 将 benchmark 专用依赖移出核心依赖；
- [ ] 将仅测试使用的 `hound` 移入 dev-dependencies，或删除；
- [ ] 明确 Symphonia `default-features`；
- [ ] 统一 decoder reset 契约和统计单位；
- [ ] 修复 progress 始终接近 1.0 的元数据覆盖问题；
- [ ] 支持非 UTF-8 本地路径传给 FFmpeg；
- [ ] 清理根目录孤立 `package-lock.json`、`compile_commands.json` 等生成物；
- [ ] 修复 Tauri 严格 Clippy 错误；
- [ ] 更新 Tauri 文档中的命令数量、权限和批处理状态；
- [ ] 修复 macOS bundle、benchmark 和 pre-commit 脚本中的过期路径/假设；
- [ ] 让 audit 忽略列表在本地和 CI 保持一致并可追踪；
- [ ] 删除或重写没有独立 oracle、只验证实现自身的弱测试；
- [ ] 为 ignored 测试记录原因、负责人和恢复条件；
- [ ] 校正文档中的“恒定内存”“单次遍历”“完全兼容”等过度表述；
- [ ] 删除未进入生产路径的重复 SIMD/Peak 状态和伪性能估算。

## 11. 当前建议决策

以下是本次审查后的建议，后续可以通过 ADR 正式确认：

| 决策 | 建议 |
| --- | --- |
| 产品核心 | 可信的离线 DR 分析库和 CLI；GUI 为薄适配层 |
| 默认算法 | 对齐完成前明确标记 provisional；对齐后默认使用 Reference profile |
| 增强功能 | Edge trim、静音过滤等与参考算法分离并显式启用 |
| 默认并发 | 文件级并行；单文件包级并行保持关闭直到重新证明 |
| 格式承诺 | 区分原生稳定、外部依赖、实验性和不可用 |
| 公共 API | 0.1.x 允许为正确性和安全性做 breaking changes |
| 发行 CPU | portable baseline + 函数级运行时 SIMD |
| 性能判断 | 只接受可复现实测，不接受硬件能力推导的理论倍数 |
| 重构方式 | 小步纵向迁移；生产切换后立即删除旧路径 |

## 12. 待确认事项

- [ ] 最终参考目标是否只锁定 `foo_dr_meter 1.0.3`，还是还要记录其他版本行为；
- [ ] 参考宿主 foobar2000 的精确版本、架构和设置；
- [ ] 是否能自动化参考插件运行，还是由 Windows 环境半自动采集；
- [ ] 参考观测和 fixture 哪些可以公开提交；
- [ ] 对齐完成后是否保留 `Provisional`/legacy profile；
- [ ] 第一批正式承诺支持的 codec 和容器范围；
- [ ] `Reference` profile 的中间数值容差如何定义；
- [ ] 何时从 0.1.x 进入新的 API 版本或 1.0 稳定承诺。

## 13. 完成定义

本路线图不能以“代码已重排”作为完成标准。项目达到下一阶段稳定状态需要同时满足：

- 已知 P0/P1 工程错误关闭；
- 生产路径只有一个分析内核；
- 参考算法具有版本化规格、证据和 conformance suite；
- 当前不存在未解释的系统性偏差；
- CLI、GUI 和库共享相同 application 行为；
- codec、外部进程、取消和并发具有明确契约；
- 构建可复现、制品可移植、发布内容与说明一致；
- 性能优化不改变参考结果；
- README 和用户输出只声明已经被证据支持的能力。

## 14. 初始审查基线

本节保存建立路线图时的验证状态，避免后续只看到任务结论而失去问题证据。它不是永久测试报告；每个事项关闭时仍需在对应 PR 中提供新的验证结果。

### 14.1 已执行检查

| 检查 | 2026-07-17 结果 | 说明 |
| --- | --- | --- |
| `cargo fmt --all -- --check` | 通过 | 根 crate |
| 根 crate 严格 Clippy | 通过 | `--all-targets --all-features -D warnings` |
| `cargo test --all-features` | 通过 | 关键缺陷仍通过，说明部分测试 oracle/断言过弱 |
| Tauri 前端 `npm run build` | 通过 | Vite 构建成功 |
| Tauri `cargo check` | 通过 | 未使用 `--locked` 时会刷新陈旧的子 lockfile |
| Tauri 严格 Clippy | 失败 | 2 个 `collapsible_if` |
| `cargo audit --no-fetch`（初始离线库） | 失败 | 陈旧的本地审计库报告 3 个 vulnerability |
| pre-commit `cargo audit`（刷新数据库后） | 警告后允许提交 | 报告 16 个 vulnerability 和 8 个 allowed warning |

审查时报告的三个 vulnerability：

- `RUSTSEC-2025-0009`：`ring 0.16.20`；
- `RUSTSEC-2024-0336`：`rustls 0.20.9`；
- `RUSTSEC-2023-0065`：`tungstenite 0.18.0`。

提交前钩子随后刷新 advisory 数据库，同一锁文件被报告：

- `crossbeam-epoch`：`RUSTSEC-2026-0204`；
- `quick-xml`：`RUSTSEC-2026-0194`、`RUSTSEC-2026-0195`；
- `quinn-proto`：`RUSTSEC-2026-0037`、`RUSTSEC-2026-0185`；
- 多个 `rustls-webpki` 版本：`RUSTSEC-2026-0049`、`RUSTSEC-2026-0098`、`RUSTSEC-2026-0099`、`RUSTSEC-2026-0104`；
- allowed warning 涉及 `audiopus_sys`、`rustls-pemfile`、`anyhow`、`memmap2`、两个 `rand` 版本和两个被 yanked 的 `spin` 版本。

这也说明离线或陈旧 advisory 数据库会低估依赖风险。依赖更新后必须使用刷新后的数据库重新审计，不能把本表或 pre-commit 的“警告后放行”当成永久豁免。

### 14.2 已复现行为

- 现有 1 秒、3 声道、非零 1 kHz 正弦 fixture 被三个声道全部报告为静音、DR 0；测试仍通过，因为只检查有限值和宽范围。
- `--json --no-save` 的 stdout 在 JSON 前包含启动横幅，不能直接由 `jq` 解析。
- 批量输入中部分文件失败时，进程仍返回退出码 0。
- EdgeTrimmer 的“短静音 + 不足迟滞的真实音频 + 长尾静音”可把真实音频一并删除。
- EdgeTrimmer 的连续长首部静音会按 `min_run` 重置状态并回灌余数。
- Tauri 子 lockfile 中包版本和根依赖信息落后于 manifest；普通检查会静默更新。

### 14.3 关键源码入口

| 主题 | 当前入口 |
| --- | --- |
| 同步流式契约 | [`src/audio/streaming.rs`](../src/audio/streaming.rs) |
| 解码路由与并行状态机 | [`src/audio/universal_decoder.rs`](../src/audio/universal_decoder.rs) |
| 并行包重排 | [`src/audio/parallel_decoder.rs`](../src/audio/parallel_decoder.rs) |
| FFmpeg 生命周期 | [`src/audio/ffmpeg_bridge.rs`](../src/audio/ffmpeg_bridge.rs) |
| 窗口/直方图状态 | [`src/core/histogram.rs`](../src/core/histogram.rs) |
| 公共 DR Calculator | [`src/core/dr_calculator.rs`](../src/core/dr_calculator.rs) |
| 生产分析路径 | [`src/tools/processor.rs`](../src/tools/processor.rs) |
| 样本转换 unsafe 包装 | [`src/processing/sample_conversion.rs`](../src/processing/sample_conversion.rs) |
| SIMD 安全包装 | [`src/processing/simd_core.rs`](../src/processing/simd_core.rs) |
| EdgeTrimmer | [`src/processing/edge_trimmer.rs`](../src/processing/edge_trimmer.rs) |
| CLI 配置与横幅 | [`src/tools/cli.rs`](../src/tools/cli.rs) |
| 批处理退出行为 | [`src/main.rs`](../src/main.rs) |
| Tauri 状态与命令 | [`tauri-app/src-tauri/src/lib.rs`](../tauri-app/src-tauri/src/lib.rs) |
| CPU 构建标志 | [`.cargo/config.toml`](../.cargo/config.toml) |
| CI 和 release | [`.github/workflows/ci-cd.yml`](../.github/workflows/ci-cd.yml) |
