#!/bin/bash

# 多声道性能基准测试脚本 / Multi-channel Performance Benchmark Script
# 用于测试 5.1 和 7.1.4 多声道音频文件的性能基线

EXECUTABLE="/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/target/release/MacinMeter-DynamicRange-Tool-foo_dr"
AUDIO_5_1_DIR="/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/audio/multiCH/5.1"
AUDIO_7_1_4_DIR="/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/audio/multiCH/7.1.4"

# 检查可执行文件是否存在
if [ ! -f "$EXECUTABLE" ]; then
    echo "错误：可执行文件不存在: $EXECUTABLE"
    exit 1
fi

# 计算中位数
calculate_median() {
    local arr=("$@")
    local sorted=($(printf '%s\n' "${arr[@]}" | sort -n))
    local len=${#sorted[@]}
    local mid=$((len / 2))

    if [ $((len % 2)) -eq 0 ]; then
        echo "scale=3; (${sorted[$((mid-1))]} + ${sorted[$mid]}) / 2" | bc
    else
        echo "${sorted[$mid]}"
    fi
}

# 计算标准差
calculate_stddev() {
    local arr=("$@")
    local mean=$1
    shift
    local arr=("$@")
    local sum_sq=0

    for val in "${arr[@]}"; do
        local diff=$(echo "scale=6; $val - $mean" | bc)
        local sq=$(echo "scale=6; $diff * $diff" | bc)
        sum_sq=$(echo "scale=6; $sum_sq + $sq" | bc)
    done

    local variance=$(echo "scale=6; $sum_sq / ${#arr[@]}" | bc)
    echo "scale=3; sqrt($variance)" | bc
}

# 运行单个声道配置的 10x 测试
run_multichannel_benchmark() {
    local channel_name=$1
    local audio_dir=$2
    local file_size=$3

    echo "=========================================================="
    echo "开始测试: $channel_name"
    echo "文件大小: $file_size"
    echo "=========================================================="

    declare -a TIME_ARRAY
    declare -a MEMORY_ARRAY
    declare -a SPEED_ARRAY

    TOTAL_TIME=0
    TOTAL_MEMORY=0
    TOTAL_SPEED=0
    TESTS=10

    for i in $(seq 1 $TESTS); do
        echo "第 $i 次测试..."

        # 运行程序并计时
        START_TIME=$(date +%s.%N)
        OUTPUT=$("$EXECUTABLE" "$audio_dir" 2>&1)
        END_TIME=$(date +%s.%N)

        # 计算运行时间
        TIME=$(echo "$END_TIME - $START_TIME" | bc)

        # 计算处理速度 (MB/s)
        SPEED=$(echo "scale=2; $file_size / $TIME" | bc)

        # 简单获取内存信息（从输出中提取，如果有的话）
        # 如果没有输出内存信息，使用默认值
        MEMORY=$(echo "$OUTPUT" | grep -o '[0-9]*\.[0-9]*MB' | head -1 | sed 's/MB//')
        if [ -z "$MEMORY" ]; then
            MEMORY=0
        fi

        echo "   运行时间: ${TIME}s, 处理速度: ${SPEED}MB/s"

        # 累加统计
        TOTAL_TIME=$(echo "$TOTAL_TIME + $TIME" | bc)
        TOTAL_SPEED=$(echo "$TOTAL_SPEED + $SPEED" | bc)

        # 存储到数组
        TIME_ARRAY+=("$TIME")
        SPEED_ARRAY+=("$SPEED")
    done

    # 计算平均值
    AVG_TIME=$(echo "scale=3; $TOTAL_TIME / $TESTS" | bc)
    AVG_SPEED=$(echo "scale=2; $TOTAL_SPEED / $TESTS" | bc)

    # 计算中位数
    MEDIAN_TIME=$(calculate_median "${TIME_ARRAY[@]}")
    MEDIAN_SPEED=$(calculate_median "${SPEED_ARRAY[@]}")

    # 计算标准差
    STDDEV_TIME=$(calculate_stddev "$AVG_TIME" "${TIME_ARRAY[@]}")
    STDDEV_SPEED=$(calculate_stddev "$AVG_SPEED" "${SPEED_ARRAY[@]}")

    echo ""
    echo "=========================================================="
    echo "📊 $channel_name 统计结果"
    echo "=========================================================="
    echo "⏱️  运行时间："
    echo "   平均值: ${AVG_TIME}s  |  中位数: ${MEDIAN_TIME}s  |  标准差: ${STDDEV_TIME}s"
    echo ""
    echo "🚀 处理速度："
    echo "   平均值: ${AVG_SPEED}MB/s  |  中位数: ${MEDIAN_SPEED}MB/s  |  标准差: ${STDDEV_SPEED}MB/s"
    echo "=========================================================="
    echo ""
}

# 主程序
echo "🎵 多声道性能基准测试 / Multi-Channel Performance Benchmark"
echo ""

# 测试 5.1 声道 (215MB)
run_multichannel_benchmark "5.1 Surround (FLAC, 215MB)" "$AUDIO_5_1_DIR" 215

# 测试 7.1.4 声道 (1700MB)
run_multichannel_benchmark "7.1.4 Dolby Atmos (WAV, 1700MB)" "$AUDIO_7_1_4_DIR" 1700

echo "✅ 多声道性能基准测试完成"
