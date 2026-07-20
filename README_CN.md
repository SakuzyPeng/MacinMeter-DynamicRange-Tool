# MacinMeter DR Tool

[English](README.md) | [中文](README_CN.md)

MacinMeter 是一个独立、本地优先的音频动态范围（DR）分析工具。它按照对
foobar2000 DR Meter 1.0.8 算法的候选重建，对 WAV、FLAC、AIFF 文件计算逐声道与
逐轨 DR 值，并以一套安全、流式的 Rust 核心同时驱动公共库、CLI 与 Tauri GUI。

> **兼容性状态：`foo_dr_meter 1.0.8 Candidate V1 / Unverified`。**
> 当前 profile 是依据 foo_dr_meter 1.0.8 x64 证据形成的候选解释；有界的 M4
> direct-PCM conformance 里程碑已经完成，但这不等于任意输入或完整
> foobar/component 兼容。结果不得称为“官方”、已认证或可与参考结果互换。

## 特点

- **唯一分析核心。** 库、CLI 与 GUI 的文件分析都通过 `Application`
  façade 到达同一 `AnalyzerSession`；直接流式调用者也使用这一
  session 类型，因此适配器无法悄悄分叉算法行为。
- **流式且有界。** 分析基于在线窗口与直方图；内存随声道数增长，不随流长增长。
- **构造即安全。** 所有第一方 crate 使用 `#![forbid(unsafe_code)]`；成功报告
  只能经检查构造器建立，无法表示非有限值。
- **声明以证据为界。** 参考 profile、规格与 conformance 记录保存在
  仓库内，并绑定固定的目标、corpus 与制品身份；声明永不超出已记录
  的证据。
- **性能只测量、不承诺。** 标量核心有可复现的本机基线、采样归因和一条以
  bit-exact 差分为门禁的优化链；不做跨机器吞吐承诺。

## 可信能力边界（0.2.0）

| 容器 | 接受的编码 |
| --- | --- |
| 经典 RIFF/WAVE | 8/16/24/32-bit 整数 PCM；IEEE 32/64-bit float |
| FLAC（原生容器） | 声明非零总样本数的 FLAC |
| AIFF | 8/16/24/32-bit 整数 PCM |

一切按内容探测，扩展名只用于目录发现。串行解码、串行批处理与 64 声道产品
分析上限都是刻意选择。WAVE_FORMAT_EXTENSIBLE、AIFC、MP3、AAC、ALAC、Vorbis、
Opus 与 DSD 不属于 0.2.0 稳定能力；能识别但不可用的媒体会返回
`unsupported_format`。FFmpeg backend、预处理、包级并行与 SIMD 执行路径则完全
不存在，不是可配置选项。各 route 的精确限制见
[支持格式](docs/SUPPORTED_FORMATS_CN.md)。

## 快速开始

需要 Rust 1.88 及以上与 Cargo。

```bash
cargo build --locked --release -p macinmeter-cli
target/release/macinmeter analyze track.flac
target/release/macinmeter batch Album/ --recursive --format json
```

### CLI

CLI 没有隐式模式：不要求就不扫描目录，不给 `--output` 就不写报告文件。

```bash
macinmeter analyze FILE [--format human|json] [--output PATH]
macinmeter batch INPUT... [--recursive] [--format human|json] [--output PATH]
```

标准输出只承载请求的结果；进度与诊断进入标准错误。输出文件先写入目标目录内的
临时文件，再原子替换。`batch` 返回相互独立的逐轨报告，不做 album 聚合。

| 退出码 | 含义 |
|---:|---|
| `0` | 所有请求均成功 |
| `1` | 失败、无输入或输出写入失败 |
| `2` | 命令行参数错误 |
| `3` | 批处理同时存在成功与失败 |
| `130` | 已取消 |

### JSON

JSON 与 Tauri GUI 共用同一带版本的 schema-v3 信封。节选示例：

```json
{
  "schemaVersion": 3,
  "toolVersion": "0.2.0",
  "kind": "analysis",
  "data": {
    "analysis": {
      "channels": [{
        "report": {
          "overallRmsLinear": 0.5,
          "overallRmsDbfs": -6.0206,
          "primaryPeakLinear": 1.0
        },
        "outcome": {
          "status": "measured",
          "measurement": {
            "loudWindowRms": 0.25,
            "drSelectedPeak": 0.5,
            "drPrimaryPeak": 1.0,
            "drSecondaryPeak": 0.5
          }
        }
      }],
      "report": {
        "overallRmsLinear": 0.5,
        "overallRmsDbfs": -6.0206,
        "primaryPeakLinear": 1.0,
        "primaryPeakDbfs": 0.0,
        "duration": { "decodedFrames": 48000, "sampleRate": 48000 }
      }
    }
  }
}
```

