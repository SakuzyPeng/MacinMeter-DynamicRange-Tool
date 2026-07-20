# M6：Analyzer validation traversal A/B

- 状态：Candidate accepted
- 日期：2026-07-21
- 方法：ADR-0007 / `m6-scalar-baseline-v1` 三项 analysis 子集
- scalar source：
  `f116f3e272dfb97d79f08e2924727fbda08083a9`（candidate 的直接父提交）
- candidate source：
  `ab09c8b67e40c52762e78cdc6ccbf8bc6e7c8000`（clean）
- canonical raw record：
  [`m6-validation-traversal-ab-v1-ab09c8b-aarch64-apple-darwin.json`](comparisons/m6-validation-traversal-ab-v1-ab09c8b-aarch64-apple-darwin.json)
- raw record SHA-256：
  `a1bdd4cfce49461bdf90596a6d8a79b0f00efe52b4efe2deb61df4a9b2e2832d`
- 前置归因：
  [`M6_SAMPLING_PROFILE_REPORT.md`](M6_SAMPLING_PROFILE_REPORT.md)

## 结论

首个 M6 优化 candidate 通过完整正确性门禁与同轮交错 A/B，保留进入产品主链。
它没有增加算法 profile、公开 API、并发、SIMD 或 `unsafe`：

- 1–4 声道保留连续 finite scan 加 channel-major numeric validation；
- 5–64 声道把 finite check 合入 frame-major transactional shadow validation；
- validation 全部成功后才运行原有 commit loop；
- 每声道样本与浮点运算顺序、invalid-chunk 原子性、错误类型、错误文本及错误优先级
  均保持不变。

选择 5 声道作为切换点不是从声道数猜测出来的。实现前的同源码本地校准显示 4
声道处 frame-major 收益仍在噪声内，从 5 声道开始才稳定转为收益；正式 suite
随后验证了低声道不退化、8 声道有明确收益、64 声道有大幅收益。校准只用于选择
candidate 形状，不作为 canonical 性能记录。

## 同轮交错结果

每个 case/variant 先 warm-up 1 次，再 measured 7 次；42 个 measured sample
使用固定 seed 完全交错，未删除任何 outlier。elapsed delta 为
`candidate / scalar - 1`；吞吐 delta 方向相反。

| Case | Scalar median | Candidate median | Elapsed delta | Throughput delta | Scalar / candidate MAD |
| --- | ---: | ---: | ---: | ---: | ---: |
| analysis / stereo | 136.508 ms | 136.454 ms | −0.04% | +0.04% | 1.182 / 0.807 ms |
| analysis / 8ch | 125.728 ms | 120.135 ms | −4.45% | +4.66% | 0.561 / 0.235 ms |
| analysis / 64ch | 156.125 ms | 125.551 ms | −19.58% | +24.35% | 0.324 / 0.367 ms |

双声道走未改变的低声道 traversal，中位差异只有 0.04%，不解释为收益。8 声道
candidate 的全部 measured elapsed 为 118.489–120.528 ms，低于 scalar 的
124.971–131.082 ms；64 声道两组范围分别为 125.127–127.134 ms 与
155.364–157.089 ms，收益远大于各自 MAD。

这只证明固定 arm64 主机、固定 synthetic dense-f64 workload 上的同机结果。它
不是跨机器吞吐声明，也不能外推到任意 block 大小、输入内容或用户文件。

## 正确性与可观察语义

runner 在形成 summary 前确认每个 case 的 scalar/candidate result fingerprint
完全相同：

| Case | Result fingerprint |
| --- | --- |
| analysis / stereo | `b5373187a2077229147afef158c67c0ea99889e004ca3bee4a4d93567691e04a` |
| analysis / 8ch | `70a035ebd128d5009280e3a2dd86ade6e2f907e01680dae21a56ac93cb371a21` |
| analysis / 64ch | `1b60a3e6a307e6ffc455d52a08fffd685aa9a25668595d34973d3bba563f9840` |

