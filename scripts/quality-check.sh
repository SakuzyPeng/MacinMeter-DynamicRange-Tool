#!/bin/bash

# MacinMeter DR Tool - 代码质量检查脚本
# 使用方法: ./scripts/quality-check.sh [--quick]

set -e  # 遇到错误立即退出

echo "🎵 MacinMeter DR Tool - 代码质量检查"
echo "=================================="

# 检查是否是快速模式
QUICK_MODE=""
if [[ "$1" == "--quick" ]]; then
    QUICK_MODE="true"
    echo "⚡ 快速检查模式"
else
    echo "🔍 完整检查模式"
fi

echo ""

# 1. 代码格式检查
echo "📝 检查代码格式..."
if cargo fmt --check; then
    echo "✅ 代码格式正确"
else
    echo "❌ 代码格式需要修复，运行: cargo fmt"
    exit 1
fi

echo ""

# 2. Clippy静态分析（包含音频项目特定检查）
echo "🔍 运行Clippy静态代码分析..."
cargo clippy -- -D warnings \
    -W clippy::cast_lossless \
    -W clippy::float_arithmetic \
    -W clippy::indexing_slicing

if [[ $? -eq 0 ]]; then
    echo "✅ Clippy检查通过"
else
    echo "❌ Clippy发现问题，请修复后重试"
    exit 1
fi

echo ""

# 3. 编译检查
echo "🏗️  编译检查..."
if cargo check; then
    echo "✅ 编译检查通过"
else
    echo "❌ 编译失败"
    exit 1
fi

echo ""

# 4. 依赖安全扫描
echo "🔒 依赖安全扫描..."
if cargo audit; then
    echo "✅ 依赖安全检查通过"
else
    echo "⚠️  发现安全漏洞，请更新相关依赖"
    if [[ -z "$QUICK_MODE" ]]; then
        exit 1
    fi
fi

echo ""

# 5. 单元测试（非快速模式）
if [[ -z "$QUICK_MODE" ]]; then
    echo "🧪 运行单元测试..."
    if cargo test; then
        echo "✅ 单元测试通过"
    else
        echo "❌ 单元测试失败"
        exit 1
    fi

    echo ""

    # 6. 发布模式编译检查
    echo "🚀 发布模式编译检查..."
    if cargo build --release; then
        echo "✅ 发布模式编译成功"
    else
        echo "❌ 发布模式编译失败"
        exit 1
    fi

    echo ""

    # 7. 依赖关系检查
    echo "📊 检查依赖关系..."
    echo "依赖树："
    cargo tree --depth 2

    echo ""
    echo "检查重复依赖："
    cargo tree --duplicates

    echo ""
fi

echo "🎉 所有代码质量检查通过！"
echo ""
echo "💡 提示："
echo "   - 快速检查: ./scripts/quality-check.sh --quick"
echo "   - 完整检查: ./scripts/quality-check.sh"
echo "   - 格式化代码: cargo fmt"
echo "   - 查看依赖: cargo tree"