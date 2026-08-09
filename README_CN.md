# MacinMeter DR Tool

[English](README.md) | [中文](README_CN.md)

MacinMeter 是一款离线、本地优先的音频动态范围（DR）分析工具。它为受支持的
WAV、FLAC、AIFF 与 MP4/M4A ALAC 文件报告逐声道和逐轨 DR。命令行工具、Tauri
桌面前端与 Rust API 共用同一套流式分析引擎。

分析算法来自对一个固定 `foo_dr_meter 1.0.8 x64` 目标的重建。已记录 projection
在固定 conformance corpus 上差分为零；准确性章节列出了对应的输入、字段和运行
边界。

## 支持格式

| 容器 | 当前稳定支持 |
| --- | --- |
| RIFF/WAVE（经典或受限 WAVE_FORMAT_EXTENSIBLE） | 8/16/24/32-bit 整数 PCM；IEEE 32/64-bit float |
| 原生 FLAC | 声明非零总样本数的 FLAC |
| AIFF | 8/16/24/32-bit 整数 PCM |
| 非 fragmented MP4/M4A | ALAC version 0、16/24-bit、1–8 个标准布局声道 |

显式文件路径按内容探测，可以使用任意扩展名。文件夹扫描会寻找 `.wav`、`.wave`、
`.flac`、`.aif`、`.aiff`、`.m4a` 与 `.mp4`；其他后缀的受支持文件仍然可以通过
直接传入路径分析。
产品分析最多接受 64 声道，但当前 Symphonia WAV backend 对经典与
WAVE_FORMAT_EXTENSIBLE 输入均只能表示 1–26 声道。受限 Extensible 路径的
channel layout 保持 unknown；完整 valid-bit 与 mask 规则见支持格式文档。

一些具有常见扩展名的文件采用了当前尚未支持的变体，例如 padded 或 valid bits
未指定的 WAVE_FORMAT_EXTENSIBLE、超过 26 声道的 Extensible、RF64/BW64、AIFC、
Ogg FLAC、fragmented MP4，以及带视频或额外 track 的 MP4。AAC、MP3、ALAC
20/32-bit 或非标准布局变体、Vorbis、Opus 和 DSD 目前也不可用。
MacinMeter 会把它们报告为不支持，不会调用 FFmpeg，也不会静默重采样或预处理。
更完整的 route 细节记录在
[支持格式](docs/SUPPORTED_FORMATS_CN.md)。

## 安装

当前从源码构建使用 Rust 1.88 或更新版本以及 Cargo：

```bash
cargo build --locked --release -p macinmeter-cli
```

Unix 类系统的 CLI 位于 `target/release/mdrmeter`，Windows 上则是
`target/release/mdrmeter.exe`。

## 分析音频

CLI 围绕显式命令组织：

```text
mdrmeter analyze FILE [--format human|json] [--output PATH]
mdrmeter batch INPUT... [--recursive] [--format human|json] [--output PATH]
mdrmeter completions SHELL
```

例如：

```bash
mdrmeter analyze "01 - Song.flac"
mdrmeter batch "My Album/" --recursive
```

`batch` 无论文件以什么顺序完成，都按稳定输入顺序输出。某一项失败不会阻止后续项目
继续运行。它返回相互独立的逐轨报告，不会隐式计算 album DR。stderr 上的进度行会跨
条目交错，每行都标明所属条目。

Shell 补全由解析器本身生成，因此永远与打印它的那个构建一致：

```bash
mdrmeter completions zsh > "${fpath[1]}/_mdrmeter"          # zsh
mdrmeter completions bash > ~/.local/share/bash-completion/completions/mdrmeter
mdrmeter completions fish > ~/.config/fish/completions/mdrmeter.fish
```

支持 `bash`、`zsh`、`fish`、`powershell` 与 `elvish`。该命令只写 stdout；补全脚本
装到哪里由调用方决定。

