# 架构整改与参考插件重新对齐路线图

> 状态：执行中（M0：`DONE`，M1：`DONE`，M2：`DOING`；
> foo_dr_meter 1.0.8 Candidate V1
> 已实施；schema-v3
> x64 safe-master 的 track DR 39/39、channel DR 62/62、overall peak 39/39、
> overall RMS 39/39、channel RMS 62/62、duration 39/39），
> 作为整改、重构和逆向研究的主记录
>
> 建立日期：2026-07-17
>
> Legacy 基线：0.1.3
>
> M0 目标版本：0.2.0
>
> 当前参考目标：foobar2000 DR Meter 1.0.8（`foo_dr_meter`）
>
> 相关授权与合规说明：[LEGAL_CN.md](LEGAL_CN.md)
>
> M0 决策记录：[ADR-0001：以 0.2.0 重建可信主干](adr/0001-m0-0.2.0-trusted-trunk-rebuild.md)
>
> M1 范围决策：[ADR-0002：限定 M1 的参考数值契约](adr/0002-m1-reference-numeric-scope.md)
>
> M2 范围决策：[ADR-0003：M2 原生解码面与工程契约加固](adr/0003-m2-native-decoder-contract-hardening.md)

## 1. 文档目的

本文档记录 2026 年 7 月项目全面架构审查后确认的问题、方向和实施顺序，覆盖：

- 当前实现中必须立即修复的正确性、安全性和产品接口问题；
- 核心分析、解码、应用层、CLI 和 GUI 的架构重构；
- 对参考插件重新进行黑盒实验、静态/动态逆向和算法对齐；
- 测试、证据、发布和兼容性声明的验收标准；
- 不影响主方向、但同样必须清理的工程债务。

本文档是路线图和事项总表，不是当前实现已经兼容参考插件的证明。

## 2. 审查结论与当前进展

本节 2.1 与 2.2 保留项目启动整改时的审查结论。2.1 所列生产路径已由 M0
重建关闭；2.2 所列算法问题不能以旧实现为答案，仍须由参考证据逐项解决。

项目当前采用的几个大方向值得保留：

- 解码后统一为有限、交错 `f64`；
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

在重新对齐完成前，当时的输出应视为 provisional implementation，不应作为参考算法的 golden truth。

截至 2026-07-18，当前目标已固定为 foo_dr_meter 1.0.8：x64/x86 二进制静态
分析、x86 初始观测和 x64 complete-v2 safe-master 观测共同形成
`FooDrMeter108CandidateV1`。x64 架构精度边界已达到 E2；产品 PCM 主链改为 f64
后，同一 observation 的整数 track DR 为 39/39、每声道两位 DR token 为 62/62。
schema v3 又以独立 report metrics 对齐 overall peak 39/39、overall RMS 39/39
与 channel RMS 62/62。这足以关闭当前 corpus 中公开同语义字段的已知系统差分，
但不等同于完整 conformance；所有输出继续标记 `Unverified`。

2026-07-19 又建立了固定 x64 target `ff3556ad` 的隔离 analyzer-core harness：
每个输入启动一个全新 worker，在不启动 foobar2000 的前提下直接调用
init/push/finish，并保存 result、session、channel state 与浮点控制位。固定
complete-v2 safe-master 的 39 项输入均完成受控执行。真实、固定的
`shared.dll` 被保留用于 DLL load/unload lifecycle；core 调用期间，目标对
`shared.dll` 的 13 个普通 IAT 入口全部由 fail-fast tripwire 接管。该记录建立了
更纯净的算法 core 动态证据，但没有验证 foobar 解码、组件注册、host lifecycle、
metadata、album 或 renderer；其声明固定为 `compatibility: none`、
`foobarParity: not_assessed`。

ADR-0002 随后把 M1 收紧为固定 x64 数值契约：per-track analyzer core 是主体，
同时保留 album 聚合与 renderer 中会改变数值结果的纯算术、窄化和舍入。foobar
host、playlist/grouping、metadata 来源、完整文本以及 production/reference
内部状态同构均不是 M1 目标，也不作为缺失证据。

## 3. 事实来源与术语

后续工作必须区分四类“真值”。

| 类型 | 定义 | 可以用于什么 | 不可以用于什么 |
| --- | --- | --- | --- |
| 工程不变量 | 不依赖参考算法的程序契约 | 内存安全、EOF、chunk 不变性、串并行一致性 | 决定插件的尾窗、峰值或舍入规则 |
| 参考观测 | 参考插件对固定输入的实际输出 | 建立黑盒 golden corpus、发现偏差 | 在没有证据时解释内部算法 |
| 算法规格 | 由实验、反汇编或动态跟踪支持的行为说明 | 实现固定目标的数值契约和针对性边界测试 | 要求不同实现具有相同内部结构，或用现有注释代替证据 |
| Legacy snapshot | 当前 0.1.x 实现的输出快照 | 观察重构改变了什么 | 证明结果正确或参考兼容 |

任何测试数据都必须标明属于哪一类。

## 4. 总体原则

