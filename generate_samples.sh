#!/bin/bash

# 生成性能测试样本
SAMPLES_DIR="/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/benchmark_samples"
SOURCE_FILE="/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/audio/large audio/未命名文件夹/HIRES . 贝多芬第九交响曲 两德统一版 自由颂 伯恩斯坦 （DRV母带重制）.flac"

cd "$SAMPLES_DIR"

# 定义生成参数
# 格式: 时长(秒) 采样率 位深 名称前缀
declare -a CONFIGS=(
    "300 96000 24 5min_96k24b"      # 5分钟 96kHz 24bit (~20MB)
    "900 96000 24 15min_96k24b"     # 15分钟 96kHz 24bit (~60MB)
    "1800 96000 24 30min_96k24b"    # 30分钟 96kHz 24bit (~120MB)
    "3600 96000 24 60min_96k24b"    # 60分钟 96kHz 24bit (~240MB)
    "300 48000 24 5min_48k24b"      # 5分钟 48kHz 24bit (~10MB)
    "900 48000 24 15min_48k24b"     # 15分钟 48kHz 24bit (~30MB)
    "1800 48000 24 30min_48k24b"    # 30分钟 48kHz 24bit (~60MB)
    "3600 48000 24 60min_48k24b"    # 60分钟 48kHz 24bit (~120MB)
    "300 44100 16 5min_44k16b"      # 5分钟 44.1kHz 16bit (~7MB)
    "900 44100 16 15min_44k16b"     # 15分钟 44.1kHz 16bit (~21MB)
    "1800 44100 16 30min_44k16b"    # 30分钟 44.1kHz 16bit (~42MB)
    "3600 44100 16 60min_44k16b"    # 60分钟 44.1kHz 16bit (~84MB)
)

echo "📁 样本生成位置: $SAMPLES_DIR"
echo "📼 源文件: $(basename "$SOURCE_FILE")"
echo "🔧 FFmpeg处理参数："
echo "   时长 采样率 位深 -> 输出文件名"
echo "==============================================="

# 生成每个样本
for config in "${CONFIGS[@]}"; do
    read -r duration sr bitdepth name <<< "$config"
    
    output_file="${name}.flac"
    
    # 检查文件是否已存在
    if [ -f "$output_file" ]; then
        size=$(du -h "$output_file" | awk '{print $1}')
        echo "⏭️  $output_file ($size) - 已存在，跳过"
        continue
    fi
    
    echo "⏳ 生成 $duration秒, ${sr}Hz, ${bitdepth}bit -> $output_file"
    
    # 使用ffmpeg生成样本
    # 采样率通过 -af "aformat=sample_rates=$sr"
    # 位深通过 -acodec flac -sample_fmt ... (对FLAC来说，位深在原始PCM转换时处理)
    ffmpeg -i "$SOURCE_FILE" \
        -t "$duration" \
        -acodec flac \
        -ar "$sr" \
        -ac 2 \
        "$output_file" 2>&1 | grep -E "(Duration|error|Error)"
    
    if [ -f "$output_file" ]; then
        size=$(du -h "$output_file" | awk '{print $1}')
        echo "✅ 生成成功: $output_file ($size)"
    else
        echo "❌ 生成失败: $output_file"
    fi
    
    echo ""
done

echo "==============================================="
echo "📊 生成完成。汇总信息："
ls -lh *.flac | awk '{printf "   %-30s %6s\n", $9, $5}'
echo "==============================================="
