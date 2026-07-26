# ADR-0012：稳定 WAV 路由扩展至 WAVE_FORMAT_EXTENSIBLE 线性 PCM

- 状态：Proposed（草案，待评审）
- 实施状态：TODO
- 日期：2026-07-21
- 修订日期：2026-07-26
- 决策范围：稳定 WAV 路由的封装面扩展（不新增 route、不改 wire schema）
- 相关路线图：[架构整改与参考插件重新对齐路线图](../ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md)
- 前置决策：
  - [ADR-0001：以 0.2.0 重建可信主干](0001-m0-0.2.0-trusted-trunk-rebuild.md)
  - [ADR-0003：M2 原生解码面与工程契约加固](0003-m2-native-decoder-contract-hardening.md)
  - [ADR-0005：M4 固定 x64 数值声明与 decoder-independent 验收](0005-m4-bounded-x64-numeric-claim.md)

## 背景

ADR-0003 §5 在建立稳定 WAV 矩阵时，**刻意**把 `WAVE_FORMAT_EXTENSIBLE`
（format tag `0xFFFE`）在 probe 阶段返回 `UnsupportedFormat`，理由是它的
valid-bits、channel mask 与 sub-format GUID 尚未形成独立 capability 证据。

这一保守选择在 0.2.0 发布后暴露出一个 bug 形状的缺口：EXTENSIBLE 并不是一种
新编码，而是线性 PCM 的一种封装变体，也是 24-bit、多声道（5.1/7.1）和 hi-res
WAV 的常见封装。用户手中的普通 24-bit 或 5.1 WAV 今天可能被报
`unsupported_format`。这比"我没有的格式不被支持"更刺痛，因为用户通常认为
WAV 本就该支持。

这是 0.2.0 路线图收口后的第一项无损封装扩展。它有意选择风险最低的一步：同一
codec、不引入第二 backend、大概率无需升级 wire schema，先把"已支持 codec 的
封装变体"这套毕业纪律跑通，再去处理 ALAC/M4A 那种"新容器 + 新 codec + schema
升级"的大改动。

## 决策

### 1. 这是既有稳定 route 的封装扩展，不是新 route

EXTENSIBLE 线性 PCM 仍映射到既有的 `(wave, pcm_integer)` 与 `(wave, pcm_float)`
capability route。**不新增 catalog route，不新增 `ContainerFormat` / `SourceCodec`
枚举值。** 因此分析 report 的 container/codec 标识集合不变，schema-v3
`WireEnvelope` 严格消费者不受影响，**wire schema 保持 v3**。

变化只体现在这两条 route 的 `limitations` 文案：从"EXTENSIBLE 被拒"改为写明
新接受的 EXTENSIBLE 约束。

### 2. 只接受 PCM 与 IEEE-float 两个精确 sub-format GUID

第一方 probe 读取 16 个原始 GUID 字节，只接受两个完整常量：

- `KSDATAFORMAT_SUBTYPE_PCM`（`00000001-0000-0010-8000-00aa00389b71`）→ 映射到
  既有 `PcmInteger` 身份；
- `KSDATAFORMAT_SUBTYPE_IEEE_FLOAT`（`00000003-0000-0010-8000-00aa00389b71`）→
  映射到既有 `PcmFloat` 身份。

其余任何 GUID（含各类 ambisonic 子类型）在 probe 阶段返回
`UnsupportedFormat / Probe`，附 GUID 上下文。实现不得只取 GUID 的低 16-bit
format tag 路由；必须按 WAVE 文件中的 little-endian 原始字节与上述两个 16-byte
常量完整比较。

### 3. fmt chunk 必须是精确的 40 字节 EXTENSIBLE 布局

经典 route 接受 fmt_size ∈ {16, 18}。EXTENSIBLE route 要求：

- fmt_size 恰好为 40（16 基础 + 2 字节 cbSize + 22 字节扩展）；
- cbSize 恰好为 22。

错误分类固定为：

- fmt_size < 40、cbSize < 22，或 `18 + cbSize > fmt_size` →
  `MalformedMedia / Probe`，因为声明不足以容纳必需结构或内部长度不自洽；
- fmt_size > 40，或 cbSize > 22 且 `18 + cbSize <= fmt_size` →
  `UnsupportedFormat / Probe`，因为它是可识别但带额外扩展数据的形状，不在本次
  稳定子集；
- 只有 fmt_size == 40 且 cbSize == 22 才继续解析。

### 4. 本切片只接受紧凑位深，valid == container

沿用既有位深矩阵：整数 8/16/24/32、float 32/64。此外只接受
`wValidBitsPerSample == wBitsPerSample`。

- `wValidBitsPerSample > wBitsPerSample` → `MalformedMedia / Probe`；
- `wValidBitsPerSample < wBitsPerSample`（包括 0，以及典型的 24-in-32 padded
  容器）**不在本切片内**，返回 `UnsupportedFormat / Probe`。

