# ADR-0018：发行 GUI 改为 egui 桌面 GUI 的两种字体构建

- 状态：Accepted
- 日期：2026-11-27
- 范围：GUI 发行制品的来源、命名、安装行为与验收深度
- 部分取代：[ADR-0010](0010-macos-arm64-gui-staging-ci.md)、
  [ADR-0011](0011-unsigned-apple-silicon-release-scope.md) 与
  [ADR-0015](0015-unsigned-windows-x64-release-scope.md) 中“GUI 制品是 Tauri
  bundler 产出的 DMG / NSIS 安装包”的部分；它们的平台范围、未签名立场、候选保留
  与“两个候选必须来自同一 source commit”的边界原样保留
- 不涉及：删除 Tauri 源码，以及 ADR-0004/0006 中 Tauri adapter 的契约

## 背景

`apps/macinmeter-desktop` 在 MacinDecode Spatial Player 的 egui 技术栈上重写了
GUI（见 [`desktop_egui.md`](../desktop_egui.md)），覆盖了 Tauri GUI 的输入、批处理、
结果卡片、JSON 与图片导出。它有两种字体构建：默认的 `bundled-fonts` 内嵌 Noto Sans
CJK SC 与 Source Serif 4 Semibold，窗口与导出图片在任何主机上一致；关闭该 feature
的构建不内嵌这两款字体，按 Tauri 样式表的字体栈使用系统字体，体积约为前者一半。

两种构建各有用户：前者换来跨主机一致的导出图片，后者换来更小的下载与系统字体的
观感。维护者决定两者同时发行。

## 决策

### 1. 每个平台发两个 GUI 制品，Tauri 不再打包

| 平台 | CLI | GUI |
| --- | --- | --- |
| macOS arm64 | archive | `…-bundled-fonts.dmg`、`…-system-fonts.dmg` |
| Windows x64 | archive | `…-bundled-fonts-setup.exe`、`…-system-fonts-setup.exe` |

`stage-release.py --include-gui` 依次构建两种字体版本并分别打包；每个 GUI 制品在
manifest 中记录 `fontBuild`。未签名候选必须恰好包含 CLI 与两种字体构建的 GUI，
缺一即拒绝。Tauri GUI 的源码、lint 与前端构建保留在仓库中，但任何制品都不再包含它；
删除它另行决定。

### 2. 沿用 Tauri GUI 的安装身份，原地替换

两种构建是同一个应用，并沿用 0.3.x Tauri GUI 的名称：

- macOS：`MacinMeter DR GUI.app`，identifier `com.macinmeter.dr.gui`，最低 macOS 11.0；
- Windows：按当前用户安装到 `%LOCALAPPDATA%\MacinMeter DR GUI`（或 Tauri 安装时
  在 `HKCU\Software\macinmeter\MacinMeter DR GUI` 记录的目录），卸载键
  `HKCU\…\Uninstall\MacinMeter DR GUI`，主程序 `macinmeter-gui.exe`，开始菜单项
  `MacinMeter DR GUI.lnk`。

因此安装任一构建都会替换 Tauri 安装或另一种构建；开始菜单、“应用和功能”与已有桌面
快捷方式继续指向同一路径。安装程序重建 Tauri 留下的快捷方式，因为 Tauri 给它们
写了本应用不设置的 AppUserModelID。用户设置位于安装目录之外，卸载时保留。

### 3. 打包脚本自己组装制品

- macOS：脚本写出 `.app`（`Info.plist`、`icon.icns`、`Contents/Resources/licenses`），
  用不带身份的 ad hoc 签名封装（Apple Silicon 运行未签名 bundle 的最低要求），
  再用 `hdiutil` 写出带 Applications 链接的 HFS+ DMG。构建时设置
  `MACOSX_DEPLOYMENT_TARGET=11.0`。
- Windows：executable 由 `build.rs` 通过 embed-resource 链接图标、带 `FontBuild`
  值的版本资源与 application manifest（`asInvoker`、Common Controls 6）。安装程序是
  手写的 `apps/macinmeter-desktop/packaging/windows/installer.nsi`，只用 NSIS 3
  自带的头文件与插件，由 `makensis -WX` 编译。CI 在 runner 缺少 `makensis` 时安装
  固定版本的 NSIS。

两种构建都随附 `LICENSE` 与 `THIRD_PARTY_NOTICES.md`；`bundled-fonts` 另附两份
SIL OFL 文本，满足 OFL 对随字体分发许可文本的要求。

### 4. 验收从字节判断字体构建

验证器不只相信 manifest 中的 `fontBuild`：bundled 构建的 executable 含两款内嵌
字体 CFF 表中的 PostScript 名称，system 构建两者都不含，只含其一即视为损坏。
macOS 另外核对 bundle 名称、executable 名称与 `LC_BUILD_VERSION` 的部署目标；
Windows 另外核对安装后 `licenses` 目录的文件集合。ADR-0010/0015 原有的 DMG 挂载、
架构、版本资源与签名缺失检查保持不变。

### 5. 发行 staging 不再需要 Node.js

GUI 制品不再含 Tauri 前端，staging 不再运行 `npm`，manifest 的 toolchain identity
只记录 Rust/Cargo，source identity 只记录 Cargo lockfile。未签名候选仍要求精确
Rust 1.98。Linux 门禁继续构建 Tauri 前端，版本镜像仍由仓库契约检查。

## 后果

- 每个平台的 GUI 下载变成二选一，发行说明需要说明两者的区别。
- 与 Tauri 版相比，GUI 不再依赖 WebView2 / WKWebView。
- CI 的 Windows 与 macOS job 不再安装 Node.js；Windows job 可能需要安装 NSIS。

## 未覆盖

- 验收仍不启动 GUI、不实际安装，也不在真实主机上验证“覆盖 Tauri 安装”的升级路径；
  这需要维护者在真实 Windows 与 macOS 主机上检查一次。
- rfd 的 `common-controls-v6` feature 仍未启用：manifest 只链接进 GUI 可执行文件，
  不进测试二进制，而该 feature 会让未带 manifest 的进程无法启动。
