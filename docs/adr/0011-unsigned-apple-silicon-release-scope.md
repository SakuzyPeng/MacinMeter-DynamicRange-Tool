# ADR-0011：0.2.0 首发限定为未签名 Apple Silicon macOS

- 状态：Accepted
- 日期：2026-07-23
- 范围：0.2.0 发行平台、GUI 制品、候选保留与发布权限

## 背景

ADR-0010 已证明 `macos-26` arm64 clean runner 可以通过 Rust/Tauri 门禁，并能构建、
挂载和反向验证 current-host CLI archive 与 DMG。该流程此前故意把所有结果标记为
`local_staging_only`，也不保留 CI 字节，因此还不能形成可供发布前复核的候选制品。

当前没有 Developer ID 签名与 notarization 需求。扩展至 Intel、universal binary 或
其他桌面平台也不会增加首发所需的核心事实，反而会扩大未经实际验收的支持面。

## 决策

0.2.0 的发行范围固定为：

- 操作系统：macOS；
- CPU：Apple Silicon，Rust target 为 `aarch64-apple-darwin`；
- 最低系统版本：macOS 11.0；
- 制品：arm64 CLI archive 与包含 arm64 `.app` 的 DMG；
- 分析结果：结构化报告保留固定数值参数，不暴露内部 profile 或状态；
- 签名：不执行 Developer ID signing；
- 公证：不执行 notarization 或 stapling。

“未签名”在用户文案中表示没有 Developer ID 身份。Mach-O 可能带有编译/链接工具链
产生的 ad-hoc metadata，但它不建立开发者身份，也不改变 Gatekeeper 边界。

`scripts/stage-release.py` 增加显式
`--unsigned-macos-arm64-candidate` 模式。该模式必须：

1. 来自 clean Git source；
2. 使用准确的 Rust 1.88、Node.js 22 与 `aarch64-apple-darwin` host，并在 manifest
   记录完整 Rust/Cargo/Node/npm identity；
3. 同时包含 CLI 与 GUI；
4. 拒绝 `--allow-dirty` 与 `--replace`；
5. 验证 DMG、bundle identity/version、唯一 arm64 executable、macOS 11.0 minimum、
   CLI/schema/固定参数、SHA-256 和签名观察；
6. 在 manifest 中标记为 `unsigned_macos_arm64_release_candidate`，而不是
   `local_staging_only` 或已公开发行。

普通 `main` push 继续只产生随 runner 丢弃的 local staging。只有无输入的手动
workflow dispatch 会在确认 ref 为 `refs/heads/main` 后生成 unsigned candidate，并
使用固定 SHA 的 `actions/upload-artifact` 保留 14 天。workflow 仍保留全局
`contents: read`，不会创建 tag、GitHub Release 或公开下载资产。

仓库保存一份双语 `docs/RELEASE_DRAFT_0.2.0.md`，作为后续 draft GitHub Release 的
说明来源。它必须醒目标明 Apple Silicon-only、macOS 11.0+、无 Developer ID、无
notarization，以及 Gatekeeper 可能阻止直接打开。

## 不在范围内

- macOS Intel、universal binary 与 Rosetta 支持声明；
- Windows/Linux GUI 或本轮新的 CLI 平台制品；
- Developer ID、hardened-runtime 签名验收、notarization、stapling；
- 自动 tag、自动 GitHub Release、自动 latest 标记或长期 artifact retention；
- 在 release 前改变算法 profile、codec 支持面或性能声明。

旧 0.1.x Release 中存在的 Intel/Windows/Linux 资产只描述历史版本，不构成 0.2.0
支持承诺。

## 最终发布边界

候选 artifact 成功只完成“可下载字节准备”。正式发布仍需要一次独立确认，并要求：

- candidate manifest 的 source commit 与准备创建的 `v0.2.0` tag 完全一致；
- 生成 candidate 的整次三平台 workflow conclusion 为 success；
- 对将上传的原始文件重新运行 `stage-release.py verify`；
- 使用固定 release draft 文案；
- 上传 DMG、CLI archive、`RELEASE_MANIFEST.json` 与 `SHA256SUMS`；
- 明确接受 unsigned Gatekeeper 体验后，才创建或发布 GitHub Release。

## 影响

- 发布面与已在真实 arm64 runner 上验证的范围一致；
- 手动 candidate 可在发布前下载、复核和试用，同时不会被误认为正式 Release；
- 不需要把发布权限或 secrets 放进普通验证 workflow；
- 用户安装体验不如签名/公证版本顺畅，release 页面必须持续承担清晰警告；
- 将来加入签名、Intel/universal 或其他平台时，需要新的证据与独立决策。

本决策扩展 ADR-0010 的“CI 不保留制品”边界；ADR-0010 的 PR/main 正确性门禁、
clean staging、只读权限及不自动发布约束继续有效。