padded PCM 会改变样本宽度语义与归一化路径，需要独立证据与独立 fixture，作为
显式后续切片处理，不在这里顺带引入。`validBits == 0` 也不隐式解释为容器位宽：
当前 Symphonia 0.5.5 会把它保留为 decoder metadata 的 0，而产品的第一方容器
metadata 交叉校验要求位宽一致；若未来要容忍该写法，必须先固定规范解释、backend
归一化和独立 fixture。

### 5. channel mask 只做一致性校验，布局仍为 Unknown；本 route 最多 26 声道

- 若 `dwChannelMask != 0`：只允许 WAVEFORMATEXTENSIBLE 定义的低 18 个 speaker
  bits（mask 必须满足 `mask & !0x0003_FFFF == 0`）；出现保留位返回
  `UnsupportedFormat / Probe`。在允许的 bit 范围内，置位数（popcount）必须等于
  声道数，否则返回 `MalformedMedia / Probe`；
- 若 `dwChannelMask == 0`：按 direct-out/未指定物理扬声器位置处理，接受。

**无论 mask 取值，`StreamSpec` 的 `ChannelLayout` 一律保持 `Unknown`。** 不从
mask 推导 `Known` 布局，也不识别 LFE。理由：

- 当前唯一生产 profile 的默认 track DR 是全声道内部 binary64 channel DR 的
  算术均值（含静音与 LFE），**布局不改变任何数值结果**；
- 保持与 META-003"未知布局不猜测"一致；
- 避免在没有独立证据时扩大 report/schema 面。

从 mask 派生 `Known` 布局与 LFE 识别是一项独立的、未来的增强，且只对非默认的
可选多声道 loudness weighting 路径有意义（参见 ADR-0005 的记录）；它不属于本
切片。

稳定 EXTENSIBLE route 另行收窄为 1–26 声道。当前 Symphonia 0.5.5 的 `Channels`
虽以 `u32` 存储，但只定义连续的低 26 bits；零 mask 会由 backend 补为低 N bits，
因此 27–31 声道无法表示，32 声道还会进入移位溢出路径。第一方 probe 必须在
decoder 创建前对 27–64 声道返回 `UnsupportedFormat / Probe`。全局
`MAX_ANALYSIS_CHANNELS = 64` 不变，经典 route 与直接 `AnalyzerSession` 的既有
边界也不因本 ADR 改写。非零标准 mask 最多自然表达 18 声道；19–26 声道仅在零
mask/direct-out 时属于本次稳定子集。

### 6. 几何校验与归一化沿用既有 PCM 路径

block_align、byte_rate 一致性校验继续按容器位宽计算，与经典线性 PCM 共用同一
套逻辑。解码仍由现有进程内 Symphonia backend 完成；workspace 已启用的 `wav`
与 `pcm` feature 足够，不新增 backend、依赖或 feature。`push_interleaved` API 与
有限交错 `f64` 归一化 oracle 均不变。

第一方 probe 还必须保存由 format tag/sub-format GUID 得出的预期 PCM 类别，并在
decoder 创建前与 Symphonia 的 codec identity 交叉核对。PCM GUID 必须得到
`PcmInteger`，IEEE-float GUID 必须得到 `PcmFloat`；不一致返回
`MalformedMedia / Probe`，不能让 backend 身份静默覆盖第一方判断。

### 7. Metamorphic 等价是核心验收

同一 PCM 分别以 EXTENSIBLE 与等价经典 WAV 呈现时，必须产生 bit-identical 的
有限交错 `f64` PCM、完整 `AnalysisResult` 与共享 report 数值 projection。只比较
最终 DR 值不足以证明 decoder 等价，因为不同 PCM 可能碰巧得到相同聚合结果。

### 8. 版本

本改动只接受更多输入、不拒绝任何原先接受的输入、不改变 wire schema，属于
新增能力。建议按 minor 发布（`0.3.0`），最终由发布决策确定。

## 明确非目标

- valid < container 的 padded PCM（24-in-32 等）；
- 把 valid == 0 隐式解释为 container width；
- 27–64 声道 EXTENSIBLE（当前 backend 的 WAV channel 表示无法安全表达）；
- 非零 channel mask 使用 WAVEFORMATEXTENSIBLE 低 18 bits 以外的保留位置；
- 从 channel mask 派生 `Known` 布局、LFE 识别或参与多声道加权；
- ambisonic 及其他 sub-format GUID；
- Ogg FLAC、AIFC、ALAC/M4A 等其他封装或 codec（各自按独立准入处理）；
- 任意 WAV 变体的穷尽支持；未列变体保持 `UnsupportedFormat`。

## 验收

- EXTENSIBLE 整数 8/16/24/32 与 float 32/64、mono/stereo/多声道、零 mask 与非零
  mask 代表 fixture 全部通过共享 `PcmSource` contract matrix；
- 每个 EXTENSIBLE fixture 与其等价经典-WAV 孪生产生 bit-identical 的 normalized
  finite interleaved `f64` PCM、完整 `AnalysisResult` 与共享 report 数值 projection；
