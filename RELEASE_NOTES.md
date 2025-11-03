# Release Notes / 发布说明

## v0.1.0pre – Pre-Release / 预发布

- DR Precision / 精度说明
  - Typical drift vs foobar2000: ±0.02–0.05 dB
    相比 foobar2000，常见偏差约 ±0.02–0.05 dB
  - Rare outliers may reach ~0.1 dB (tail-window selection, master variations)
    少量曲目可能出现约 0.1 dB 的偏差（尾窗是否纳入计算、不同母带的首尾采样差异等）

- Format Coverage / 格式覆盖
  - Limited testing across container / codec combinations
    容器 / 编解码组合测试尚不充分
  - Some legacy logs and SDK assets removed
    已清理旧版日志和 SDK 资源

- Known Dependencies / 已知依赖风险
  - Upstream advisories via songbird / rustls / ring remain unresolved
    由于 songbird → rustls → ring 链路，继承的安全公告仍未修复（不影响本地离线使用）

- Platform Packages / 平台产物
  - Windows / macOS / Linux builds are published as zipped artifacts with the `-pre` suffix (e.g., `..._linux-x64-pre.zip`)
    Windows／macOS／Linux 可执行文件均以带 `-pre` 后缀的压缩包形式提供（如 `..._windows-x64.exe-pre.zip`）
  - macOS builds are unsigned; Gatekeeper may show “Apple can’t verify…” prompts—use Security & Privacy or `xattr -d com.apple.quarantine` if you trust the download
    macOS 产物未签名，可能触发“Apple 无法验证……”提示；若确认来源可信，可通过“安全性与隐私”或执行 `xattr -d com.apple.quarantine` 解除限制
- Linux package is untested on real hosts; treat as experimental
  Linux 产物尚未在真实环境验证，使用时请视为实验性质

- Testing Invitation / 测试邀请
  - Seeking help with container / codec format coverage and cross-platform validation
    欢迎协助扩充容器 / 编解码格式覆盖以及多平台验证
  - Audio sample feedback (attach or reference source files when possible) can be sent to **ruuokk208@gmail.com**
    音频样本反馈（如可附源文件）请发送至 **ruuokk208@gmail.com**

Please treat this tag as a pre-release; verify DR results with foobar2000 when they are critical.
此版本为预发布，重要曲目建议使用 foobar2000 交叉验证 DR 结果。
