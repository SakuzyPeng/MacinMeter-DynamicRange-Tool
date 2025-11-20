#!/bin/bash

#=============================================================================
# MacinMeter DR Tool - 包装脚本调试工具
# Wrapper Script Debug Tool
#=============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../MacinMeter DR Tool.app/Contents/MacOS" && pwd)"
WRAPPER_SCRIPT="${SCRIPT_DIR}/MacinMeter-DR-Tool"

echo "================================"
echo "MacinMeter DR Tool - Wrapper Debug"
echo "================================"
echo ""
echo "启用调试模式... / Enabling debug mode..."
echo ""

# 创建临时备份
BACKUP="${WRAPPER_SCRIPT}.backup"
if [ ! -f "$BACKUP" ]; then
    cp "$WRAPPER_SCRIPT" "$BACKUP"
    echo "已创建备份 / Backup created: $BACKUP"
fi

# 编辑脚本，启用调试输出
sed -i '' 's/^# echo "DEBUG:/echo "DEBUG:/' "$WRAPPER_SCRIPT"

echo "调试模式已启用"
echo ""
echo "使用说明 / Usage:"
echo "1. 双击 MacinMeter DR Tool.app 或拖拽文件"
echo "2. 查看终端输出，看是否显示 DEBUG 信息"
echo "3. 如果显示 'Argument count: 0'，表示拖拽时参数未被传递"
echo ""
echo "运行以恢复原始脚本："
echo "  ./scripts/debug-wrapper.sh --disable"
echo ""

if [ "$1" = "--disable" ]; then
    echo "关闭调试模式... / Disabling debug mode..."
    if [ -f "$BACKUP" ]; then
        cp "$BACKUP" "$WRAPPER_SCRIPT"
        rm "$BACKUP"
        echo "已恢复原始脚本 / Original script restored"
    fi
fi
