# M6：0.2.0 性能工程收口

- 状态：M6 closed
- 日期：2026-07-21
- final source：`df594c48c67ef7881e446c37259106a3a0666dc9`（clean）
- pre-optimization scalar：
  `f116f3e272dfb97d79f08e2924727fbda08083a9`
- accepted-candidate parent：
  `e09cd736bcdca6a35db760991803f464dd32c12d`
- refinement A/B raw record：
  [`m6-validation-slow-path-ab-v1-df594c4-aarch64-apple-darwin.json`](comparisons/m6-validation-slow-path-ab-v1-df594c4-aarch64-apple-darwin.json)
- refinement raw SHA-256：
  `a39a87ca7bc2d87f808b16fffe7c6eb2f7fc1d1ac6860bb65c2838ffdb1d4567`
- final cumulative A/B raw record：
  [`m6-final-analysis-ab-v1-df594c4-aarch64-apple-darwin.json`](comparisons/m6-final-analysis-ab-v1-df594c4-aarch64-apple-darwin.json)
- final raw SHA-256：
  `a946570d5c86265c32798964b0f5a9483452a2f5d05be2e08de47e3311368747`

## 结论

M6 已完成“先建立可信标量基线、再 profile、只优化已确认热点、以 bit-exact 同轮
A/B 裁决”的闭环。最终生产路径仍是安全标量、串行、唯一 `AnalyzerSession`：

- 1–4 声道保留连续 finite scan 加 channel-major numeric validation；
- 5–64 声道使用合并 finite check 的 frame-major transactional shadow；
- 有效高声道输入使用紧凑索引循环和无 failure-state shadow；
- 极端有限数值 overflow 只读回放 channel-major inspector，以保留旧错误优先级；
- validation 全部成功后才进入原 commit loop。

相对 pre-optimization scalar 的最终同轮 A/B：

| Case | Scalar median | Final median | Elapsed delta | Throughput delta | Scalar / final MAD |
| --- | ---: | ---: | ---: | ---: | ---: |
| analysis / stereo | 133.163 ms | 132.713 ms | −0.34% | +0.34% | 0.686 / 1.455 ms |
| analysis / 8ch | 124.178 ms | 108.131 ms | −12.92% | +14.84% | 0.440 / 1.235 ms |
| analysis / 64ch | 155.462 ms | 113.917 ms | −26.72% | +36.47% | 0.503 / 0.123 ms |

stereo 走未改变的低声道路径，0.34% 差异小于 final MAD，不解释为收益。8ch 与
64ch 的分布明显分离，收益分别远大于两组 MAD。

这些数字只描述固定 Apple M4 Pro、固定 toolchain、固定 synthetic dense-f64
workload。它们不是用户吞吐保证，也不能外推到其他主机、block size 或输入内容。

## 最终 refinement

accepted candidate 的 post-profile 显示 64ch frame-major inspector 仍占 61.27%；
其中 iterator 链为 9.22%，per-sample failure-state 查询为 2.99%。最终 refinement
把错误保留为 cold slow path，并在有效输入上删除这两层状态。与直接父提交的
同轮 A/B 为：

| Case | Accepted median | Refined median | Elapsed delta | Throughput delta | Accepted / refined MAD |
| --- | ---: | ---: | ---: | ---: | ---: |
| analysis / stereo | 134.291 ms | 133.365 ms | −0.69% | +0.69% | 0.664 / 0.877 ms |
| analysis / 8ch | 118.224 ms | 107.098 ms | −9.41% | +10.39% | 0.620 / 0.265 ms |
| analysis / 64ch | 125.132 ms | 111.754 ms | −10.69% | +11.97% | 1.115 / 1.006 ms |

refinement run 和 cumulative run 处于不同系统负载区间，绝对 median 不应横向相减
或拼接。每张表只比较其同一次 fully interleaved run 内的 variant；cumulative
结论来自独立 scalar/final run，不是把两轮百分比相乘。

## 正确性

两次正式 run 都在 summary 前验证了 cross-variant fingerprint。最终 cumulative
run 的三个 fingerprint 为：

