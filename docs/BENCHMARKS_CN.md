[English](BENCHMARKS.md) | [中文](BENCHMARKS_CN.md)

# 性能状态

MacinMeter 0.2.0 不发布性能保证。M6 现在已经建立可复现的本地标量基线协议，但其
结果仍只是绑定 source、binary、corpus 与 environment 的证据，不是面向用户的
吞吐承诺。

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

后续优化会优先考虑由 application 统一管理、具有共享资源预算的单一文件级并行轴。
本文档不暗示包级并行、SIMD 或外部解码进程会恢复。准确测量方法与声明边界见
[`ADR-0007`](adr/0007-m6-reproducible-performance-baseline.md)，首次 clean-source
结果与原始样本见
[`M6 标量基线报告`](performance/M6_SCALAR_BASELINE_REPORT.md)。