1. 正确性和可解释性优先于性能。
2. 重新对齐的实现可以后期收口，但参考证据采集从现在开始。
3. 只允许一套生产 DR 状态机和一处结果构造逻辑。
4. 解码、算法、应用编排、CLI/GUI 和报告格式保持单向依赖。
5. 参考兼容模式忠实复现参考行为；产品增强不得悄悄修改参考结果。
6. 默认只启用已经证明可靠的路径；实验功能必须显式标记。
7. M0 直接建立可运行的 0.2.0 纵向主干，不维护长期失联的“大重写分支”。
8. 新旧实现只在迁移验证期间短暂并存；切换生产路径后立即删除旧实现和适配层。
9. 0.2.0 明确允许 Rust API、CLI、JSON 和 GUI IPC breaking change，不保留 0.1.x 行为兼容层。
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
| COR-001 | DONE | 删除 Pending/EOF 混淆路径 | 旧包级并行解码器已删除 | `PcmSource` 只返回 `Data / Eof / Error` | EOF 与 terminal error 均 sticky |
| COR-002 | DONE | 删除并行 worker 缺口路径 | worker/序号模型已从 M0 删除 | 坏包严格失败，不生成部分结果 | 损坏 FLAC/WAV sticky error 测试 |
| ALG-001 | DONE | 统一窗口累计和最终结算 | 唯一 `AnalyzerSession` | `push_interleaved()` + consuming `finish()` | 1/2/3/6/8/16 声道和随机 chunk 切分一致 |
| SAFE-001 | DONE | 删除不安全的泛型样本转换 | 旧转换层已删除 | Symphonia 安全 `SampleBuffer<f64>` | 所有第一方 crate `forbid(unsafe_code)` |
| SAFE-002 | DONE | 删除手写 SIMD 包装 | 旧 SIMD 层已删除 | M0 只保留 safe scalar | 无裸指针或 target CPU 假设 |
| META-001 | DONE | 分离实际 PCM stream 信息 | DSD/转码不属于 M0 | `PcmStreamInfo.spec` 只描述实际 PCM | 不猜测或混用源率 |
| TRIM-001 | DONE | 从生产管线移除 EdgeTrimmer | 旧实现已删除 | 未来只能作为独立显式前处理重建 | M0 请求/结果无 trim 字段 |
| CLI-001 | DONE | 恢复机器输出契约 | 显式 analyze/batch 命令 | stdout 只放结果，进度/诊断走 stderr | JSON 黑盒测试直接解析 |
| CLI-002 | DONE | 定义批量失败语义 | tagged item outcome + 稳定退出码 | 全成功 0、失败 1、部分成功 3、取消 130 | 黑盒覆盖全部分支与输出失败 |
| BUILD-001 | DONE | 恢复便携 CPU 基线 | 删除 `target-cpu=native`/AVX2 | portable safe scalar release | workspace 无本机 CPU 编译标志 |

### 5.2 P1：核心重构与可靠性

| ID | 状态 | 事项 | 当前问题 | 目标 |
| --- | --- | --- | --- | --- |
| ARCH-001 | DONE | 消除模块反向依赖 | virtual workspace 强制包边界 | `domain -> analysis/codecs -> application -> adapters` 单向依赖 |
| ARCH-002 | DONE | 删除双 DR 引擎 | 旧 `core/processing` 生产路径已删除 | 所有入口调用唯一 `AnalyzerSession` |
| ARCH-003 | DONE | 拆分请求和展示配置 | 分析只接收路径/profile；渲染开关留在 adapter | `AnalyzeRequest / BatchRequest / ExecutionControl` |
| ARCH-004 | DONE | 具名化输出模型 | 不再使用长元组 | `AnalysisReport`、`ChannelResult`、`DecodeDiagnostics` |
| CODEC-001 | DONE | 收口一次性探测入口 | 单一 `DecoderFactory` 按内容探测 | M0 只有一个 Symphonia backend，无重复路由 |
| CODEC-002 | DONE | 固定 M0 能力矩阵 | 文档与 `SUPPORTED_EXTENSIONS` 区分发现/解码 | WAV/FLAC/AIFF stable，其余 unavailable |
| FFMPEG-001 | DONE | M0 移除 FFmpeg 生产路径 | 外部进程 supervisor 不再存在 | 如恢复必须重新建立完整生命周期契约 |
| META-002 | DONE | 拆分格式和进度状态 | expected 与 decoded 独立 | `SourceInfo / PcmStreamInfo / DecodeProgress / Diagnostics` |
| META-003 | DONE | 建立 LFE 三态模型 | 未知布局不猜测 | `Unknown / KnownNoLfe / Known(positions)` |
| ERROR-001 | DONE | 结构化错误 | 稳定 code/stage + backend/path/details | CLI、JSON 与 Tauri 共用 |
| CONC-001 | TODO | 建立全局资源预算 | 文件并发、包并发、codec 线程池相互嵌套 | application 层统一 CPU、内存和任务配额 |
| GUI-001 | DONE | 请求级取消与进度 | `jobId -> CancellationToken` registry | 取消隔离与 RAII 清理测试 |
| GUI-002 | DONE | 移除全局 FFmpeg 环境变量修改 | GUI 不再包含 FFmpeg 配置 | 无环境变量修改 |
| SECURITY-001 | DONE | 收紧 Tauri 权限 | 生产/开发 CSP 已设置 | 仅 core default + dialog open |
| DEPS-001 | DONE | 删除 M0 Opus 依赖 | Songbird 与旧网络/TLS 链移除 | Opus 明确 unavailable |

