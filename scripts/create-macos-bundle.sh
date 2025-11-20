#!/bin/bash

#=============================================================================
# MacinMeter DR Tool - macOS App Bundle 生成脚本
# Create a macOS App Bundle (.app) with drag-and-drop support
#=============================================================================

set -e

# 颜色定义 / Color definitions
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# 项目根目录 / Project root
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="${PROJECT_ROOT}/target"
APP_NAME="MacinMeter DR Tool"
APP_BUNDLE="${PROJECT_ROOT}/${APP_NAME}.app"
EXECUTABLE_NAME="MacinMeter-DR-Tool"

#=============================================================================
# 工具函数 / Helper functions
#=============================================================================

print_header() {
    echo -e "\n${BLUE}═══════════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}$1${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}\n"
}

print_success() {
    echo -e "${GREEN}✓ $1${NC}"
}

print_error() {
    echo -e "${RED}✗ $1${NC}"
}

print_warning() {
    echo -e "${YELLOW}⚠ $1${NC}"
}

print_info() {
    echo -e "${BLUE}ℹ $1${NC}"
}

#=============================================================================
# 主流程 / Main process
#=============================================================================

print_header "MacinMeter DR Tool — macOS App Bundle 生成器"

# 1. 查找最新的可执行文件 / Find latest executable
print_info "正在扫描可执行文件... / Scanning for executables..."

LATEST_BINARY=$(ls -t "${TARGET_DIR}"/MacinMeter-DR-Tool-* 2>/dev/null | head -1)

if [ -z "$LATEST_BINARY" ]; then
    print_error "未找到可执行文件！请先运行 'cargo build --release' / No executable found! Please run 'cargo build --release' first"
    echo "Expected location: ${TARGET_DIR}/MacinMeter-DR-Tool-*"
    exit 1
fi

BINARY_NAME=$(basename "$LATEST_BINARY")
BINARY_SIZE=$(du -h "$LATEST_BINARY" | cut -f1)

print_success "找到可执行文件 / Found executable: $BINARY_NAME ($BINARY_SIZE)"

# 2. 确认用户操作 / Confirm with user
echo -e "\n${YELLOW}即将创建以下内容：/ About to create:${NC}"
echo "  App Bundle:    ${APP_BUNDLE}"
echo "  Binary:        $LATEST_BINARY"
echo ""
read -p "是否继续? 输入 y 确认 / Continue? Enter 'y' to confirm: " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    print_warning "操作已取消 / Operation cancelled"
    exit 0
fi

# 3. 清理旧的 App Bundle / Clean old bundle
if [ -d "$APP_BUNDLE" ]; then
    print_info "删除旧的 App Bundle / Removing old bundle..."
    rm -rf "$APP_BUNDLE"
fi

# 4. 创建目录结构 / Create directory structure
print_info "创建目录结构 / Creating directory structure..."
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"
print_success "目录结构已创建 / Directory structure created"

# 5. 复制可执行文件 / Copy executable
print_info "复制可执行文件 / Copying executable..."
cp "$LATEST_BINARY" "$APP_BUNDLE/Contents/MacOS/macinmeter-binary"
chmod +x "$APP_BUNDLE/Contents/MacOS/macinmeter-binary"
print_success "可执行文件已复制 / Executable copied"

# 6. 创建包装脚本 / Create wrapper script
print_info "创建包装脚本 / Creating wrapper script..."
cat > "$APP_BUNDLE/Contents/MacOS/$EXECUTABLE_NAME" << 'WRAPPER_SCRIPT'
#!/bin/bash

# MacinMeter DR Tool 包装脚本
# Wrapper script for MacinMeter DR Tool
# 支持双击启动和文件拖拽 / Supports double-click launch and drag-and-drop

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY="${SCRIPT_DIR}/macinmeter-binary"

