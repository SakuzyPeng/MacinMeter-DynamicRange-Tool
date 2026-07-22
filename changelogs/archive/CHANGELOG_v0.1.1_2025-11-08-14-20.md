# Archived changelog v0.1.1 (2025-11-08-14-20)

> **Historical / superseded:** this file records 0.1.1-era behavior and claims.
> It is not evidence for current 0.2.0 results; statements below such as
> “official,” “identical,” or “proved correct” must be read only as historical
> assertions.
>
> **历史记录 / 已取代：** 本文只记录 0.1.1 当时的行为和说法，不能证明 0.2.0
> 与参考插件一致。下文“官方”“完全一致”或
> “证明正确”等措辞仅是历史陈述，不是现有证据。
>
> 本文没有为下文所称 `v1.0.3` / `v1.1.1` 保存可验证的目标 manifest、组件
> 二进制哈希或宿主身份；原文还把这些标签写成了“foobar2000 版本”。这些标签
> 因此只能按 0.1.1 时代的未验证记录读取，不能解释为已固定的插件版本，也不能
> 与当前固定的 `foo_dr_meter 1.0.8 x64` target 混用。

## 修复：FLAC/ALAC比特率显示错误 / Fix: FLAC/ALAC Bitrate Display Error

---

## 问题描述 / Problem Description

### 错误现象 / Symptom

FLAC/ALAC压缩格式显示的是**理论未压缩比特率**而非**实际压缩后比特率**：

**测试文件**：flac (48kHz/24bit/8ch)

| 工具 | 显示比特率 | 类型 |
|------|-----------|------|
| **MacinMeter v0.1.0 (修复前)** | 9216 kbps | 理论PCM比特率 (错误) |
| **foobar2000 DR Meter** | 5558 kbps | 实际压缩比特率 (正确) |
| **ffprobe** | 5560 kbps | 实际压缩比特率 (正确) |

**差异**: 9216 - 5558 = **3658 kbps** (错误65.7%)

### 理论比特率计算错误

```
错误公式: 采样率 × 位深 × 声道数
48000 Hz × 24 bit × 8 ch = 9,216,000 bps = 9216 kbps
```

这是**未压缩PCM**的理论比特率，不适用于FLAC/ALAC压缩格式。

---

## 根本原因 / Root Cause

### 概念混淆

代码混淆了两个独立的概念：

1. **压缩 vs 未压缩** → 影响比特率计算方法
2. **有损 vs 无损** → 影响音质保真度

| 格式 | 压缩类型 | 音质类型 | 正确计算方法 |
|------|----------|----------|--------------|
| FLAC | 压缩 | 无损 | 文件大小 ÷ 时长 |
| ALAC | 压缩 | 无损 | 文件大小 ÷ 时长 |
| MP3 | 压缩 | 有损 | 文件大小 ÷ 时长 |
| AAC | 压缩 | 有损 | 文件大小 ÷ 时长 |
| Opus | 压缩 | 有损 | 文件大小 ÷ 时长 |
| WAV | 未压缩 | 无损 | 采样率 × 位深 × 声道 |
| PCM | 未压缩 | 无损 | 采样率 × 位深 × 声道 |

### 错误代码逻辑

**修复前** (`src/tools/formatter.rs:58-70`):

```rust
/// 根据真实编解码器类型判断是否为有损压缩
fn is_lossy_codec_type(codec_type: CodecType) -> bool {
    matches!(
        codec_type,
        CODEC_TYPE_AAC |      // AAC - 有损
        CODEC_TYPE_MP3 |      // MP3 - 有损
        CODEC_TYPE_VORBIS |   // OGG Vorbis - 有损
        CODEC_TYPE_OPUS       // Opus - 有损
    )
    // 无损格式：CODEC_TYPE_FLAC, CODEC_TYPE_ALAC, CODEC_TYPE_PCM_*
}
```

**问题**：
- 只有**有损格式**使用文件大小÷时长 (错误)
- FLAC/ALAC被归为"无损"→ 错误地使用PCM公式 (错误)
- 正确逻辑：所有**压缩格式**（有损+无损）都应使用文件大小÷时长 (正确)

---

## 修复方案 / Fix Solution

### 代码变更

**文件**: `src/tools/formatter.rs`

#### 1. 重命名函数并重新分类