### 5.3 P1：参考插件重新逆向与对齐

| ID | 状态 | 事项 | 交付物 |
| --- | --- | --- | --- |
| REF-001 | DONE | 固定参考目标身份 | foo_dr_meter 1.0.8 x64/x86 hash、宿主与配置记录 |
| REF-002 | DONE | 建立授权与来源档案 | 公开最小摘要、私人打印快照 digest、保管位置与未授权边界均已登记 |
| REF-003 | DONE | 建立可重复的参考运行 harness | 离线 observation importer 与隔离 x64 numeric parent/worker/suite 均固定输入、target、runtime、worker 身份和执行契约；harness 已演进至 schema v2，39 项历史 safe-master 与 38 项 numeric-boundary 每项使用全新 worker，范围不含 foobar host |
| REF-004 | DONE | 建立合成 PCM 实验生成器 | 可精确控制窗口边界、幅度、峰值顺序和多声道 |
| REF-005 | DONE | 完成目标行为矩阵 | x86 15 项历史导出、x64 39 项 safe-master/隔离 core，以及 duration/weighting/histogram 38 项隔离边界记录已登记；block-size 与 fresh-worker 稳定性检查已固定，host-edge、album playlist 和 host repeat 不属于 M1 |
| REF-006 | DONE | 开展静态与动态逆向 | x64 analyzer/session/channel/result 已形成 accepted 隔离 core 动态记录；duration leaf、可选多声道 weighting 与 histogram endpoint 已有专门动态交叉；album 聚合、完整 renderer、foobar host/metadata/text parity 保持各自证据边界 |
| REF-007 | DONE | 编写版本化数值规格 | Candidate 已纳入 x64 core、report 数值、duration 舍入、histogram clamp、多声道 weighting 和 album 聚合规则，并明确证据等级与非目标；DONE 不表示 profile 已升级为 accepted/verified |
| REF-008 | DONE | 实现 Candidate profile | 唯一 f64 生产 profile；schema v3 的六组公开 DR/report/duration 字段完全匹配，仍不宣称参考兼容 |
| REF-009 | DONE | 建立有界参考 conformance suite | clean-commit successor 覆盖六组字段、四项 footer consistency 与 DR0 反事实；isolated core 对既有报告四类字段达到 39/39、62/62、62/62、39/39；production intermediate 差分不是目标 |
| REF-010 | DONE | 固定范围与兼容性声明 | 保持 `CandidateV1 / Unverified`，只陈述固定 x64 数值证据，不声称完整 foobar/component parity |

### 5.4 P2：测试、发布、性能和维护

| ID | 状态 | 事项 | 目标 |
| --- | --- | --- | --- |
| TEST-001 | DONE | 建立 M0 工程不变量测试 | chunk、声道、窗口边界、有限值、长流有界内存 |
| TEST-002 | DONE | 建立固定参考 observation corpus | x64 39-track foobar single pass、确定性离线 importer、39 项 accepted 隔离 core、38 项 accepted numeric-boundary 及 block/repeat 辅助检查已固定；它是 M1 判别 corpus，不冒充任意音频的穷尽 oracle |
| TEST-003 | DONE | 建立 CLI 黑盒测试 | stdout/stderr、JSON、0/1/2/3/130、原子输出 |
| TEST-004 | TODO | 建立 malformed corpus 与手动 fuzz | 每例独立 timeout；发现的问题最小化后进入本地回归 corpus |
| TEST-005 | DONE | 处理 ignored/弱断言测试 | 旧弱测试随 legacy 路径删除；新测试使用明确 oracle |
| CI-001 | DONE | 缩减为 opt-in workspace 验证 | 单手动 Ubuntu job，不再使用旧 path filter/release |
| CI-002 | DONE | 固定 M0 构建基线 | Rust 1.88、根 lockfile、CI `--locked` |
| CI-003 | TODO | 修复 GUI release 链 | release 依赖 GUI build 并上传实际 GUI 制品 |
| CI-004 | TODO | 管理安全 advisory | 忽略项记录原因、负责人和到期日 |
| RELEASE-001 | TODO | 增加制品验证 | smoke test、checksum；后续评估签名、SBOM、provenance |
| PERF-001 | DONE | 删除伪性能指标 | M0 不再输出推导吞吐或理论加速比 |
| PERF-002 | TODO | 重建 benchmark 方法 | 随机/交错 A/B、进程树监控、环境与二进制哈希 |
| DOC-001 | DONE | 修正过度兼容性声明 | 所有当前输出标记 `foo_dr_meter 1.0.8 Candidate V1 / Unverified` |
| DOC-002 | DONE | 清理陈旧文档和脚本 | Tauri、格式、MSRV、CLI、性能与法律文档已同步 |

