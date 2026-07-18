# M0 workspace CI

架构重建期间，本目录只保留一个手动触发的最小工作流：
[`ci-cd.yml`](ci-cd.yml)。

## 当前行为

- 仅由 GitHub Actions 页面上的 `workflow_dispatch` 手动触发；
- 仅使用一个 `ubuntu-latest` job；
- 安装 workspace 中 Tauri/Rust crate 所需的 Linux 系统依赖；
- 对整个 Cargo workspace 依次执行：

  ```bash
  cargo fmt --all -- --check
  cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
  cargo test --locked --workspace --all-targets --all-features
  ```

工作流当前没有：

- push、pull request 或 tag 自动触发；
- OS/target 矩阵和跨平台构建；
- cargo-audit 或 advisory 忽略列表；
- coverage、benchmark、artifact 上传；
- Tauri 安装包、GitHub Release 或发布权限。

## 为什么暂时缩减

M0 正在把 0.1.x 单 crate 和旧适配层重建为 0.2.0 workspace。此时保留旧矩阵、
旧二进制路径和旧发布脚本只会验证即将删除的结构。最小 CI 的目标是持续确认
workspace 能格式化、通过严格 Clippy 并完成测试，而不是提前恢复发行承诺。

这不是对安全审计、多平台构建或发布验证的永久豁免。恢复条件包括：

1. workspace 成员和依赖边界稳定；
2. 0.2.0 CLI 与 Tauri 已切换到同一 application API；
3. lockfile、MSRV 和首批支持平台已固定；
4. portable CPU baseline、制品 smoke test 和 release 内容已有明确契约；
5. advisory 例外具有原因、负责人和到期日。

决策背景见
[`docs/adr/0001-m0-0.2.0-trusted-trunk-rebuild.md`](../../docs/adr/0001-m0-0.2.0-trusted-trunk-rebuild.md)。

## 手动运行

1. 打开仓库的 **Actions** 页面；
2. 选择 **M0 Workspace CI**；
3. 选择 **Run workflow**。

该工作流没有跳过测试或选择目标平台的输入；一次运行总是执行完整的三个
workspace 门禁。
