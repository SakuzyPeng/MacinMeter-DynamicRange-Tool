# MacinMeter DR GUI

基于 Tauri 2 的跨平台图形界面，直接复用 `macinmeter-dr-tool` CLI 的全部解码和 DR 计算逻辑。

## 功能特性

### 分析能力
- **单文件分析**：选择音频文件，查看详细 DR 结果
- **批量分析**：扫描目录或深度递归扫描，并行处理多个文件
- **多文件选择**：一次选择多个文件进行批量分析
- **实时进度**：显示当前分析进度和已完成文件数

### 结果展示
- **Official/Precise DR**：显示官方整数值和精确小数值
- **每声道详情**：DR、RMS、Peak 等完整数据
- **边界风险提示**：DR 值接近 X.5 边界时的警告
- **静音/LFE 标记**：识别静音声道和 LFE 声道

### 导出功能
- **Copy MD**：复制 Markdown 格式结果到剪贴板
- **Export JSON**：导出完整分析数据为 JSON 文件
- **Export Image**：导出结果区域为 PNG 图片
- **单条复制**：批量分析时可单独复制某个文件的 MD 或 PNG

### 选项控制
- **排除 LFE**：从 DR 聚合计算中剔除 LFE 声道
- **隐藏路径**：导出时隐藏文件路径信息
- **排序模式**：按原顺序或 DR 精细值升序/降序排列
- **搜索过滤**：按文件名/路径搜索并跳转

## 安装与构建

### 环境要求
- Node.js 18+
- Rust 1.80+
- 系统依赖：参见 [Tauri Prerequisites](https://tauri.app/start/prerequisites/)

### 安装依赖

```bash
cd tauri-app
npm install
```

### 开发模式

```bash
npm run tauri dev
```

自动启动 Vite 开发服务器和 Tauri 窗口，支持热重载。

### 构建发行版

```bash
npm run tauri build
```

生成平台对应的安装包：
- macOS: `.app` / `.dmg`
- Windows: `.exe` / `.msi`
- Linux: `.AppImage` / `.deb`

输出位置：`src-tauri/target/release/bundle/`

## 使用说明

### 单文件分析
1. 点击「选择文件」选择音频文件
2. 点击「开始分析」
3. 查看结果面板中的 DR 数据

### 批量分析
1. 点击「选择目录」扫描单层目录，或「深度扫描目录」递归扫描
2. 扫描完成后显示文件列表，点击「开始分析」
3. 结果按顺序显示，可使用排序和搜索功能

### 自定义 FFmpeg
如需使用自定义 FFmpeg（用于 DSD 或特殊格式），在「自定义 ffmpeg 路径」输入框填入路径并点击「应用」。

### 取消分析
批量分析过程中可点击「取消分析」停止，已完成的结果会保留。

## 技术架构

### 前端
- **框架**：Vanilla TypeScript + Vite
- **样式**：原生 CSS，支持打印样式
- **状态管理**：模块级变量 + 事件监听

### 后端命令
`src-tauri/src/lib.rs` 注册的 Tauri 命令：

| 命令 | 功能 |
|------|------|
| `analyze_audio` | 分析单个音频文件 |
| `analyze_directory` | 扫描并分析目录中所有音频 |
| `analyze_files` | 分析指定的多个文件 |
| `scan_audio_directory` | 扫描目录（不分析） |
| `deep_scan_audio_directory` | 递归扫描目录 |
| `copy_image_to_clipboard` | 复制 PNG 图片到剪贴板 |
| `set_ffmpeg_override` | 设置自定义 FFmpeg 路径 |
| `cancel_analysis` | 取消当前分析任务 |

### 并行处理
- **文件级并行**：默认 4 线程并行分析多个文件
- **解码级并行**：每个文件可启用并行解码（适合大文件）
- 环境变量 `MACINMETER_GUI_PARALLEL_FILES` 可调整文件并行度

### 剪贴板
使用 `arboard` crate 跨平台操作剪贴板，无需外部命令。

## 开发说明

### 目录结构

```
tauri-app/
├── src/                    # 前端代码
│   ├── main.ts            # 主逻辑
│   └── styles.css         # 样式
├── src-tauri/             # Rust 后端
│   ├── src/lib.rs         # Tauri 命令
│   └── Cargo.toml         # 依赖配置
├── index.html             # 入口页面
└── package.json           # Node 依赖
```

### 调试
- 开发模式下 WebView 开发者工具可用（右键 → 检查）
- Rust 端日志通过 `eprintln!` 输出到终端

### 与 CLI 的关系
GUI 直接调用 `macinmeter_dr_tool::analyze_file`，与 CLI 共享：
- 解码器（Symphonia + FFmpeg 桥接）
- DR 计算算法
- 所有实验性功能

## IDE 推荐配置

- [VS Code](https://code.visualstudio.com/)
- [Tauri 插件](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode)
- [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## 许可证

与主项目相同。