## 6. 参考插件研究计划

### 6.1 授权与研究边界

现有法律文档与
[`AUTH-foo-dr-meter-author-reply-20250908`](../reference/authorization/README.md)
记录：

- 原作者不反对项目维护者逆向 `foo_dr_meter` component；
- 原作者不反对本独立项目选择 MIT License；
- 原作者提供了 DR 测量技术规范文档。

私人原件继续由维护者在公开仓库外保管；仓库只登记邮箱显示日期、通信对象、
范围摘要、打印快照 digest 和明确未授权事项。回复没有指定插件版本、架构或
二进制 hash，因此每个实际研究对象仍由独立 target 档案固定，不能用授权摘要
替代版本化证据身份。

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

M0 已建立独立 [`reference/`](../reference/README.md) 目录，结构如下：

```text
reference/
├── README.md
├── targets/          # 版本、平台、哈希和宿主配置
├── experiments/      # 实验定义和输入生成参数
├── observations/     # 参考插件原始输出与环境信息
├── static-analysis/  # 固定二进制的受控静态事实
├── native/           # 隔离 core worker 与受控 native 边界
├── tools/            # observation、core suite 与差分工具
├── fixtures/         # 可公开、可重复生成的测试输入
├── specs/            # 版本化算法规格和证据等级
└── conformance/      # 参考结果和差分摘要
```

私人授权原文不应因目录建议而直接提交到公开仓库。

### 6.5 固定 x64 数值对齐验收标准

M1 范围内的参考对齐至少需要满足：

1. 固定判别 corpus 的同语义最终字段与参考结果完全一致；
2. 窗口、RMS、histogram、peak 和聚合规则均可追溯到固定二进制的静态分析、
   隔离执行或参考观测；
3. album 与 renderer 中纳入范围的纯数值算术、窄化和舍入具有产品边界测试；
4. 残差不随采样率、时长、幅度或声道数呈系统趋势；
5. 极短音频、尾窗、重复峰、静音和多声道没有未解释例外；
6. Candidate profile 不依赖解码 chunk 大小或内部优化路径；
7. 对参考插件本身已确定的奇怪数值行为保持忠实复现，不擅自修正。

“最终差值小于某个 dB”不能单独作为对齐完成的标准，因为多个内部错误可能相互
抵消。但 conformance 也不要求两个实现的中间结构、累计顺序或每个检查点数值
相等；reference raw state 可以作为逆向证据，MacinMeter 只需满足已声明的最终
语义和自身工程不变量。只有最终结果出现反例时才增加中间路径诊断。

## 7. 目标架构

### 7.1 逻辑分层

```text
domain
├── AnalysisProfile / StreamSpec
├── SourceInfo / PcmStreamInfo
├── AnalysisReport / ChannelResult
├── FiniteF32 / FiniteF64 / DecodedDuration
├── DecodeDiagnostics / BatchItemOutcome
└── AnalysisError / ErrorCode

analysis
└── AnalyzerSession
    ├── push_interleaved()
    └── finish() -> Result

codecs
├── DecoderFactory
└── Symphonia adapter（M0：WAV/FLAC/AIFF）

application
├── analyze_file / serial BatchRunner
├── explicit AlbumAggregator
├── progress / cancellation
└── versioned WireEnvelope

adapters
├── CLI + Human/JSON formatter
├── Tauri commands and events
└── frontend rendering
```

`domain` 不依赖 CLI、Tauri、Symphonia 或 FFmpeg。`analysis` 不知道文件路径和输出格式。`adapters` 不直接构造算法内部状态。

### 7.2 分析 profile

当前生产分析只保留一个 profile：

```text
FooDrMeter108CandidateV1
```

- Rust 名称固定为 `FooDrMeter108CandidateV1`，wire 名称固定为
  `foo_dr_meter_1_0_8_candidate_v1`；
- `CandidateV1 / Unverified` 同时标识目标版本、候选规则修订和未完成的
  conformance，不得简写成已经兼容；
- 只有 conformance 出口条件全部满足后，才讨论增加或改名为稳定 Reference
  profile；
- Edge trim、静音过滤等作为显式 preprocessing pipeline，不伪装成另一套“官方”算法；
- 不保留原 `ProvisionalV1` 兼容别名，避免同一生产规则出现两个身份。

### 7.3 解码契约

同步 `PcmSource` 的读取结果必须只有：

```text
Data(chunk)
Eof
Error
```

若未来需要非阻塞模型，应另建明确的异步契约；M0 `PcmSource` 不增加
`Pending`，也不能复用 `Eof`。

`PcmStreamInfo.spec.sample_rate` 永远表示送入分析器的实际 PCM 率。源元数据、
预期帧数和已解码帧数分别存储。M0 不含 DSD 或转码路径。

### 7.4 并发策略