[`ADR-0014`](docs/adr/0014-deterministic-decode-analysis-pipeline.md) 已接受有界的
packet、文件与窗口级并行，每条轴都必须各自通过正确性、资源与性能门禁后才启用。
route-specific packet 解码、解码-分析重叠与批量 file lane 已通过并在 0.3.0 中启用；
窗口级并行尚未实现。不提供公开的线程、batch size 或队列控制，也不发布任何吞吐数字。

下面是仓库内固定合成 fixture 实际产生的 stdout：

```bash
target/release/mdrmeter analyze tests/fixtures/edge_cases.wav
```

```text
MacinMeter
Source: tests/fixtures/edge_cases.wav
PCM: 44100 Hz, 2 channels, 308700 frames
Duration: 0:07

CH 1: DR2 (2.4300 dB), overall RMS -2.43 dBFS, selected DR peak 1.00000000
CH 2: DR2 (2.4300 dB), overall RMS -2.43 dBFS, selected DR peak 1.00000000

Track aggregate: DR2 (2.4300 dB; 2 contributing channels)
Report levels: peak 0.00 dBFS, RMS -2.43 dBFS

Elapsed: 0.002 s (2929.6x realtime)
```

该命令的进度信息会另外写入 stderr。

### 如何理解结果

- `DR2` 是整数 track aggregate。在这一度量内，数值越大
  表示 selected peak 与 loud-window RMS 的比值越大。DR 高本身不代表录音质量
  好；DR 很低则往往是强压缩的负面信号，更可能对应表现受损的母带，不过音乐
  类型和创作意图仍然会影响判断。
- 每条 `CH` 包含该声道的 DR、overall RMS 和 DR 状态机选择的 peak。
- `Report levels` 是全轨报告指标；其中的 report peak 与 selected DR peak
  不是同一个量。
- dBFS 以归一化幅度 `1.0` 作为 0 dB 参考。受支持的 IEEE float PCM 可以包含
  高于该参考的有限样本，所以 0 dBFS 不是普适的削波边界。
- 当前算法会显式保留静音声道并让其贡献数值 DR0；数据不足的声道则明确排除。
- `Elapsed` 是本次运行在本机的耗时，其后的实时倍率是解码音频秒数除以该墙钟
  时间。两者描述的是主机与当次运行，而不是分析本身，因此只出现在人类可读输出
  里；JSON 仍然是输入的纯函数，两次运行逐字节相同。
- `--timing` 会为每个角色额外输出一行：

  ```text
  decode   active 0.230 s · other 0.005 s (span 0.235 s)
  analysis active 0.203 s · other 0.032 s (span 0.235 s)
  ```

  对单个文件，一个角色的 span 从它第一次活动持续到最后一次，`active + other`
  恰好还原该 span，在显示到毫秒时也保持闭合。**这些数字能划分的只有这一个
  span。** `other` 是未落在该角色计时调用内、无法进一步归类的时间，可能包括 block
  交接、进度事件、串行路径上的另一个角色以及主机调度；它本身不能判定瓶颈。
  **两个 span 之间可能重叠，重叠多少无法从这些数字还原** —— 两组区间各自的总长
  不决定它们的交集。因此它们不可相加、不可当作总耗时的百分比、也不可反演成串行
  占比。

  batch 输出会改用 `active total`、`other total` 和 `item spans` 标签。这些数字是各
  文件对应值的总和，不是整个 batch 从第一次到最后一次活动的单一区间。不同 file
  lane 上，同一角色的文件区间也可能互相重叠，因此区间总和可以超过 batch 总耗时。

  每个计时间隔都要在开始和结束各读一次时钟；普通数据 block 的解码与分析各读两次，
  共四次。因此它需要显式开启；不加该 flag 时，分析器不会进行这些按 block 的时钟
  读取。两种方式的分析结果完全相同。

上述 fixture 用于确定性自动化测试，不代表典型音乐发行物。

### 退出码

| 代码 | 含义 |
| ---: | --- |
| `0` | 所有请求均成功 |
| `1` | 失败、无输入、batch 全部失败或输出写入失败 |
| `2` | 命令行参数错误 |
| `3` | batch 同时存在成功与失败 |
| `130` | 已取消 |

### 保存与 JSON

