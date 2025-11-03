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

Please treat this tag as a pre-release; verify DR results with foobar2000 when they are critical.  
此版本为预发布，重要曲目建议使用 foobar2000 交叉验证 DR 结果。