- M0 使用串行文件处理和串行解码作为确定性基线；
- 文件级并行只在 application 层具有统一资源预算和差分测试后恢复；
- 包级并行在重新证明 codec 独立性、EOF 和错误传播前保持关闭；
- 有状态 codec 不按文件扩展名猜测并行安全性；
- application 层为文件任务、decoder worker 和外部进程分配统一预算；
- 若实测证明包级并行收益不足以抵消复杂度，允许直接删除。

## 8. 分阶段实施

逆向研究是一条从现在开始的并行轨道，不是所有重构完成后的最后一步。

### M0：0.2.0 可信主干重建

状态：`DONE`。

目标版本：0.2.0。M0 不再以 0.1.4 补丁修补旧生产路径，而是完成一条可以
持续演进的最小纵向主干。具体决策见
[ADR-0001](adr/0001-m0-0.2.0-trusted-trunk-rebuild.md)。

- 建立 Cargo workspace；`analysis` 与 `codecs` 分别只依赖 `domain`，
  `macinmeter` application 组合二者，CLI/Tauri 只依赖 application；
- 以有效领域类型、具名 `AnalysisReport` 和结构化 `AnalysisError/ErrorCode`
  取代公共 `AppConfig`、长元组和双语字符串错误；
- 建立唯一、串行、无输出副作用的生产分析状态机，先支持经过共同契约验证的
  最小 codec 集；
- 建立 application 层单文件与批处理编排；一个文件的 batch 不再退化为旧
  单文件自动保存路径；
- 重建显式 CLI 操作、JSON schema、stdout/stderr 和全部/部分失败退出语义；
- 让 CLI 与 Tauri 最终消费同一 application API 和 wire DTO；
- 默认关闭包级并行，EdgeTrimmer 不进入生产管线、公共请求或结果 schema；
- 建立 reference 目录与 provisional v1 规格，明确当前尚未具有参考兼容证明；
- 施工期 CI 只保留手动 Ubuntu workspace fmt/clippy/test，pre-commit 只保留
  快速格式与 workspace 编译检查；
- 新纵向路径切换后直接删除旧 API、旧 CLI 适配和重复状态机，不提供 0.1.x
  兼容别名。

出口条件：

- 0.2.0 workspace 具有可检查的单向依赖，生产入口只调用一套分析状态机；
- 默认路径不再有已知静默截断、公开安全 API UB 或 EdgeTrimmer 误删；
- 同一 PCM 对合法 chunk 切分和声道路径满足工程不变量；
- JSON 可直接解析，stdout/stderr、退出码和批量 outcome 有黑盒测试；
- CLI 与 Tauri 共享报告、错误和 application 语义；
- 所有对外兼容性声明不超出 provisional v1 证据。

本地验收记录（2026-07-18）：

- `cargo +1.88.0 fmt --all -- --check`：通过；
- `cargo +1.88.0 clippy --locked --workspace --all-targets -- -D warnings`：通过；
- `cargo +1.88.0 test --locked --workspace`：61 项测试通过；
- `cargo +1.88.0 check --locked -p macinmeter-gui`：通过；
- `npm run build`（`tauri-app/`）：TypeScript 与 Vite production build 通过；
- `npm audit --json`（`tauri-app/`）：0 项已知漏洞；
- 未触发、未等待 GitHub Actions。

### M1：事实与证据基础

状态：`DONE`。

- 固定 reference 目标、实验生成器、运行 harness 与判别 corpus；
- 保存参考观测，不用当前实现反向生成 correctness golden；
- 建立固定 x64 per-track core 的版本化算法规格和证据等级；
- 保留 album 聚合与 renderer 中影响数值的算术、窄化和舍入；
- 将 M0 `ProvisionalV1` 输出仅保存为历史工程 snapshot。

当前 clean-commit successor 已把六组公开字段固定到可由提交源码重建的实现身份，
隔离 core 动态证据也已经补入。M1 不等待 foobar host 的解码、组件注册、
metadata、playlist/album grouping、完整 renderer 或独立 host repeat run；
这些行为不属于固定 analyzer 数值契约。

2026-07-18 至 2026-07-19 的后续检查点补齐了四条证据基础设施：

- observation importer 会从 canonical manifest、实际 fixture bytes、未经修改的
  report 和显式 target/run metadata 重建并复核 path-free 包；它不运行参考插件，
  也不从 MacinMeter 输出反推 golden；
- 固定 x64 二进制的 analyzer/session/channel/result/album 布局已有版本化动态
  探针计划；CDB guarded runner 会绑定输入、触发、进程和完整生命周期，IDA
  模板则只生成 session/result 内部一致、由操作者声明输入身份的 diagnostic；
- 隔离 x64 core parent/worker/suite 固定 PCM、target、runtime、worker、block
  size 和请求身份，每个输入使用全新 worker；固定 39 项 safe-master 已形成
  accepted 动态记录。真实 `shared.dll` 被保留用于 load/unload lifecycle，core
  执行期间 13 个目标普通 IAT 入口均由 fail-fast tripwire 接管；
- 同一 worker 的 schema v2 又一次性完成 38 个高区分度数值向量：duration
  24/24、multichannel weighting track bits 8/8、channel 前提 8/8、配对
  不变量 4/4、histogram endpoint 6/6，全部满足预注册判据。