没有 `--output` 时，结果留在 stdout，不会创建报告文件。提供输出路径后，完成的
报告会原子替换该文件。

```bash
mdrmeter analyze track.flac --format json
mdrmeter analyze track.flac --format json --output track.json
```

JSON 与 Tauri 使用同一套带版本的 schema-v4 `WireEnvelope`。信封包含
`schemaVersion`、`toolVersion`、`kind` 与 `data`，不包含时间戳。成功结果中的
数值都是有限值；零振幅 dBFS 等情况会在相应位置显式表示为 `null`。stdout
只包含请求结果，进度和诊断进入 stderr。

## 桌面 GUI

仓库包含 Tauri 2 桌面前端源码：

```bash
cd tauri-app
npm install
npm run tauri dev
```

GUI 调用与 CLI 相同的 `Application` façade，并消费相同的 wire schema。每个
job 拥有独立取消 token，共享 application 预算则保证顶层工作有界且串行。

0.3.0 为两个平台打包 CLI 与 GUI：运行 macOS 11.0 或更新系统的 Apple Silicon Mac，
以及 Windows x64。两个平台各自在自己的主机上 staging —— macOS 产出 DMG，Windows
产出 NSIS 安装包 —— 本地 staging 与有界 CI 门禁都会构建并**打开**最终制品做结构化
验证：DMG 会被挂载并检查其中的 `.app`，安装包会被解包并核对内层
`macinmeter-gui.exe` 实测为 x86_64 PE、版本资源匹配且没有 Authenticode 签名；
外层 installer 也必须是未签名 PE。解包目录位于 candidate 目录之外。

两个安装包都不签名。macOS 没有 Developer ID 签名也未经公证，因此可能要求用户显式
选择“打开”或“仍要打开”；Windows 没有 Authenticode 签名，因此 SmartScreen 会提示
未知发布者。这在两个平台上是同一个决定，而不是某一边的疏漏：面向个人的代码签名
证书会把维护者的法定姓名嵌入每一份公开制品。

Intel/universal macOS、ARM64 Windows 与 Linux GUI 包不属于 0.3.0 发行范围。
目前的打包情况汇总在[发行与制品状态](docs/RELEASE_CN.md)。

## Rust API

workspace 的公共门面是 `macinmeter` crate。它尚未发布到 crates.io，请按 tag 依赖：

```toml
[dependencies]
macinmeter = { git = "https://github.com/SakuzyPeng/MacinMeter-DynamicRange-Tool", tag = "v0.3.0" }
```

manifest 已经为注册表发布做好准备 —— 四个库 crate 都带 description，并把同 workspace
的兄弟依赖钉在 workspace 版本上 —— 但在公开面稳定之前，发布是刻意推迟的。真要发布时
必须按 `macinmeter-domain`、`macinmeter-analysis`、`macinmeter-codecs`、`macinmeter`
的顺序上传：每个 crate 的依赖都必须已经在注册表上，因此没有任何 dry run 能提前验证
整条链路。

```rust
use macinmeter::{AnalyzeRequest, Application};

fn main() -> Result<(), macinmeter::AnalysisError> {
    let application = Application::new();
    let report = application.analyze_file(AnalyzeRequest::new("track.flac"))?;

    if let Some(dr) = report.analysis().aggregates().track.rounded_dr {
        println!("DR{dr}");
    }
    Ok(())
}
```

同一个 `Application` 的 clone 共享有界顶层执行队列，当前只准入一个 active job；
分别构造的 `Application` 彼此独立，队列大小可以通过
`Application::with_budget` 配置。未来 ADR-0014 内部 worker 仍必须受该
application 执行域统一管理。

已经持有有限、帧对齐、交错 `f64` PCM 的调用者可以直接使用
`AnalyzerSession`。`AlbumAggregator` 是针对逐轨报告的独立数值操作，支持
unweighted 与 decoded-duration weighting。playlist 分组、metadata、footer
渲染和其他 album 子系统内容属于这项数值 API 之外的层面。

## 准确度