- malformed 增补至少覆盖：不支持的 sub-format GUID、GUID 模板尾部不符、
  fmt_size < 40、可自洽的 fmt_size/cbSize > 40/22、fmt_size/cbSize 不自洽、
  valid == 0、`wValidBitsPerSample > wBitsPerSample`、padded（0 < valid < container）、
  channel mask 保留位、channel mask popcount 与声道数不符，以及 27/32/64 声道，
  各自返回本 ADR 预先固定的结构化错误并纳入 `malformed-media-v1`；另以零 mask
  覆盖 26 声道可接受边界；现有仅把 classic fmt tag 改为 `0xFFFE`
  的 16-byte case 必须从 `UnsupportedFormat` 重登记为 `MalformedMedia`，不能保留
  过时预期；
- 通过 test seam 固定第一方预期 PCM 类别与 backend codec identity 不一致时的
  `MalformedMedia / Probe`；
- capability catalog 的 `(wave, pcm_integer)` 与 `(wave, pcm_float)` limitations
  文案更新；`SUPPORTED_FORMATS.md` 与 `SUPPORTED_FORMATS_CN.md` 同步；
- Rust API、CLI 与共享 report 各至少一条 EXTENSIBLE 端到端路径；GUI picker
  无需改动（扩展名不变）；
- 显式确认 wire schema 保持 v3（无 enum 变化）；
- 本地 fmt、严格 Clippy、workspace 测试、仓库契约、Python 工具测试与前端 build
  通过；不触发远端 CI。

## 实施切片

按依赖顺序：

1. **probe 解析**：扩展 `crates/macinmeter-codecs/src/container.rs` 的
   `inspect_wave`，在 `0xFFFE` 分支解析 cbSize / valid-bits / channel mask /
   sub-format GUID，执行第 2–5 节校验并返回第一方预期 PCM 类别；替换现有的直接
   拒绝分支，并在 `open_source` 中与 backend codec identity 交叉核对。
2. **确定性 fixture**：扩展 `scripts/generate-native-pcm-v1.py`（或新增
   `native-pcm-extensible-v1` corpus）生成 EXTENSIBLE 变体及其经典孪生，manifest
   记录 bytes、geometry、channel mask、sub-format 与 normalized-f64 oracle。
3. **测试**：现有共享 contract 覆盖新 fixture；新增 EXTENSIBLE↔经典的 decoded
   PCM、完整结果和 report projection 三层 bit-identical 测试；按验收矩阵更新既有
   EXTENSIBLE mutation、增补 malformed corpus 与再生成审计。
4. **catalog 与文档**：更新 `capability.rs` 的 WAV limitations 文案与
   `SUPPORTED_FORMATS{,_CN}.md`；确认 catalog snapshot 测试相应更新。
5. **毕业复核**：按 ADR-0003 §9 准入条件逐条核对，显式记录"无新 route、无
   schema 变化"，并在本 ADR 记录收口结论。

## 后果

正面：

- 用户手中普通的 24-bit / 多声道 / hi-res WAV 不再被误拒；
- 以最低风险跑通"已支持 codec 的封装变体"毕业流程，为后续 ALAC/M4A 铺路；
- 不扩大 backend、不改 wire schema、不改变任何既有数值结果。

代价：

- 第一方 WAV parser 增加 EXTENSIBLE 扩展解析与若干校验分支；
- padded PCM 与 channel-mask 布局派生成为已记录的后续欠账，需各自独立证据；
  它们不因本切片落地而被视为已完成。
- EXTENSIBLE route 暂时比产品全局分析上限更窄，只稳定接受至 26 声道；非零 mask
  还只接受标准定义的低 18 个 speaker bits。

## 未采用的方案

### 直接透传给 Symphonia 的 EXTENSIBLE 支持

拒绝。这会让稳定支持面隐式继承 backend 行为，与 ADR-0003 §5"不继承 Symphonia
隐式支持"一致地不可取；sub-format GUID、valid-bits 与 channel mask 必须由第一方
probe 显式校验并记录为 capability 证据。

### 本切片一并支持 24-in-32 padded PCM

拒绝。padded 容器改变样本宽度语义与归一化，需要独立 fixture 与 oracle；与
紧凑位深混在一起会扩大本切片的正确性表面。作为显式后续处理。

### 把 validBits == 0 当作 container width

拒绝。该容忍规则不是本切片已有规范与 backend metadata 的共同语义；当前 backend
会保留 0，而产品会将它与第一方容器位宽判为不一致。先返回
`UnsupportedFormat / Probe`，未来只有在固定解释、归一化与 fixture 后再扩展。

### 接受 27–64 声道的零 channel mask EXTENSIBLE

拒绝。零 mask 可以表达 direct-out/未指定布局，但当前 backend 的 WAV channel
表示只定义连续的低 26 bits。本切片在 decoder 创建前稳定拒绝，避免把 backend
表示上限误当作 64 声道产品分析上限已经兑现。

### 从 channel mask 直接建立 Known 布局

拒绝。默认 profile 的数值结果不依赖布局；在没有独立证据、也没有默认使用场景
（多声道加权非默认）时引入布局推导，只会扩大 report/schema 与证据表面。
