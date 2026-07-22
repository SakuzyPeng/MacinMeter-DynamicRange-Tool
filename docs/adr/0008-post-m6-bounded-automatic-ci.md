# ADR-0008：M6 后恢复有界自动 CI

- 状态：Accepted；单平台边界由 ADR-0009 扩展
- 日期：2026-07-22
- 范围：GitHub Actions 验证触发、权限与资源边界

## 背景

M0 施工期关闭自动 CI，是为了避免在主干、接口和测试仍持续换轨时反复消费远端
资源。M5 随后固定 workspace、lockfile、版本镜像、测试分层与本地 release staging；
M6 又完成了性能证据链。原先“施工过程不值得持续远端验证”的条件已经结束。

当前恢复 CI 的目的，是在变更进入 `main` 前自动执行现有正确性门禁，而不是恢复
旧版的多平台矩阵、自动发布或把本地研究任务搬到 GitHub runners。

## 决策

仓库继续只保留一个 `.github/workflows/workspace-validation.yml`，其触发固定为：

- 每个 pull request；
- push 到 `main`；
- 无输入参数的 `workflow_dispatch`。

普通 feature-branch push 不触发 workflow；分支进入 pull request 后才消费远端资源。
同一 trigger 与 pull request/ref 的新提交会取消旧的 in-progress run；手动验证不会
取消自动 `main` 验证。PR 不使用 path filter，避免 required check 因路径判断而
长期处于未运行状态。

自动运行使用一个 Ubuntu 24.04 job、45 分钟 timeout 和只读 `contents` 权限。
checkout 不保留写凭据；第三方 action 固定到完整 commit SHA。工具链继续固定 Rust
1.88 与 Node.js 22，Cargo/npm 都只使用已提交 lockfile。

pull request 与 `main` push 的标准门禁包括：

1. repository contract；
2. `cargo fmt`；
3. workspace/all-targets/all-features strict Clippy；
4. workspace/all-targets tests；
5. reference-tool 与 repository-tool Python 单元测试；
6. `npm ci` 与 TypeScript/Vite production build。

`workflow_dispatch` 在相同门禁上额外执行 release CLI build。它仍不执行 local
release staging，也不上传制品。

以下任务继续明确排除在普通 CI 之外：

- hostile malformed-media subprocess verifier 与 fuzz；
- M6 performance corpus、baseline 和 profiler；
- DMG/CLI release staging、签名、notarization、上传和 GitHub Release；
- 网络 advisory 数据库；
- OS/architecture/toolchain matrix。

`scripts/check-repository-contract.py` 固定上述 trigger、main-only push、取消策略、
只读权限与禁止任务，防止一次普通依赖或文档修改无意扩张 CI 权限和成本。

## 影响

- pull request 与合并后的 `main` 都会得到远端、clean-runner 的确定性反馈；
- feature 分支在建立 PR 前仍只承担本地验证成本；
- 同一 PR 的过时运行会被取消，但 PR run 与合并后的 main run 各自保留；
- release、性能和 hostile-input 风险边界没有改变；
- CI 成功只说明该 commit 通过声明的单 Ubuntu 门禁，不表示跨平台发行就绪。

本决策取代 ADR-0006 §4 和 ADR-0007 §7 中对“当前远端 CI 继续纯手动”的持续性
约束；那些文字仍准确记录 M5/M6 当时的执行状态，不回写为自动 CI。

## 未采用方案

### 所有分支 push 都运行

存在 pull request 时会为同一提交重复运行 push 与 PR gate，不能证明额外性质。

### 只保留 main push

反馈发生在合并之后，不能作为进入主干前的门禁。

### 使用 path filter 跳过文档或局部变更

节省有限，但会使 required-check 状态和跨层依赖判断变得含糊。当前单 job 与并发取消
已经提供更直接的成本边界。

### 同时恢复发布与平台矩阵

签名、notarization、provenance、平台覆盖与发布权限仍没有独立决策。验证恢复不能被
解释为这些能力已经完成。
