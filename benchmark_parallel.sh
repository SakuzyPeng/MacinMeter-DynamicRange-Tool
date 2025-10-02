#!/bin/bash

# 🚀 多文件并行性能对比测试脚本
# 测试不同并发度下的性能表现

RELEASE_BIN="/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/target/release/MacinMeter-DynamicRange-Tool-foo_dr"
TEST_DIR="/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/audio"

echo "🚀 多文件并行性能对比测试"
echo "========================================================"
echo "测试目录: $TEST_DIR"
echo "可执行文件: $RELEASE_BIN"
echo ""

# 统计测试文件
FILE_COUNT=$(find "$TEST_DIR" -maxdepth 1 -type f \( -iname "*.flac" -o -iname "*.wav" -o -iname "*.mp3" -o -iname "*.aac" -o -iname "*.m4a" -o -iname "*.ogg" \) | wc -l | tr -d ' ')
echo "📊 测试文件数: $FILE_COUNT 个"
echo ""

# 测试函数
run_test() {
    local MODE=$1
    local ARGS=$2
    local DESC=$3

    echo "🔄 测试模式: $DESC" >&2
    echo "   参数: $ARGS" >&2

    # 清理之前的输出文件
    rm -f "$TEST_DIR"/*_BatchDR_*.txt "$TEST_DIR"/*_DR_Analysis.txt 2>/dev/null

    # 使用 /usr/bin/time 获取内存和时间信息
    START_TIME=$(date +%s.%N)

    # Mac上使用 /usr/bin/time -l 获取详细统计
    TIME_OUTPUT=$(/usr/bin/time -l $RELEASE_BIN "$TEST_DIR" $ARGS 2>&1 > /dev/null)

    END_TIME=$(date +%s.%N)

    # 计算耗时
    TIME=$(echo "$END_TIME - $START_TIME" | bc)

    # 提取内存峰值（Mac的time命令输出格式）
    MEMORY_BYTES=$(echo "$TIME_OUTPUT" | grep "maximum resident set size" | awk '{print $1}')
    MEMORY_MB=$(echo "scale=2; $MEMORY_BYTES / 1024 / 1024" | bc)

    echo "   ⏱️  耗时: ${TIME}s" >&2
    echo "   💾 内存峰值: ${MEMORY_MB}MB" >&2
    echo "" >&2

    # 返回耗时和内存，用逗号分隔（只输出到stdout）
    echo "${TIME},${MEMORY_MB}"
}

# 1. 串行模式测试
echo "========================================================"
echo "测试1: 串行模式（禁用多文件并行）"
echo "========================================================"
SERIAL_RESULT=$(run_test "serial" "--no-parallel-files" "串行处理")
SERIAL_TIME=$(echo "$SERIAL_RESULT" | cut -d',' -f1)
SERIAL_MEM=$(echo "$SERIAL_RESULT" | cut -d',' -f2)

# 2. 并发度2测试
echo "========================================================"
echo "测试2: 并发度 2"
echo "========================================================"
PARALLEL_2_RESULT=$(run_test "parallel-2" "--parallel-files 2" "2并发")
PARALLEL_2_TIME=$(echo "$PARALLEL_2_RESULT" | cut -d',' -f1)
PARALLEL_2_MEM=$(echo "$PARALLEL_2_RESULT" | cut -d',' -f2)

# 3. 并发度4测试（默认）
echo "========================================================"
echo "测试3: 并发度 4（默认）"
echo "========================================================"
PARALLEL_4_RESULT=$(run_test "parallel-4" "--parallel-files 4" "4并发（默认）")
PARALLEL_4_TIME=$(echo "$PARALLEL_4_RESULT" | cut -d',' -f1)
PARALLEL_4_MEM=$(echo "$PARALLEL_4_RESULT" | cut -d',' -f2)

# 4. 并发度8测试
echo "========================================================"
echo "测试4: 并发度 8"
echo "========================================================"
PARALLEL_8_RESULT=$(run_test "parallel-8" "--parallel-files 8" "8并发")
PARALLEL_8_TIME=$(echo "$PARALLEL_8_RESULT" | cut -d',' -f1)
PARALLEL_8_MEM=$(echo "$PARALLEL_8_RESULT" | cut -d',' -f2)

# 计算加速比
SPEEDUP_2=$(echo "scale=2; $SERIAL_TIME / $PARALLEL_2_TIME" | bc)
SPEEDUP_4=$(echo "scale=2; $SERIAL_TIME / $PARALLEL_4_TIME" | bc)
SPEEDUP_8=$(echo "scale=2; $SERIAL_TIME / $PARALLEL_8_TIME" | bc)

# 计算内存增长比
MEM_RATIO_2=$(echo "scale=2; $PARALLEL_2_MEM / $SERIAL_MEM" | bc)
MEM_RATIO_4=$(echo "scale=2; $PARALLEL_4_MEM / $SERIAL_MEM" | bc)
MEM_RATIO_8=$(echo "scale=2; $PARALLEL_8_MEM / $SERIAL_MEM" | bc)

# 汇总结果
echo "========================================================"
echo "📊 性能对比汇总"
echo "========================================================"
echo "模式          耗时(s)    内存(MB)   加速比    内存比"
echo "----------------------------------------------------------------"
echo "串行          $SERIAL_TIME    $SERIAL_MEM      1.00x     1.00x"
echo "并发度2       $PARALLEL_2_TIME    $PARALLEL_2_MEM      ${SPEEDUP_2}x     ${MEM_RATIO_2}x"
echo "并发度4       $PARALLEL_4_TIME    $PARALLEL_4_MEM      ${SPEEDUP_4}x     ${MEM_RATIO_4}x"
echo "并发度8       $PARALLEL_8_TIME    $PARALLEL_8_MEM      ${SPEEDUP_8}x     ${MEM_RATIO_8}x"
echo "========================================================"

# 结论
echo ""
echo "✅ 测试完成！"
echo ""
echo "💡 性能分析："
echo "   - 最佳并发度: $(if (( $(echo "$SPEEDUP_8 > $SPEEDUP_4" | bc -l) )); then echo "8"; elif (( $(echo "$SPEEDUP_4 > $SPEEDUP_2" | bc -l) )); then echo "4"; else echo "2"; fi)"
echo "   - 最大加速比: $(echo "$SPEEDUP_2 $SPEEDUP_4 $SPEEDUP_8" | tr ' ' '\n' | sort -rn | head -1)x"
echo ""
echo "📝 建议："
if (( $(echo "$SPEEDUP_4 > 3.5" | bc -l) )); then
    echo "   ✅ 并发效果显著，推荐使用 --parallel-files 4"
elif (( $(echo "$SPEEDUP_4 > 2.0" | bc -l) )); then
    echo "   ✅ 并发效果良好，可以使用 --parallel-files 4"
elif (( $(echo "$SPEEDUP_4 > 1.5" | bc -l) )); then
    echo "   ⚠️  并发效果一般，可能存在I/O瓶颈"
else
    echo "   ❌ 并发效果不佳，建议检查磁盘性能"
fi
