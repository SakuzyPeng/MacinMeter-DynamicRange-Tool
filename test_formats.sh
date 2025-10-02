#!/bin/bash
# 测试不同格式的音频文件DR值一致性

AUDIO_DIR="/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/audio"
TOOL="./target/release/MacinMeter-DynamicRange-Tool-foo_dr"

echo "=== 编译Release版本 ==="
cargo build --release

echo -e "\n=== 测试各格式DR值 ==="
for file in "$AUDIO_DIR"/test_compatibility.{wav,flac,aac,ogg,mp3,m4a}; do
    if [ -f "$file" ]; then
        filename=$(basename "$file")
        echo -e "\n📁 $filename:"
        $TOOL "$file" | grep -E "DR|Number of samples"
    fi
done

echo -e "\n=== 总结 ==="
echo "✅ 所有格式应该具有相似的DR值（误差<0.5dB）"
echo "⚠️  MP3自动使用串行解码器"
echo "🚀 其他格式使用并行解码器"
