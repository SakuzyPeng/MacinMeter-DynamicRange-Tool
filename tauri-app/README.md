# MacinMeter GUI

MacinMeter 0.3.0 的 Tauri 2 桌面界面。GUI 与 CLI 都只调用 workspace 中的 `macinmeter` application façade，不维护独立的解码、分析或批处理实现。

## 0.3.0 能力边界

- 支持单文件与串行批量分析。
- 支持整窗拖入单文件、多文件、目录或混合输入；目录可选择当前层或递归发现。
- 提供中英文界面、结果搜索与 DR 精细值排序、路径隐藏，以及 Markdown、共享
  `WireEnvelope` JSON、PNG 和 SVG 导出。
- 稳定格式为 WAV（PCM integer / IEEE float）、FLAC、AIFF（PCM integer），以及
  受限的 MP4/M4A + ALAC（16/24-bit、1–8 声道）。
- 目录发现使用扩展名筛选；实际解码器仍按文件内容探测。
- 每个分析 job 使用前端生成的 `jobId` 和独立 `CancellationToken`。
- 分析、批量和错误共用 schema version 4 的 `WireEnvelope`；channel/track
  report metrics、精确 decoded duration 与 DR diagnostics 分层保存。
- 不包含 FFmpeg、DSD、Opus、预处理、文件级并行或环境变量修改。

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

0.3.0 的 GUI 发行目标只包含 Apple Silicon macOS 11.0+，不包含 Intel/universal
macOS 或 Windows/Linux GUI。workspace 模式下，Rust/Tauri 产物位于仓库根目录的
`target/`。本地构建不会自动触发 GitHub Actions；macOS 26 arm64 clean runner 会
执行同一 GUI 制品契约。

仓库根目录的发行 staging 可以进一步验证当前 host 的实际 DMG：

```bash
python3 scripts/stage-release.py stage --include-gui
```

它会校验并只读挂载 DMG，核对 bundle version、identifier、executable 和准确
architecture，再生成 SHA-256。当前制品仍明确是未签名、未 notarize 的 staging；
`main` CI 会丢弃这些字节，手动 CI 只保留 14 天 candidate，两者都不构成
Gatekeeper 或公开分发声明。详见
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
