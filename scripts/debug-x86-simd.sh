#!/bin/bash
# 🔍 x86 SIMD调试脚本 - 获取详细的失败信息

set -e

echo "🔍 MacinMeter DR Tool - x86 SIMD调试"
echo "===================================="

# 检查Docker是否可用
if ! command -v docker &> /dev/null; then
    echo "❌ Docker未安装，无法进行x86调试"
    exit 1
fi

# 创建轻量调试Dockerfile
cat > Dockerfile.debug-x86 << 'EOF'
FROM --platform=linux/amd64 rust:1.88

# 安装基本依赖
RUN apt-get update && apt-get install -y build-essential pkg-config && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .

# 配置x86_64目标
RUN rustup target add x86_64-unknown-linux-gnu

# 只编译，不运行测试，避免panic中断
RUN echo "🔧 编译x86_64版本..." && \
    cargo build --target x86_64-unknown-linux-gnu --release

# 尝试运行测试，使用continue-on-error方式
RUN echo "🧪 运行SIMD测试 (debug模式)..." && \
    (timeout 30 cargo test --target x86_64-unknown-linux-gnu processing::simd::tests::test_simd_vs_scalar_consistency -- --nocapture 2>&1 || true) | head -50

# 如果上面失败，尝试更简单的单元测试
RUN echo "🔍 尝试运行其他SIMD测试..." && \
    (cargo test --target x86_64-unknown-linux-gnu processing::simd::tests::test_simd_capability_detection -- --nocapture 2>&1 || true) | head -20

EOF

echo ""
echo "📦 构建x86调试环境..."
if docker build --platform=linux/amd64 -f Dockerfile.debug-x86 -t debug-x86-simd . --no-cache; then
    echo "✅ 调试构建完成"
else
    echo "❌ 调试构建失败"
    rm -f Dockerfile.debug-x86
    exit 1
fi

# 清理
echo ""
echo "🧹 清理临时文件..."
rm -f Dockerfile.debug-x86
docker rmi debug-x86-simd --force &> /dev/null || true

echo ""
echo "🎯 调试完成！"
echo "   从上面的输出中寻找具体的数值差异信息"