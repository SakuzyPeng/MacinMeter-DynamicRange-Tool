# M6：Accepted validation traversal post-profile

- 状态：Post-profile established；one bounded refinement selected
- 日期：2026-07-21
- 方法：ADR-0007 / `m6-sampling-profile-v1` 的 64-channel analysis 子集
- source：`2f6c26207ed95b73a2df2c426b001d1375dd2e37`（clean）
- canonical raw record：
  [`m6-validation-traversal-post-v1-2f6c262-aarch64-apple-darwin.json`](profiles/m6-validation-traversal-post-v1-2f6c262-aarch64-apple-darwin.json)
- raw record SHA-256：
  `e63ab51eac525262bb8ca94c9394a3df302cd686d7a88551c55015151c20c644`
- 前置 A/B：
  [`M6_VALIDATION_TRAVERSAL_AB_REPORT.md`](M6_VALIDATION_TRAVERSAL_AB_REPORT.md)

## 结论

accepted candidate 的 64-channel post-profile 证实局部性优化命中了原归因路径：

- 旧 profile 中独立 finite scan 与 channel-major numeric shadow 合计占
  69.20%，三次累计 scoped weight 为 9.956 s；
- 新 profile 中合并后的 frame-major inspector 占 61.27%，累计 scoped weight
  为 6.680 s；
- 原 commit-loop `ZipImpl` 为 4.156 s，新 profile 为 3.975 s，数量级基本不变；
- 三次新 capture 的总 scoped weight 为 10.903 s，旧 profile 为 14.387 s。

这些 profile 来自不同 source/binary 的独立 capture，绝对 weight 只用于支持调用
路径归因，不能代替同轮 timing A/B。前一报告中 64ch elapsed −19.58% /
throughput +24.35% 才是 candidate 收益的 canonical 比较。

post-profile 同时显示 frame-major inspector 仍是最大可控 subtree，但其剩余成本
主要是必须保留的有限 `f64` 算术与 window-RMS 检查。只选择一个进一步的有界
refinement：

1. 有效输入不再为每个 sample 查询并携带 `Option<failure>`；
2. 用紧凑索引循环替代 `enumerate().copied()` iterator 链；
3. 一旦遇到有限数值 overflow，丢弃 shadow 并用现有 channel-major inspector
   回放同一只读 chunk，恢复旧错误优先级；
4. non-finite 仍直接优先失败，session 仍只在 validation 全部成功后 commit。

错误输入的回放只影响拒绝路径，不改变错误、状态或有效输入结果。该 refinement
仍需完整差分门禁和同轮 A/B；收益若落在噪声内就删除。

## 捕获质量与身份

| 字段 | 值 |
| --- | --- |
| worker SHA-256 | `f3a5faa35c16fcfcbd6400a5d550aea01edcb25ba6553f448705d16d2cd118c4` |
| worker size | 1,300,064 bytes |
| worker build | release opt-level 3；thin LTO；codegen-units 1；debug 1；unstripped |
| profiler | Xcode Time Profiler / xctrace 16.0（27A5194q） |
| sampling | 1 ms；3 次独立 capture |
| scoped samples | 3,620 / 3,635 / 3,648；合计 10,903 |
| coverage | 0.9979 / 0.9971 / 0.9996 |
| trace bundles | 3 个；合计 21,987,546 bytes；保留在 ignored `target/` |
| suite SHA-256 | `aee833b80dfbeb90e2fa2515005d1ccc265955ecb9874b93793e00ad2d36f563` |
| corpus manifest SHA-256 | `c985486a6317b927e95c5933f6b8e76eb5f2b6a8b1a0dd9c38f451fab27946b0` |
| machine | Apple M4 Pro / Mac16,8 / 12 CPU / 48 GiB |
| OS | macOS 27.0 build 26A5378n；Darwin arm64 27.0.0 |
| Rust | rustc 1.96.0；LLVM 22.1.2；Cargo 1.96.0 |

三次 result fingerprint 均为：

```text
bd111ded4ddc723607fa0291fd97eabdac8a2fe329a87419f910bf45e7e7d7b6
```

它与旧 64-channel profile 完全相同。每次 capture 都远超 1,000 scoped-sample
下限，coverage 也严于协议的 `0.85..1.15`。

## 剩余调用树

以下为 inclusive subtree，父子项会重叠，不能相加：

| Subtree | Scoped weight |
| --- | ---: |
| frame-major numeric inspector | 61.27% |
| `NumericSafetyShadow::observe` | 36.82% |
| commit-loop `ZipImpl` | 36.46% |
| `window_rms_squared` | 11.02% |
| `enumerate().copied()` iterator chain | 9.22% |
| per-sample `Option::is_some` | 2.99% |
| `ChannelAccumulator::add_sample` project function | 1.42% |
| histogram/window finalization | <0.1% |

iterator 和 failure-state 项给出了上述 refinement 的边界；它们没有授权删除
numeric validation。即使 refinement 全部消除这些可见开销，validation/commit
双阶段与逐 sample 浮点运算仍会占主导。

## 不变的边界

- 不把 validation 与 commit 合并，不设计 histogram rollback；
- 不更改 per-channel arithmetic order、window/profile/rounding 或结果精度；
- 不增加 SIMD、unsafe、并行、第二 analyzer 或公共配置；
- 不因 profile elapsed 下降而发布跨机器吞吐数字；
- 不修改 FLAC、checksum、decoder 或 application execution budget。
