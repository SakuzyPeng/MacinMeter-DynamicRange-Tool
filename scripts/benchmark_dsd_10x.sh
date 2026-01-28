#!/bin/bash
# DSD 性能基准测试 - 10 次取平均

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
DSD_DIR="$PROJECT_ROOT/audio/dsd"
EXE="$PROJECT_ROOT/target/release/MacinMeter-DynamicRange-Tool-foo_dr"

# 检查
if [ ! -f "$EXE" ]; then
    echo "Error: executable not found: $EXE"
    echo "Run: cargo build --release"
    exit 1
fi

if [ ! -d "$DSD_DIR" ]; then
    echo "Error: DSD directory not found: $DSD_DIR"
    exit 1
fi

DSD_COUNT=$(ls -1 "$DSD_DIR"/*.dsf 2>/dev/null | wc -l | tr -d ' ')
echo "========================================================"
echo "DSD Benchmark - 10 runs average"
echo "Directory: $DSD_DIR"
echo "Files: $DSD_COUNT DSF files"
echo "========================================================"

declare -a TIMES

for i in $(seq 1 10); do
    START=$(date +%s.%N)
    "$EXE" "$DSD_DIR" --serial > /dev/null 2>&1
    END=$(date +%s.%N)
    TIME=$(echo "$END - $START" | bc)
    TIMES+=("$TIME")
    printf "Run %2d: %.3fs\n" "$i" "$TIME"
done

# 计算统计
TOTAL=0
for t in "${TIMES[@]}"; do
    TOTAL=$(echo "$TOTAL + $t" | bc)
done
AVG=$(echo "scale=3; $TOTAL / 10" | bc)

# 排序取中位数
SORTED=($(printf '%s\n' "${TIMES[@]}" | sort -n))
MEDIAN=$(echo "scale=3; (${SORTED[4]} + ${SORTED[5]}) / 2" | bc)

# 最小/最大
MIN=${SORTED[0]}
MAX=${SORTED[9]}

echo "========================================================"
echo "Results:"
echo "  Average: ${AVG}s"
echo "  Median:  ${MEDIAN}s"
echo "  Min:     ${MIN}s"
echo "  Max:     ${MAX}s"
echo "========================================================"
