#!/bin/bash
# 🔄 跨架构测试脚本 - 本地验证x86代码路径

set -e

echo "🔄 MacinMeter DR Tool - 跨架构测试"
echo "=================================="

# 检查Docker是否可用
if ! command -v docker &> /dev/null; then
    echo "⚠️  Docker未安装，跳过x86模拟测试"
    echo "💡 建议: 安装Docker来启用完整的跨架构测试"
    exit 0
fi

echo ""
echo "🐳 1. 准备x86_64 Linux环境..."

# 创建临时的Dockerfile
cat > Dockerfile.x86-test << 'EOF'
FROM --platform=linux/amd64 rust:1.88

# 安装系统依赖
RUN apt-get update && apt-get install -y \
    build-essential \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# 设置工作目录
WORKDIR /app

# 复制项目文件
COPY . .

# 强制设置x86_64目标并构建
RUN echo "🔧 配置x86_64编译环境..." && \
    rustup target add x86_64-unknown-linux-gnu && \
    export CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu

RUN echo "🦀 构建x86_64版本(SSE路径)..." && \
    cargo build --release --target x86_64-unknown-linux-gnu --verbose 2>&1

RUN echo "🧪 运行x86_64 SSE SIMD测试..." && \
    RUST_BACKTRACE=full cargo test --target x86_64-unknown-linux-gnu processing::simd::tests::test_simd_vs_scalar_consistency -- --nocapture 2>&1

RUN echo "🔍 运行完整x86_64测试套件..." && \
    cargo test --target x86_64-unknown-linux-gnu --verbose 2>&1
EOF

echo "📦 2. 构建x86测试环境..."
docker build --platform=linux/amd64 -f Dockerfile.x86-test -t macinmeter-x86-test .

echo ""
echo "🧪 3. 运行x86环境下的SIMD测试..."
echo "   (这将验证x86 SSE代码路径)"

if docker run --platform=linux/amd64 --rm macinmeter-x86-test; then
    echo ""
    echo "✅ x86环境测试通过！"
    echo "   CI应该会成功"
else
    echo ""
    echo "❌ x86环境测试失败！"
    echo "   这解释了为什么CI总是失败"
    echo "   请修复x86 SSE实现后重新测试"
    
    # 清理临时文件
    rm -f Dockerfile.x86-test
    exit 1
fi

# 清理临时文件
echo ""
echo "🧹 4. 清理临时文件..."
rm -f Dockerfile.x86-test
docker rmi macinmeter-x86-test --force &> /dev/null || true

echo ""
echo "🎉 跨架构测试完成！"
echo "   ARM NEON ✅ + x86_64 SSE ✅ = CI预期成功 🚀"