# 如果有参数（拖拽传递或CLI调用），直接传递给二进制
# If arguments are provided (drag-and-drop or CLI), pass them directly to binary
if [ $# -gt 0 ]; then
    exec "$BINARY" "$@"
else
    # 无参数时，扫描脚本所在的父目录
    # Without arguments, scan the directory containing the app
    PARENT_DIR="$(dirname "$(dirname "$(dirname "$SCRIPT_DIR")")")"
    exec "$BINARY" "$PARENT_DIR"
fi
WRAPPER_SCRIPT

chmod +x "$APP_BUNDLE/Contents/MacOS/$EXECUTABLE_NAME"
print_success "包装脚本已创建 / Wrapper script created"

# 7. 创建 Info.plist / Create Info.plist
print_info "创建 Info.plist..."
cat > "$APP_BUNDLE/Contents/Info.plist" << 'PLIST_CONTENT'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>zh_CN</string>
	<key>CFBundleExecutable</key>
	<string>MacinMeter-DR-Tool</string>
	<key>CFBundleIdentifier</key>
	<string>com.sakuzy.macinmeter-dr-tool</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>MacinMeter DR Tool</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>0.1.0</string>
	<key>CFBundleVersion</key>
	<string>1</string>
	<key>CFBundleDocumentTypes</key>
	<array>
		<dict>
			<key>CFBundleTypeExtensions</key>
			<array>
				<string>flac</string>
				<string>wav</string>
				<string>mp3</string>
				<string>aac</string>
				<string>m4a</string>
				<string>opus</string>
				<string>ogg</string>
				<string>wv</string>
				<string>ape</string>
				<string>alac</string>
			</array>
			<key>CFBundleTypeIconFile</key>
			<string></string>
			<key>CFBundleTypeName</key>
			<string>Audio File</string>
			<key>CFBundleTypeRole</key>
			<string>Editor</string>
		</dict>
	</array>
	<key>NSHumanReadableCopyright</key>
	<string>MIT License</string>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>NSRequiresIPhoneOS</key>
	<false/>
</dict>
</plist>
PLIST_CONTENT

print_success "Info.plist 已创建 / Info.plist created"

# 8. 验证结构 / Verify structure
print_info "验证 App Bundle 结构 / Verifying bundle structure..."

check_file() {
    if [ -f "$1" ]; then
        print_success "✓ $(basename $1)"
        return 0
    else
        print_error "✗ $(basename $1) 缺失 / missing"
        return 1
    fi
}

check_executable() {
    if [ -x "$1" ]; then
        print_success "✓ $(basename $1) (executable)"
        return 0
    else
        print_error "✗ $(basename $1) 不可执行 / not executable"
        return 1
    fi
}

all_good=true
check_file "$APP_BUNDLE/Contents/Info.plist" || all_good=false
check_executable "$APP_BUNDLE/Contents/MacOS/$EXECUTABLE_NAME" || all_good=false
check_executable "$APP_BUNDLE/Contents/MacOS/macinmeter-binary" || all_good=false

if [ "$all_good" = true ]; then
    print_success "所有文件验证通过 / All files verified"
else
    print_error "某些文件验证失败 / Some files failed verification"
    exit 1
fi

# 9. 显示完成信息 / Display completion info
print_header "✓ App Bundle 创建成功！/ App Bundle created successfully!"

echo -e "${GREEN}位置 / Location:${NC}"
echo "  ${APP_BUNDLE}"
echo ""
echo -e "${GREEN}使用方式 / Usage:${NC}"
echo "  1. 双击启动 / Double-click to launch:"
echo "     ${APP_BUNDLE}"
echo ""
echo "  2. 拖拽文件或文件夹到应用图标 / Drag audio files/folders onto the app icon"
echo ""
echo "  3. 命令行使用 / CLI usage:"
echo "     open -a 'MacinMeter DR Tool' ~/Music/song.flac"
echo ""
echo "  4. 直接运行 / Direct execution:"
echo "     '${APP_BUNDLE}/Contents/MacOS/${EXECUTABLE_NAME}' /path/to/audio"
echo ""
echo -e "${GREEN}下一步建议 / Next steps:${NC}"
echo "  - 在 Finder 中测试应用 / Test the app in Finder"
echo "  - 尝试拖拽音频文件 / Try dragging audio files onto it"
echo "  - 可选：使用 create-dmg 创建分发镜像 / Optional: use create-dmg to create DMG"
echo ""

# 10. 提供打包成 DMG 的建议 / Suggest DMG packaging
print_info "如何创建 DMG 分发镜像？/ How to create a DMG distribution image?"
echo "  安装工具 / Install create-dmg:"
echo "    brew install create-dmg"
echo ""
echo "  创建 DMG / Create DMG:"
echo "    create-dmg --volname '${APP_NAME}' --window-pos 200 120 --window-size 600 400 '${APP_NAME}.dmg' '${APP_BUNDLE}'"
echo ""
