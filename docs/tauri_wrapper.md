## MacinMeter DR GUI（Tauri 包装层）

Tauri 前端提供了一个轻量图形界面，直接复用 `macinmeter-dr-tool` 库中的全部解码和 DR 计算逻辑，可用于快速测试单个音频文件、查看 Official/Precise DR 详情，并预览裁切/静音过滤诊断信息。

### 目录结构

- `tauri-app/` – 前端（Vite + TypeScript）与 Tauri 后端。
  - `src/` – UI 代码（表单、状态、结果展示）。
  - `src-tauri/` – Rust 命令，直接调用 `macinmeter_dr_tool::analyze_file`。
  - `docs/tauri_wrapper.md` – 本文件。

### 安装依赖

1. 安装 Node.js 18+ 与 npm。
2. 安装 Rust 1.80+，同时安装 `tauri-cli`（已在 `package.json` / `Cargo.toml` 中声明）。
3. 初次运行时执行：

   ```bash
   cd tauri-app
   npm install
   ```

### 常用命令

| 命令 | 作用 |
| ---- | ---- |
| `npm run tauri dev` | 启动开发模式（自动启动 Vite + Tauri 窗口） |
| `npm run tauri build` | 生成桌面发行版（macOS App、Windows/MSI、Linux AppImage 等） |
| `npm run build` | 仅构建前端静态资源（CI 用） |

Tauri 构建过程中会自动编译 `macinmeter-dr-tool`，因此 CLI 和 GUI 共享相同的解码/处理能力。

### 后端命令

`src-tauri/src/lib.rs` 注册了三个命令：

- `analyze_audio(path, options)`：分析单个音频文件，返回官方/精确 DR、每声道结果、静音过滤与首尾裁切报告。内部使用 `macinmeter_dr_tool::analyze_file`，保持与 CLI 一致。
- `scan_audio_directory(path)`：调用 `tools::scan_audio_files` 列出目录中支持的音频文件，供 UI 一键选择。
- `load_app_metadata()`：返回默认配置与支持格式列表，确保前端默认值与 CLI 常量同步。

所有耗时操作都通过 `tauri::async_runtime::spawn_blocking` 运行，避免阻塞 UI 线程。

### 权限与插件

- `tauri-plugin-dialog`：用于系统文件/目录选择，对应 `capabilities/default.json` 中的 `dialog:default` 权限。
- `tauri-plugin-opener`：保留模板中的链接打开功能（如将来需要跳转文档）。
- 核心文件访问在 Rust 端完成，不使用 WebView FS API，因此无需额外 `fs` 权限。

### UI 功能概述

- 音频路径选择：系统对话框选择单文件或扫描目录（单击列表写入输入框）。
- 分析选项：并行解码开关、Verbose、RMS/Peak 表格、LFE 剔除、静音窗口过滤、首尾裁切、DSD 转换参数。
- 结果展示：官方/精确 DR、每声道 DR 表格、边界风险提示、裁切/静音报告摘要。
- 错误处理：渲染 `AudioError` 信息，提供建议与支持格式列表。

### 打包输出

执行 `npm run tauri build` 后，可在 `tauri-app/src-tauri/target/release/bundle/` 下获得对应平台安装包/可执行文件。GUI 与 CLI 共存，不会影响根目录的 `cargo build --release`。

### 后续工作建议

- 批处理：GUI 当前专注单文件分析，可扩展为自动遍历目录并串行调用 `analyze_audio`。
- 国际化：`main.ts` 已使用中文标签，可在 UI 内加入语言切换（本地化字符串 map）。
- 深色主题：CSS 已保留变量，可根据偏好媒体查询进行切换。
