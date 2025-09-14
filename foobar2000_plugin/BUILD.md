# foobar2000插件构建说明

## 🚀 快速构建（推荐）

使用自动化构建脚本：
```bash
./build_plugin.sh
```

这个脚本会：
1. 清理并重新构建Rust核心库
2. 清理并重新构建C++插件
3. 验证所有构建产物
4. 显示安装说明

## 🔧 手动构建

### 1. 构建Rust核心库
```bash
cd rust_core
cargo clean && cargo build --release
```

### 2. 构建C++插件
```bash
mkdir -p build && cd build
cmake .. && make -j4
```

### 3. 安装插件
生成的插件文件：`build/foo_dr_macinmeter.fb2k-component`

## 🛠️ 构建系统改进

### 自动依赖检查
CMakeLists.txt现在包含：
- Rust源文件依赖跟踪
- 自动Cargo.toml依赖检查
- 正确的库路径引用

### 防止构建问题
1. **源文件变更检测**：任何`.rs`或`Cargo.toml`文件变更都会触发Rust重新构建
2. **正确的库路径**：使用`RUST_LIB_FULL_PATH`变量确保路径一致
3. **依赖顺序**：确保Rust库先于C++插件构建
4. **清理构建**：脚本包含完整清理步骤

### 常见问题解决

#### 问题：插件使用旧版本Rust库
**原因**：CMake缓存了旧的库文件路径

**解决**：
```bash
rm -rf build && ./build_plugin.sh
```

#### 问题：Rust库路径错误
**原因**：CMakeLists.txt中的硬编码路径不正确

**解决**：现已使用`${RUST_LIB_FULL_PATH}`变量

#### 问题：构建时序问题
**原因**：C++插件在Rust库构建完成前开始链接

**解决**：现已添加正确的依赖关系：
```cmake
add_dependencies(foo_dr_macinmeter build_rust_core)
```

## 📁 构建产物

成功构建后，以下文件应该存在且时间戳一致：
- `rust_core/target/release/libmacinmeter_dr_core.dylib`
- `build/foo_dr_macinmeter.fb2k-component`
- `build/plugin_bundle/mac/foo_dr_macinmeter.component/Contents/Resources/libmacinmeter_dr_core.dylib`

## 🔍 验证构建

检查所有文件的时间戳是否匹配：
```bash
ls -la rust_core/target/release/libmacinmeter_dr_core.dylib
ls -la build/foo_dr_macinmeter.fb2k-component
```

如果时间戳不匹配，说明存在构建依赖问题，需要使用`./build_plugin.sh`重新构建。