#!/bin/bash

# --- 配置 ---
# 要查找的程序名称前缀（当没有指定参数时使用）
EXECUTABLE_PREFIX="MacinMeter"
# 采样间隔（秒）- 缩短到0.1秒以提高采样频率
SAMPLE_INTERVAL=0.1

# --- 使用说明 ---
show_usage() {
    echo "用法: $0 [可执行文件路径]"
    echo ""
    echo "参数说明:"
    echo "  可执行文件路径  - 可选，指定要测试的可执行文件的完整路径"
    echo "                   如果不提供，脚本将自动在当前目录查找以 '${EXECUTABLE_PREFIX}' 开头的文件"
    echo ""
    echo "示例:"
    echo "  $0                                    # 自动查找MacinMeter开头的文件"
    echo "  $0 ./my-custom-executable            # 测试指定的可执行文件"
    echo "  $0 /path/to/MacinMeter-Tool          # 测试完整路径的文件"
}

# --- 脚本主体 ---
# 获取脚本所在的目录
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)

# 1. 处理命令行参数和查找可执行文件
# --------------------------------------------------
if [ "$1" = "--help" ] || [ "$1" = "-h" ]; then
    show_usage
    exit 0
fi

# 🎯 解析额外参数（如--serial）
EXTRA_ARGS=""
if [ "$2" = "--serial" ]; then
    EXTRA_ARGS="--serial"
    echo "🐌 使用串行解码模式"
else
    echo "⚡ 使用并行解码模式（默认）"
fi

