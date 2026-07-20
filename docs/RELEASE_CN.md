[English](RELEASE.md) | [中文](RELEASE_CN.md)

# 本地发行 staging

MacinMeter 0.2.0 具有一套本地、显式的制品契约。staging 只在
`target/release-staging` 下构建和验证字节，不上传、不签名、不公证，也不创建
GitHub Release。

## 环境要求

- release candidate 必须来自干净的 Git 工作树；
- Python 3.11 或更新版本；
- Rust 1.88 或更新版本，以及 locked Cargo graph；
- 纳入 GUI 时还需要 Node.js 和当前平台的 Tauri prerequisites。

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
- smoke document 必须走 WAV integer-PCM route，并继续标记为
  `foo_dr_meter_1_0_8_candidate_v1 / unverified`。

## 当前 host 的 macOS GUI 制品

在 macOS 上显式纳入 Tauri DMG：

```bash
python3 scripts/stage-release.py stage --include-gui
```

它只构建当前 Rust host 架构；`aarch64-apple-darwin` 结果不代表 universal 或
x86_64。验证器会：

- 使用 `hdiutil` 校验 DMG 结构；
- 只读挂载，并要求恰好一个顶层 `.app`；
- 核对 bundle version 与 identifier；
- 要求 executable 架构与制品名完全一致；
- 记录严格 `codesign` 验证是否成功；
- 在不启动 GUI 的情况下卸载镜像。

当前未签名、未 notarize 的构建会明确标记为
`local_staging_only` / `local_unnotarized`。结构 smoke 成功不等于已经通过
Gatekeeper、Developer ID、公证或公开分发要求。Windows/Linux GUI，以及 macOS
x86_64/universal 制品仍需在真实目标上构建和检查后才能形成声明。

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
