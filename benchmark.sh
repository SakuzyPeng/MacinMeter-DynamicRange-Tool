#!/bin/bash

# MacinMeter DR Tool 性能监控脚本
# 自动检测并启动同目录下的可执行文件，统计执行时间和资源占用

set -e

# 脚本所在目录
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "🔍 正在搜索MacinMeter可执行文件..."

# 查找所有包含"MacinMeter"的可执行文件（增加搜索深度以支持更复杂的目录结构）
FOUND_EXECUTABLES=()
while IFS= read -r -d '' file; do
    if [[ -x "$file" && -f "$file" ]]; then
        FOUND_EXECUTABLES+=("$file")
    fi
done < <(find "$SCRIPT_DIR" -maxdepth 5 -name "*MacinMeter*" -type f -executable -print0 2>/dev/null)

if [[ ${#FOUND_EXECUTABLES[@]} -eq 0 ]]; then
    echo "❌ 错误: 在当前目录及子目录中找不到包含'MacinMeter'的可执行文件"
    echo ""
    echo "💡 可能的解决方案:"
    echo "  1. 如果是源码目录，请先构建项目："
    echo "     cargo build --release  (推荐，用于性能测试)"
    echo "     cargo build            (调试版本)"
    echo "  2. 确保可执行文件位于脚本同目录或子目录中"
    echo "  3. 确保可执行文件有执行权限: chmod +x 文件名"
    exit 1
fi

# 如果找到多个，选择最合适的（优先release版本，然后按文件名排序）
EXEC_PATH=""
BUILD_TYPE="Unknown"

for exec_file in "${FOUND_EXECUTABLES[@]}"; do
    filename=$(basename "$exec_file")
    dirname=$(dirname "$exec_file")

    # 优先选择release目录下的文件
    if [[ "$dirname" == *"/release"* || "$dirname" == *"/Release"* ]]; then
        EXEC_PATH="$exec_file"
        BUILD_TYPE="Release"
        break
    # 其次选择非debug目录的文件
    elif [[ "$dirname" != *"/debug"* && "$dirname" != *"/Debug"* ]]; then
        if [[ -z "$EXEC_PATH" ]]; then
            EXEC_PATH="$exec_file"
            BUILD_TYPE="Standalone"
        fi
    # 最后考虑debug版本
    elif [[ -z "$EXEC_PATH" ]]; then
        EXEC_PATH="$exec_file"
        BUILD_TYPE="Debug"
    fi
done

# 如果还是没找到，使用第一个
if [[ -z "$EXEC_PATH" ]]; then
    EXEC_PATH="${FOUND_EXECUTABLES[0]}"
fi

echo "✅ 找到可执行文件: $(basename "$EXEC_PATH")"

# 显示基本信息
echo "🎯 MacinMeter DR Tool 性能监控"
echo "=================================================================================="
echo "📁 工作目录: $SCRIPT_DIR"
echo "🔧 可执行文件: $EXEC_PATH"
echo "📦 构建类型: $BUILD_TYPE"

# 获取文件大小
FILE_SIZE=$(ls -lh "$EXEC_PATH" | awk '{print $5}')
echo "📊 文件大小: $FILE_SIZE"

# 检查是否有测试音频文件
TEST_FILES=(
    "$SCRIPT_DIR"/*.flac
    "$SCRIPT_DIR"/*.mp3
    "$SCRIPT_DIR"/*.wav
    "$SCRIPT_DIR"/*.m4a
)

FOUND_FILES=()
for pattern in "${TEST_FILES[@]}"; do
    if ls $pattern 1> /dev/null 2>&1; then
        for file in $pattern; do
            if [[ -f "$file" ]]; then
                FOUND_FILES+=("$file")
            fi
        done
    fi
done

if [[ ${#FOUND_FILES[@]} -eq 0 ]]; then
    echo "⚠️  警告: 当前目录没有找到音频测试文件"
    echo "💡 建议: 放置一些 .flac, .mp3, .wav 或 .m4a 文件到项目根目录进行测试"
    echo ""
    echo "将使用 --help 参数运行程序..."
    ARGS="--help"
else
    # 选择第一个找到的音频文件
    TEST_FILE="${FOUND_FILES[0]}"
    echo "🎵 测试文件: $(basename "$TEST_FILE")"

    # 获取音频文件信息
    AUDIO_SIZE=$(ls -lh "$TEST_FILE" | awk '{print $5}')
    echo "📊 音频大小: $AUDIO_SIZE"

    ARGS="\"$TEST_FILE\""
fi

echo "=================================================================================="
echo ""

# 创建临时文件存储详细统计信息
TEMP_LOG=$(mktemp)

echo "🚀 开始执行程序..."
echo "⌛ 执行命令: $EXECUTABLE_NAME $ARGS"
echo ""

# 记录开始时间
START_TIME=$(date +%s.%N)

# 获取可执行文件名用于显示
EXECUTABLE_NAME=$(basename "$EXEC_PATH")

# 使用 /usr/bin/time 获取详细统计信息 + 实时监控内存
if [[ "$ARGS" == "--help" ]]; then
    /usr/bin/time -l "$EXEC_PATH" --help 2> "$TEMP_LOG"
else
    eval "/usr/bin/time -l \"$EXEC_PATH\" $ARGS 2> \"$TEMP_LOG\""
fi

# 记录结束时间
END_TIME=$(date +%s.%N)

# 计算执行时间
EXECUTION_TIME=$(echo "$END_TIME - $START_TIME" | bc)

echo ""
echo "=================================================================================="
echo "📈 性能统计结果"
echo "=================================================================================="

# 解析 time 命令的输出
REAL_TIME=$(grep "real" "$TEMP_LOG" | tail -1 | awk '{print $1}' || echo "N/A")
USER_TIME=$(grep "user" "$TEMP_LOG" | tail -1 | awk '{print $1}' || echo "N/A")
SYS_TIME=$(grep "sys" "$TEMP_LOG" | tail -1 | awk '{print $1}' || echo "N/A")

# 解析内存信息 (从 /usr/bin/time -l 输出)
MAX_RESIDENT=$(grep "maximum resident set size" "$TEMP_LOG" | awk '{print $1}' || echo "N/A")
PEAK_MEMORY_KB=$(echo "$MAX_RESIDENT / 1024" | bc 2>/dev/null || echo "N/A")

# 解析其他统计信息
PAGE_FAULTS=$(grep "page faults" "$TEMP_LOG" | awk '{print $1}' || echo "N/A")
VOLUNTARY_SWITCHES=$(grep "voluntary context switches" "$TEMP_LOG" | awk '{print $1}' || echo "N/A")

echo "⏱️  执行时间统计:"
if [[ "$REAL_TIME" != "N/A" ]]; then
    echo "   总耗时: ${REAL_TIME}秒"
    echo "   用户态: ${USER_TIME}秒"
    echo "   内核态: ${SYS_TIME}秒"
else
    printf "   总耗时: %.3f秒\n" "$EXECUTION_TIME"
fi

echo ""
echo "💾 内存使用统计:"
if [[ "$PEAK_MEMORY_KB" != "N/A" ]] && [[ "$PEAK_MEMORY_KB" -gt 0 ]]; then
    PEAK_MEMORY_MB=$(echo "scale=2; $PEAK_MEMORY_KB / 1024" | bc)
    echo "   峰值内存: ${PEAK_MEMORY_MB}MB"
else
    echo "   峰值内存: 无法获取"
fi

echo ""
echo "🔧 系统资源统计:"
if [[ "$PAGE_FAULTS" != "N/A" ]]; then
    echo "   页面错误: $PAGE_FAULTS"
fi
if [[ "$VOLUNTARY_SWITCHES" != "N/A" ]]; then
    echo "   上下文切换: $VOLUNTARY_SWITCHES"
fi

echo ""
echo "🎯 性能评价:"

# 根据文件大小给出性能评价
if [[ ${#FOUND_FILES[@]} -gt 0 ]]; then
    if [[ "$REAL_TIME" != "N/A" ]]; then
        TIME_NUM=$(echo "$REAL_TIME" | sed 's/s$//')
        if (( $(echo "$TIME_NUM < 0.1" | bc -l) )); then
            echo "   ⚡ 处理速度: 极快 (< 0.1秒)"
        elif (( $(echo "$TIME_NUM < 0.5" | bc -l) )); then
            echo "   🚀 处理速度: 很快 (< 0.5秒)"
        elif (( $(echo "$TIME_NUM < 2.0" | bc -l) )); then
            echo "   ✅ 处理速度: 正常 (< 2秒)"
        else
            echo "   ⏳ 处理速度: 较慢 (≥ 2秒)"
        fi
    fi

    if [[ "$PEAK_MEMORY_MB" != "N/A" ]] && (( $(echo "$PEAK_MEMORY_MB < 50" | bc -l) )); then
        echo "   💚 内存效率: 优秀 (< 50MB)"
    elif [[ "$PEAK_MEMORY_MB" != "N/A" ]] && (( $(echo "$PEAK_MEMORY_MB < 100" | bc -l) )); then
        echo "   💙 内存效率: 良好 (< 100MB)"
    elif [[ "$PEAK_MEMORY_MB" != "N/A" ]]; then
        echo "   💛 内存效率: 一般 (≥ 100MB)"
    fi
fi

echo ""
echo "=================================================================================="

# 清理临时文件
rm -f "$TEMP_LOG"

echo "✅ 性能监控完成"
echo ""
echo "💡 提示:"
echo "   - 使用 Release 版本 (cargo build --release) 获得最佳性能"
echo "   - 测试不同大小的音频文件以评估性能表现"
echo "   - 多次运行取平均值以获得稳定的性能数据"