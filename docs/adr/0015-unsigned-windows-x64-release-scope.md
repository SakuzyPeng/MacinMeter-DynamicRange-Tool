# ADR-0015：0.3.0 同时发行未签名 Windows x64 与 Apple Silicon macOS

- 状态：Accepted
- 日期：2026-08-09
- 范围：0.3.0 发行平台、CLI/GUI 制品、候选保留与验收深度
- 扩展：[ADR-0011](0011-unsigned-apple-silicon-release-scope.md) 的发行范围；其
  macOS 制品定义、未签名含义与最终发布边界原样保留
- 依赖：[ADR-0009](0009-windows-x64-ci-expansion.md) 的 `windows-2025` 门禁

## 背景

ADR-0011 把 0.2.0 首发限定为未签名 Apple Silicon macOS，理由是当时扩展到其他桌面
平台“不会增加首发所需的核心事实，反而会扩大未经实际验收的支持面”。

那个理由在 0.3.0 已经不成立：

- ADR-0009 起 `windows-2025` runner 就在跑同一套 clippy、workspace test 与 release
  CLI 冒烟，Windows CLI 从未只是“能编译”；
- Windows Tauri GUI 已在 CI 以 workflow-dispatch 测试构建产出 NSIS 安装包，并由
  维护者在真实 Windows 主机上安装、运行、使用；
- 本轮 ADR-0014 的双主机性能测量本身就以 Windows 为主要计时主机。

也就是说 Windows 侧缺的不是验收，是 staging 流水线：没有任何流程把那些字节整理
成带 manifest、校验和与结构化验证的候选制品。本 ADR 补上这一段，并把它与 macOS
放进同一次发行。

## 决策

### 1. 0.3.0 的发行范围是两个平台，四个制品

| 平台 | Rust target | CLI | GUI |
| --- | --- | --- | --- |
| macOS | `aarch64-apple-darwin` | archive | DMG |
| Windows | `x86_64-pc-windows-msvc` | archive | NSIS 安装包 |

macOS 侧最低系统版本保持 11.0。Windows 侧不声明低于 `windows-2025` runner 与
Tauri 2 webview 依赖所要求的系统版本，也不承诺 ARM64 Windows、32 位或 Linux。

### 2. 两个平台都不签名，理由相同

macOS 不做 Developer ID 签名与 notarization，Windows 不做 Authenticode 签名。

这不是“Windows 那边省一步”：面向个人的 OV/EV 代码签名证书由 CA 核验身份，证书
subject 就是法定姓名，并在文件属性与 SmartScreen 发布者提示中可见。两个平台上
“签名”都意味着把维护者法定姓名嵌入每一份公开制品，因此结论一致。

代价也必须一致地写进用户文案：macOS 上 Gatekeeper 可能要求显式“打开”，Windows
上 SmartScreen 会显示未知发布者。release 页面必须同时承担这两条警告。

### 3. Windows GUI 的验收必须真的打开安装包

`hdiutil` 在 Windows 上没有对应物，但“只确认安装包是一个合法 PE”与 macOS 侧挂载
DMG 后检查 `.app` 不对等，会让两个平台的同一句“已验证”含义不同。

因此 Windows GUI 候选必须用 `7z` 把 NSIS 安装包解到临时目录，并在其中：

1. 找到唯一的 `macinmeter-gui.exe`；
2. 确认它是 PE 可执行文件；
3. 确认其版本资源等于 workspace 版本；
4. 记录安装包与内层 executable 各自的 SHA-256；
5. 无论成功失败都清理临时目录。

`7z` 因此成为 Windows staging 的显式前置依赖，与 macOS 侧依赖 `hdiutil` 同级。
缺失时必须直接失败，不得降级为较浅的检查后仍标记为候选。

### 4. Windows 候选与 macOS 候选走同一形状

`scripts/stage-release.py` 增加 `--unsigned-windows-x64-candidate`，与既有
`--unsigned-macos-arm64-candidate` 约束对称：clean source、精确 toolchain、CLI 与
GUI 同时在场、拒绝 `--allow-dirty` 与 `--replace`、在 manifest 中标记为
`unsigned_windows_x64_release_candidate`。

一次 `stage` 只产出一个平台的候选。两个候选各自在自己的 runner 上生成，由发布时
的人工步骤合并成一次 Release，而不是让任一平台交叉编译另一平台的 GUI。

### 5. CI 形状镜像 macOS

Windows job 与 macOS job 采取同一模式：

- `main` push：`stage --include-gui`，构建并结构化验证，产物随 runner 丢弃；
- 无输入 workflow dispatch：确认 ref 为 `refs/heads/main` 后生成保留候选，固定 SHA
  的 `actions/upload-artifact` 保留 14 天。

`macinmeter-windows-test-build-*` 这个只作测试用的 artifact 名称随之退役。

仓库合同检查中“Windows 构建不得声称或产出 release candidate”的三条闸不是删除，
而是改写为新的发行形状：Windows job 必须同时具备 stage 命令、候选 flag、`main`
ref 断言与候选 artifact 名。闸的作用始终是“未经上述验收的字节不得被称为候选”，
本 ADR 只是把验收补齐后同步移动了闸的位置。

## 不在范围内

- Authenticode、EV 证书、SmartScreen 声誉积累；
- ARM64 Windows、32 位 Windows、Linux GUI 或新的 CLI 平台；
- macOS Intel 与 universal binary；
- MSI/WiX 安装包：NSIS 是当前唯一已构建并实际安装使用过的 Windows 形态；
- 自动 tag、自动 GitHub Release 与长期 artifact retention；
- 在 release 前改变算法、codec 支持面或性能声明。

## 最终发布边界

在 ADR-0011 既有条件之上追加：两个平台的候选必须来自**同一个 source commit**，
且各自整次 workflow conclusion 为 success。任一平台缺席时，不得以“先发一个平台”
的方式发布 0.3.0——那会让 release 页面上的支持声明与实际验收范围不一致。

## 影响

- 发行面与已在真实 runner 与真实主机上验证的范围一致，Windows 不再是“能构建但
  没有制品”的中间态；
- 两个平台的“已验证”含义相同，因为两边都真的打开了安装包；
- `7z` 成为 Windows staging 的硬依赖，本地复现需要它；
- 用户在两个平台都会遇到未签名提示，文案必须持续承担；
- 发布步骤从一个平台变成两个平台的人工合并，比单平台更容易出现“只更新了一半”的
  Release，因此 source commit 一致性成为显式发布前条件。
