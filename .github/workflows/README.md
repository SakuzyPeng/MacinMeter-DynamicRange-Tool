# 🚀 GitHub Actions CI/CD 工作流说明

## 📋 概述

这个GitHub Actions工作流专门为私有仓库优化，实现了高效的多平台构建和质量保证。

## 🎯 核心特性

### 🔍 智能变更检测
- **路径过滤**: 只有Rust代码或配置文件变更时才触发构建
- **跳过无关变更**: 纯文档修改不会触发完整构建流程
- **条件执行**: 大幅减少不必要的资源消耗

### 🚀 私有仓库优化

#### 📦 多层缓存策略
```yaml
# 分离式缓存 - 最大化命中率
- Registry Cache: Cargo依赖注册表
- Git Cache: Git依赖数据库  
- Build Cache: 平台特定的构建产物
- Tools Cache: 工具安装缓存
```

#### 💰 成本控制
- **并发限制**: `max-parallel: 2` 避免资源争用
- **存储优化**: 14天artifact保留期
- **压缩传输**: Unix系统二进制gzip压缩
- **网络优化**: 减少重试和超时时间

### 🏗️ 多平台构建

支持的目标平台：
- **Windows x64**: `x86_64-pc-windows-msvc`
- **macOS Intel**: `x86_64-apple-darwin` 
- **macOS ARM64**: `aarch64-apple-darwin`
- **Linux x64**: `x86_64-unknown-linux-gnu`

### 🕐 分支和时间戳命名

生成的可执行文件格式：
```
dr-meter-{branch}_2025-01-15_14-30-45_UTC_platform-arch[.exe]
```

包含分支名、精确到秒的时间戳和UTC时区信息，避免不同分支之间的文件名冲突。

## 🔧 使用方式

### 自动触发
- **Push到主分支**: 自动运行完整流程
- **Pull Request**: 运行质量检查和构建
- **Tag推送**: 额外创建GitHub Release

### 手动触发
在Actions页面可以手动运行，支持选项：
- `skip_tests`: 跳过测试以加快构建
- `build_targets`: 指定构建平台 (如: "windows,macos")

### 发布流程
1. 创建版本tag: `git tag v1.0.0`
2. 推送tag: `git push origin v1.0.0`  
3. 自动创建GitHub Release并上传所有平台二进制文件

## 🛡️ 质量保证

### 代码检查流程
1. **格式检查**: `cargo fmt --check`
2. **静态分析**: `cargo clippy -- -D warnings`
3. **编译检查**: `cargo check --all-features`
4. **安全审计**: `cargo audit`
5. **单元测试**: `cargo test --all-features`

### 构建验证
- 二进制文件完整性检查
- 文件大小验证
- 可执行权限设置

## 📊 性能数据

### 缓存效率
- **首次构建**: ~5-8分钟
- **缓存命中**: ~2-3分钟  
- **仅文档变更**: ~30秒 (跳过构建)

### 存储优化
- **压缩率**: Unix二进制 ~40-60% 减少
- **Artifact大小**: 通常 < 10MB per平台
- **总存储**: 所有平台 < 40MB

## 🎛️ 高级配置

### 环境变量
```yaml
CARGO_TERM_COLOR: always        # 彩色输出
RUST_BACKTRACE: 1               # 调试信息
CARGO_NET_RETRY: 2              # 网络重试次数
CARGO_NET_TIMEOUT: 30           # 网络超时时间
```

### 并发控制
```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true       # 取消重复运行
```

## 🔮 未来扩展

可以考虑添加：
- [ ] 基准测试集成
- [ ] 代码覆盖率报告
- [ ] Docker镜像构建
- [ ] 性能回归检测
- [ ] 自动依赖更新

## ❓ 故障排除

### 常见问题

**构建失败**
- 检查Rust toolchain版本兼容性
- 验证依赖项licenses
- 确认平台特定依赖可用性

**缓存问题**
- 清理: 删除仓库中的Actions缓存
- 重新生成: 修改`Cargo.lock`强制缓存刷新

**权限问题**
- 确保`GITHUB_TOKEN`有足够权限
- 检查仓库settings中的Actions权限

---

*这个工作流经过精心优化，平衡了构建速度、资源消耗和代码质量，特别适合私有仓库的持续集成需求。*