| Case | Result fingerprint |
| --- | --- |
| analysis / stereo | `b5373187a2077229147afef158c67c0ea99889e004ca3bee4a4d93567691e04a` |
| analysis / 8ch | `70a035ebd128d5009280e3a2dd86ade6e2f907e01680dae21a56ac93cb371a21` |
| analysis / 64ch | `1b60a3e6a307e6ffc455d52a08fffd685aa9a25668595d34973d3bba563f9840` |

每项 scalar/final 完全相同。产品差分测试另外覆盖：

- 1/2/3/4/5/6/8/16/64 声道；
- 多种完整窗口与尾窗前缀；
- 任意 frame-aligned chunk 分割；
- NaN、正负 infinity、sample-square/window/overall accumulation overflow；
- non-finite 全局优先、较低 channel index 优先；
- invalid chunk 前后完整 session bit projection 与继续分析；
- lane isolation、signed zero、subnormal、overfull finite f64 与最终 finite JSON。

slow-path replay 不修改 session，也不绕过原 inspector。它只在将被拒绝的有限数值
chunk 上增加一次只读遍历；有效输入与所有公开结果不经过第二次 validation。

## 运行身份

两次 closure run 都使用 suite 子集 SHA-256：

```text
cc4a612bc04701cf01198ce6c682c51abbc039983d320385165072d363844455
```

| 字段 | Refinement A/B | Final cumulative A/B |
| --- | --- | --- |
| seed | `1295380770` | `1295380771` |
| schedule | 1 warm-up + 7 measured / variant / case；fully interleaved | 同左 |
| measured samples | 42 | 42 |
| base worker | `81e02a9b...` / 1,030,912 bytes | `05879505...` / 1,030,912 bytes |
| final worker | `72a7e9fd...` / 1,030,912 bytes | 同左 |
| run UTC | 18:21:48–18:21:55 | 18:22:32–18:22:39 |
| load average start/end | 26.97 → 23.51（1 min） | 17.02 → 15.03（1 min） |

两轮均为 clean final source、AC Power、Apple M4 Pro / Mac16,8、48 GiB、
macOS 27.0 / Darwin arm64、rustc 1.96.0 / LLVM 22.1.2。较高 load average
进一步说明不能把不同 run 的绝对 elapsed 当作小幅差异证据；同轮交错调度是这里
能够成立的比较边界。

process-tree RSS 在 cumulative run 的三个 case 中 scalar/final 中位完全相同。
64ch native max RSS 中位从 5,488,640 bytes 到 5,521,408 bytes，相差两个 16 KiB
page，低于本协议可形成内存回归结论的粒度。duration-independent 内存仍由结构与
产品 invariant test 保证。

## 完整门禁

最终 source 在正式 A/B 前通过：

- `cargo fmt --all -- --check`
- `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`
- `cargo test --locked --workspace --all-targets`
- `cargo build --locked --release -p macinmeter-cli`
- `cargo check --locked --manifest-path tauri-app/src-tauri/Cargo.toml`
- repository script tests：29 项
- reference tool tests：121 项
- Tauri frontend production build
- repository contract check

所有 benchmark/profile 都是显式本地任务；没有触发或等待 GitHub Actions。

## M6 收口边界

M6 到此停止主动性能改写：

- analyzer 剩余主体是必须保留的有限数值 validation、逐声道浮点运算和 commit；
  不为继续追逐局部百分比合并两阶段、设计 histogram rollback 或增加 SIMD/unsafe；
- FLAC 主成本位于 Symphonia decoder 与完整性校验；没有禁用 checksum、fork
  decoder 或增加第二 backend 的依据；
- scalar batch 基线没有显示 scheduler/discovery 瓶颈，也没有真实用户需求证明
  文件级并发值得扩大 M3 execution budget；
- 包级并行、Rayon/Tokio、外部 decoder、DSD 与旧预处理仍保持删除；
- benchmark 不进入普通 test、pre-commit、手动 CI 或发布性能承诺。

未来只有出现新的用户可感知问题、可复现 corpus 或平台需求时，才从本 ADR 的
source-bound protocol 重新立 candidate；“还可以再快一点”本身不构成立项证据。
