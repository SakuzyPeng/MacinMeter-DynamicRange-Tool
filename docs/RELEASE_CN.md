[English](RELEASE.md) | [中文](RELEASE_CN.md)

# 发行制品 staging

MacinMeter 0.3.1 具有一套显式的制品契约。staging 只在
`target/release-staging` 下构建和验证字节，不上传、不签名、不公证，也不创建
GitHub Release。

有界 GitHub Actions 会在 `main` push 后，于 Windows Server 2025 x64 与 macOS 26
arm64 job 中运行相同的 clean staging 契约。每个 job 的 CLI archive 与 GUI installer
通过验证后会随 runner 丢弃；显式手动触发则使用下文的 unsigned candidate 契约，并
将两份结果保留 14 天。两条路径都不会创建 tag 或 GitHub Release。

## 0.3.1 发行范围

0.3.1 包含两个平台 slice：

- Apple Silicon macOS：target 为 `aarch64-apple-darwin`，最低 macOS 11.0，包含
  arm64 CLI archive 与 arm64 Tauri DMG；
- Windows x64：target 为 `x86_64-pc-windows-msvc`，包含 x64 CLI archive 与承载
  x64 Tauri GUI 的 NSIS installer。

两个 slice 都未签名：macOS 不执行 Developer ID 签名、notarization 或 stapling，
Windows 不执行 Authenticode 签名。因此用户可能需要在 macOS 上显式“打开”，或在
Windows 上通过 SmartScreen 的未知发布者提示。0.3.1 不提供 Intel/universal macOS、
Windows ARM64/32-bit 或 Linux GUI 制品。

未签名是既定立场，不是待办项。代码签名证书颁发给具体个人，签名即等于把维护者的法定
姓名随每一份制品一起公开；阻碍是隐私而非成本或工作量，可预见的未来内不会改变。用户
向材料必须明确说明这一点，不得使用“尚未签名”一类措辞——那会让读者以为只要等一等就会
有已签名的构建。

## 环境要求

- release candidate 必须来自干净的 Git 工作树；
- Python 3.11 或更新版本；
- Rust 1.88 或更新版本，以及 locked Cargo graph；
- 共享版本契约需要 Node.js；纳入 GUI 时还需要当前平台的 Tauri prerequisites；
- 在 Windows 上 staging 或验证 NSIS installer 时需要 7-Zip。

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

- `mdrmeter --version` 必须报告 workspace version；
- 仓库内 WAV fixture 必须产生唯一的 schema-v4 JSON document；
- smoke document 必须走 WAV integer-PCM route，包含固定算法参数，且不暴露内部 profile 或状态字段。

## 当前 host 的 GUI 制品

在受支持的 macOS 或 Windows host 上，显式纳入当前平台的 Tauri installer：

```bash
python3 scripts/stage-release.py stage --include-gui
```

macOS 路径只支持 `aarch64-apple-darwin`。验证器会：

- 使用 `hdiutil` 校验 DMG 结构；
- 只读挂载，并要求恰好一个顶层 `.app`；
- 核对 bundle version 与 identifier；
- 要求 executable 架构与制品名完全一致；
- 要求 `LSMinimumSystemVersion` 为 macOS 11.0；
- 记录严格 `codesign`、Developer ID 与其他 Apple identity signature 观察，并且
  不把 ad-hoc metadata 当作开发者身份；
- 在不启动 GUI 的情况下卸载镜像。

Windows 路径只支持 `x86_64-pc-windows-msvc`。它要求 7-Zip，并会：

- 验证外层 installer 的 DOS/PE header，并记录其 COFF machine type；
- 把 NSIS 解到 candidate 目录之外的临时目录，并要求恰好一个
  `macinmeter-gui.exe` payload；
- 验证 payload 的 DOS/PE header，并要求实测 COFF machine type 为 x86_64；
- 读取其 file-version resource，并要求与 workspace version 相同；
- 查询 installer 与 payload 的 Authenticode，要求两者均报告 `NotSigned` 且没有
  signer certificate；
- 记录解包后 payload 的 SHA-256，清理临时目录，不启动也不安装程序。

本地 GUI 构建都会标记为 `local_staging_only`；macOS 另标记
`local_unnotarized`，Windows 另标记 `local_unsigned`。结构 smoke 成功不等于已经
具备 Gatekeeper、Developer ID、公证、SmartScreen 声誉或公开分发条件。

## 未签名 release candidate

面向发布的两个 candidate mode 与本地 staging 分离，且必须在对应 host 上运行：

```bash
python3 scripts/stage-release.py stage \
  --include-gui \
  --unsigned-macos-arm64-candidate

python scripts/stage-release.py stage \
  --include-gui \
  --unsigned-windows-x64-candidate
```

它们分别写入 `target/release-candidates/0.3.1/aarch64-apple-darwin` 与
`target/release-candidates/0.3.1/x86_64-pc-windows-msvc`。每个 mode 都拒绝脏 source、
不匹配的 host、Rust 1.88/Node.js 22 以外的工具链、缺少 GUI、`--allow-dirty` 或
`--replace`。manifest 记录完整 Rust/Cargo/Node/npm identity，并分别标记为
`unsigned_macos_arm64_release_candidate` 或
`unsigned_windows_x64_release_candidate`；两者都不会声称已经签名、公证、具备
Gatekeeper/SmartScreen 条件或完成发布。

手动触发 **Workspace validation** 时必须选择 `main`。Windows 与 macOS job 会分别
构建自己的 candidate，并各保留一个 14 天有效的 workflow artifact。workflow 继续
只有只读仓库权限，不能创建 tag 或 Release。只有整次 workflow 成功，且两份 manifest
记录同一个 source commit 时，两份 candidate 才能进入最终人工复核；失败 run 中保留
的字节不构成发行证据，也不是公开资产。

拟用于 GitHub Release 的双语文案保存在
[`RELEASE_DRAFT_0.3.1.md`](RELEASE_DRAFT_0.3.1.md)。

## Checksum 与反向验证

每个 staging 目录包含：

- `RELEASE_MANIFEST.json`；
- 一个或多个分发制品；
- 覆盖 release manifest 与全部制品的 `SHA256SUMS`。

对最终字节重新验证：

```bash
python3 scripts/stage-release.py verify \
  target/release-staging/0.3.1/aarch64-apple-darwin

python scripts/stage-release.py verify \
  target/release-staging/0.3.1/x86_64-pc-windows-msvc
```

同一 verifier 也接受 unsigned candidate 目录，并额外核对 clean source、target、
准确制品集合与 distribution contract。

验证器要求 checksum 精确覆盖目录内容，并重新运行所有制品 smoke。checksum 只建立
字节身份，不是签名。

在 payload 字节与记录身份相同的条件下，CLI tar container 是确定性的；本流程不
声称 Rust binary、Tauri DMG 或 NSIS installer 能跨 toolchain、SDK、机器或签名
环境复现。

## 仅供开发的 staging

开发发行脚本时，可显式允许脏工作树：

```bash
python3 scripts/stage-release.py stage --allow-dirty
```

目录和制品名都会带上 `-dirty`，manifest 也会记录
`source.state = "dirty"`；这种制品绝不是 release candidate。`--replace` 只会
替换生成的 staging 目录，或已经具有 release manifest 与 checksum marker 的目录。
