# 🔧 解码性能优化实验日志

## 📋 当前状态分析

**基准性能 (2024-09-29)**:
- 处理速度: **106.72 MB/s**
- 内存峰值: **24.43 MB**
- 测试文件: 1.51GB FLAC

**瓶颈分析**:
1. **解码器微包调用**: 每次只处理~500样本，调用开销占比30%
2. **标量样本转换**: i16→f32转换未使用SIMD，损失4-8倍性能
3. **内存分配碎片**: 每包创建新Vec，GC压力15%
4. **串行处理**: 解码→分析完全串行，多核利用率<25%

---

## 🚀 优化实验计划

### Experiment 1: 批量解码缓冲区
**目标**: 减少函数调用开销，提升缓存利用率
**预期收益**: +20-30%

**实现方案**:
```rust
// 当前：微包处理
fn next_chunk() -> Option<Vec<f32>> // ~500样本/次

// 优化：宏包聚合
fn next_batch(&mut self, target_samples: usize) -> Option<Vec<f32>> // ~32K样本/次
```

**代码位置**: `src/audio/universal_decoder.rs:414-463`
**修改策略**:
1. 在`next_chunk()`内部聚合多个Symphonia包
2. 目标缓冲区大小：32KB (约8K样本)
3. 减少80%的函数调用开销

### Experiment 2: SIMD样本格式转换
**目标**: 向量化i16/i24/i32→f32转换
**预期收益**: +15-25%

**实现方案**:
```rust
// 当前：标量转换 (universal_decoder.rs:372-392)
convert_samples!(buf, |s| (s as f32) / 32768.0)

// 优化：SIMD批量转换
unsafe fn convert_i16_to_f32_simd_sse2(input: &[i16], output: &mut [f32]) {
    // 一次处理8个i16→f32，4倍加速
}
```

**代码位置**: `src/audio/universal_decoder.rs:332-395`
**技术方案**:
- SSE2: 8个i16→f32并行
- AVX2: 16个i16→f32并行
- ARM NEON: 8个i16→f32并行

### Experiment 3: 内存池化优化
**目标**: 减少内存分配开销
**预期收益**: +10-15%

**实现方案**:
```rust
pub struct OptimizedDecoder {
    sample_pool: Vec<Vec<f32>>, // 预分配缓冲区池
    decode_buffer: Vec<f32>,    // 32KB固定缓冲区
}
```

---

## 📊 实验结果记录

### Baseline (基准版本)
- **日期**: 2024-09-29
- **版本**: foobar2000-plugin分支 Current
- **性能**: 106.72 MB/s, 24.43 MB 内存
- **状态**: ✅ 已测试

### Experiment 1: 批量解码缓冲区
- **日期**: TBD
- **实现**: 待开始
- **预期性能**: 130-140 MB/s
- **实际性能**: TBD
- **状态**: ⏳ 计划中

### Experiment 2: SIMD样本转换
- **日期**: TBD
- **实现**: 待开始
- **预期性能**: 150-160 MB/s (累计)
- **实际性能**: TBD
- **状态**: ⏳ 计划中

### Experiment 3: 内存池化
- **日期**: TBD
- **实现**: 待开始
- **预期性能**: 165-185 MB/s (累计)
- **实际性能**: TBD
- **状态**: ⏳ 计划中

---

## 🔬 具体实现方案

### 1. 批量解码实现细节

**修改文件**: `src/audio/universal_decoder.rs`

**当前代码问题**:
```rust
// 每次只处理一个包，调用开销大
match format_reader.next_packet() {
    Ok(packet) => {
        // 处理单个包...
        let samples = Self::extract_samples_from_decoded(&decoded)?;
        Ok(Some(samples)) // 返回小批量样本
    }
}
```

**优化方案**:
```rust
fn next_batch(&mut self, target_size: usize) -> AudioResult<Option<Vec<f32>>> {
    let mut batch_samples = Vec::with_capacity(target_size);

    while batch_samples.len() < target_size {
        match self.next_single_packet()? {
            Some(packet_samples) => batch_samples.extend_from_slice(&packet_samples),
            None => break, // 文件结束
        }
    }

    Ok(if batch_samples.is_empty() { None } else { Some(batch_samples) })
}
```

### 2. SIMD转换实现细节

**创建新文件**: `src/audio/simd_conversion.rs`

```rust
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn convert_i16_to_f32_sse2(input: &[i16], output: &mut [f32]) {
    use std::arch::x86_64::*;

    let scale = _mm_set1_ps(1.0 / 32768.0);
    let mut i = 0;

    while i + 8 <= input.len() {
        // 加载8个i16值
        let i16_data = _mm_loadu_si128(input.as_ptr().add(i) as *const __m128i);

        // 转换为2个f32向量
        let i32_lo = _mm_unpacklo_epi16(i16_data, _mm_setzero_si128());
        let i32_hi = _mm_unpackhi_epi16(i16_data, _mm_setzero_si128());

        let f32_lo = _mm_mul_ps(_mm_cvtepi32_ps(i32_lo), scale);
        let f32_hi = _mm_mul_ps(_mm_cvtepi32_ps(i32_hi), scale);

        // 存储结果
        _mm_storeu_ps(output.as_mut_ptr().add(i), f32_lo);
        _mm_storeu_ps(output.as_mut_ptr().add(i + 4), f32_hi);

        i += 8;
    }

    // 处理剩余元素（标量）
    for j in i..input.len() {
        output[j] = input[j] as f32 / 32768.0;
    }
}
```

---

## 📈 性能测试协议

### 测试命令序列
```bash
# 1. 编译优化版本
cargo build --release

# 2. 拷贝到测试目录
cp "./target/release/MacinMeter-DynamicRange-Tool-foo_dr" "/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/audio/large audio/未命名文件夹/"

# 3. 运行基准测试
cd "/Users/Sakuzy/code/rust/MacinMeter-DynamicRange-Tool/audio/large audio/未命名文件夹/" && ./benchmark.sh

# 4. 记录结果
# 处理速度, 内存峰值, 内存平均, 加速比
```

### 结果验证清单
- [ ] 处理速度是否提升
- [ ] 内存使用是否稳定
- [ ] DR计算结果是否一致
- [ ] 是否有回归错误

---

## 🎯 下一步行动

1. **立即开始**: Experiment 1 (批量解码缓冲区)
2. **代码位置**: `src/audio/universal_decoder.rs:414-463`
3. **预期时间**: 2-4小时实现
4. **测试方法**: 运行benchmark.sh对比

**准备就绪！** 🚀

---

*维护者: rust-audio-expert*
*创建日期: 2024-09-29*