[English](RELEASE.md) | [中文](RELEASE_CN.md)

# 发行制品 staging

MacinMeter 0.2.0 具有一套显式的制品契约。staging 只在
`target/release-staging` 下构建和验证字节，不上传、不签名、不公证，也不创建
GitHub Release。

有界 GitHub Actions 会在 `main` push 或手动触发后，于 macOS 26 arm64 job 中运行
clean staging。`main` push 产生的 CLI archive 与 DMG 通过验证后会随 runner 丢弃；
显式手动触发则使用下文的 unsigned candidate 契约，并将结果保留 14 天。两条路径都
不会创建 tag 或 GitHub Release。

## 0.2.0 发行范围

0.2.0 只面向 Apple Silicon macOS：

- target：`aarch64-apple-darwin`；
- 最低系统：macOS 11.0；
- 制品：arm64 CLI archive 与 arm64 Tauri DMG；
- 不执行 Developer ID 签名、notarization 或 stapling。

0.2.0 不提供 Intel/universal macOS 构建，也不提供 Windows/Linux GUI 包。“未签名”
表示没有 Developer ID 身份；编译器或链接器产生的 ad-hoc metadata 不构成开发者
签名或 Gatekeeper 声明。

## 环境要求

- release candidate 必须来自干净的 Git 工作树；
- Python 3.11 或更新版本；
- Rust 1.88 或更新版本，以及 locked Cargo graph；
- 共享版本契约需要 Node.js；纳入 GUI 时还需要当前平台的 Tauri prerequisites。

脚本会记录准确 source commit/state、host target、Rust/Cargo 版本及两个 lockfile
hash；默认拒绝脏工作树。

## CLI 制品

在仓库根目录运行：

```bash
python3 scripts/stage-release.py stage
```

host CLI 使用以下命令构建：

```bash
cargo build --locked --release -p macinmeter-cli
```

生成的 `macinmeter-cli-<version>-<host>.tar.gz` 包含：

- CLI executable；
- `LICENSE`；
- `README.md`；
- `RELEASE_NOTES.md`；
- `THIRD_PARTY_NOTICES.md`；
- 记录每个 payload 文件尺寸和 SHA-256 的 `ARTIFACT_MANIFEST.json`。

验证器会安全解包、核对准确 member 集合与 payload hash，再运行解包后的 executable：

- `macinmeter --version` 必须报告 workspace version；
- 仓库内 WAV fixture 必须产生唯一的 schema-v3 JSON document；
- smoke document 必须走 WAV integer-PCM route，包含固定算法参数，且不暴露内部 profile 或状态字段。

## 当前 host 的 macOS GUI 制品

在 macOS 上显式纳入 Tauri DMG：

```bash
python3 scripts/stage-release.py stage --include-gui
```

GUI staging 只支持 `aarch64-apple-darwin` Rust host。验证器会：

- 使用 `hdiutil` 校验 DMG 结构；
- 只读挂载，并要求恰好一个顶层 `.app`；
- 核对 bundle version 与 identifier；
- 要求 executable 架构与制品名完全一致；
- 要求 `LSMinimumSystemVersion` 为 macOS 11.0；
- 记录严格 `codesign`、Developer ID 与其他 Apple identity signature 观察，并且
  不把 ad-hoc metadata 当作开发者身份；
- 在不启动 GUI 的情况下卸载镜像。

当前未签名、未 notarize 的构建，无论来自本地还是临时 CI 门禁，都会明确标记为
`local_staging_only` / `local_unnotarized`。结构 smoke 成功不等于已经通过
Gatekeeper、Developer ID、公证或公开分发要求。Windows/Linux GUI，以及 macOS
x86_64/universal 制品不属于 0.2.0 发行范围。

## 未签名 Apple Silicon release candidate

面向发布的 candidate mode 与本地 staging 分离：

```bash
python3 scripts/stage-release.py stage \
  --include-gui \
  --unsigned-macos-arm64-candidate
```

它写入 `target/release-candidates/0.2.0/aarch64-apple-darwin`，并拒绝脏 source、
非 arm64 host、Rust 1.88/Node.js 22 以外的工具链、缺少 GUI、`--allow-dirty` 或
`--replace`。manifest 记录完整 Rust/Cargo/Node/npm identity，并标记为
`unsigned_macos_arm64_release_candidate`，不会声称已经签名、公证、通过 Gatekeeper
或完成发布。

手动触发 **Workspace validation** 时必须选择 `main`。macOS job 会构建这份准确
candidate，并保留一个 14 天有效的 workflow artifact。workflow 继续只有只读仓库
权限，不能创建 tag 或 Release；candidate 是最终人工确认的输入，不是公开资产。
只有整次三平台 workflow 成功时，这份 candidate 才能进入最终复核；失败 run 中保留
的字节不构成发行证据。

拟用于 GitHub Release 的双语文案保存在
[`RELEASE_DRAFT_0.2.0.md`](RELEASE_DRAFT_0.2.0.md)。

## Checksum 与反向验证

每个 staging 目录包含：

- `RELEASE_MANIFEST.json`；
- 一个或多个分发制品；
- 覆盖 release manifest 与全部制品的 `SHA256SUMS`。

对最终字节重新验证：

```bash
python3 scripts/stage-release.py verify \
  target/release-staging/0.2.0/aarch64-apple-darwin
```

同一 verifier 也接受 unsigned candidate 目录，并额外核对 clean source、target、
准确制品集合与 distribution contract。

验证器要求 checksum 精确覆盖目录内容，并重新运行所有制品 smoke。checksum 只建立
字节身份，不是签名。

在 payload 字节与记录身份相同的条件下，CLI tar container 是确定性的；本流程不
声称 Rust binary 或 Tauri DMG 能跨 toolchain、SDK、机器或签名环境复现。

## 仅供开发的 staging

开发发行脚本时，可显式允许脏工作树：

```bash
python3 scripts/stage-release.py stage --allow-dirty
```

目录和制品名都会带上 `-dirty`，manifest 也会记录
`source.state = "dirty"`；这种制品绝不是 release candidate。`--replace` 只会
替换生成的 staging 目录，或已经具有 release manifest 与 checksum marker 的目录。
