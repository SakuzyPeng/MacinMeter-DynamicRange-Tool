# ADR-0006：M5 产品、仓库与发行契约收敛

- 状态：Accepted
- 日期：2026-07-20
- 范围：0.2.0 workspace、CLI、Tauri GUI、验证与发行输入

## 背景

M0–M4 已经完成主链重建、参考事实边界、原生 decoder 契约、application 资源预算
与固定 x64 数值声明收口。生产实现已经是单一安全主链，但仓库外围仍保留几类会让
发行身份漂移的历史状态：

- 部分直接依赖的版本与 feature 仍散落在成员 manifest；
- GUI build/dev 在启动前自动改写版本镜像，构建不是只读动作；
- 活跃文档和手动 workflow 仍把仓库称为“M0 重建中”；
- 普通 Cargo test 曾在自身进程中解码带伪造多 GiB 长度的 hostile corpus；若
  分配防线回归，测试本身可能成为资源事故；
- release CLI、GUI frontend、版本、lockfile 与制品还没有一个可执行的总契约。

M5 的目的不是扩展算法、格式或并发能力，而是让已经成立的产品边界能够被一致地
构建、验证、说明和发行。

## 决策

### 1. 根 workspace 是 Rust 依赖与包身份的唯一事实源

所有第一方 package 继续从 `[workspace.package]` 继承 version、edition、MSRV、
authors、license 与 repository。所有直接第三方 Rust 依赖，包括仅由测试、CLI
或 Tauri 使用的依赖，都必须在根 `[workspace.dependencies]` 固定版本和 feature；
成员 manifest 只能使用 `.workspace = true`。第一方依赖只允许指向另一个
workspace member。

这条规则只集中直接依赖政策，不试图手工钉住每一个传递依赖；准确解析结果仍由根
`Cargo.lock` 固定。

### 2. 版本镜像只能显式写入，普通构建必须只读

根 `[workspace.package].version` 是产品版本源。以下文件是需要提交的镜像：

- `tauri-app/package.json`
- `tauri-app/package-lock.json` 的根版本与 `packages[""]` 版本
- `tauri-app/src-tauri/tauri.conf.json`

`npm run check-version` 只读检查它们；`npm run sync-version` 才执行显式写入。
`npm run build` 与 `npm run tauri ...` 必须先检查并在漂移时失败，不能在构建过程
中静默修改源码。Tauri Cargo package 继续使用 `version.workspace = true`。

仓库只提交两个 lockfile：根 `Cargo.lock` 与 `tauri-app/package-lock.json`。

### 3. 验证按资源风险分层

日常门禁分为：

1. pre-commit：只读仓库契约、rustfmt、locked workspace check；
2. 标准 workspace：严格 Clippy、默认产品 feature 的测试、reference 工具单元
   测试、release CLI build 与 TypeScript/Vite build；
3. hostile/fuzz/reference observation：各自使用显式入口，不进入前两层。

`malformed-media-v1` 中含有伪造的接近 4 GiB 内部长度。标准 workspace 测试只
核对 corpus manifest、hash 与尺寸，不把这些字节交给进程内 decoder。具体
结构化失败由逐 case 子进程 verifier 验证；默认必须存在有效的地址空间上限，否则
拒绝执行。sticky decoder error 继续由安全、确定性的 route-specific
fault-injection 测试覆盖。

本节修正 ADR-0003 中“普通 workspace 测试直接消费完整 malformed corpus”的
实施细节；其 fail-closed、固定 corpus 与手动 fuzz 原则不变。

### 4. 远端验证仍是 opt-in

GitHub Actions 继续只允许 `workflow_dispatch`，不因 push、PR 或 tag 自动运行。
唯一手动 job 验证仓库身份、Rust workspace、reference 工具、release CLI 与 GUI
frontend，但不执行 hostile corpus verifier、不发布制品、不创建 release。

是否恢复自动 CI 是独立资源决策，不是 M5 完成条件。

### 5. 0.2.0 的稳定能力不再称为“M0 能力”

用户文档使用“0.2.0 stable/trusted surface”。M0–M4 只作为路线图和 ADR 历史
术语保留。所有结果继续醒目标记
`FooDrMeter108CandidateV1 / Unverified`；M4 的有界零差分记录不能扩张成任意输入、
host、decoder 或完整 foobar/component parity。

### 6. 发行制品需要独立、可复核的身份

M5 后续切片必须建立本地、显式的 release staging：

- 从 clean、locked source 构建 release CLI；
- staging 内容包含二进制、LICENSE、RELEASE_NOTES 与第三方声明；
- 对 staging 中实际分发文件执行 smoke test；
- 由这些最终字节生成 SHA-256 清单，并反向验证清单；
- GUI 只对实际支持且本地完成 smoke 的平台形成制品声明；
- 打包、签名、上传和 GitHub Release 不由普通 build 或手动验证 workflow
  隐式执行。

签名、SBOM/provenance、多平台矩阵和自动发布仍需后续证据与独立授权；checksum
不能冒充代码签名。

## M5 实施切片

1. 建立本 ADR、直接依赖/版本/lockfile 契约和只读仓库检查；
2. 清理活跃的 M0 文案，统一 CLI/GUI/格式/兼容性与构建说明；
3. 建立 release staging、制品 smoke 与 checksum；
4. 审计无效依赖、feature、脚本与生成物，完成本地全门禁并记录 M5 收口。

## 出口条件

- 所有直接第三方 Rust 依赖由根 workspace 管理，契约检查可检测成员绕过；
- Cargo/npm/Tauri 版本与两个 lockfile 的身份由只读门禁固定；
- 普通 build/test 不修改 tracked files，也不在进程内消费 hostile corpus；
- CLI release 与 GUI frontend 可从 locked source 构建；
- 0.2.0 用户文档不再把已完成的 M0 当作当前阶段；
- release staging 可重复产生通过 smoke 的制品和可反向验证的 SHA-256 清单；
- 本地标准 Rust、reference-tool 与 GUI 门禁通过；
- Actions 保持纯手动，M5 不触发或等待远端 CI。

## 后果

仓库的“能编译”与“可发行”成为两个明确层次：普通开发保持快速和安全，发行则多出
可审计的 staging 与最终字节验证。显式版本同步会让版本升级多一步命令，但也让
任何漂移在 build 之前暴露，而不是由 build 悄悄写回。

M5 不改变算法输出、wire schema、支持格式、decoder backend、执行预算或兼容性
标签，因此不需要新的参考观测。

## 未采用方案

### 继续让 `npm run build` 自动同步版本

这会让验证命令兼具写入行为，工作树漂移可能被误认为构建产物，且 CI 无法区分
“输入正确”与“构建替开发者修了输入”。

### 在普通 Cargo test 中保留 hostile corpus 解码

timeout 无法阻止同进程的过量分配伤害 test runner；而保护逻辑回归正是测试需要
覆盖的失败场景。逐 case、有内存上限的子进程才是合适边界。

### 现在恢复自动 CI 与自动发布

它既会消耗此前明确要求节省的远端资源，也会在制品契约完成前把不完整流程固化。
M5 先建立本地可复核事实，再另行决定触发与发布权限。
