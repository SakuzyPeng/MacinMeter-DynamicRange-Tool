# MacinMeter DR Tool

[English](README.md) | [中文](README_CN.md)

MacinMeter 是一个独立、本地优先的音频动态范围分析项目。0.2.0 将项目重建为一套安全、
流式的 Rust 核心，并由公共库、CLI 与 Tauri GUI 共同使用。

> **兼容性状态：`foo_dr_meter 1.0.8 Candidate V1 / Unverified`。**
> 当前 profile 是依据 foo_dr_meter 1.0.8 证据形成的候选解释；有界的 M1 证据
> 里程碑已经完成，但这不等于任意输入或完整 foobar/component 兼容。结果不得称为
> “官方”、已认证或可与参考结果互换。

## M0 范围

0.2.0 有意只保留一小块可信能力：

- WAV：8/16/24/32-bit 整数 PCM 与 IEEE 32/64-bit float
- FLAC
- AIFF：8/16/24/32-bit 整数 PCM
- 串行解码与串行批处理
- 有界内存的流式分析
- 唯一的 `FooDrMeter108CandidateV1` profile
- 结构化错误、取消、进度与带版本的 JSON

输入按内容探测；扩展名只用于目录发现。AIFC、MP3、AAC、ALAC、Vorbis、Opus、
FFmpeg 路径、DSD、预处理、包级并行和 SIMD 均不属于 M0，遇到时返回
`unsupported_format`。

## 构建与测试

需要 Rust 1.88 或更高版本。

```bash
cargo build --locked --workspace
cargo test --locked --workspace
cargo build --locked --release -p macinmeter-cli
```

Release CLI 位于 `target/release/macinmeter`。

## CLI

CLI 不再隐式扫描目录，也不会在未指定 `--output` 时自动保存报告。

```bash
macinmeter analyze FILE [--format human|json] [--output PATH]
macinmeter batch INPUT... [--recursive] [--format human|json] [--output PATH]
```

human `analyze` 输出包含每声道 overall RMS 以及 track report peak/RMS。
`batch` 返回相互独立的逐 track report，不执行 album 聚合。

标准输出只包含请求的结果；进度与诊断写入标准错误。输出文件先写入目标目录内的临时
文件，再原子替换目标。

退出码：

| 代码 | 含义 |
|---:|---|
| `0` | 所有请求均成功 |
| `1` | 失败、无输入或输出写入失败 |
| `2` | 命令行参数错误 |
| `3` | 批处理部分成功 |
| `130` | 已取消 |

JSON 与 Tauri 共用 schema v3 封装。下面是突出 report/diagnostic 分层的精简
analysis 示例：

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

payload 不含时间戳。`FiniteF32`/`FiniteF64` wrapper 使非有限 report 数值无法
构造；零幅度的 dBFS 使用显式 `null`。每声道具有独立的 public-f32 overall RMS
与 primary peak report metrics。track RMS 按参考路径先做 public-f32 平方、再以
f64 累加，track peak 则取 public primary peak 的最大值。`DecodedDuration`
保留精确的 decoded-frame/sample-rate 数对，而不是保存舍入后的秒数。

DR 计算诊断与 report metrics 分离：`loudWindowRms`、`drSelectedPeak`、
`drPrimaryPeak` 和可空的 `drSecondaryPeak` 只描述 DR 状态机实际使用的值，不能
替代 report 字段。

解码器会把受支持输入统一为有限、交错的 `f64`，与固定 x64 核心的 PCM 宽度
一致。固定 39-track schema-v3 safe-master 实测中，track DR 为 39/39、channel
DR 为 62/62、overall peak 为 39/39、overall RMS 为 39/39、channel RMS 为
62/62、渲染时长为 39/39。参考 footer 的 track 数、采样率集合、声道数集合和
`DR12` token 也与实现报告一致；若排除三个数值 DR0 track，则反事实结果会是
DR13。这个局部 footer 检查不证明 host metadata、精确 album 内部算术、
duration weighting 或完整文本 parity，也不以内部实现状态同构为目标。M1 数值
范围纳入静态恢复的 album 算术与 renderer 舍入规则；host 行为、playlist
grouping、metadata 来源、完整文本 parity 和任意音频仍不在声明范围内。另一个
38-vector 隔离运行已经交叉验证 duration 半秒/进位、可选多声道 loudness
weighting 和 RMS histogram 两个 clamp 端点；它没有扩大兼容性范围，因此
profile 继续是 `Unverified`。

## 公共库

公共门面位于 `macinmeter` crate：

```rust
use macinmeter::{AnalyzeRequest, Analyzer};

fn main() -> Result<(), macinmeter::AnalysisError> {
    let _report = Analyzer::new().analyze_file(AnalyzeRequest::new("track.flac"))?;
    Ok(())
}
```

