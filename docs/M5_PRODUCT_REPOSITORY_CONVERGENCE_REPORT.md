# M5：产品与仓库收敛报告

- 状态：Closed
- 日期：2026-07-20
- 决策基线：ADR-0006
- clean staging source：`78fb266bdc9434be898154a0f64dfb381638d533`

## 结论

M5 已把 M0–M4 建立的可信生产主链收敛为可一致构建、验证、说明和本地 staging
的 0.2.0 产品仓库。它没有改变算法、wire schema、稳定格式、decoder backend、
application 资源预算或兼容性标签。

当前 release artifact contract 已成立，但边界是“本地可复核制品”，不是公开发行：

- CLI archive 从 locked host build 产生，解包后的实际 binary 通过版本与合法 WAV
  schema-v3 JSON smoke；
- 当前 host 的 `aarch64-apple-darwin` DMG 通过镜像、挂载、bundle identity、
  executable 与准确 arm64 architecture 检查；
- release manifest 与全部制品由 SHA-256 精确覆盖，并可对最终字节反向重跑 smoke；
- 当前 GUI bundle 的严格代码签名验证结果为 `false`，manifest 因此固定
  `local_staging_only` / `local_unnotarized`，不声称 Gatekeeper、Developer ID、
  notarization 或公开分发就绪。

## 仓库契约

M5 固定了以下唯一事实源：

- 根 `[workspace.package]`：version、edition、MSRV、authors、license、repository；
- 根 `[workspace.dependencies]`：全部直接第三方 Rust 依赖及 feature；
- 根 `Cargo.lock` 与 `tauri-app/package-lock.json`：仅有的两个 tracked lockfile；
- 根 workspace version：npm package、npm lock root 与 Tauri config 的版本源；
- `Application`、Candidate profile、稳定 codec capability 与手动 CI 边界继续沿用
  M2–M4 已有事实源。

`scripts/check-repository-contract.py` 以只读方式拒绝：

- 成员自行固定第三方版本；
- package metadata 不再继承 workspace；
- 嵌套 Cargo/npm lockfile；
- GUI build/dev 自动写入版本；
- 非 `workflow_dispatch` 的 GitHub Actions trigger；
- ordinary workflow 调用 hostile malformed-media verifier。

对应 4 项 repository-contract 负向/正向测试均通过。

## 验证风险分层

普通 Cargo test 不再把带接近 4 GiB 伪造长度的 corpus bytes 送入 test runner
decoder，只校验 manifest/hash/size。结构化失败保留在逐 case 子进程 verifier；
默认没有可执行 `RLIMIT_AS` 时直接拒绝。安全的 route-specific fault injection
继续覆盖 sticky terminal error。

日常层级最终为：

1. pre-commit：repository contract、fmt、locked workspace check；
2. standard local/manual validation：Clippy、普通 workspace tests、repository 与
   reference tool tests、release CLI build、GUI frontend build；
3. hostile verifier、fuzz 和 reference observation：各自显式运行，不进入前两层。

GitHub Actions 仍只有一个纯手动 workflow；M5 没有触发或等待远端 CI。

## Clean artifact staging 记录

执行：

```bash
python3 scripts/stage-release.py stage --include-gui
python3 scripts/stage-release.py verify \
  target/release-staging/0.2.0/aarch64-apple-darwin
```

记录身份：

| 字段 | 值 |
| --- | --- |
| source state | `clean` |
| source commit | `78fb266bdc9434be898154a0f64dfb381638d533` |
| target | `aarch64-apple-darwin` |
| rustc | `1.96.0` |
| Cargo | `cargo 1.96.0 (30a34c682 2026-05-25)` |
| workspace MSRV | `1.88` |
| Cargo.lock SHA-256 | `005f4f937e7e63fe4e2f2bb1898cf5afbc91f20f9fc5194598deb28a89a5bb05` |
| package-lock SHA-256 | `12a6803d0ad2d437e37099cb73415ca6042d80e69fed9eb738d4c035517b47db` |

本次本地 clean staging 的最终字节：

| 文件 | SHA-256 | 结果 |
| --- | --- | --- |
| `RELEASE_MANIFEST.json` | `144033505a66fec0251e528e8a40c12c83d86e707b3ce66581cbcb995a6f0299` | checksum 覆盖 |
| `macinmeter-cli-0.2.0-aarch64-apple-darwin.tar.gz` | `6f338d0552dd3a2878067fe5e3d3477e21c410e1caed8305580f49265e494c5d` | 解包 version + WAV/schema/profile smoke |
| `macinmeter-gui-0.2.0-aarch64-apple-darwin.dmg` | `a636cd42069618e2463586b2ebf6e0ea287ba0929e6620565b8510a2b19fecab` | DMG + mounted bundle + arm64 smoke；strict code signature `false` |

这些 hash 是本机 clean run 的可审计记录，不是跨 SDK/toolchain 可复现构建声明，也
不是签名。制品位于 ignored `target/`，不提交到源码仓库。

## 本地出口门禁

以下命令通过：

```bash
python3 scripts/check-repository-contract.py
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
python3 -m unittest discover -s reference/tools/tests -p 'test_*.py'
npm --prefix tauri-app run build
cargo build --locked --release -p macinmeter-cli
```

结果包括：

- 10 项 repository/release tool tests；
- 121 项 reference tool tests；
- 全部 Rust workspace/adapter tests；
- TypeScript/Vite frontend build；
- 实际 Tauri `.app` 与 `.dmg` host build；
- clean staging 与独立二次 verify。

## 清理结果

- 全部直接 Rust/npm 依赖都有生产或测试入口；
- 删除空 `.claude/package.json`；
- 删除已归档 foobar 分支专用 ignore/build 规则；
- 删除 5 个零引用 legacy WAV；
- 保留仍由 application/CLI/Tauri integration 使用的 legacy fixture；
- 不移动或删除 `audio/`、`dr14_t.meter/`、`master-branch/` 等本地数据。

## 明确留到后续

- Developer ID 签名、notarization、Gatekeeper、GitHub Release 与上传权限；
- Windows/Linux GUI，以及 macOS x86_64/universal 的真实目标构建与验证；
- 完整第三方 license inventory、SBOM 与 provenance；
- advisory 数据库与例外生命周期；
- M6 benchmark、profiling、文件级并发或 SIMD 决策。

这些事项不削弱 M5 的本地仓库/制品结论，但任何公开分发声明都必须先补齐相应目标
证据，不能只沿用本次 arm64 本地 smoke。
