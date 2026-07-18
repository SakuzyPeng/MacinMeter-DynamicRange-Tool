# MacinMeter DR Tool

[English](README.md) | [中文](README_CN.md)

MacinMeter 是一个独立、本地优先的音频动态范围分析项目。0.2.0 将项目重建为一套安全、
流式的 Rust 核心，并由公共库、CLI 与 Tauri GUI 共同使用。

> **兼容性状态：`ProvisionalV1 / Unverified`。** 当前算法是一份可复现的工程基线，
> 不是已经验证的参考插件实现。结果不得称为“官方”、已认证或可与参考结果互换。

## M0 范围

0.2.0 有意只保留一小块可信能力：

- WAV：8/16/24/32-bit 整数 PCM 与 IEEE 32/64-bit float
- FLAC
- AIFF：8/16/24/32-bit 整数 PCM
- 串行解码与串行批处理
- 有界内存的流式分析
- 唯一的 `ProvisionalV1` profile
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

JSON 与 Tauri 共用同一封装：

```json
{
  "schemaVersion": 1,
  "toolVersion": "0.2.0",
  "kind": "analysis",
  "data": {}
}
```

payload 不含时间戳，非有限数值不会作为 JSON number 输出。

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
    let mut session = AnalyzerSession::new(spec, AnalysisProfile::ProvisionalV1)?;
    session.push_interleaved(&[0.25, -0.25, 0.5, -0.5])?;
    let _result = session.finish();
    Ok(())
}
```

`finish` 会消费 session；输入样本必须有限且按完整 frame 对齐。

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
- [架构与参考对齐路线图](docs/ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md)
- [支持格式](docs/SUPPORTED_FORMATS_CN.md)
- [`ProvisionalV1` 规格](reference/specs/provisional-v1.md)
- [参考证据策略](reference/README.md)

## 参考工作与致谢

本项目研究 foobar2000 DR Meter 1.0.3（`foo_dr_meter`，作者 Janne Hyvärinen）的行为。
我们已经取得作者对逆向插件的许可；私人授权原文不存入本仓库。

授权和致谢不代表数值兼容已经成立。参考二进制、哈希、实验、观测和 conformance
fixture 将在后续里程碑中作为证据记录；M0 只建立承接这些材料的边界。

## 许可证

MacinMeter 采用 [MIT License](LICENSE)。另见[法律说明](docs/LEGAL_CN.md)和
[第三方许可](THIRD_PARTY_NOTICES.md)。
