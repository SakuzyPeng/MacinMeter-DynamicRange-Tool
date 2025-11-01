#!/bin/bash

# 🎯 使用说明
show_usage() {
    echo "用法: $0 [选项]"
    echo ""
    echo "选项:"
    echo "  --exe PATH        指定可执行文件路径（默认：target/release/MacinMeter-DynamicRange-Tool-foo_dr）"
    echo "  --path DIR        指定测试数据集目录（默认：脚本所在目录）"
    echo "  --serial          使用串行解码（默认：并行）"
    echo "  --help, -h        显示此帮助信息"
    echo ""
    echo "示例:"
    echo "  $0                                    # 并行模式基准测试（默认可执行文件和路径）"
    echo "  $0 --serial                           # 串行模式基准测试"
    echo "  $0 --exe ./old-version/binary         # 测试指定的旧版本"
    echo "  $0 --path /path/to/audio/dataset      # 指定测试数据集目录"
    echo "  $0 --exe baseline.exe --serial --path ./audio  # 组合选项"
}

# 获取脚本所在目录
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# 默认值
DATASET_DIR="$SCRIPT_DIR"
RELEASE_EXECUTABLE="/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/target/release/MacinMeter-DynamicRange-Tool-foo_dr"
MODE_FLAG=""

# 解析命令行参数
while [[ $# -gt 0 ]]; do
    case $1 in
        --help|-h)
            show_usage
            exit 0
            ;;
        --serial)
            MODE_FLAG="--serial"
            shift
            ;;
        --exe)
            if [ -z "$2" ] || [[ "$2" == --* ]]; then
                echo "❌ 错误：--exe 需要指定可执行文件路径"
                show_usage
                exit 1
            fi
            RELEASE_EXECUTABLE="$2"
            shift 2
            ;;
        --path)
            if [ -z "$2" ] || [[ "$2" == --* ]]; then
                echo "❌ 错误：--path 需要指定数据集目录路径"
                show_usage
                exit 1
            fi
            DATASET_DIR="$2"
            shift 2
            ;;
        *)
            echo "❌ 未知选项: $1"
            show_usage
            exit 1
            ;;
    esac
done

# 检查可执行文件是否存在
if [ ! -f "$RELEASE_EXECUTABLE" ]; then
    echo "❌ 错误：可执行文件不存在: $RELEASE_EXECUTABLE"
    exit 1
fi

# 检查数据集目录是否存在
if [ ! -d "$DATASET_DIR" ]; then
    echo "❌ 错误：数据集目录不存在: $DATASET_DIR"
    exit 1
fi

# 构建 benchmark.sh 路径
BENCHMARK_SCRIPT="$DATASET_DIR/benchmark.sh"

# 检查 benchmark.sh 是否存在
if [ ! -f "$BENCHMARK_SCRIPT" ]; then
    echo "❌ 错误：benchmark.sh 不存在于数据集目录: $BENCHMARK_SCRIPT"
    echo "💡 提示：请确保在数据集目录中有 benchmark.sh 脚本"
    exit 1
fi

# 显示测试信息
echo "========================================================"
if [ -n "$MODE_FLAG" ]; then
    echo "🚀 开始10次串行解码性能测试"
else
    echo "🚀 开始10次并行解码性能测试（默认）"
fi
echo "📦 可执行文件: $(basename "$RELEASE_EXECUTABLE")"
echo "📁 完整路径: $RELEASE_EXECUTABLE"
echo "📂 数据集目录: $DATASET_DIR"
TOTAL_TIME=0
TOTAL_MEMORY=0
TOTAL_SPEED=0
TOTAL_CPU_PEAK=0
TOTAL_CPU_AVG=0
TESTS=10

# 数组存储每次测试结果（用于计算中位数和标准差）
declare -a TIME_ARRAY
declare -a MEMORY_ARRAY
declare -a SPEED_ARRAY

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

    # 存储到数组（用于中位数和标准差计算）
    TIME_ARRAY+=("$TIME")
    MEMORY_ARRAY+=("$MEMORY")
    SPEED_ARRAY+=("$SPEED")
done

# 📊 统计函数：计算中位数
calculate_median() {
    local arr=("$@")
    local sorted=($(printf '%s\n' "${arr[@]}" | sort -n))
    local len=${#sorted[@]}
    local mid=$((len / 2))

    if [ $((len % 2)) -eq 0 ]; then
        # 偶数个元素：取中间两个的平均值
        echo "scale=3; (${sorted[$((mid-1))]} + ${sorted[$mid]}) / 2" | bc
    else
        # 奇数个元素：取中间值
        echo "${sorted[$mid]}"
    fi
}

# 📊 统计函数：计算标准差
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

# 计算平均值
AVG_TIME=$(echo "scale=3; $TOTAL_TIME / $TESTS" | bc)
AVG_MEMORY=$(echo "scale=2; $TOTAL_MEMORY / $TESTS" | bc)
AVG_SPEED=$(echo "scale=2; $TOTAL_SPEED / $TESTS" | bc)
AVG_CPU_PEAK=$(echo "scale=2; $TOTAL_CPU_PEAK / $TESTS" | bc)
AVG_CPU_AVG=$(echo "scale=2; $TOTAL_CPU_AVG / $TESTS" | bc)

# 计算中位数
MEDIAN_TIME=$(calculate_median "${TIME_ARRAY[@]}")
MEDIAN_MEMORY=$(calculate_median "${MEMORY_ARRAY[@]}")
MEDIAN_SPEED=$(calculate_median "${SPEED_ARRAY[@]}")

# 计算标准差
STDDEV_TIME=$(calculate_stddev "$AVG_TIME" "${TIME_ARRAY[@]}")
STDDEV_MEMORY=$(calculate_stddev "$AVG_MEMORY" "${MEMORY_ARRAY[@]}")
STDDEV_SPEED=$(calculate_stddev "$AVG_SPEED" "${SPEED_ARRAY[@]}")

echo "========================================================"
echo "📊 10次测试统计结果："
echo ""
echo "⏱️  运行时间："
echo "   平均值: ${AVG_TIME}s  |  中位数: ${MEDIAN_TIME}s  |  标准差: ${STDDEV_TIME}s"
echo ""
echo "💾 内存峰值："
echo "   平均值: ${AVG_MEMORY}MB  |  中位数: ${MEDIAN_MEMORY}MB  |  标准差: ${STDDEV_MEMORY}MB"
echo ""
echo "🚀 处理速度："
echo "   平均值: ${AVG_SPEED}MB/s  |  中位数: ${MEDIAN_SPEED}MB/s  |  标准差: ${STDDEV_SPEED}MB/s"
echo ""
echo "📈 CPU使用率："
echo "   峰值平均: ${AVG_CPU_PEAK}%  |  平均值平均: ${AVG_CPU_AVG}%"
echo "========================================================"
echo "✅ 批量缓冲I/O优化性能基准测试完成"