```rust
/// 根据真实编解码器类型判断是否为压缩格式
///
/// 压缩格式（有损+无损压缩）需要用文件大小÷时长计算实际比特率
/// 未压缩格式（PCM）使用采样率×位深×声道计算理论比特率
fn is_compressed_codec_type(codec_type: CodecType) -> bool {
    matches!(
        codec_type,
        // 有损压缩格式
        CODEC_TYPE_AAC |      // AAC - 有损压缩
        CODEC_TYPE_MP3 |      // MP3 - 有损压缩
        CODEC_TYPE_VORBIS |   // OGG Vorbis - 有损压缩
        CODEC_TYPE_OPUS |     // Opus - 有损压缩
        // 无损压缩格式
        CODEC_TYPE_FLAC |     // FLAC - 无损压缩 (新增)
        CODEC_TYPE_ALAC       // ALAC - 无损压缩 (新增)
    )
    // 未压缩格式：CODEC_TYPE_PCM_*（WAV/AIFF等）
}
```

#### 2. 更新比特率计算逻辑

```rust
/// 智能比特率计算：根据真实编解码器类型选择合适的计算方法
///
/// 压缩格式(FLAC/ALAC/MP3/AAC/Opus/OGG): 使用文件大小÷时长计算实际比特率
/// 未压缩格式(WAV/PCM): 使用采样率×声道×位深计算理论比特率
fn calculate_actual_bitrate(...) -> AudioResult<u32> {
    // 优先使用真实的编解码器信息
    let is_compressed = if let Some(codec_type) = format.codec_type {
        is_compressed_codec_type(codec_type)  // 使用新函数
    } else {
        // 回退到扩展名判断（包括有损和无损压缩格式）
        matches!(
            codec_fallback,
            "FLAC" | "ALAC" | "OPUS" | "MP3" | "AAC" | "OGG"  // 添加FLAC/ALAC
        )
    };

    if is_compressed {
        // 压缩格式（有损+无损）：使用文件大小和时长计算实际比特率
        let bitrate_bps = (file_size_bytes as f64 * 8.0) / duration_seconds;
        Ok((bitrate_bps / 1000.0).round() as u32)
    } else {
        // 未压缩格式(WAV/PCM)：使用理论PCM比特率公式
        let bitrate_bps = format.sample_rate as u64
            * format.channels as u64
            * format.bits_per_sample as u64;
        Ok((bitrate_bps / 1000) as u32)
    }
}
```

#### 3. 更新注释和文档

所有相关注释从"有损 vs 无损"修改为"压缩 vs 未压缩"，准确反映实际逻辑。

---

## 验证结果 / Verification Results

### 测试1：高采样率FLAC (192kHz/24bit/2ch)

| 指标 | 值 |
|------|-----|
| 文件大小 | 200 KB |
| 时长 | 1秒 |
| **MacinMeter** | **1642 kbps** (正确) |
| **ffprobe** | **1642.3 kbps** (正确) |
| 差异 | 0.3 kbps (0.02%) |

### 测试2：7.1声道FLAC (48kHz/24bit/8ch)

> 下表中的 `foobar2000 v1.0.3` 是历史原标签，目标身份未固定。该行只保存当时
> 记录的显示值，不构成与任一参考插件版本的一致性比较。

| 工具 | 比特率 | 差异 |
|------|--------|------|
| **MacinMeter (修复后)** | **5561 kbps** | - |
| foobar2000 v1.0.3 | 5558 kbps | +3 kbps (+0.05%) |
| ffprobe | 5560 kbps | +1 kbps (+0.02%) |

**历史原结论**：`完全一致`。**当前解释**：显示的整数比特率接近，但没有固定
目标和计算路径证据，不能据此声明实现一致。

### 测试3：5.1声道FLAC (48kHz/24bit/6ch)

| 工具 | 比特率 |
|------|--------|
| **MacinMeter** | **4342 kbps** |
| ffprobe | **4342 kbps** |

**历史原结论**：`完全一致`。**当前解释**：这里只能说明两份历史显示值相近。

### 测试4：WAV未压缩格式 (192kHz/24bit/2ch)

| 工具 | 比特率 | 计算方法 |
|------|--------|----------|
| **MacinMeter** | **9216 kbps** | 采样率×位深×声道 (正确) |
| 理论值 | 9216 kbps | 192000×24×2÷1000 |

**结论**: 未压缩格式仍使用理论公式，正确

---

## 历史版本标签对比（非 conformance） / Historical labels (not conformance)

