# MacinMeter 技术说明

[English](INTERNALS.md) | [中文](INTERNALS_CN.md)

面向基于 MacinMeter 开发、复现其测量或阅读源码的读者。日常使用见
[README](../README_CN.md)。

## 从源码构建

需要 Rust 1.88 或更新版本以及 Cargo：

```bash
cargo build --locked --release -p macinmeter-cli
```

CLI 位于 `target/release/mdrmeter`，Windows 上是 `target/release/mdrmeter.exe`。

桌面前端是一个 Tauri 2 应用：

```bash
cd tauri-app
npm install
npm run tauri dev
```

它调用与 CLI 相同的 `Application` façade，消费相同的 wire schema。每个 job 拥有
独立取消 token，共享 application 预算则保证顶层工作有界且同一时刻只有一个 active job。

## JSON 输出

```bash
mdrmeter analyze track.flac --format json
mdrmeter analyze track.flac --format json --output track.json
```

没有 `--output` 时结果留在 stdout，不会创建报告文件。提供输出路径后，完成的报告会
原子替换该文件。

CLI 与 GUI 输出同一套带版本的 schema-v4 `WireEnvelope`，包含 `schemaVersion`、
`toolVersion`、`kind` 与 `data`，不含时间戳。成功结果中的数值都是有限值；零振幅
dBFS 等情况会在相应位置显式表示为 `null`。stdout 只包含请求结果，进度和诊断进入
stderr。

该信封是输入的纯函数：同一文件两次运行的序列化结果完全相同。因此墙钟计时永远不会
进入信封，只出现在人类可读输出中。

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
分别构造的 `Application` 彼此独立，队列大小可以通过 `Application::with_budget`
配置。内部 worker 始终受该 application 执行域统一管理。

已经持有有限、帧对齐、交错 `f64` PCM 的调用者可以直接使用 `AnalyzerSession`。
`AlbumAggregator` 是针对逐轨报告的独立数值操作，支持 unweighted 与
decoded-duration weighting。playlist 分组、metadata、footer 渲染和其他 album
子系统内容属于这项数值 API 之外的层面。

## Workspace

MacinMeter 0.3.0 是一个单向依赖的 virtual Cargo workspace：

```text
macinmeter-domain
├── macinmeter-analysis
├── macinmeter-codecs
└── macinmeter
    ├── macinmeter-cli
    └── macinmeter-gui
```

所有第一方 Rust crate 都使用 `#![forbid(unsafe_code)]`。

## 内部并行

产品只有一个分析器实现。在同一份 application 自有的 worker 与内存计划下，它以
packet worker 解码 ALAC route 与可证明有界的 FLAC 流，在 route 未花掉的 permit 上
重叠解码与分析，并按同一 plan 推导的 file lane 运行批量条目。单个文件独占整个
decoder，窗口级并行尚未实现。

无论走哪条路径结果都相同。每条轴都是有界且确定的，并且只有在被证明不改变结果之后，
才逐 route、逐轴地启用。不提供公开的线程、batch size 或队列控制，也不发布任何吞吐
数字。

## 准确度

参考目标是哈希前缀为 `ff3556ad…` 的固定 `foo_dr_meter 1.0.8 x64` 二进制。针对固定
记录输入，仓库保存了以下有界结果：

| 证据 | 已记录结果 |
| --- | --- |
| schema-v3 safe-master 的 track DR、overall peak、overall RMS 与渲染时长 | 各 39/39 |
| 同一 run 的 channel DR 与 channel RMS | 各 62/62 |
| decoder-independent direct-PCM final-field projection | 固定 39 项输入差分为 0 |
| 隔离 x64 analyzer-core 运行 | 39 项输入的预注册断言全部满足 |
| duration、weighting 与 histogram 端点数值边界向量 | 24/24、8/8、6/6 |

这张表描述的是一个指定 target、corpus、字段集合和运行边界。任意音频、x86 及其他
插件版本、foobar2000 解码、宿主与 playlist 行为、metadata 来源、完整文本渲染和内部
实现状态都不在这些观测之中。

相应记录包括 [M4 证据矩阵](M4_X64_NUMERIC_CLAIM_MATRIX.md)、
[M4 数值对齐报告](M4_X64_NUMERIC_ALIGNMENT_REPORT.md)与
[算法规格](../reference/specs/foo-dr-meter-1.0.8-candidate-v1.md)。

许可与署名提供的是研究的法律与历史背景；数值主张来自上述有界记录。历史 M0/1.0.3
材料作为已被取代的存档保留，与当前目标分开。目标身份、实验、观测、规格及其边界索引
在 [`reference/`](../reference/README.md) 下。

## 性能

分析采用流式状态：分析器内存随声道数增长，不随曲目时长增长。当前公开的性能材料是
一组本机测量记录，而不是通用的吞吐或内存数字。

已记录的 release worker 测量来自一台固定 Apple M4 Pro、固定工具链、生成 corpus 与
合成 workload。其中的音频 case 在该主机上快于实时，验收的优化在保持结果 fingerprint
不变的前提下降低了固定 8/64 声道 analyzer 中位耗时。不同机器、格式、声道数和输入的
实际速度与内存占用仍会不同；这些数字是一份本机基线，并不预测其他环境的表现。

可复现测量脚本及其解读集中在[性能说明](BENCHMARKS_CN.md)与[记录](performance/README.md)。

## 打包

0.3.0 为两个平台打包 CLI 与 GUI：运行 macOS 11.0 或更新系统的 Apple Silicon Mac，
以及 Windows x64。两个平台各自在自己的主机上 staging —— macOS 产出 DMG，Windows
产出 NSIS 安装包 —— 本地 staging 与有界 CI 门禁都会构建并**打开**最终制品做结构化
验证：DMG 会被挂载并检查其中的 `.app`；安装包会在候选目录之外解包，并核对内层
`macinmeter-gui.exe` 的 PE machine 实测为 x86_64、版本资源匹配、Authenticode 为未
签名。外层 installer 同样必须是未签名 PE。

Intel/universal macOS、ARM64 Windows 与 Linux GUI 包不属于 0.3.0 发行范围。目前的
打包情况汇总在[发行与制品状态](RELEASE_CN.md)。

## 设计历史

- [架构与参考对齐路线图](ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md)
- [架构决策记录](adr/)
- [格式指南](SUPPORTED_FORMATS_CN.md)