第三项关闭了 analyzer core “首次受控动态记录”这一缺口，并能保存 raw bits
用于精确比较；它不启动 foobar2000，因此没有把 foobar decode、registration、
metadata、album subsystem、完整 renderer 或 host parity 升级成已验证事实。
第四项把此前仍可能改变 per-track 可见输出、但只有静态证据的 duration 半秒与
长时 token、可选多声道加权、histogram 两端 clamp 交叉为 E2；duration 只执行
固定 numeric leaf，仍不是完整 renderer 动态记录。
这些外围路径被保留为可选研究材料，不是 M1 缺口。album 的
unweighted/weighted/fallback/binary32 窄化与整数显示，以及 renderer 的 report
peak/RMS、DR/dB 与 duration 舍入，仅按纯数值叶子规则进入规格；固定静态数据流
已足以确定公式，不要求人工制作 album playlist 或完整文本报告。

M1 的固定目标、输入域、证据与非目标由
[ADR-0002](adr/0002-m1-reference-numeric-scope.md) 收口。Candidate 继续标记
`Unverified`，表示没有声明任意 PCM 或完整 foobar/component parity；这不否定
“事实与证据基础”里程碑已经完成。

出口条件：

- [x] 关键工程行为都有测试；
- [x] 固定目标的算法事实可从已保存证据重复审计；
- [x] 每条纳入范围的算法结论可追溯到证据，外围行为明确列为非目标。

### M2：可信主干扩展

状态：`DOING`。

M2 不以增加格式数量为完成标准，而是先加固当前可信主干。具体边界与实施顺序见
[ADR-0003](adr/0003-m2-native-decoder-contract-hardening.md)：

- 让 `PcmBlock` 保留构造时使用的声道数，并在 application 边界拒绝
  block/spec channel geometry 不一致；
- 固定 `MAX_ANALYSIS_CHANNELS = 64`：codec 在创建 decoder 前以
  `UnsupportedFormat / Probe` 拒绝超限源，直接 analyzer API 以
  `ResourceExhausted / Analysis` 拒绝超限 session；
- codec crate 已建立可复用的 `PcmSource` contract matrix；WAV/FLAC/AIFF
  各有一条基础合法 fixture 使用同一 harness 验证内容探测、immutable stream
  info、PCM oracle、progress、diagnostics 与 sticky EOF；
- `native-pcm-v1` 产品 corpus 已闭合 classic WAV integer 8/16/24/32、float
  32/64、AIFF signed integer 8/16/24/32 和 stereo multi-block FLAC；manifest
  固定 bytes、normalized PCM、生成方式与许可，AIFF/FLAC 同时通过 API/CLI 共享
  report；
- WAV header 几何和 AIFF 80-bit sample rate 在 probe 层严格核对；
  `WAVE_FORMAT_EXTENSIBLE` 与非零 AIFF SSND offset/block-size 在形成独立证据前
  明确为 unavailable；
- 当前 Symphonia source 使用单一 terminal-state enum，并通过独立、可注入故障的
  harness 验证 sticky error；失败 block 只有在完整校验通过后才提交帧计数；
- application、CLI、Tauri 继续分别保留各自层级的集成测试；
- 每条 route 继续覆盖自身可稳定构造的损坏输入；
- 扩充 bit-exact chunk、声道 lane、排列和失败事务性工程不变量；
- 建立固定 malformed corpus 与独立手动 fuzz 入口；
- 审计 report/progress 裸浮点与反序列化入口，收紧 domain 有效性边界；
- 在首次新增 route 前建立单一 Rust capability catalog；
- 只有通过共同契约、跨 adapter 和文档同步验收的原生 Symphonia route 才能
  标为 stable。

Candidate 在 M2 冻结；只有新的充分静态/动态证据、最终反例或实现转写缺陷，才
按 ADR-0002 重新打开。只有规格语义变化提升 profile version，修复既有规格的
实现错误不制造新 profile。

第二 backend、Opus/FFmpeg 与全局资源预算属于 M3；benchmark、是否启用文件级
并发、SIMD 和其他性能优化属于 M6。EdgeTrimmer 和其他 preprocessing 没有需求
时不实施，有需求时另立独立 ADR。

出口条件：

- application decode 路径的 block/spec channel geometry mismatch 不可能静默
  进入 analyzer；
- 超过 64 声道的媒体在 decoder creation 前返回
  `UnsupportedFormat / Probe`，直接 session API 返回
  `ResourceExhausted / Analysis`；
- 当前所有正式 route 满足共同 `PcmSource` contract matrix 及其已声明位深矩阵，
  当前 source 实现另行满足 sticky terminal-state harness；
- 合法 chunk 切分、lane 隔离和声道映射满足完整结果 raw-bit 工程不变量；
- 固定 malformed corpus 逐例在独立 timeout 下不产生 panic、超时或 partial
  success；资源限制按平台实际能力记录，不外推为全部字节输入的证明；
- 新能力不绕过 M0 建立的 application/wire 边界，支持矩阵不在 Rust、GUI 与
  文档之间漂移；
