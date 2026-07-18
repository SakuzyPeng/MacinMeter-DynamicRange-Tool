# 架构整改与参考插件重新对齐路线图

> 状态：执行中（M0：`DONE`，foo_dr_meter 1.0.8 Candidate V1 已实施；schema-v3
> x64 safe-master 的 track DR 39/39、channel DR 62/62、overall peak 39/39、
> overall RMS 39/39、channel RMS 62/62、duration 39/39，验收范围待扩充），
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
| REF-003 | DOING | 建立可重复的参考运行 harness | 固定输入、半自动运行、原始输出采集 |
| REF-004 | DONE | 建立合成 PCM 实验生成器 | 可精确控制窗口边界、幅度、峰值顺序和多声道 |
| REF-005 | DOING | 完成黑盒行为矩阵 | x86 15 项与 x64 39 项 safe master 已登记；isolated 属 host 外围，x64 repeat 是否进入 accepted 仍待定 |
| REF-006 | DOING | 开展静态与动态逆向 | x64/x86 核心、album/report、WAV decoder/metadata 已有静态记录；动态中间状态未跟踪 |
| REF-007 | DOING | 编写版本化算法规格 | Candidate 已纳入 x64 精度、短时 `m:ss`、已观测 ordinal `0..5, 9, 10` 标签与 album DR0 E2 子规则；未覆盖 renderer 分支继续保留 |
| REF-008 | DONE | 实现 Candidate profile | 唯一 f64 生产 profile；schema v3 的六组公开 DR/report/duration 字段完全匹配，仍不宣称参考兼容 |
| REF-009 | DOING | 建立参考 conformance suite | clean-commit successor 已覆盖六组字段、四项 footer consistency 与 DR0 反事实；中间状态、精确 album/weighting 与 host metadata 未验收 |
| REF-010 | TODO | 修订兼容性声明 | 仅在验收通过后恢复明确的参考兼容承诺 |

### 5.4 P2：测试、发布、性能和维护

| ID | 状态 | 事项 | 目标 |
| --- | --- | --- | --- |
| TEST-001 | DONE | 建立 M0 工程不变量测试 | chunk、声道、窗口边界、有限值、长流有界内存 |
| TEST-002 | DOING | 建立固定参考 observation corpus | x64 39-track single pass 已固定并可规范化；accepted oracle、动态中间状态及可选 repeat policy 待验收 |
| TEST-003 | DONE | 建立 CLI 黑盒测试 | stdout/stderr、JSON、0/1/2/3/130、原子输出 |
| TEST-004 | TODO | 后续引入 sanitizer/fuzz | M0 已无第一方 unsafe；重点转为 decoder/parser 异常输入 |
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

状态：`DOING`。

- 填充 reference 目标档案、实验生成器和运行 harness；
- 保存第一批参考观测，不用当前实现反向生成 correctness golden；
- 建立算法规格模板和证据等级；
- 将 M0 `ProvisionalV1` 输出仅保存为历史工程 snapshot。

当前 clean-commit successor 已把六组公开字段固定到可由提交源码重建的实现身份，
但 M1 不因此自动完成：中间状态仍没有动态证据；若最终 accepted policy 要求参考
runtime 重复性，还必须补同一 x64 target 的独立 repeat run。

出口条件：

- 关键工程行为都有测试；
- 参考插件结果可重复采集；
- 每条算法结论可追溯到证据或明确标为未知。

### M2：可信主干扩展

- 将剩余稳定 codec/backend 迁移到 M0 建立的共同 `PcmSource` 和 PCM 契约；
- 扩充 chunk、声道、scalar/SIMD 和异常输入工程不变量；
- 根据 reference 证据校正 Candidate 算法，不用 legacy snapshot 反向定义正确性；
- 如仍有产品需求，将 EdgeTrimmer 重写为独立、显式 preprocessing stage。

出口条件：

- 所有正式声明支持的 backend 满足共同契约；
- chunk、声道和优化路径不影响 Candidate 结果；
- 新能力不绕过 M0 已建立的 application 和 wire 边界。

### M3：解码与应用层

- 如多 backend 的实际需求成立，引入显式 `DecodePlan` 与 backend registry；
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

- 完成并审查 `FooDrMeter108CandidateV1`，只实现 REF 轨道有证据支持的规则；
- 对齐窗口、RMS、量化、Peak、20%、舍入和多声道聚合；
- 已将产品 PCM 入口改为 `f64`，关闭 complete-v2 暴露的两处 source-f64
  量化边界差分；
- schema v3 已分离并对齐公开 overall channel RMS、track RMS、primary peak 与
  duration token；
- 显式 `AlbumAggregator` 已实现静态 E1 完整公式，但不把 batch 自动解释成
  album；DR0 纳入子规则由静态路径与 footer 反事实达到 E2，精确 internal mean、
  length weighting 与 host metadata 不随之升级；
- 建立 final + intermediate conformance suite；
- 处理所有系统性残差和未解释边界；
- 完成兼容性报告。

出口条件：

- 满足第 6.5 节全部标准；
- 不再存在“已知有偏但归因不明”的结果族；
- 参考 profile 可独立于 decoder、chunk 和 SIMD 路径验证。

### M5：产品与仓库收敛

- 在 M0 建立的 Cargo workspace 上收紧依赖和 feature 边界；
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

## 9. 实施顺序记录

M0 作为一次明确的 breaking branch 完成前七项并整体切换，不发布中间双轨：

1. 接受 0.2.0 ADR 并建立 reference 目录；
2. 建立 workspace/domain；
3. 建立唯一 AnalyzerSession；
4. 建立最小 codecs/application；
5. 替换 CLI/JSON/退出码；
6. 迁移 Tauri；
7. 删除 legacy 生产路径。

后续建议按证据和能力独立提交：

8. `test: add reference observations and conformance harness`
9. `feat: add one evidence-backed backend behind the PcmSource contract`
10. `feat: implement and verify the reference plugin profile`
11. `chore: restore reproducible multi-platform CI and release artifacts`
12. `perf: re-profile and selectively restore proven optimizations`

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
| 发行 CPU | portable baseline + 函数级运行时 SIMD |
| 性能判断 | 只接受可复现实测，不接受硬件能力推导的理论倍数 |
| 重构方式 | 小步纵向迁移；生产切换后立即删除旧路径 |

## 12. 待确认事项

- [x] 当前参考目标锁定 `foo_dr_meter 1.0.8`；其他版本必须建立独立目标记录；
- [x] 黑盒观测已分别记录 foobar2000 2.0 x86 与 2.25.10 x64、插件配置和目标 hash；
- [x] 当前采用 Windows 环境半自动采集并保存原始报告；
- [x] 可公开生成 fixture、v1/v2 manifest 与 x86/x64 observation 已按证据目录分层；
- [x] `ProvisionalV1` 不作为生产兼容 profile 保留；
- [x] M0 第一批稳定矩阵固定为 WAV PCM integer/IEEE float、FLAC、AIFF PCM integer；
- [ ] Reference profile 的未导出中间状态如何观测，以及相应数值容差如何定义；
- [x] Candidate 结果结构使用 wire schema v3；schema 版本只表示结构契约，不表示算法兼容。

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
