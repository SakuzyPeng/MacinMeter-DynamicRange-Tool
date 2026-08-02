[English](BENCHMARKS.md) | [中文](BENCHMARKS_CN.md)

# 性能状态

MacinMeter 0.2.0 不发布性能保证。M6 已完成可复现的本地标量基线、
sampling-profile、交错 A/B 协议与一条有界 analyzer 优化链，但其结果仍只是绑定
source、binary、corpus 与 environment 的证据，不是面向用户的吞吐承诺。

此前记录在这里的数字来自已经删除的 0.1.x 包级并行、文件级并行、SIMD 和
FFmpeg/DSD 路径，不能代表当前架构，也不能作为正确性或吞吐承诺。

0.2.0 有意保留安全标量、全串行的产品路径。M6 suite 分层测量：

- 2、8、64 声道 direct finite-f64 analysis；
- WAV、AIFF、FLAC 与 WAV-float64 原生解码；
- 共享 `Application` 路径与当前串行的 8-track batch；
- 递归 discovery 与 wire-v3 pretty JSON rendering。

生成语料不含私人音频，只存在 ignored `target/`。manifest 固定媒体 bytes、
geometry、归一化 decoded-f64 oracle 与 generator identity。

生成或核对语料：

```bash
python3 scripts/generate-performance-corpus.py
python3 scripts/generate-performance-corpus.py --check
```

从干净工作树构建 release worker 并运行完整基线：

```bash
python3 scripts/run-performance-baseline.py
```

runner 会记录 source commit、release binary hash、corpus/suite hash、toolchain、
机器/OS/电源身份、原生资源计数、采样得到的 descendant-process RSS、全部原始样本
与摘要。默认每项先 warmup 1 次，再 measured 7 次；使用固定 seed 完全交错调度，
不删除 outlier。

未来同机 A/B 必须把所有 protocol-compatible 的预构建 worker 放在同一次运行：

```bash
python3 scripts/run-performance-baseline.py \
  --variant scalar=/path/to/scalar-worker \
  --variant-source scalar=SCALAR_SOURCE_COMMIT \
  --variant candidate=/path/to/candidate-worker \
  --variant-source candidate=CANDIDATE_SOURCE_COMMIT
```

如果 result fingerprint、decoded PCM oracle、work unit 或同 PCM 的 application
结果不同，runner 会在形成摘要前失败。

在装有完整 Xcode 的 macOS 上，从干净工作树复现三项 sampling profile：

```bash
python3 scripts/run-performance-profile.py
```

首次 clean profile 把 stereo analysis 的 39.48% 与 64-channel analysis 的
69.20% 归因到独立 finite-input scan 加事务性 numeric-safety shadow traversal。
FLAC 有 79.07% 位于 Symphonia decoder 内，产品 sample materialization 与
`PcmBlock` 构造明显更小。因此 M6 选择优化 analyzer validation traversal，而
不是文件级并发、SIMD、禁用 checksum 或增加 decoder。经过一次 post-profile
限定的 refinement，最终 clean 交错 A/B 中 stereo 保持在噪声内，8 声道中位
耗时下降 12.92%，64 声道下降 26.72%，跨 variant result fingerprint 完全相同。

历史 M6 证据本身不授权 packet/file/window 并行、SIMD、unsafe 或外部 decoder。
后继独立决策
[`ADR-0014`](adr/0014-deterministic-decode-analysis-pipeline.md) 现已允许有界确定性
并行 candidate，并把 route-specific packet 解码设为第一优先级；当前产品在各
candidate 独立毕业前仍保持串行。所有比较与未来加速声明仍必须遵循
[`ADR-0007`](adr/0007-m6-reproducible-performance-baseline.md)。首次 clean-source
结果与原始样本见
[`M6 标量基线报告`](performance/M6_SCALAR_BASELINE_REPORT.md)、
[`M6 sampling-profile 报告`](performance/M6_SAMPLING_PROFILE_REPORT.md)和
[`validation-traversal A/B 报告`](performance/M6_VALIDATION_TRAVERSAL_AB_REPORT.md)；
完整决策链与最终边界见
[`M6 收口报告`](performance/M6_PERFORMANCE_ENGINEERING_CLOSURE_REPORT.md)。
