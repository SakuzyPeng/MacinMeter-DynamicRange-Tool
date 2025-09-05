#!/bin/bash
# 📦 MacinMeter DR Tool - 预提交钩子安装脚本

set -e

echo "📦 MacinMeter DR Tool - 安装预提交钩子"
echo "======================================="

# 检查是否在git仓库中
if [ ! -d ".git" ]; then
    echo "❌ 错误: 当前目录不是git仓库"
    echo "请在项目根目录运行此脚本"
    exit 1
fi

# 检查是否存在预提交钩子模板
if [ ! -f "scripts/pre-commit" ]; then
    echo "❌ 错误: 找不到预提交钩子模板 scripts/pre-commit"
    exit 1
fi

# 备份现有的预提交钩子（如果存在）
if [ -f ".git/hooks/pre-commit" ]; then
    echo "📋 备份现有预提交钩子..."
    cp .git/hooks/pre-commit .git/hooks/pre-commit.backup.$(date +%Y%m%d_%H%M%S)
    echo "✅ 已备份到 .git/hooks/pre-commit.backup.$(date +%Y%m%d_%H%M%S)"
fi

# 安装新的预提交钩子
echo "📦 安装预提交钩子..."
cp scripts/pre-commit .git/hooks/pre-commit

# 设置执行权限
chmod +x .git/hooks/pre-commit

echo "✅ 预提交钩子安装完成！"
echo ""

# 测试钩子是否工作
echo "🧪 测试预提交钩子..."
if .git/hooks/pre-commit; then
    echo ""
    echo "🎉 预提交钩子测试成功！"
    echo ""
    echo "📝 使用说明:"
    echo "  • 现在每次 'git commit' 时都会自动运行质量检查"
    echo "  • 如果检查失败，提交将被阻止"
    echo "  • 临时跳过钩子: git commit --no-verify"
    echo "  • 卸载钩子: rm .git/hooks/pre-commit"
    echo ""
    echo "🔧 包含的检查项目:"
    echo "  ✓ 代码格式检查 (cargo fmt --check)"
    echo "  ✓ 静态分析 (cargo clippy)"
    echo "  ✓ 编译检查 (cargo check)"
    echo "  ✓ 单元测试 (cargo test)"
    echo "  ✓ 安全审计 (cargo audit, 可选)"
    echo ""
    echo "💡 提示: 这些检查与GitHub Actions CI完全一致"
else
    echo ""
    echo "⚠️  预提交钩子测试失败，但已安装"
    echo "   请先修复上述问题再进行提交"
fi