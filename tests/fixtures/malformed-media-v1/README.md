# malformed-media-v1 regression corpus

本目录是 ADR-0003 §8 定义的固定 malformed/mutation 回归 corpus。每个 case 都是
对已提交 `native-pcm-v1` fixture 的确定性字节级派生（截断、定点补丁、固定
xorshift64 seed 的 XOR），或确定性合成字节串；不含任何个人音频或外部媒体。

## 判据

- 每个 case 必须以 [`manifest.json`](manifest.json) 记录的结构化错误码和阶段
  失败；
- 任何 case 都不得产生 EOF、成功报告或 partial success；
- decode 阶段的终态错误必须 sticky。

这些判据只覆盖已提交的 corpus 文件，不声称所有字节输入永不 hang 或分配有界。

## 验证层

| 层 | 入口 | 说明 |
| --- | --- | --- |
| workspace 测试 | `crates/macinmeter-codecs/tests/malformed_corpus.rs` | 进程内快速回归，校验字节身份与错误码/阶段 |
| 扩展验证 | `python3 scripts/verify-malformed-corpus.py` | 每 case 独立子进程 + 30s timeout；POSIX 上加 `RLIMIT_AS`（默认 2 GiB），无该接口的平台（如 Windows）跳过内存限制并在输出中记录 |
| 再生成审计 | `python3 scripts/generate-malformed-media-v1.py --check` | 确认提交字节与确定性再生成一致 |

## 字节接缝与 fuzz 入口

第一方 WAV/AIFF chunk parser 接受 `Read + Seek`，crate 内测试可直接消费
in-memory bytes。外部 fuzz runner 使用 `macinmeter-codecs` 的非默认
`malformed-dev` feature 暴露的隐藏 dev 入口（`dev::probe_container_bytes`）；
默认产品 API 仍是基于 `Path` 的 `DecoderFactory`。fuzz 是独立本地任务，不进入
pre-commit，也不自动触发远端 CI；fuzz 发现的 crash 最小化后应作为新 case 回灌
本 corpus 并重新登记 manifest。