report 指标与 DR 状态诊断刻意分离：`loudWindowRms`、`drSelectedPeak`、
`drPrimaryPeak` 与可空的 `drSecondaryPeak` 描述 DR 状态机使用的值，不得替代
report 指标。`FiniteF32`/`FiniteF64` 使非有限报告值不可表示；零振幅 dBFS 为
显式 `null`；`DecodedDuration` 保存精确的解码帧数/采样率对，而不是舍入后的
秒数。

### GUI

Tauri 2 前端与 CLI 使用完全相同的 application façade 与 wire schema：

```bash
cd tauri-app
npm install
npm run tauri dev
```

每个 GUI job 拥有独立取消 token，并在进入 blocking runtime 前预留共享
application 预算；排队中的取消不影响 active job。GUI 不配置 FFmpeg、不修改
进程环境变量，也没有第二套批处理引擎。

## 库

公共门面是 `macinmeter` crate：

```rust
use macinmeter::{AnalyzeRequest, Application};

fn main() -> Result<(), macinmeter::AnalysisError> {
    let application = Application::new();
    let _report = application.analyze_file(AnalyzeRequest::new("track.flac"))?;
    Ok(())
}
```

同一 `Application` 的 clone 共享一个有界 FIFO 执行域：同时最多一个 active
顶层 analyze/batch/discovery job，最多 64 个排队。CLI/Tauri 因此保持串行，
且不依赖隐藏的进程级全局单例。

更低层的分析入口是帧对齐的流式会话：

```rust
use macinmeter::{AnalysisProfile, AnalyzerSession, ChannelLayout, StreamSpec};

fn main() -> Result<(), macinmeter::AnalysisError> {
    let spec = StreamSpec::new(48_000, 2, ChannelLayout::Unknown)?;
    let mut session =
        AnalyzerSession::new(spec, AnalysisProfile::FooDrMeter108CandidateV1)?;
    session.push_interleaved(&[0.25, -0.25, 0.5, -0.5])?;
    let _result = session.finish()?;
    Ok(())
}
```

样本必须是有限、交错的 `f64`；`finish` 消费会话且可失败，数值或资源故障不会
泄漏非有限输出。成功的 `AnalysisResult`/`AnalysisReport` 根不可变，只经只读
getter 检视；非产品输入的 result/report 与共享 batch/event/wire 类型只支持
序列化，不支持反序列化。这些 Rust API 约束不改变 schema-v3 JSON 的键、tag 或
数值表示。

album 聚合是显式库操作，永远不是批处理的隐含属性：

```rust
use macinmeter::{
    AlbumAggregator, AlbumTrackMetrics, AlbumWeighting, AnalyzeRequest, Application,
};

fn main() -> Result<(), macinmeter::AnalysisError> {
    let report = Application::new().analyze_file(AnalyzeRequest::new("track.flac"))?;
    let track = AlbumTrackMetrics::try_from(&report)?;
    let _album = AlbumAggregator::aggregate(&[track], AlbumWeighting::Unweighted)?;
    Ok(())
}
```

unweighted album 值是公开 f32 轨 DR 的算术均值，包含数值 DR0 轨；可选的时长
加权使用每轨的精确解码时长。该数值 API 不声明 playlist 分组、footer 或其他
album 子系统 parity。

## Conformance 证据

解码器把受支持输入归一化为有限、交错的 `f64`，与固定 x64 core 的 PCM 位宽
一致。针对固定 `foo_dr_meter 1.0.8 x64` 目标（`ff3556ad…`），已记录证据为：

| 证据 | 结果 |
| --- | --- |
| 39 轨 schema-v3 safe-master：track DR / overall peak / overall RMS / 渲染时长 | 各 39/39 |
| 同一 run：channel DR / channel RMS | 各 62/62 |
| M4 decoder-independent direct-PCM Candidate conformance | 固定 39 项输入的 final-field projection 差分数为 0 |
| 39 项隔离 x64 analyzer-core 观测（不启动 foobar2000） | 预注册断言全部满足 |
| 38 向量隔离数值边界：duration 半秒/进位、可选多声道 loudness weighting、histogram clamp 端点 | 24/24、8/8、6/6 |

