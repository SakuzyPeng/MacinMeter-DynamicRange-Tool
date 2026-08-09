# Third-party notices / 第三方许可说明

MacinMeter depends on third-party Rust and JavaScript packages. Copyright and
license terms remain with their respective authors.

The 0.3.1 source tree directly uses projects including:

- Symphonia for native audio container/codec support. The locked graph now
  includes `symphonia-codec-alac` and `symphonia-format-isomp4` 0.5.5; these
  Symphonia packages declare MPL-2.0;
- Serde and serde_json for data serialization;
- Clap for CLI argument parsing;
- Walkdir for deterministic filesystem discovery;
- Tauri and its dialog plugin for the desktop adapter;
- Vite and TypeScript for the frontend build.

The application icon sets the letters "DR" in Source Serif 4 at weight 700,
converted to outlines so the source SVG renders without the font installed.
Source Serif 4 is Copyright 2014 The Source Serif 4 Project Authors and is
licensed under the SIL Open Font License 1.1; the licence text is kept beside
the source at `tauri-app/icons-src/OFL-SourceSerif4.txt`. No font file is
redistributed with MacinMeter.
应用图标以 Source Serif 4 Bold 排出 "DR" 并转为轮廓，源 SVG 因此不依赖字体安装。
Source Serif 4 采用 SIL Open Font License 1.1，许可全文随源文件保存在
`tauri-app/icons-src/OFL-SourceSerif4.txt`；MacinMeter 不随附任何字体文件。

The authoritative package versions are recorded in `Cargo.lock` and
`tauri-app/package-lock.json`. Transitive dependencies change independently,
so this short notice is not a substitute for a release license inventory.
Before distributing binaries or bundled frontend assets, generate a complete
license/SBOM report from the exact locked graph and include all notices required
by those licenses.

The 0.3.1 product does not include Songbird, FFmpeg bindings, an FFmpeg runtime,
or the former networking/TLS dependency chain used by the Opus route. FFmpeg
8.0.1 is used only as the pinned, opt-in `native-alac-v1` fixture regeneration
tool. It is not linked into MacinMeter, copied into artifacts, or required by
ordinary builds and tests; the repository commits only synthetic generated
media and records the generator command and identity.

---

MacinMeter 使用第三方 Rust 与 JavaScript 软件包，各组件的著作权和许可证条款归其
各自作者所有。

准确版本记录在 `Cargo.lock` 与 `tauri-app/package-lock.json`。传递依赖可能独立
变化，因此本简表不能替代正式发行的许可证清单。分发二进制或前端 bundle 前，应针对
准确的 locked graph 生成完整 license/SBOM 报告，并附带各许可证要求的声明。

0.3.1 产品不包含 Songbird、FFmpeg binding、FFmpeg runtime，或旧 Opus 路径引入
的网络/TLS 依赖链。FFmpeg 8.0.1 只用于可选的 `native-alac-v1` fixture 固定再生成，
不会链接进 MacinMeter、复制进发行制品，也不是普通构建或测试的前置条件；仓库只
提交合成生成的媒体，并记录生成命令和工具身份。新增的
`symphonia-codec-alac` 与 `symphonia-format-isomp4` 0.5.5 声明 MPL-2.0。
