# ADR-0009：自动 CI 扩展至 Windows x64

- 状态：Accepted
- 日期：2026-07-22
- 范围：GitHub Actions 的第二平台验证层

## 背景

ADR-0008 恢复的首次 Ubuntu 自动运行在 commit `d84c7d3` 上成功完成，job 用时约
2 分 50 秒。其中 Linux 依赖安装约 46 秒、Clippy 约 47 秒、workspace tests 约
55 秒。当前单平台门禁成本可控，但它不能执行 Windows 条件编译，也不能验证 README
中已有的 Windows CLI 构建入口。

扩展 CI 应优先增加新的事实，而不是复制平台无关的 formatter、Python reference
tools 或 frontend 工作。

## 决策

现有单一 workflow 增加一个与 Ubuntu job 并行的 Windows Server 2025 x64 job。
它和 Ubuntu job 使用相同的 PR、`main` push、manual trigger、只读权限、并发取消、
45 分钟 timeout、MSRV 1.88、根 Cargo lockfile 与完整 SHA action identity。

Windows job 对所有 trigger 执行：

1. repository contract；
2. workspace/all-targets/all-features strict Clippy；
3. workspace/all-targets tests，包括 Windows CLI black-box 与 Tauri Rust targets。

pull request 不额外构建 release profile。`main` push 与 manual dispatch 会继续：

4. 构建 Windows release CLI；
5. 运行 `--version` smoke；
6. 以仓库内 WAV fixture 运行 release analysis，并解析 schema-v3 analysis JSON。

release executable 只在 runner 工作目录中接受 smoke，不上传、不进入 release
staging，也不是发行制品。Linux job 的 release build 仍只由 manual dispatch 增量
执行。

Windows job 不重复 Node/frontend 与 Python tool suites；这些平台无关门禁继续由
Ubuntu job 负责。两个 job 是职责不同的明确平台层，不改造成组合式 OS/toolchain
matrix。

## 边界

本决策不加入：

- Windows GUI packaging、MSI/NSIS、签名或 artifact upload；
- macOS hosted runner、x86_64/universal DMG 或 notarization；
- Linux/Windows architecture matrix；
- performance、hostile corpus、fuzz、advisory network 或 release staging；
- branch protection/ruleset 变更。

因此 Windows job 成功只证明固定 Windows Server 2025 x64 clean runner 上的 Rust
workspace 与临时 release CLI smoke 成功，不表示 Windows GUI 包或公开发行已经验收。

`scripts/check-repository-contract.py` 固定两个 runner identity、Windows Clippy/test、
main/manual-only release build/smoke 及原有禁止任务。

## 影响

- PR 在合并前得到实际 Windows 编译与测试反馈；
- `main` 获得未上传的 Windows release CLI 执行证据；
- 两个平台并行运行，不把 Linux 约三分钟基线串行叠加到反馈时间；
- Windows runner 会增加远端资源消耗，因此暂不继续增加 macOS 或平台矩阵。

本决策取代 ADR-0008 的“单 Ubuntu job”和“暂不增加第二 OS”边界；ADR-0008 对
trigger、权限、并发取消及排除高风险任务的其余决定继续有效。