> 本节保留 0.1.1 changelog 中的原始数值，但版本标签没有可验证的 target 身份。
> `v1.0.3` / `v1.1.1` 不得与当前固定的 `foo_dr_meter 1.0.8 x64` target 比较或
> 外推；下列“发现”是已撤回的历史推断，不是现行项目结论。

### 测试文件
- flac
- 48kHz / 24bit / 8声道 (7.1)
- 时长: 5:25

### 比特率对比

| 版本 | 比特率 | 差异 |
|------|--------|------|
| foobar2000 v1.1.1 (最老) | 5558 kbps | - |
| foobar2000 v1.0.3 (标准) | 5558 kbps | - |
| foobar2000 v1.0.3 (加权) | 5558 kbps | - |
| **MacinMeter v0.1.1 (修复后)** | **5561 kbps** | **+3 kbps (+0.05%)** |

### 声道DR值对比

| 声道 | v1.1.1 (最老) | v1.0.3 (标准) | MacinMeter | 差异 |
|------|---------------|---------------|------------|------|
| Ch1/FL | 13.99 | 14.01 | 14.06 | ±0.07 |
| **Ch2/FR** | **14.39** | **13.61** | **14.40** | **v1.1.1一致** |
| Ch3/FC | 10.77 | 10.84 | 10.76 | ±0.08 |
| Ch4/LFE | 15.24 | 14.83 | 15.28 | ±0.45 |
| Ch5/BL | 10.82 | 10.84 | 10.81 | ±0.03 |
| Ch6/BR | 11.11 | 11.17 | 11.12 | ±0.06 |
| Ch7/SL | 12.69 | 12.79 | 12.70 | ±0.10 |
| Ch8/SR | 15.03 | 14.61 | 14.41 | ±0.62 |

**历史记录的 overall DR token**：各行均为 **DR13**。这不是“官方值”或
correctness oracle。

### 0.1.1 时代的推断（已撤回）

1. **当时推断 MacinMeter 与标签为 v1.1.1 的结果高度接近**
   - 7/8声道差异 < 0.1 dB
   - FR声道几乎完全一致 (14.39 vs 14.40)
   - ~~证明 MacinMeter 实现正确~~：该结论证据不足，现已撤回

2. **当时推测标签为 v1.0.3 的结果存在 FR 声道问题**
   - FR声道显示13.61 dB
   - 其他版本显示~14.40 dB
   - 差异0.79 dB (显著)
   - 没有固定二进制和规则证据，不能据此判断参考实现存在缺陷

3. **当时记录的比特率修复结果**
   - 修复前: 9216 kbps (错误65.7%)
   - 修复后: 5561 kbps
   - 与foobar2000差异仅3 kbps (0.05%)
   - 该显示值比较不证明 DR 算法或参考插件兼容性

---

## 技术细节 / Technical Details

### 比特率计算公式

#### 压缩格式（FLAC/ALAC/MP3/AAC/Opus）

```
实际比特率 = (文件字节数 × 8) ÷ 时长(秒) ÷ 1000
单位: kbps
```

**示例** (8声道FLAC):
```
文件大小: 225,899,656 bytes
时长: 325 秒 (15,600,000 samples ÷ 48,000 Hz)
比特率 = (225,899,656 × 8) ÷ 325 ÷ 1000 = 5,564 kbps
```

#### 未压缩格式（WAV/PCM）

```
理论比特率 = 采样率 × 位深 × 声道数 ÷ 1000
单位: kbps
```

**示例** (192kHz/24bit/2ch):
```
比特率 = 192,000 × 24 × 2 ÷ 1000 = 9,216 kbps
```

### 误差来源分析

**为什么会有±1-3 kbps的差异？**

1. **文件大小包含元数据**
   - 封面图片（MJPEG/PNG）
   - 标签信息（艺术家、专辑等）
   - 容器头部开销

2. **时长计算精度**
   - MacinMeter: 样本数 ÷ 采样率
   - ffprobe: 可能包含额外的容器时间戳

3. **四舍五入策略**
   - MacinMeter: `round()` 四舍五入到整数
   - ffprobe: 可能使用不同的舍入规则

**结论**: ±3 kbps (0.05%) 属于正常精度范围

---

**文档版本**: v0.1.1 (2025-11-08-14-20)
**作者**: MacinMeter Development Team
**历史原审核状态**：`已验证`

**当前归档状态**：未满足现行 target 身份和证据要求，不作为验证记录。