参考 footer 的轨数、采样率集合、声道数集合与 `DR12` token 也与实现报告一致。
宿主行为、playlist 分组、metadata 来源、完整文本 parity、内部实现状态同构与
任意音频仍在声明之外——这正是 profile 保持 `Unverified` 的原因。精确范围与
限制见 [M4 证据矩阵](docs/M4_X64_NUMERIC_CLAIM_MATRIX.md)、
[M4 conformance 报告](docs/M4_X64_NUMERIC_COMPATIBILITY_REPORT.md)与
[候选规格](reference/specs/foo-dr-meter-1.0.8-candidate-v1.md)。

## 性能

0.2.0 不发布性能保证。M6 建立的是可复现的本机测量协议：确定性生成语料、
15-case 标量基线、采样归因，以及要求先复现 bit-identical 结果才允许比较的
同轮交错 A/B。一条 analyzer 验证遍历优化链通过该门禁进入产品；在固定基线
主机上，stereo 差异落在测量噪声内，8/64 声道的中位耗时则约降低 13% 与
27%。这些数字只描述一台固定机器、固定工具链与合成负载——它们是工程
决策证据，不是面向用户的吞吐声明。

复现或扩展测量：

```bash
python3 scripts/generate-performance-corpus.py
python3 scripts/run-performance-baseline.py
```

详见[性能测量契约](docs/BENCHMARKS_CN.md)与
[M6 报告](docs/performance/README.md)。

## 验证

本地门禁按资源风险从低到高：

```bash
python3 scripts/check-repository-contract.py
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
python3 -m unittest discover -s reference/tools/tests -p 'test_*.py'
cd tauri-app && npm run build
```

恶意输入验证被刻意隔离：提交的 41-case malformed 媒体 corpus 逐例在带
wall-clock timeout 与 Linux 地址空间上限的子进程中执行
（`python3 scripts/verify-malformed-corpus.py`），无法施加内存上限时拒绝解码
恶意字节。远程 CI 保持仅 `workflow_dispatch` 手动触发。经验证的本地发行
staging（校验和、CLI 冒烟测试、可选未签名 macOS DMG）见
[发行制品契约](docs/RELEASE_CN.md)。

## 架构

仓库是单向依赖的 virtual Cargo workspace：

```text
macinmeter-domain
├── macinmeter-analysis
├── macinmeter-codecs
└── macinmeter（application façade）
    ├── macinmeter-cli
    └── macinmeter-gui
```

`domain` 拥有有效类型与错误；`analysis` 拥有唯一流式分析器；`codecs` 拥有
探测、严格 PCM 源与唯一原生 capability catalog；application 层是唯一组合
解码与分析的位置；CLI 与 GUI 只做解析、渲染和 I/O 适配。

0.2.0 重建按七个受评审的里程碑执行，每个都以架构决策记录收口：

| 里程碑 | 决策记录 |
| --- | --- |
| M0 — 可信主干重建 | [ADR-0001](docs/adr/0001-m0-0.2.0-trusted-trunk-rebuild.md) |
| M1 — 参考数值范围 | [ADR-0002](docs/adr/0002-m1-reference-numeric-scope.md) |
| M2 — 原生解码契约加固 | [ADR-0003](docs/adr/0003-m2-native-decoder-contract-hardening.md) |
| M3 — application 执行预算 | [ADR-0004](docs/adr/0004-m3-application-execution-budget.md) |
| M4 — 固定 x64 数值声明 | [ADR-0005](docs/adr/0005-m4-bounded-x64-numeric-claim.md) |
| M5 — 产品与仓库收敛 | [ADR-0006](docs/adr/0006-m5-product-repository-convergence.md) |
| M6 — 可复现性能基线 | [ADR-0007](docs/adr/0007-m6-reproducible-performance-baseline.md) |

总览文档是
[架构整改与参考插件重新对齐路线图](docs/ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md)。

## 参考研究与致谢

当前参考目标是 Janne Hyvärinen 的 foobar2000 DR Meter 1.0.8
（`foo_dr_meter`）。对该插件进行逆向研究已获得作者许可。私人许可信件不保存在
本仓库中，仅保留[最小公开范围摘要](reference/authorization/README.md)。

许可与致谢不构成数值兼容性。目标哈希、实验、观测、候选规格与全部 conformance
记录保存在 [`reference/`](reference/README.md)，各自声明范围与限制——包括
不启动 foobar2000 而直接执行固定目标的
[隔离 x64 analyzer-core harness](reference/observations/CORE_HARNESS.md)。
除非未来经独立评审的兼容性声明支持更强表述，profile 保持 `Unverified`。

## 许可证

MacinMeter 以 [MIT License](LICENSE) 发布。另见
[法律说明](docs/LEGAL_CN.md)与[第三方声明](THIRD_PARTY_NOTICES.md)。
