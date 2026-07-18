# Third-party notices / 第三方许可说明

MacinMeter depends on third-party Rust and JavaScript packages. Copyright and
license terms remain with their respective authors.

The M0 source tree directly uses projects including:

- Symphonia for native audio container/codec support;
- Serde and serde_json for data serialization;
- Clap for CLI argument parsing;
- Walkdir for deterministic filesystem discovery;
- Tauri and its dialog plugin for the desktop adapter;
- Vite and TypeScript for the frontend build.

The authoritative package versions are recorded in `Cargo.lock` and
`tauri-app/package-lock.json`. Transitive dependencies change independently,
so this short notice is not a substitute for a release license inventory.
Before distributing binaries or bundled frontend assets, generate a complete
license/SBOM report from the exact locked graph and include all notices required
by those licenses.

M0 no longer includes Songbird, FFmpeg bindings, or the former networking/TLS
dependency chain used by the Opus route.

---

MacinMeter 使用第三方 Rust 与 JavaScript 软件包，各组件的著作权和许可证条款归其
各自作者所有。

准确版本记录在 `Cargo.lock` 与 `tauri-app/package-lock.json`。传递依赖可能独立
变化，因此本简表不能替代正式发行的许可证清单。分发二进制或前端 bundle 前，应针对
准确的 locked graph 生成完整 license/SBOM 报告，并附带各许可证要求的声明。

M0 已不再包含 Songbird、FFmpeg binding，或旧 Opus 路径引入的网络/TLS 依赖链。