产品测试另行把新 frame-major inspector 与原 channel-major 实现做差分，覆盖
1/2/3/4/5/6/8/16/64 声道、多个窗口前缀、有效输入、NaN、正负 infinity、
sample-square overflow、window-RMS overflow、多重声道失败以及 non-finite
全局优先级。完整 session bit projection、chunk-boundary、lane isolation 与失败后
继续分析测试全部保持一致。

特别保留了两个容易被遍历重排破坏的旧契约：

1. 任一 non-finite sample 都优先于同一 chunk 内的有限数值 overflow；
2. 多个声道都有有限数值 overflow 时，较低 channel index 的首个失败优先，而
   不是交错数据中时间位置更早的失败。

## 身份与环境

| 字段 | 值 |
| --- | --- |
| scalar worker | `05879505c0dcd745f9f773462bc8ca635c547a82f1a52b8216c9852e52ebadb7`；1,030,912 bytes |
| candidate worker | `23b0fd0308c61c13578c17ebd79d85b3e5548b929903195d41c8a590fca8ddcd`；1,030,912 bytes |
| suite SHA-256 | `cc4a612bc04701cf01198ce6c682c51abbc039983d320385165072d363844455` |
| corpus manifest SHA-256 | `c985486a6317b927e95c5933f6b8e76eb5f2b6a8b1a0dd9c38f451fab27946b0` |
| schedule | seed `1295380769`；1 warm-up + 7 measured；fully interleaved |
| machine | Apple M4 Pro / Mac16,8 / 12 CPU / 48 GiB |
| OS | macOS 27.0 build 26A5378n；Darwin arm64 27.0.0 |
| Rust | rustc 1.96.0；LLVM 22.1.2；Cargo 1.96.0 |
| build | release；opt-level 3；thin LTO；codegen-units 1；strip；overflow checks |
| power | AC Power；`powermode = 0` |
| run UTC | 2026-07-20 18:09:04–18:09:12 |

run 开始与结束的 load average 分别为 15.84/13.77/9.71 与
15.13/13.66/9.69。绝对耗时因此不能和较早 canonical baseline 直接作小幅比较；
本次结论只使用同一高负载区间中完全交错的两个 variant。8ch/64ch 分布仍明显分离，
stereo 则按噪声内处理。

native max RSS 的 candidate/scalar 中位差异只有数十 KiB，且 process-tree RSS
在 8ch 完全相同、64ch 中位也相同。该粒度不足以形成内存收益或回归声明；产品的
duration-independent 内存仍由结构与 invariant test 保证。

## 验收门禁

candidate source 在正式 A/B 前完成：

- `cargo fmt --all -- --check`
- `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`
- `cargo test --locked --workspace --all-targets`
- `cargo build --locked --release -p macinmeter-cli`
- `cargo check --locked --manifest-path tauri-app/src-tauri/Cargo.toml`
- repository script tests：29 项
- reference tool tests：121 项
- Tauri frontend production build
- repository contract check

这些门禁与 A/B 均在本地执行，没有触发或等待 GitHub Actions。

## 决策边界与下一步

candidate 以“高声道改善明确、低声道不退化、结果 bit-exact、复杂度有界”通过。
它不授权继续合并 validation/commit、设计 histogram rollback、恢复文件级并发、
增加 SIMD/unsafe 或修改 FLAC 完整性校验。

该 source-bound sampling profile 已完成：64-channel validation 的绝对采样权重
按预期下降，剩余调用树只支持一个不改变算法的 error-slow-path refinement，见
[`M6_VALIDATION_POST_PROFILE_REPORT.md`](M6_VALIDATION_POST_PROFILE_REPORT.md)。
refinement 之后若没有新的明显收益，M6 停止 analyzer 微优化；FLAC 与文件级并发
仍需新的真实需求和独立证据才重新立项。