- 单一 Rust capability catalog 驱动 discovery、application query 与 GUI
  picker；
- M1 派生出的普通产品 numeric boundary/regression tests 保持通过；历史
  observation/conformance artifact 不要求日常重跑。

### M3：多 backend 与资源编排

- 只有出现明确的外部或多 backend 产品需求时，才引入显式 `DecodePlan` 与
  backend registry；
- 对实际选中的外部 decoder 建立完整进程 supervisor 契约，包括启动、取消、
  timeout、stderr、退出状态、回收和失败传播；Opus/FFmpeg 只是候选，不是承诺；
- 在 application 层建立统一 CPU、内存和任务预算及调度接口，M3 产品执行仍保持
  串行；如果 M6 后续由 profile 证明需要并发，唯一允许的轴是 application
  文件级并发；
- 保持 M0 已完成的请求级取消、job 隔离以及 CLI/Tauri 薄 adapter 边界；
- capability catalog 必须反映 backend 的编译时和运行时真实可用性。

出口条件：

- 所有实际启用的 backend 满足相同的 `PcmSource` 语义和跨层验收；
- 外部进程失败不会被视为正常 EOF；
- 所有 backend 和任务服从统一资源预算，取消和失败互不污染；
- 支持格式列表反映运行时真实能力。

### M4：参考兼容声明收口

- 完成并审查 `FooDrMeter108CandidateV1`，只实现 REF 轨道有证据支持的规则；
- 对齐窗口、RMS、量化、Peak、20%、舍入和多声道聚合；
- 已将产品 PCM 入口改为 `f64`，关闭 complete-v2 暴露的两处 source-f64
  量化边界差分；
- schema v3 已分离并对齐公开 overall channel RMS、track RMS、primary peak 与
  duration token；
- 显式 `AlbumAggregator` 已实现静态 E1 完整公式，但不把 batch 自动解释成
  album；DR0 纳入子规则由静态路径与 footer 反事实达到 E2，精确 internal mean、
  length weighting 与 host metadata 不随之升级；
- 扩展最终可观测数值 conformance，并复核 album/renderer 的纯数值边界；
- 处理所有系统性残差和未解释边界；
- 完成固定目标、固定数值字段范围内的兼容性报告；不追求 host 或文本 parity。

出口条件：

- 满足第 6.5 节全部标准；
- 不再存在“已知有偏但归因不明”的结果族；
- 参考 profile 可独立于 decoder、chunk 和任何实际存在的可选执行路径验证。

### M5：产品与仓库收敛

- 在 M0 建立的 Cargo workspace 上收紧依赖和 feature 边界；
- 统一 CLI、GUI、版本、MSRV、lockfile 和 release；
- 修订用户文档、支持格式和兼容性声明；
- 清理遗留脚本、无效依赖和过期构建文件；
- 建立制品 smoke test 和 checksum。

### M6：重新性能工程

- 删除未实现 AVX/AVX2 的理论加速报告；
- 使用可复现 benchmark 重新 profile；
- 根据 profile 决定是否需要任何优化，包括文件级并行或函数级 SIMD；包级并行
  除非出现新的必要性证据，否则维持删除状态；
- 只优化已确认瓶颈；
- 性能路径必须通过与标量参考路径的差分测试。

## 9. 实施顺序记录

M0 作为一次明确的 breaking branch 完成前七项并整体切换，不发布中间双轨：

1. 接受 0.2.0 ADR 并建立 reference 目录；
2. 建立 workspace/domain；
3. 建立唯一 AnalyzerSession；
4. 建立最小 codecs/application；
5. 替换 CLI/JSON/退出码；
6. 迁移 Tauri；
7. 删除 legacy 生产路径。

M1 已按证据独立完成。M2 继续使用小步纵向提交，但不以增加格式数量作为进度：

8. [x] `fix: bind PCM blocks to their channel geometry`
9. [x] `fix: enforce analyzer session resource limits`
10. [x] `test: establish the shared PcmSource contract matrix`
11. [x] `test: close the declared native PCM matrix`
12. [ ] `test: expand bit-exact analyzer invariants`
13. [ ] `refactor: enforce valid domain result construction`
14. [ ] `test: add malformed media regression corpus`
15. [ ] `refactor: centralize native codec capabilities`
16. [ ] `feat: graduate evidence-backed native routes`（只有实际通过准入时）

多 backend/application 资源预算继续属于 M3；文件级并发是否启用与其他可复现
性能工程继续属于 M6。

每项后续提交应包含对应测试、证据链接和验收说明。

## 10. 小项清理清单

以下事项不决定总体架构，但同样必须处理：

