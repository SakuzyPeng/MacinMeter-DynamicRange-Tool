#!/bin/bash

# 🎯 使用说明
show_usage() {
    echo "用法: $0 [选项]"
    echo ""
    echo "选项:"
    echo "  --serial          使用串行解码（默认：并行）"
    echo "  --help, -h        显示此帮助信息"
    echo ""
    echo "示例:"
    echo "  $0                # 并行模式基准测试（默认）"
    echo "  $0 --serial       # 串行模式基准测试"
}

# 解析命令行参数
MODE_FLAG=""
if [ "$1" = "--help" ] || [ "$1" = "-h" ]; then
    show_usage
    exit 0
elif [ "$1" = "--serial" ]; then
    MODE_FLAG="--serial"
    echo "🚀 开始10次串行解码性能测试..."
else
    echo "🚀 开始10次并行解码性能测试（默认）..."
fi

echo "========================================================"

BENCHMARK_SCRIPT="/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/audio/large audio/未命名文件夹/benchmark.sh"
RELEASE_EXECUTABLE="/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/target/release/MacinMeter-DynamicRange-Tool-foo_dr"
TOTAL_TIME=0
TOTAL_MEMORY=0
TOTAL_SPEED=0
TOTAL_CPU_PEAK=0
TOTAL_CPU_AVG=0
TESTS=10

for i in $(seq 1 $TESTS); do
    echo "🔄 第 $i 次测试..."

    # 运行benchmark脚本并捕获输出和时间（传递模式参数）
    START_TIME=$(date +%s.%N)
    OUTPUT=$("$BENCHMARK_SCRIPT" "$RELEASE_EXECUTABLE" $MODE_FLAG 2>&1)
    END_TIME=$(date +%s.%N)

    # 计算运行时间
    TIME=$(echo "$END_TIME - $START_TIME" | bc)

    # 提取内存峰值（从运行总结报告）
    MEMORY=$(echo "$OUTPUT" | grep "内存使用峰值" | awk '{print $3}' | sed 's/MB//')

    # 提取处理速度
    SPEED=$(echo "$OUTPUT" | grep "处理速度" | awk '{print $3}' | sed 's/MB\/s//')

    # 提取CPU占用
    CPU_PEAK=$(echo "$OUTPUT" | grep "CPU使用峰值" | awk '{print $3}' | sed 's/%//')
    CPU_AVG=$(echo "$OUTPUT" | grep "CPU使用平均值" | awk '{print $3}' | sed 's/%//')

    echo "   运行时间: ${TIME}s, 内存峰值: ${MEMORY}MB, CPU峰值: ${CPU_PEAK}%, 处理速度: ${SPEED}MB/s"

    # 累加统计
    TOTAL_TIME=$(echo "$TOTAL_TIME + $TIME" | bc)
    TOTAL_MEMORY=$(echo "$TOTAL_MEMORY + $MEMORY" | bc)
    TOTAL_SPEED=$(echo "$TOTAL_SPEED + $SPEED" | bc)
    TOTAL_CPU_PEAK=$(echo "$TOTAL_CPU_PEAK + $CPU_PEAK" | bc)
    TOTAL_CPU_AVG=$(echo "$TOTAL_CPU_AVG + $CPU_AVG" | bc)
done

# 计算平均值
AVG_TIME=$(echo "scale=3; $TOTAL_TIME / $TESTS" | bc)
AVG_MEMORY=$(echo "scale=2; $TOTAL_MEMORY / $TESTS" | bc)
AVG_SPEED=$(echo "scale=2; $TOTAL_SPEED / $TESTS" | bc)
AVG_CPU_PEAK=$(echo "scale=2; $TOTAL_CPU_PEAK / $TESTS" | bc)
AVG_CPU_AVG=$(echo "scale=2; $TOTAL_CPU_AVG / $TESTS" | bc)

echo "========================================================"
echo "📊 10次测试统计结果："
echo "   平均运行时间: ${AVG_TIME}s"
echo "   平均内存峰值: ${AVG_MEMORY}MB"
echo "   平均CPU峰值: ${AVG_CPU_PEAK}% (整体CPU使用率)"
echo "   平均CPU平均值: ${AVG_CPU_AVG}% (整体CPU使用率)"
echo "   平均处理速度: ${AVG_SPEED}MB/s"
echo "========================================================"
echo "✅ 批量缓冲I/O优化性能基准测试完成"