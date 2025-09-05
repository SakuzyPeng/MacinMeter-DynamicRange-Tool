# 🚀 MacinMeter DR Tool - 预提交钩子

本目录包含预提交钩子，用于在本地提交前执行与GitHub Actions CI相同的代码质量检查。

## 📋 功能特性

### ✅ 包含的检查项目
- **🎨 代码格式检查** - `cargo fmt --check`
- **📎 静态分析** - `cargo clippy --all-targets --all-features -- -D warnings`
- **🔨 编译检查** - `cargo check --all-targets --all-features`
- **🧪 单元测试** - `cargo test`
- **🛡️ 安全审计** - `cargo audit` (可选，需要安装)

### 🎯 优势
- **提前发现问题**: 在本地就捕获CI中可能出现的错误
- **节省时间**: 避免推送后等待CI反馈的时间
- **保持一致**: 与GitHub Actions完全相同的检查逻辑
- **灵活控制**: 可以临时跳过或完全卸载

## 📦 安装方法

### 1. 自动安装（推荐）
```bash
# 在项目根目录运行
chmod +x scripts/install-pre-commit.sh
./scripts/install-pre-commit.sh
```

### 2. 手动安装
```bash
# 复制钩子到正确位置
cp scripts/pre-commit .git/hooks/pre-commit

# 设置执行权限
chmod +x .git/hooks/pre-commit
```

## 📝 使用方法

### 正常使用
```bash
# 正常提交 - 会自动运行所有检查
git commit -m "你的提交信息"
```

### 临时跳过钩子
```bash
# 紧急情况下跳过预提交检查
git commit --no-verify -m "紧急修复"
```

### 单独测试钩子
```bash
# 不提交，只运行检查
.git/hooks/pre-commit
```

## 🔧 故障排除

### 检查失败怎么办？
1. **代码格式问题**: 运行 `cargo fmt`
2. **Clippy警告**: 根据提示修复代码
3. **编译错误**: 修复代码中的错误
4. **测试失败**: 修复失败的测试用例
5. **安全问题**: 检查 `cargo audit` 的建议

### 卸载钩子
```bash
# 完全移除预提交钩子
rm .git/hooks/pre-commit

# 如果有备份，可以恢复原来的钩子
ls .git/hooks/pre-commit.backup.*
```

### 钩子无法执行？
```bash
# 确保有执行权限
chmod +x .git/hooks/pre-commit

# 检查文件是否存在
ls -la .git/hooks/pre-commit
```

## 🚀 性能优化

### 加速检查
- **安装cargo-audit缓存**: 第一次运行较慢，之后会快很多
- **并行检查**: 钩子按顺序执行，失败时立即停止
- **增量编译**: Cargo会利用增量编译加速

### 预期时间
- **首次运行**: ~30-60秒 (包含依赖下载)
- **后续运行**: ~10-20秒 (利用缓存)
- **仅格式检查**: ~2-5秒 (最常见的失败)

## 💡 最佳实践

### 开发工作流建议
```bash
# 1. 开发过程中定期检查
cargo fmt && cargo clippy

# 2. 提交前确保测试通过
cargo test

# 3. 正常提交 - 钩子会自动运行
git commit -m "feature: 添加新功能"

# 4. 如果钩子失败，修复后重试
# ... 修复问题 ...
git commit -m "feature: 添加新功能"
```

### 团队协作
- **统一标准**: 所有开发者使用相同的钩子
- **CI一致性**: 本地检查通过 = 远程CI通过
- **快速反馈**: 避免"提交 → 等待 → 修复 → 重新提交"循环

## 📊 与GitHub Actions的关系

| 检查项目 | 预提交钩子 | GitHub Actions |
|---------|------------|----------------|
| 代码格式 | ✅ cargo fmt | ✅ cargo fmt |
| 静态分析 | ✅ cargo clippy | ✅ cargo clippy |
| 编译检查 | ✅ cargo check | ✅ cargo check |
| 单元测试 | ✅ cargo test | ✅ cargo test |
| 安全审计 | ✅ cargo audit | ✅ cargo audit |
| 多平台构建 | ❌ 仅本机 | ✅ 4个平台 |
| 二进制生成 | ❌ | ✅ |
| Release创建 | ❌ | ✅ |

**结论**: 预提交钩子覆盖了CI中所有的代码质量检查，确保推送的代码能通过远程构建。