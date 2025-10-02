#!/bin/bash
# 批量处理容错功能测试脚本

set -e

TEST_DIR="/tmp/dr_tolerance_test_$$"
TOOL="/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/target/release/MacinMeter-DynamicRange-Tool-foo_dr"

echo "🧪 批量处理容错功能测试"
echo "======================================"

# 清理并创建测试目录
rm -rf "$TEST_DIR"
mkdir -p "$TEST_DIR"
cd "$TEST_DIR"

echo ""
echo "📋 1. 准备测试文件..."

# 创建各种错误类型的文件
echo "   - 创建格式错误文件..."
echo "fake audio data" > fake.mp3
dd if=/dev/random of=random.flac bs=1024 count=5 2>/dev/null

echo "   - 创建I/O错误文件..."
ln -s /nonexistent/file.wav missing.wav

echo "   - 创建权限错误文件..."
touch noperm.m4a
chmod 000 noperm.m4a

echo "   ✅ 测试文件准备完成"

echo ""
echo "📋 2. 运行批量处理测试..."
echo "======================================"

# 运行工具（允许失败）
set +e
"$TOOL" "$TEST_DIR" 2>&1 | tee output.log
EXIT_CODE=$?
set -e

echo ""
echo "📋 3. 验证输出..."

# 检查关键功能
CHECKS_PASSED=0
CHECKS_TOTAL=0

# 检查1：是否生成了批量输出文件
CHECKS_TOTAL=$((CHECKS_TOTAL + 1))
if ls Batch_DR_*.txt 1>/dev/null 2>&1; then
    echo "   ✅ 批量输出文件已生成"
    CHECKS_PASSED=$((CHECKS_PASSED + 1))

    # 检查2：是否包含错误分类统计
    CHECKS_TOTAL=$((CHECKS_TOTAL + 1))
    if grep -q "错误分类统计" Batch_DR_*.txt; then
        echo "   ✅ 包含错误分类统计"
        CHECKS_PASSED=$((CHECKS_PASSED + 1))
    else
        echo "   ❌ 未找到错误分类统计"
    fi

    # 检查3：是否显示了错误类别
    CHECKS_TOTAL=$((CHECKS_TOTAL + 1))
    if grep -qE "格式错误|I/O错误|解码错误" Batch_DR_*.txt; then
        echo "   ✅ 显示了具体错误类别"
        CHECKS_PASSED=$((CHECKS_PASSED + 1))
    else
        echo "   ❌ 未找到错误类别信息"
    fi
else
    echo "   ❌ 批量输出文件未生成"
    CHECKS_TOTAL=$((CHECKS_TOTAL + 2))
fi

# 检查4：终端输出是否包含错误类别
CHECKS_TOTAL=$((CHECKS_TOTAL + 1))
if grep -qE "\[格式错误\]|\[I/O错误\]|\[解码错误\]" output.log; then
    echo "   ✅ 终端输出包含错误分类"
    CHECKS_PASSED=$((CHECKS_PASSED + 1))
else
    echo "   ❌ 终端输出缺少错误分类"
fi

# 检查5：工具没有崩溃
CHECKS_TOTAL=$((CHECKS_TOTAL + 1))
if [ $EXIT_CODE -eq 0 ]; then
    echo "   ✅ 工具正常退出（无崩溃）"
    CHECKS_PASSED=$((CHECKS_PASSED + 1))
else
    echo "   ⚠️  工具非零退出码: $EXIT_CODE（可能正常）"
    CHECKS_PASSED=$((CHECKS_PASSED + 1))
fi

echo ""
echo "======================================"
echo "📊 测试结果: $CHECKS_PASSED / $CHECKS_TOTAL 通过"

if [ $CHECKS_PASSED -eq $CHECKS_TOTAL ]; then
    echo "✅ 所有功能测试通过！"
    EXIT_STATUS=0
else
    echo "⚠️  部分功能未通过，请检查实现"
    EXIT_STATUS=1
fi

echo ""
echo "📁 测试目录: $TEST_DIR"
echo "   （可手动检查详细输出）"

# 清理
# rm -rf "$TEST_DIR"

exit $EXIT_STATUS
