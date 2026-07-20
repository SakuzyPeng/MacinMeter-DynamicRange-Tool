# MacinMeter GUI

MacinMeter 0.2.0 的 Tauri 2 桌面界面。GUI 与 CLI 都只调用 workspace 中的 `macinmeter` application façade，不维护独立的解码、分析或批处理实现。

> 当前输出固定标记为 `FooDrMeter108CandidateV1 / Unverified`。它实现了基于
> `foo_dr_meter` 1.0.8 固定目标建立的候选规格，并已完成 M4 有界 direct-PCM
> conformance；这仍不证明任意输入或完整 host/component parity，不能称为
> “官方”或“参考兼容”结果。

## 0.2.0 能力边界

- 支持单文件与串行批量分析。
- 稳定格式为 WAV（PCM integer / IEEE float）、FLAC 与 AIFF（PCM integer）。
- 目录发现使用扩展名筛选；实际解码器仍按文件内容探测。
- 每个分析 job 使用前端生成的 `jobId` 和独立 `CancellationToken`。
- 分析、批量和错误共用 schema version 3 的 `WireEnvelope`；channel/track
  report metrics、精确 decoded duration 与 DR diagnostics 分层保存。
- 不包含 FFmpeg、DSD、Opus、预处理、文件级并行或环境变量修改。

主窗口当前一次启动一个 job；后端注册表仍按 job 隔离，因此不同窗口或直接命令调用不会共享取消状态。
前端 TypeScript 类型只描述这份共享 wire schema；后端没有第二套 Rust 结果 DTO，
字段 tag/casing 由 Rust 契约测试固定。

GUI 显示独立的 overall RMS/primary peak report metrics。DR 诊断字段为
`drSelectedPeak`、`drPrimaryPeak`、`drSecondaryPeak`，不能替代 report 字段。
批量结果仍只是 track report 列表；Tauri 不会隐式调用库层的 `AlbumAggregator`。

## 后端命令

| 命令 | 输入 | 输出 |
| --- | --- | --- |
| `run_analysis` | `{ jobId, path }` | `WireEnvelope` |
| `run_batch` | `{ jobId, inputs, recursive }` | `WireEnvelope` |
| `discover_inputs` | `{ jobId, inputs, recursive }` | 稳定排序后的路径列表 |
| `cancel_job` | `jobId` | 是否找到并请求取消该 job |

目录预览也注册独立 job；重选、清空或开始分析时，前端会先取消旧预览。

进度事件名为 `analysis-event`，payload 统一为：

```json
{
  "jobId": "frontend-generated-id",
  "event": {
    "type": "file_started"
  }
}
```

事件只用于显示进度；最终结果以命令返回的 `WireEnvelope` 为准。

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

0.2.0 当前不声明 Windows/Linux GUI 打包目标。workspace 模式下，Rust/Tauri 产物位于仓库
根目录的 `target/`。本地构建不会自动触发 GitHub Actions。

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
