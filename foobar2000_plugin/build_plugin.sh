#!/bin/bash
# MacinMeter DR Plugin 构建脚本
# 防止构建问题并确保所有依赖都是最新的

set -e  # 任何错误时退出

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_DIR="${SCRIPT_DIR}/build"

echo "🚀 MacinMeter DR Plugin 构建脚本"
echo "=================================="

# 1. 清理并重新构建Rust核心库
echo "📦 1. 构建Rust核心库..."
cd "${SCRIPT_DIR}/rust_core"
cargo clean
cargo build --release
echo "✅ Rust核心库构建完成"

# 2. 清理并重新构建C++插件
echo "🔨 2. 构建C++插件..."
cd "${SCRIPT_DIR}"
rm -rf "${BUILD_DIR}"
mkdir -p "${BUILD_DIR}"
cd "${BUILD_DIR}"

cmake ..
make -j$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)

echo "✅ C++插件构建完成"

# 3. 验证构建产物
echo "🔍 3. 验证构建产物..."
PLUGIN_FILE="${BUILD_DIR}/foo_dr_macinmeter.fb2k-component"
RUST_LIB="${SCRIPT_DIR}/rust_core/target/release/libmacinmeter_dr_core.dylib"

if [[ -f "${PLUGIN_FILE}" ]]; then
    PLUGIN_SIZE=$(stat -f%z "${PLUGIN_FILE}" 2>/dev/null || stat -c%s "${PLUGIN_FILE}")
    echo "✅ 插件文件: ${PLUGIN_FILE} (${PLUGIN_SIZE} bytes)"
else
    echo "❌ 插件文件未找到: ${PLUGIN_FILE}"
    exit 1
fi

if [[ -f "${RUST_LIB}" ]]; then
    RUST_SIZE=$(stat -f%z "${RUST_LIB}" 2>/dev/null || stat -c%s "${RUST_LIB}")
    echo "✅ Rust库: ${RUST_LIB} (${RUST_SIZE} bytes)"
else
    echo "❌ Rust库未找到: ${RUST_LIB}"
    exit 1
fi

# 4. 显示安装说明
echo ""
echo "🎉 构建完成！"
echo "=================================="
echo "插件位置: ${PLUGIN_FILE}"
echo ""
echo "安装步骤:"
echo "1. 在foobar2000中，进入 文件 > 首选项 > 组件"
echo "2. 点击 '安装...' 并选择上述插件文件"
echo "3. 重启foobar2000"
echo ""
echo "或者手动复制到foobar2000插件目录"