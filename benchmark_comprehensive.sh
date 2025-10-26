#!/bin/bash

# 综合性能测试脚本 - 对比串行vs并行
SAMPLES_DIR="/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/benchmark_samples"
EXE="/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/target/release/MacinMeter-DynamicRange-Tool-foo_dr"
RESULT_FILE="/tmp/benchmark_results_$$.csv"

echo "🎯 串行 vs 并行性能对比基准测试"
echo "=================================================="
echo "📁 样本目录: $SAMPLES_DIR"
echo "🚀 可执行文件: $EXE"
echo "📊 结果文件: $RESULT_FILE"
echo ""

# 创建结果文件头
echo "文件名,大小MB,串行时间s,并行时间s,串行速度MBs,并行速度MBs,加速比" > "$RESULT_FILE"

# 测试每个样本文件
sample_count=0
for sample in $(ls "$SAMPLES_DIR"/*.flac 2>/dev/null | sort); do
    sample_count=$((sample_count + 1))
    filename=$(basename "$sample")
    filesize_mb=$(du -m "$sample" | awk '{print $1}')
    
    echo "[$sample_count/12] 📊 测试: $filename (${filesize_mb}MB)"
    echo "─────────────────────────────────────────"
    
    # 创建临时目录用于单个文件测试
    tmpdir="/tmp/macinmeter_benchmark_$$_$(basename "$sample" .flac)"
    mkdir -p "$tmpdir"
    cp "$sample" "$tmpdir/"
    
    # 串行模式
    echo -n "  ⏳ 串行模式 ... "
    serial_output=$("$EXE" "$tmpdir" --serial 2>&1)
    serial_time=$(echo "$serial_output" | grep "运行时间" | head -1)
    serial_t=$(echo "$serial_time" | grep -oE "[0-9]+\.[0-9]+" | head -1)
    serial_s=$(echo "$serial_output" | grep "处理速度" | grep -oE "[0-9]+\.[0-9]+" | head -1)
    
    if [ -z "$serial_t" ]; then
        echo "❌ 失败"
        rm -rf "$tmpdir"
        continue
    fi
    echo "✓ (${serial_t}s, ${serial_s}MB/s)"
    
    # 清理缓存
    sleep 2
    rm -rf "$tmpdir"
    mkdir -p "$tmpdir"
    cp "$sample" "$tmpdir/"
    
    # 并行模式
    echo -n "  ⚡ 并行模式 ... "
    parallel_output=$("$EXE" "$tmpdir" 2>&1)
    parallel_time=$(echo "$parallel_output" | grep "运行时间" | head -1)
    parallel_t=$(echo "$parallel_time" | grep -oE "[0-9]+\.[0-9]+" | head -1)
    parallel_s=$(echo "$parallel_output" | grep "处理速度" | grep -oE "[0-9]+\.[0-9]+" | head -1)
    
    if [ -z "$parallel_t" ]; then
        echo "❌ 失败"
        rm -rf "$tmpdir"
        continue
    fi
    echo "✓ (${parallel_t}s, ${parallel_s}MB/s)"
    
    # 计算加速比
    if [ -n "$serial_t" ] && [ -n "$parallel_t" ]; then
        speedup=$(echo "scale=2; $serial_t / $parallel_t" | bc)
        echo "  📈 加速比: ${speedup}x"
    else
        speedup="N/A"
    fi
    
    # 写入CSV
    echo "$filename,$filesize_mb,$serial_t,$parallel_t,$serial_s,$parallel_s,$speedup" >> "$RESULT_FILE"
    
    # 清理
    rm -rf "$tmpdir"
    echo ""
done

echo "=================================================="
echo "📊 完整性能对比表"
echo ""
awk -F, '
NR==1 {
    printf "%-26s | %6s | %9s | %9s | %12s | %12s | %8s\n", 
           "文件名", "大小MB", "串行(s)", "并行(s)", "串行(MB/s)", "并行(MB/s)", "加速比"
    next
}
{
    printf "%-26s | %6s | %9s | %9s | %12s | %12s | %8s\n",
           substr($1, 1, 26), $2, $3, $4, $5, $6, $7
}' "$RESULT_FILE"

echo ""
echo "✅ 测试完成！结果已保存到: $RESULT_FILE"
echo ""
echo "📈 性能分析："
awk -F, '
NR>1 && $7!="N/A" {
    size = $2
    speedup = $7
    if (size < 50) {
        if (speedup < 1.1) small_no++; else small_yes++
    } else if (size < 200) {
        if (speedup < 1.1) mid_no++; else mid_yes++
    } else {
        if (speedup < 1.1) large_no++; else large_yes++
    }
}
END {
    print "  小文件(<50MB): " small_no " 无加速, " small_yes " 有加速"
    print "  中等文件(50-200MB): " mid_no " 无加速, " mid_yes " 有加速"
    print "  大文件(>200MB): " large_no " 无加速, " large_yes " 有加速"
}' "$RESULT_FILE"