if [ -n "$1" ]; then
    # 用户指定了可执行文件路径
    EXECUTABLE_PATH="$1"

    # 如果路径是相对路径，转换为绝对路径
    if [[ "$EXECUTABLE_PATH" != /* ]]; then
        EXECUTABLE_PATH="$SCRIPT_DIR/$EXECUTABLE_PATH"
    fi

    # 验证文件是否存在
    if [ ! -f "$EXECUTABLE_PATH" ]; then
        echo "错误: 指定的文件不存在: $EXECUTABLE_PATH"
        exit 1
    fi

    # 验证文件是否可执行
    if [ ! -x "$EXECUTABLE_PATH" ]; then
        echo "错误: 指定的文件不可执行: $EXECUTABLE_PATH"
        echo "提示: 请使用 'chmod +x $EXECUTABLE_PATH' 添加执行权限"
        exit 1
    fi

    EXECUTABLE_NAME=$(basename "$EXECUTABLE_PATH")
    echo "使用指定的程序: ${EXECUTABLE_NAME}"
    echo "完整路径: ${EXECUTABLE_PATH}"
else
    # 没有指定参数，使用原有的自动查找逻辑
    echo "正在查找以 '${EXECUTABLE_PREFIX}' 开头的可执行文件..."
    # 使用 find 命令查找匹配的文件
    EXECUTABLE_PATH=$(find "$SCRIPT_DIR" -maxdepth 1 -type f -name "${EXECUTABLE_PREFIX}*" -perm +111 -print -quit)

    # 检查是否找到了文件
    if [ -z "$EXECUTABLE_PATH" ]; then
        echo "错误: 未在脚本目录中找到以 '${EXECUTABLE_PREFIX}' 开头的可执行文件。"
        echo "提示: 请使用 '$0 <可执行文件路径>' 指定要测试的文件"
        exit 1
    fi

    # 检查是否找到多个匹配项
    MATCH_COUNT=$(find "$SCRIPT_DIR" -maxdepth 1 -type f -name "${EXECUTABLE_PREFIX}*" -perm +111 | wc -l | xargs)
    if [ "$MATCH_COUNT" -gt 1 ]; then
        echo "错误: 找到多个匹配的可执行文件，无法确定启动哪一个。"
        echo "提示: 请使用 '$0 <可执行文件路径>' 明确指定要测试的文件"
        exit 1
    fi

    EXECUTABLE_NAME=$(basename "$EXECUTABLE_PATH")
    echo "找到目标程序: ${EXECUTABLE_NAME}"
fi

# 2. 初始化监控变量
# --------------------------------------------------
peak_memory_kb=0
total_memory_kb=0
mem_sample_count=0

# CPU 统计（改为基于进程累计CPU时间的差分法，获得更准确的瞬时占用）
peak_cpu=0
total_cpu_percent=0
cpu_sample_count=0

# 获取CPU核心数（用于计算整体CPU使用率）
CPU_CORES=$(sysctl -n hw.ncpu)

# 3. 启动与监控
# --------------------------------------------------
echo "正在启动 ${EXECUTABLE_NAME}..."
# 记录开始时间戳（纳秒精度）
START_TIME=$(date +%s.%N)
# 在后台启动目标程序（传递脚本所在目录和额外参数）
"$EXECUTABLE_PATH" "$SCRIPT_DIR" $EXTRA_ARGS &
# 获取其进程ID (PID)
PID=$!

# 尝试检测音频文件大小（用于计算处理速度）
AUDIO_FILES=$(find "$SCRIPT_DIR" -type f \( -name "*.flac" -o -name "*.wav" -o -name "*.mp3" -o -name "*.aac" -o -name "*.m4a" \) 2>/dev/null)
TOTAL_SIZE_KB=0
FILE_COUNT=0

if [[ -n "$AUDIO_FILES" ]]; then
    while IFS= read -r file; do
        if [[ -f "$file" ]]; then
            FILE_SIZE_KB=$(du -k "$file" | cut -f1)
            TOTAL_SIZE_KB=$((TOTAL_SIZE_KB + FILE_SIZE_KB))
            FILE_COUNT=$((FILE_COUNT + 1))
        fi
    done <<< "$AUDIO_FILES"
fi

echo "程序已启动 (PID: ${PID})。正在后台监控，请等待程序运行结束..."
if [[ $FILE_COUNT -gt 0 ]]; then
    TOTAL_SIZE_MB=$(echo "scale=2; $TOTAL_SIZE_KB / 1024" | bc)
    echo "📁 检测到 ${FILE_COUNT} 个音频文件，总大小约 ${TOTAL_SIZE_MB} MB"
fi
echo "================================================================="

## 将 ps 的累计CPU时间转为秒
# 支持格式：dd-hh:mm:ss, hh:mm:ss, m:ss.cs
time_to_seconds() {
    local t="$1"
    echo "$t" | awk '{
        # 初始化
        seconds = 0
        time_str = $0

        # 检查是否有dash（表示有天数）
        if (match(time_str, /[0-9]+-[0-9]+:[0-9]+:[0-9]+/)) {
            # dd-hh:mm:ss 格式
            split(time_str, parts, /-/)
            days = parts[1]
            rest = parts[2]
            split(rest, time_parts, /:/)
            hours = time_parts[1]
            minutes = time_parts[2]
            secs = time_parts[3]
            seconds = days * 86400 + hours * 3600 + minutes * 60 + secs
        } else if (match(time_str, /[0-9]+:[0-9]+:[0-9]+/)) {
            # hh:mm:ss 格式
            split(time_str, parts, /:/)
            hours = parts[1]
            minutes = parts[2]
            secs = parts[3]
            seconds = hours * 3600 + minutes * 60 + secs
        } else if (match(time_str, /[0-9]+:[0-9]+\.[0-9]+/)) {
            # m:ss.cs 格式（分:秒.厘秒）
            split(time_str, parts, /:/)
            minutes = parts[1]
            sec_and_centisec = parts[2]
            split(sec_and_centisec, sec_parts, /\./)
            secs = sec_parts[1]
            centisecs = (sec_parts[2] != "") ? sec_parts[2] : 0
            seconds = minutes * 60 + secs + centisecs / 100
        }

        printf("%.2f", seconds)
    }'
}

# 读取初始CPU时间（作为基准）
initial_cpu_time_str=$(ps -o time= -p $PID 2>/dev/null | awk '{print $1}')
initial_cpu_sec=$(time_to_seconds "$initial_cpu_time_str")
final_cpu_sec=0

# 循环监控，直到进程消失
while ps -p $PID > /dev/null 2>&1; do
    # 获取当前内存占用 (RSS, Resident Set Size，单位为 KB)
    current_memory_kb=$(ps -o rss= -p $PID 2>/dev/null | awk '{print $1}')

    # 采样间隔等待前，抓取一次当前CPU累计时间和墙钟时间
    sleep $SAMPLE_INTERVAL
    curr_cpu_time_str=$(ps -o time= -p $PID 2>/dev/null | awk '{print $1}')
    curr_wall_ts=$(date +%s.%N)

    # 进程可能在sleep期间结束，保证字段存在
    if [[ -z "$curr_cpu_time_str" ]]; then
        break
    fi

    curr_cpu_sec=$(time_to_seconds "$curr_cpu_time_str")

    # 保存最后读取的CPU时间（用于最终计算）
    # 使用awk进行浮点数比较（bash [[ ]] 无法处理浮点数）
    is_valid=$(awk -v val="$curr_cpu_sec" 'BEGIN { print (val > 0 ? 1 : 0) }')
    if [[ "$is_valid" == "1" ]]; then
        final_cpu_sec=$curr_cpu_sec

        # 计算从程序启动到现在的总运行时间
        # 使用 awk 安全计算浮点数，避免bc的复杂性
        # CPU占用率 = (累计CPU秒数 / 运行时间秒数) / CPU核心数 * 100
        avg_cpu_percent=$(awk -v cpu_time="$curr_cpu_sec" -v wall_start="$START_TIME" -v wall_now="$curr_wall_ts" -v cores="$CPU_CORES" 'BEGIN {
            elapsed = wall_now - wall_start
            if (elapsed > 0.001) {  # 避免除以极小的数
                percent = (cpu_time / elapsed) / cores * 100
                printf("%.2f", percent)
            } else {
                print "0.00"
            }
        }')

        # 累计与峰值（跟踪最大的平均CPU使用率）
        if [[ -n "$avg_cpu_percent" ]]; then
            # 防止bc出错，直接用awk比较
            is_greater=$(awk -v curr="$avg_cpu_percent" -v peak="$peak_cpu" 'BEGIN { print (curr > peak ? 1 : 0) }')
            if [[ "$is_greater" == "1" ]]; then
                peak_cpu=$avg_cpu_percent
            fi

            total_cpu_percent=$(awk -v total="$total_cpu_percent" -v curr="$avg_cpu_percent" 'BEGIN { printf("%.2f", total + curr) }')
            cpu_sample_count=$((cpu_sample_count + 1))
        fi
    fi

    # 统计内存（与CPU分开计数，更稳健）
    if [[ -n "$current_memory_kb" && "$current_memory_kb" -gt 0 ]]; then
        total_memory_kb=$((total_memory_kb + current_memory_kb))
        mem_sample_count=$((mem_sample_count + 1))
        if (( current_memory_kb > peak_memory_kb )); then
            peak_memory_kb=$current_memory_kb
        fi
    fi
done

# 等待进程完全消失
wait $PID 2>/dev/null

# 记录结束时间戳（纳秒精度）
END_TIME=$(date +%s.%N)

# 如果在while循环中没有采集到CPU数据，尝试最后一次读取
# （此时进程已结束，但可能仍在wait缓存中）
if (( cpu_sample_count == 0 )); then
    # 尝试从系统获取进程的最终CPU时间（可能已从内核缓存消失）
    # 如果失败，就使用wait的返回值推断
    : # 占位符，保持逻辑完整
fi

# 4. 计算并生成报告
# --------------------------------------------------
echo # 输出一个空行，让格式更清晰
echo "======================= 运行总结报告 ======================="
echo "程序 '${EXECUTABLE_NAME}' (PID: ${PID}) 已停止运行。"
echo

# 计算总运行时长（精确到毫秒）
ELAPSED_SECONDS=$(echo "$END_TIME - $START_TIME" | bc)
# 提取整数部分和小数部分
ELAPSED_INT=$(echo "$ELAPSED_SECONDS" | cut -d. -f1)
ELAPSED_FRAC=$(echo "$ELAPSED_SECONDS" | cut -d. -f2 | cut -c1-3)
# 格式化为 时:分:秒.毫秒
HOURS=$(($ELAPSED_INT/3600))
MINUTES=$(($ELAPSED_INT%3600/60))
SECONDS=$(($ELAPSED_INT%60))
RUNNING_TIME=$(printf '%02d:%02d:%02d.%03d' $HOURS $MINUTES $SECONDS $ELAPSED_FRAC)
echo "  - 总运行时长: ${RUNNING_TIME} (精确到毫秒)"

# 检查是否有采样数据
reported_any=false

# 内存统计
if (( mem_sample_count > 0 )); then
    average_memory_kb=$((total_memory_kb / mem_sample_count))
    peak_memory_mb=$(echo "scale=2; $peak_memory_kb / 1024" | bc)
    average_memory_mb=$(echo "scale=2; $average_memory_kb / 1024" | bc)
    echo "  - 内存使用峰值: ${peak_memory_mb} MB (${peak_memory_kb} KB)"
    echo "  - 内存使用平均值: ${average_memory_mb} MB (${average_memory_kb} KB)"
    reported_any=true
fi

# CPU统计（基于累计CPU时间计算）
if (( cpu_sample_count > 0 )); then
    # 采样法：使用多次采样的平均值
    average_cpu=$(awk -v total="$total_cpu_percent" -v count="$cpu_sample_count" 'BEGIN { printf("%.2f", total / count) }')
    echo "  - CPU使用峰值: ${peak_cpu}% (整体，${CPU_CORES}核)"
    echo "  - CPU使用平均值: ${average_cpu}% (整体，${CPU_CORES}核)"
    echo "  - CPU采样次数: ${cpu_sample_count} 次 (每${SAMPLE_INTERVAL}秒采样)"
    reported_any=true
else
    # 最终统计法：当采样失败时，用最后读取的CPU时间计算
    if [[ -n "$final_cpu_sec" ]]; then
        final_cpu_percent=$(awk -v cpu_time="$final_cpu_sec" -v wall_start="$START_TIME" -v wall_end="$END_TIME" -v cores="$CPU_CORES" 'BEGIN {
            elapsed = wall_end - wall_start
            if (elapsed > 0.001) {
                percent = (cpu_time / elapsed) / cores * 100
                printf("%.2f", percent)
            } else {
                print "0.00"
            }
        }')
        echo "  - CPU使用峰值: ${final_cpu_percent}% (整体，${CPU_CORES}核)"
        echo "  - CPU使用平均值: ${final_cpu_percent}% (整体，${CPU_CORES}核)"
        echo "  - 说明: 基于进程最终CPU累计时间计算"
        reported_any=true
    fi
fi

if [[ "$reported_any" = false ]]; then
    echo "  - 内存/CPU使用情况: 程序运行时间不足 ${SAMPLE_INTERVAL} 秒，未采集到有效数据。"
fi

# 计算处理速度（如果检测到了音频文件）
is_elapsed_valid=$(awk -v elapsed="$ELAPSED_SECONDS" 'BEGIN { print (elapsed > 0 ? 1 : 0) }')
if [[ $FILE_COUNT -gt 0 && "$is_elapsed_valid" -eq 1 ]]; then
    PROCESSING_SPEED_MB=$(awk -v size="$TOTAL_SIZE_MB" -v elapsed="$ELAPSED_SECONDS" 'BEGIN { printf("%.2f", size / elapsed) }')
    echo "  - 处理速度: ${PROCESSING_SPEED_MB} MB/s (${TOTAL_SIZE_MB} MB ÷ ${ELAPSED_SECONDS}s)"
fi

echo "================================================================="
