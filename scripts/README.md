# Pre-commit checks

M0 期间，预提交钩子只提供快速、本地、确定性的基础反馈：

```bash
cargo fmt --all -- --check
cargo check --locked --workspace
```

钩子不运行 Clippy、完整测试、Docker 或 `cargo audit`，也不会安装或刷新网络审计
数据库。完整 workspace Clippy 和测试由手动 GitHub Actions 工作流执行；开发者
也可以在本地按需运行。

## 安装

在仓库根目录执行：

```bash
chmod +x scripts/install-pre-commit.sh
./scripts/install-pre-commit.sh
```

安装脚本会备份现有 `.git/hooks/pre-commit`，复制
[`scripts/pre-commit`](pre-commit)，并显示当前两项检查。它不会在安装时执行验证。

也可以手动安装：

```bash
cp scripts/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

## 直接运行

```bash
scripts/pre-commit
```

脚本通过 `git rev-parse --show-toplevel` 定位仓库根目录，因此从子目录或安装后的
`.git/hooks/pre-commit` 调用都使用同一 workspace。

## 失败处理

- 格式检查失败：运行 `cargo fmt --all`，检查 diff 后重试；
- 编译检查失败：运行 `cargo check --locked --workspace` 查看完整诊断并修复；
- 紧急跳过：`git commit --no-verify`。跳过只绕过本地钩子，不代表改动已验证。

## 完整本地验证

提交较大变更或手动触发远程 CI 前，建议运行：

```bash
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
```

安全审计、多平台构建和发布验证将在 0.2.0 workspace 与依赖边界稳定后恢复为
独立门禁。当前缩减范围记录于
[`ADR-0001`](../docs/adr/0001-m0-0.2.0-trusted-trunk-rebuild.md)。

## 卸载

```bash
rm .git/hooks/pre-commit
```

如安装脚本创建了备份，可从 `.git/hooks/pre-commit.backup.*` 中选择需要的版本
恢复。
