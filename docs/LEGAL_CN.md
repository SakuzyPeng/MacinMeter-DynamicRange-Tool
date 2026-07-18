[English](LEGAL.md) | [中文](LEGAL_CN.md)

# 法律说明与致谢

MacinMeter 是独立编写并以 MIT License 发布的 Rust 项目。当前参考目标是 Janne
Hyvärinen 所著 foobar2000 DR Meter 1.0.8（`foo_dr_meter`）；
本仓库不包含原插件或其源代码。

项目维护者已经取得作者对逆向该插件的许可。私人通信保存在公开仓库之外。公开工程
记录只应保留解释研究边界所需的最小授权摘要；未经另行同意，不应发布私人消息原文。

公开的来源与范围摘要登记在
[`reference/authorization/README.md`](../reference/authorization/README.md)。
作者回复在维护者邮箱中显示为 2025-09-08；回复明确不反对逆向该 component，也不
反对本独立项目选择 MIT License。这不表示原 component 本身改用 MIT，也不授权
再分发原 component。

获准研究目标不代表本实现已经与目标一致。MacinMeter 的结果状态为
`foo_dr_meter 1.0.8 Candidate V1 / Unverified`，项目不作参考兼容、认证、背书
或“官方结果”声明。名称和版本只用于标识证据研究对象，不表示从属关系或结果对等。

## 独立实现边界

项目工作应当：

- 只使用合法取得的目标二进制与工具；
- 记录目标身份、hash、宿主版本和实验条件；
- 区分可观察结果、假设与实现决策；
- 未经另行授权，不复制原始源代码或再分发目标二进制；
- 避免以名称或展示方式暗示原作者背书。

私人授权材料和不可再分发的二进制不进入 `reference/`。该目录只记录适合公开的规格、
可复现实验定义、获准使用的 fixture、观测和 conformance 摘要。

## 第三方软件

Rust 与前端依赖分别遵守其自身许可证。当前发行依赖集合见
[`THIRD_PARTY_NOTICES.md`](../THIRD_PARTY_NOTICES.md)与锁文件。正式分发时应根据
准确的 locked dependency graph 生成并附带许可证报告。

## 免责声明

本文只记录项目工程政策和已知授权背景，不构成法律意见，也不对所有司法辖区内的逆向
工程法律作一般性结论。贡献者与分发者应在其使用场景需要时自行寻求专业法律意见。