当前目标是哈希前缀为 `ff3556ad…` 的固定 `foo_dr_meter 1.0.8 x64` 二进制。
针对固定记录输入，仓库保存了以下有界结果：

| 证据 | 已记录结果 |
| --- | --- |
| schema-v3 safe-master 的 track DR、overall peak、overall RMS 与渲染时长 | 各 39/39 |
| 同一 run 的 channel DR 与 channel RMS | 各 62/62 |
| decoder-independent direct-PCM final-field projection | 固定 39 项输入差分为 0 |
| 隔离 x64 analyzer-core 运行 | 39 项输入的预注册断言全部满足 |
| duration、weighting 与 histogram 端点数值边界向量 | 24/24、8/8、6/6 |

这张表描述的是一个指定 target、corpus、字段集合和运行边界。任意音频、x86
及其他插件版本、foobar2000 解码、宿主与 playlist 行为、metadata 来源、完整
文本渲染和内部实现状态都不在这些观测之中。

相应记录包括 [M4 证据矩阵](docs/M4_X64_NUMERIC_CLAIM_MATRIX.md)、
[M4 数值对齐报告](docs/M4_X64_NUMERIC_ALIGNMENT_REPORT.md)与
[算法规格](reference/specs/foo-dr-meter-1.0.8-candidate-v1.md)。

## 性能

分析采用流式状态：分析器内存随声道数增长，不随曲目时长增长。当前公开的性能
材料是一组本机测量记录，而不是通用的吞吐或内存数字。

M6 在一台固定 Apple M4 Pro、固定工具链、生成 corpus 与合成 workload 上记录了
release worker 测量。其中的音频 case 在该主机上快于实时，验收的优化在保持结果
fingerprint 不变的前提下降低了固定 8/64 声道 analyzer 中位耗时。不同机器、格式、
声道数和输入的实际速度与内存占用仍会不同；这些数字是一份本机基线，并不预测其他
环境的表现。

可复现测量脚本及其解读集中在
[性能说明](docs/BENCHMARKS_CN.md)与[M6 记录](docs/performance/README.md)。

## 底层设计

MacinMeter 0.3.0 是一个单向依赖的 virtual Cargo workspace：

```text
macinmeter-domain
├── macinmeter-analysis
├── macinmeter-codecs
└── macinmeter
    ├── macinmeter-cli
    └── macinmeter-gui
```

所有第一方 Rust crate 都使用 `#![forbid(unsafe_code)]`。当前产品只有一个分析器
实现。在同一份 application 自有的 worker 与内存计划下，它以 packet worker 解码
ADR-0013 的 ALAC route 与可证明有界的 FLAC 流，在 route 未花掉的 permit 上重叠
解码与分析，并按同一 plan 推导的 file lane 运行批量条目；单个文件独占整个 decoder，
窗口级并行尚未实现。无论走哪条路径结果都相同。ADR-0014 只允许通过逐 route/逐轴
毕业的有界确定性内部并行，不恢复已删除的 0.1.x 并行 decoder。设计历史与进一步技术
资料集中在：

- [架构与参考对齐路线图](docs/ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md)
- [架构决策记录](docs/adr/)
- [支持格式](docs/SUPPORTED_FORMATS_CN.md)
- [性能说明](docs/BENCHMARKS_CN.md)
- [发行与打包说明](docs/RELEASE_CN.md)

## 参考研究与致谢

当前唯一候选目标是 Janne Hyvärinen 的 `foo_dr_meter 1.0.8 x64` 组件。对该固定
目标的逆向研究已获得作者许可。私人信件不保存在仓库中，只保留
[最小公开授权摘要](reference/authorization/README.md)。

授权与致谢提供了这项研究的法律和历史背景；数值结论来自上面的有界记录。历史
M0/1.0.3 材料作为已取代的档案保留，与当前 target 分开。目标身份、实验、观测、
规格及其限制统一索引在 [`reference/`](reference/README.md)。

## 许可证

MacinMeter 使用 [MIT License](LICENSE)。相关材料集中在
[法律说明](docs/LEGAL_CN.md)与[第三方声明](THIRD_PARTY_NOTICES.md)。
