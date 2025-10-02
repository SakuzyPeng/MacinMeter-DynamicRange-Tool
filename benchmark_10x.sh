#!/bin/bash

echo "🚀 开始10次批量缓冲I/O优化性能测试..."
echo "========================================================"

BENCHMARK_SCRIPT="/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/audio/large audio/未命名文件夹/benchmark.sh"
RELEASE_EXECUTABLE="/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/target/release/MacinMeter-DynamicRange-Tool-foo_dr"
TOTAL_TIME=0
TOTAL_MEMORY=0
TOTAL_SPEED=0
TESTS=10

for i in $(seq 1 $TESTS); do
    echo "🔄 第 $i 次测试..."

    # 运行benchmark脚本并捕获输出和时间
    START_TIME=$(date +%s.%N)
    OUTPUT=$("$BENCHMARK_SCRIPT" "$RELEASE_EXECUTABLE" 2>&1)
    END_TIME=$(date +%s.%N)

    # 计算运行时间
    TIME=$(echo "$END_TIME - $START_TIME" | bc)

    # 提取内存峰值（从运行总结报告）
    MEMORY=$(echo "$OUTPUT" | grep "内存使用峰值" | awk '{print $3}' | sed 's/MB//')

    # 提取处理速度
    SPEED=$(echo "$OUTPUT" | grep "处理速度" | awk '{print $3}' | sed 's/MB\/s//')

    echo "   运行时间: ${TIME}s, 内存峰值: ${MEMORY}MB, 处理速度: ${SPEED}MB/s"

    # 累加统计
    TOTAL_TIME=$(echo "$TOTAL_TIME + $TIME" | bc)
    TOTAL_MEMORY=$(echo "$TOTAL_MEMORY + $MEMORY" | bc)
    TOTAL_SPEED=$(echo "$TOTAL_SPEED + $SPEED" | bc)
done

# 计算平均值
AVG_TIME=$(echo "scale=3; $TOTAL_TIME / $TESTS" | bc)
AVG_MEMORY=$(echo "scale=2; $TOTAL_MEMORY / $TESTS" | bc)
AVG_SPEED=$(echo "scale=2; $TOTAL_SPEED / $TESTS" | bc)

echo "========================================================"
echo "📊 10次测试统计结果："
echo "   平均运行时间: ${AVG_TIME}s"
echo "   平均内存峰值: ${AVG_MEMORY}MB"
echo "   平均处理速度: ${AVG_SPEED}MB/s"
echo "========================================================"
echo "✅ 批量缓冲I/O优化性能基准测试完成"