- [x] 设置准确且实际验证的 `rust-version = 1.88`；
- [x] 统一 workspace、Tauri Cargo、package.json、tauri.conf 和 lockfile 版本；
- [x] 手动 CI 使用根 lockfile 与 `--locked`；
- [x] 删除旧 `panic = "abort"` / `catch_unwind` 组合；
- [x] 删除非测试代码中的无保护 `expect`；
- [x] 删除线程优先级控制；
- [x] 删除旧 benchmark 依赖和脚本；
- [x] 删除 `hound`；
- [x] 明确 Symphonia `default-features = false` 与四个 M0 feature；
- [x] 删除 reset/Pending 契约，进度统一为实际解码 frame；
- [x] M0 不含 FFmpeg；本地非 UTF-8 `Path` 仍直接传给原生 decoder；
- [x] 清理根目录孤立 lockfile、`compile_commands.json` 等生成物；
- [x] 修复 Tauri 严格 Clippy，并更新命令/权限/批处理文档；
- [x] 删除过期 bundle/benchmark 脚本并缩减 pre-commit；
- [x] 删除 M0 的 audit 忽略列表；审计恢复时重新建立可追踪策略；
- [x] 删除 legacy 弱测试，以明确工程不变量和 CLI/codec contract 测试取代；
- [x] M0 无 ignored 测试；
- [x] 校正文档中的内存、性能和兼容性过度表述；
- [x] 删除重复 SIMD/Peak 状态和伪性能估算。

## 11. 当前建议决策

以下是本次审查后的建议，后续可以通过 ADR 正式确认：

| 决策 | 建议 |
| --- | --- |
| 产品核心 | 可信的离线 DR 分析库和 CLI；GUI 为薄适配层 |
| 默认算法 | 当前使用 `FooDrMeter108CandidateV1 / Unverified`；只有完成 conformance 后才升级声明 |
| 增强功能 | Edge trim、静音过滤等与参考算法分离并显式启用 |
| 默认并发 | M0 使用确定性串行基线；后续只由 application 的统一预算恢复并发 |
| 格式承诺 | 区分原生稳定、外部依赖、实验性和不可用 |
| 公共 API | 0.2.0 重置 Rust API、CLI、JSON 和 GUI IPC；不保留 0.1.x 兼容层 |
| 发行 CPU | portable baseline；任何函数级加速由 M6 profiling 与差分证据决定 |
| 性能判断 | 只接受可复现实测，不接受硬件能力推导的理论倍数 |
| 重构方式 | 小步纵向迁移；生产切换后立即删除旧路径 |

## 12. 待确认事项

- [x] 当前参考目标锁定 `foo_dr_meter 1.0.8`；其他版本必须建立独立目标记录；
- [x] 黑盒观测已分别记录 foobar2000 2.0 x86 与 2.25.10 x64、插件配置和目标 hash；
- [x] 当前采用 Windows 环境半自动采集并保存原始报告；
- [x] 可公开生成 fixture、v1/v2 manifest 与 x86/x64 observation 已按证据目录分层；
- [x] `ProvisionalV1` 不作为生产兼容 profile 保留；
- [x] M0 第一批稳定矩阵固定为 WAV PCM integer/IEEE float、FLAC、AIFF PCM integer；
- [x] Reference profile 的未导出中间状态已有固定 x64 布局与 ASLR-safe 探针计划；
- [x] 首次受控 analyzer-core 动态记录已按固定 target/runtime/worker/input 身份验收；原始 core bits 使用精确比较，外围 host/album subsystem/完整 renderer 为非目标，只有纯数值投影规则继续留档；
- [x] Candidate 结果结构使用 wire schema v3；schema 版本只表示结构契约，不表示算法兼容。
- [x] M1 不要求 production/reference 中间状态同构；album/renderer 只纳入纯数值规则，host、playlist、metadata 与文本 parity 明确排除。

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

本节保存建立路线图时对 **0.1.3 legacy 主干** 的验证状态，避免后续只看到任务
结论而失去问题证据。下列行为和 advisory 已被 M0 主干取代，不能当作 0.2.0 的
当前能力或兼容性证据。它不是永久测试报告；每个事项关闭时仍需提供新的验证结果。

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

### 14.3 Legacy 基线源码入口

下表记录 0.1.3 审查时的历史路径。M0 切换生产主干后这些文件可以删除，因此
这里使用路径文本而不是指向当前工作树的链接。

| 主题 | 0.1.3 历史入口 |
| --- | --- |
| 同步流式契约 | `src/audio/streaming.rs` |
| 解码路由与并行状态机 | `src/audio/universal_decoder.rs` |
| 并行包重排 | `src/audio/parallel_decoder.rs` |
| FFmpeg 生命周期 | `src/audio/ffmpeg_bridge.rs` |
| 窗口/直方图状态 | `src/core/histogram.rs` |
| 公共 DR Calculator | `src/core/dr_calculator.rs` |
| 生产分析路径 | `src/tools/processor.rs` |
| 样本转换 unsafe 包装 | `src/processing/sample_conversion.rs` |
| SIMD 安全包装 | `src/processing/simd_core.rs` |
| EdgeTrimmer | `src/processing/edge_trimmer.rs` |
| CLI 配置与横幅 | `src/tools/cli.rs` |
| 批处理退出行为 | `src/main.rs` |
| Tauri 状态与命令 | `tauri-app/src-tauri/src/lib.rs` |
| CPU 构建标志 | `.cargo/config.toml` |
| CI 和 release | [`.github/workflows/ci-cd.yml`](../.github/workflows/ci-cd.yml) |
