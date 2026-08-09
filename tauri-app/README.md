# MacinMeter GUI

MacinMeter 0.3.1 的 Tauri 2 桌面界面。GUI 与 CLI 都只调用 workspace 中的 `macinmeter` application façade，不维护独立的解码、分析或批处理实现。

## 0.3.1 能力边界

- 支持单文件与受共享资源计划约束的批量分析；最终结果始终保持输入顺序。
- 支持整窗拖入单文件、多文件、目录或混合输入；目录可选择当前层或递归发现。
- 提供中英文界面、结果搜索与 DR 精细值排序、路径隐藏，以及 Markdown、共享
  `WireEnvelope` JSON、PNG 和 SVG 导出。
- 稳定格式为 WAV（PCM integer / IEEE float）、FLAC、AIFF（PCM integer），以及
  受限的 MP4/M4A + ALAC（16/24-bit、1–8 声道）。
- 目录发现使用扩展名筛选；实际解码器仍按文件内容探测。
- 每个分析 job 使用前端生成的 `jobId` 和独立 `CancellationToken`；取消作用于整个 job。
- 分析、批量和错误共用 schema version 4 的 `WireEnvelope`；channel/track
  report metrics、精确 decoded duration 与 DR diagnostics 分层保存。
- 大批量完成项会随分析进度分批渲染；JSON 与分页 PNG/SVG 导出不会改变后端报告。
- 不包含 FFmpeg、DSD、Opus、预处理或环境变量修改。

[`ADR-0014`](../docs/adr/0014-deterministic-decode-analysis-pipeline.md) 约束共享
`Application` 内部已经毕业的 packet、解码-分析重叠与 file-lane 并行。Tauri
不建立独立 scheduler 或线程池，窗口级并行仍未实现。

拖放和文件选择最终都进入同一 `discover_inputs` / `run_batch` 路径；图片、语言、
排序和路径显示只属于前端呈现，不会形成第二套分析配置。JSON 导出保留后端返回的
原始 schema-v4 envelope，不附加时间戳或界面私有字段。
报告中的固定数值参数用于结果复现；界面不向用户暴露内部算法名称或状态标签。

主窗口当前一次启动一个 job；后端注册表仍按 job 隔离，因此不同窗口或直接命令调用不会共享取消状态。
前端 TypeScript 类型只描述这份共享 wire schema；后端没有第二套 Rust 结果 DTO，
字段 tag/casing 由 Rust 契约测试固定。

GUI 显示独立的 overall RMS/primary peak report metrics。DR 诊断字段为
`drSelectedPeak`、`drPrimaryPeak`、`drSecondaryPeak`，不能替代 report 字段。
批量结果仍只是 track report 列表；Tauri 不会隐式调用库层的 `AlbumAggregator`。

## 后端命令

| 命令 | 输入 | 输出 |
| --- | --- | --- |
| `run_analysis` | `{ jobId, path }` + invocation-scoped channel | `WireEnvelope` |
| `run_batch` | `{ jobId, inputs, recursive }` + invocation-scoped channel | `WireEnvelope` |
| `discover_inputs` | `{ jobId, inputs, recursive }` | 稳定排序后的路径列表 |
| `cancel_job` | `jobId` | 是否找到并请求取消该 job |

目录预览也注册独立 job；重选、清空或开始分析时，前端会先取消旧预览。

分析进度通过每次命令调用独有的 Tauri Channel 传递，普通进度消息形如：

```json
{
  "type": "event",
  "event": { "type": "file_started" }
}
```

批量完成项会合并为 `batch_items` 消息，以减少大批量时的 WebView 回调。Channel
消息只用于显示进度和增量渲染；最终结果仍以命令返回的 `WireEnvelope` 为准。

## 开发与验证

环境要求：

- Rust 1.88 或更新版本
- Node.js 18、20 或 22+（建议使用当前 LTS）
- 当前平台对应的 [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)

安装前端依赖：

```bash
cd tauri-app
npm install
```

前端静态构建：

```bash
npm run build
```

从 workspace 根目录检查 Rust 后端：

```bash
cargo check --locked -p macinmeter-gui
```

启动桌面开发环境，或在 macOS 生成当前配置的 `.app` / `.dmg`：

```bash
npm run tauri dev
npm run tauri build
```

0.3.1 的 GUI 发行目标包含 Apple Silicon macOS 11.0+ 与 Windows x64，不包含
Intel/universal macOS、Windows ARM64/32-bit 或 Linux GUI。workspace 模式下，
Rust/Tauri 产物位于仓库根目录的 `target/`。本地构建不会自动触发 GitHub Actions；
macOS 26 arm64 与 Windows Server 2025 x64 clean runner 会执行同一 GUI 制品契约。

仓库根目录的发行 staging 可以进一步验证当前 host 的实际 GUI installer：

```bash
python3 scripts/stage-release.py stage --include-gui
```

macOS 会校验并只读挂载 DMG；Windows 会解开 NSIS 并核对内层 executable。两条
路径都会核对版本、架构与 SHA-256。当前制品仍明确未签名，macOS 也未 notarize；
`main` CI 会丢弃这些字节，手动 CI 只保留 14 天 candidate，两者都不构成公开分发
声明。详见
[`docs/RELEASE_CN.md`](../docs/RELEASE_CN.md)。

## 版本同步

普通 `npm run build` 与 `npm run tauri ...` 会先用 `npm run check-version`
只读核对版本。只有显式执行 `npm run sync-version` 才会从根 `Cargo.toml` 的
`[workspace.package].version` 写入：

- `tauri-app/package.json`
- `tauri-app/package-lock.json`
- `tauri-app/src-tauri/tauri.conf.json`

`src-tauri/Cargo.toml` 必须保留 `version.workspace = true`。版本不一致时普通
build 会失败而不会改写 tracked files。

## 目录

```text
tauri-app/
├── src/
│   ├── main.ts
│   └── styles.css
├── src-tauri/
│   ├── src/lib.rs
│   ├── src/main.rs
│   └── tauri.conf.json
├── scripts/sync-version.cjs
├── index.html
└── package.json
```

项目许可证与仓库根目录声明一致。
