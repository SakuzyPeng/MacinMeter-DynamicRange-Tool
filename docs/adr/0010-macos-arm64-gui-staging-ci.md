# ADR-0010：自动 CI 扩展至 macOS arm64 与 GUI staging

- 状态：Accepted；候选保留边界由 ADR-0011 扩展
- 日期：2026-07-22
- 范围：GitHub Actions 的第三平台验证层与发布前 GUI 制品边界

## 背景

ADR-0008 恢复了 Ubuntu 自动门禁，ADR-0009 随后加入 Windows Server 2025 x64。
两层都已在 clean hosted runner 上通过，但它们没有构建 macOS `.app`/`.dmg`，也
无法执行依赖 `hdiutil`、`lipo` 与 bundle metadata 的现有 release artifact smoke。

M5 已经建立 `scripts/stage-release.py stage --include-gui`：从 locked source 构建
current-host CLI 与 Tauri DMG，反向运行 CLI、只读挂载 DMG，并核对 bundle identity、
version、executable、准确架构及 SHA-256。缺失的是一个独立于开发机的 clean macOS
执行环境，而不是第二套 packaging 实现。

## 决策

现有单一 workflow 增加一个与 Ubuntu、Windows 并行的 `macos-26` job。该 GitHub
hosted label 固定为 arm64；不使用会随平台迁移的 `macos-latest`，也不把 Intel
larger runner、x86_64 或 universal binary 纳入本决策。

macOS job 沿用相同的 PR、`main` push、manual trigger、只读权限、并发取消、完整
SHA action identity、MSRV 1.88 与根 lockfile。timeout 单独设为 60 分钟，以容纳
release CLI、Tauri app/DMG 和挂载验证。

所有 trigger 执行：

1. repository contract；
2. workspace/all-targets/all-features strict Clippy；
3. workspace/all-targets tests，包括 macOS CLI black-box 与 Tauri Rust targets。

pull request 到此结束，不为每次候选变更额外构建 release profile。`main` push 与
manual dispatch 继续安装固定 Node.js 22 和 committed npm lockfile，然后执行：

```bash
python3 scripts/stage-release.py stage --include-gui
```

该命令必须从 clean checkout 写入新的 staging directory，不使用 `--allow-dirty` 或
`--replace`。它同时验证最终 CLI archive 与 arm64 DMG。产物只存在于 runner 工作目录，
workflow 不上传、不签名、不 notarize，也不创建 GitHub Release。manifest 继续明确记录
`local_staging_only` / `local_unnotarized`；这里的 staging 是发布前结构门禁，不是
公开发行。

repository contract 固定 runner label、macOS Clippy/test、main/manual-only Node/npm/
GUI staging、clean staging 参数以及唯一允许的 staging 位置。同时继续禁止 hostile
corpus、performance、artifact upload、签名、notarization 与 release publication。

## 边界

本决策不验证：

- macOS x86_64 或 universal GUI；
- Developer ID identity、hardened runtime、entitlements、notarization ticket 或
  Gatekeeper 下载路径；
- GUI 启动、交互或真实文件选择器；
- artifact retention、provenance、SBOM 或 GitHub Release；
- Windows/Linux GUI packaging。

因此 macOS job 成功表示固定 `macos-26` arm64 clean runner 能通过 Rust 契约，并能
构建和结构化验证 current-host CLI/DMG；它仍不是“用户可下载的正式版本已经发布”。

## 影响

- PR 在合并前得到第三个平台的 Rust/Tauri 编译与测试反馈；
- `main` 获得独立 clean host 上的 arm64 CLI/DMG staging 证据；
- 本地与 CI 复用同一 artifact verifier，不增加一条只服务 CI 的 packaging 路径；
- macOS runner 与 release profile 会增加主干运行成本，因此 PR 不执行 GUI staging，
  也不扩张为 OS/architecture matrix；
- 下一步发布准备可以集中处理 Developer ID、notarization、制品保留与发布权限，而
  不再把“GUI 能否形成并通过结构 smoke 的 DMG”混入其中。

本决策取代 ADR-0009 中“暂不继续增加 macOS”的资源边界。ADR-0008/0009 对 trigger、
权限、Windows 职责和其余禁止任务的决定继续有效。
