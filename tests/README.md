# 测试固件和边界测试

## 概述

本目录包含音频DR分析器的边界和异常测试，验证各种边界条件、异常输入和数值边界的处理。

## 测试文件结构

```
tests/
├── audio_test_fixtures.rs  # 测试固件生成器（生成各种特殊音频文件）
├── boundary_tests.rs        # 边界和异常测试
├── fixtures/                # 自动生成的测试文件目录
└── README.md               # 本文件
```

## 运行测试

### 运行所有边界测试
```bash
cargo test --test boundary_tests
```

### 运行单个测试
```bash
# 测试静音处理
cargo test --test boundary_tests test_silence_handling -- --show-output

# 测试空文件
cargo test --test boundary_tests test_empty_file -- --show-output

# 测试高采样率
cargo test --test boundary_tests test_high_sample_rate -- --show-output
```

### 运行压力测试（手动）
```bash
cargo test --test boundary_tests --ignored
```

## 测试覆盖范围

### 1. 边界条件测试（3个）
- **零长度音频**：0个样本，只有头部
- **单采样点**：仅1个样本的音频
- **极短音频**：10ms（441个样本）

### 2. 数值边界测试（3个）
- **静音文件**：全0样本，测试RMS=0情况
- **全削波**：满刻度方波，极小动态范围
- **边缘值**：最大/最小/零值混合模式

### 3. 格式边界测试（2个）
- **高采样率**：192kHz, 24bit
- **3声道拒绝**：验证多声道拒绝逻辑

### 4. 异常文件测试（3个）
- **空文件**：0字节文件
- **伪装文件**：文本文件伪装成WAV
- **截断文件**：头部正常但数据不完整

### 5. 压力测试（1个，ignore）
- **连续处理**：批量处理多个测试文件

## 生成的测试固件

运行测试时会自动生成以下文件到 `tests/fixtures/`:

| 文件名 | 类型 | 用途 |
|--------|------|------|
| `zero_length.wav` | 0样本 | 测试空音频处理 |
| `single_sample.wav` | 1样本 | 测试极小样本量 |
| `tiny_duration.wav` | 10ms | 测试极短音频 |
| `silence.wav` | 1秒静音 | 测试全0样本 |
| `full_scale_clipping.wav` | 方波 | 测试削波 |
| `edge_cases.wav` | 边缘值 | 测试数值边界 |
| `high_sample_rate.wav` | 192kHz/24bit | 测试高规格 |
| `3_channels.wav` | 3声道 | 测试拒绝逻辑 |
| `empty.wav` | 0字节 | 测试空文件 |
| `fake_audio.wav` | 文本 | 测试伪装文件 |
| `truncated.wav` | 截断 | 测试损坏文件 |

## 测试结果示例

```bash
$ cargo test --test boundary_tests

running 12 tests
test test_zero_length_audio ... ok
test test_single_sample_audio ... ok
test test_tiny_duration_audio ... ok
test test_silence_handling ... ok
test test_full_scale_clipping ... ok
test test_edge_value_patterns ... ok
test test_high_sample_rate ... ok
test test_3_channels_rejection ... ok
test test_empty_file ... ok
test test_fake_audio_file ... ok
test test_truncated_wav ... ok
test audio_test_fixtures::tests::test_fixture_generation ... ok

test result: ok. 12 passed; 0 failed; 1 ignored; 0 measured
```

## 自定义测试固件

如果需要生成自定义测试文件：

```rust
use audio_test_fixtures::AudioTestFixtures;

let fixtures = AudioTestFixtures::new();

// 生成所有固件
fixtures.generate_all();

// 或生成单个固件
let path = fixtures.create_silence();
println!("生成静音文件: {:?}", path);
```

## 测试维护

### 添加新测试
1. 在 `audio_test_fixtures.rs` 添加固件生成方法
2. 在 `boundary_tests.rs` 添加测试函数
3. 使用 `#[test]` 标记普通测试，`#[test] #[ignore]` 标记压力测试

### 清理测试文件
测试固件会自动保留在 `tests/fixtures/`，方便手动验证。
如需清理：
```bash
rm -rf tests/fixtures/
```

## 测试哲学

这些测试遵循"边界优先"原则：
- **边界值测试**：验证极端输入（0样本、满刻度等）
- **异常输入测试**：验证错误处理（损坏文件、伪装文件等）
- **回归测试**：防止修改破坏已有功能

**测试目标**：确保工具在所有情况下都能优雅地处理输入，不会panic或产生不可预期的行为。