更底层的分析入口是 frame-aligned 的流式 session：

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

`finish` 会消费 session，并以可失败结果阻止数值/资源错误泄漏成非有限输出；输入
样本必须有限且按完整 frame 对齐。

Album 聚合是显式库操作，不会把 batch 隐式当成 album：

```rust
use macinmeter::{
    AlbumAggregator, AlbumTrackMetrics, AlbumWeighting, AnalyzeRequest, Analyzer,
};

fn main() -> Result<(), macinmeter::AnalysisError> {
    let report = Analyzer::new().analyze_file(AnalyzeRequest::new("track.flac"))?;
    let track = AlbumTrackMetrics::try_from(&report)?;
    let _album = AlbumAggregator::aggregate(&[track], AlbumWeighting::Unweighted)?;
    Ok(())
}
```

unweighted album 值对 public-f32 track DR 做算术平均，并纳入数值 DR0 track；
可选 duration weighting 使用每首 track 的精确 decoded duration。这个数值 API
不声明 playlist grouping、footer 或其他 album 子系统 parity；除非调用方显式
调用它，batch 与 GUI 结果始终只是相互独立的 track report 集合。

## GUI

Tauri 2 前端与 CLI 使用完全相同的 application façade 和 wire schema：

```bash
cd tauri-app
npm install
npm run tauri dev
```

每个 GUI job 拥有独立取消 token。GUI 不再配置 FFmpeg、不修改进程环境变量，也不再
维护另一套批处理引擎。

## 架构

仓库根目录是 virtual Cargo workspace：

```text
macinmeter-domain
├── macinmeter-analysis
├── macinmeter-codecs
└── macinmeter（application façade）
    ├── macinmeter-cli
    └── macinmeter-gui
```

所有第一方 crate 均使用 `#![forbid(unsafe_code)]`。只有 application 层组合解码和分析，
因此前端无法静默分叉算法行为。

进一步阅读：

- [M0 架构决策](docs/adr/0001-m0-0.2.0-trusted-trunk-rebuild.md)
- [M1 参考数值范围决策](docs/adr/0002-m1-reference-numeric-scope.md)
- [架构与参考对齐路线图](docs/ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md)
- [支持格式](docs/SUPPORTED_FORMATS_CN.md)
- [`foo_dr_meter 1.0.8 Candidate V1` 规格](reference/specs/foo-dr-meter-1.0.8-candidate-v1.md)
- [参考证据策略](reference/README.md)
- [隔离 x64 analyzer-core harness](reference/observations/CORE_HARNESS.md)
- [隔离 x64 numeric-boundary 观测](reference/observations/obs-foo-dr-meter-108-x64-numeric-boundaries-v1-run1-20260719/record.md)

## 参考工作与致谢

当前参考目标是 foobar2000 DR Meter 1.0.8（`foo_dr_meter`，作者 Janne
Hyvärinen）。
我们已经取得作者对逆向插件的许可；私人授权原文不存入本仓库，只保留
[最小公开范围摘要](reference/authorization/README.md)。

授权和致谢不代表数值兼容已经成立。目标 hash、实验、观测和候选规格记录在
`reference/`；当前
[绑定干净提交的 schema-v3 x64 safe-master conformance 记录](reference/conformance/conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-report-v3-clean-20260718/record.md)
明确列出精确比较范围与声明边界。除非未来另行审查并建立更强兼容性声明，profile
继续保持 `Unverified`。

已经验收的
[39 项隔离 x64 analyzer-core 观测](reference/observations/obs-foo-dr-meter-108-x64-isolated-core-safe-master-run1-20260719/record.md)
可在不启动 foobar2000 的情况下直接执行固定目标：每个输入使用全新 worker，并在
core 调用期对 13 个普通 `shared.dll` IAT 入口设置 fail-fast tripwire。它不验证
foobar 解码、注册、metadata、album grouping 或完整 renderer；这些是明确非目标，
不是尚未补齐的 M1 证据。该记录的声明仍为 `compatibility: none` 与
`foobarParity: not_assessed`。

同一个 hardened 边界还完成了
[38-vector numeric 观测](reference/observations/obs-foo-dr-meter-108-x64-numeric-boundaries-v1-run1-20260719/record.md)：
24 个 duration、8 个多声道 weighting 和 6 个 histogram endpoint worker
全部满足预注册判据。它关闭这些可能改变 per-track 输出的证据缺口，但没有执行
完整 renderer，也没有扩大兼容性声明。

## 许可证

MacinMeter 采用 [MIT License](LICENSE)。另见[法律说明](docs/LEGAL_CN.md)和
[第三方许可](THIRD_PARTY_NOTICES.md)。
