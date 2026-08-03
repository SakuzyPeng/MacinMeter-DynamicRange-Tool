# CONF-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-030-adr0014-20260803

## 结论

- conformance run ID：
  `CONF-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-030-adr0014-20260803`
- 事实类别：reference-to-implementation report-metrics conformance
- 状态：`match`
- 参考规格：`foo-dr-meter-1.0.8-candidate-v1`
- target ID：`TARGET-foo-dr-meter-1.0.8-foobar2000-2.25.10-x64-win10-19045`
- experiment ID：`EXP-foo-dr-meter-108-complete-v2`
- 参考观测：
  [`OBS-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718`](../../observations/obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/observation.json)
- fixture：corpus `foo-dr-meter-108-complete-v2`、playlist `00-safe-master` 的
  39 项，按 manifest 顺序，首项 `window-minus-one-control`、末项
  `host-decode-f64`；完整集合与顺序见
  [manifest](../../fixtures/foo-dr-meter-108-complete-v2.manifest.json)
- 被测实现提交：`768670b186c4c62e4fd5ff30e759e9e30cee1a94`
- wire schema / tool version：4 / 0.3.0

本记录是
[`clean-commit schema-v3 记录`](../conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-report-v3-clean-20260718/record.md)
的 0.3.0 successor。它复用同一次已登记的 reference 导出，不重复运行 reference
runtime，也不回写旧记录或产物。

登记目的是 ADR-0014 要求的回归对照：0.3.0 引入了 ADR-0013 的 ALAC route，并在
ADR-0014 下重构了解码路径——共用 `decode_engine`、按输入序提交的
`PacketReorderBuffer`、application 所有的 allocation plan，以及 ALAC packet
workers。这 39 项输入全部是 WAV，不经过 ALAC route，因此本对照回答的是这些改动
是否波及既有 PCM 路径的数值，而不是 packet workers 自身的正确性。

对同一批 39 个 manifest-ordered safe-master 输入，固定 reference observation 与
实现 WireEnvelope 得到：

| 字段 | 精确匹配 |
| --- | ---: |
| 整数 track DR | 39/39 |
| 每声道两位 DR token | 62/62 |
| overall primary peak token | 39/39 |
| overall RMS token | 39/39 |
| 每声道 overall RMS token | 62/62 |
| duration token | 39/39 |
| footer 可比较子集 | 4/4 |

逐 track/channel 比较的数值容差为 0，差分数为 0；fixture 集合与实现输出顺序完全
一致。有限结果不会把 candidate 规格升级为 accepted，也不会把兼容性改为 verified。

## 固定身份

### Reference

| 对象 | SHA-256 |
| --- | --- |
| 原始 x64 报告 | `e9afbde86ccb21cae56826803da5492e37135c8594a657130b3868b42956d11c` |
| 规范化报告 | `50205960b9850addb7f18bdb5f3c2c3c59897a5a2c5efc8e408870d5a3a2ffce` |
| complete-v2 manifest | `479e535a7196487fdb67a54f0c4de681f925920453e8092bac9eeb04eec4bbf8` |

参考运行身份、foobar2000 2.25.10 x64、插件 1.0.8 x64 及 Windows 环境由上述
observation 和 target 记录固定。

### Corpus

corpus 由 `reference/tools/generate_foo_dr_meter_108_complete_v2.py` 在本机重新
生成并自校验：42 个 case、62 个受检文件、39 个 safe-master entry，聚合
`filesSha256 = ef870bd91e3a63a0ab14610116db6ebb5b21bf1c72ec6b3b18d0a367f7ca0a35`。

重新生成的 manifest 与提交在
[`reference/fixtures/foo-dr-meter-108-complete-v2.manifest.json`](../../fixtures/foo-dr-meter-108-complete-v2.manifest.json)
的版本逐 case 一致：case id 集合相同，全部 `dataSha256` 与 `fileSha256` 相同，
`00-safe-master` playlist 顺序相同；唯一差异是记录 generator 自身身份的
`generator` 字段。因此比较所用的 WAV 输入与既有记录逐字节相同。

### MacinMeter

| 对象 | 身份 |
| --- | --- |
| source commit | `768670b186c4c62e4fd5ff30e759e9e30cee1a94` |
| build worktree | clean detached worktree at the source commit |
| build command | `cargo --manifest-path "$WORKTREE/Cargo.toml" build --locked --release -p macinmeter-cli` |
| build host | macOS 27.0 arm64（Apple M4 Pro） |
| Rust / Cargo | 1.96.0 / 1.96.0 |

| 对象 | SHA-256 |
| --- | --- |
| release CLI binary | `1e44a6f679eb0dd6fb163337275fb206adce7a80adde295403e5ccb176642254` |
| schema-v4 WireEnvelope | `45c249425dbbe8fff04606377a969c3a5231ba795df1c7f83e0fc3f7d535d622` |
| comparison | `80f849312f4905e9434adc188e12309dc6b20a3e22b83f1971e1a4c34ccc2f0f` |
| comparator | `db328a27adf465fe0f1c9e209571a293f29097d6d3b0a6ec12d9bd22fd0c7d33` |

实现输出保存在
[`implementation/schema4-wire.json`](implementation/schema4-wire.json)，规范化
差分保存在 [`comparison.json`](comparison.json)。保存的实现输出与随后一次独立
重跑逐 byte 相同；两次 CLI 均退出 0、stdout 为 0 bytes、stderr 各 14111 bytes。
这个检查只说明同一实现、同一输入和同一本地环境的输出重复性。

同一 wire 输出也由一个先前构建的、二进制 SHA-256 不同的 release CLI 产生。这说明
本次比较的结果不依赖该二进制的具体构建产物，但不构成任何可复现构建声明。

## 命令

`$WORKTREE` 是一个尚不存在的本机路径，`$CORPUS` 是一个空的本机目录，`$PATHS`
是第 3 步产生的 39 行路径列表，`$WIRE` 与 `$COMPARISON` 分别是实现输出和差分输出
的落点。它们均为本机路径；本记录不声称替换这些路径后 artifact 仍逐 byte 相同。
全部命令退出状态为 0。

```bash
# 1. 固定并核对被测 source worktree（各 exit 0）
git worktree add --detach "$WORKTREE" \
  768670b186c4c62e4fd5ff30e759e9e30cee1a94
test "$(git -C "$WORKTREE" rev-parse HEAD)" = \
  768670b186c4c62e4fd5ff30e759e9e30cee1a94
test -z "$(git -C "$WORKTREE" status --porcelain)"

# 2. 生成并自校验 corpus（各输出一行 JSON 摘要，exit 0）
python3 "$WORKTREE/reference/tools/generate_foo_dr_meter_108_complete_v2.py" \
  --output "$CORPUS"
python3 "$WORKTREE/reference/tools/generate_foo_dr_meter_108_complete_v2.py" \
  --verify "$CORPUS"

# 3. 按 manifest 的 00-safe-master 顺序构造 39 项路径列表（exit 0）
python3 - "$CORPUS" > "$PATHS" <<'ORDER'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
manifest = json.loads((root / "manifest.json").read_text(encoding="utf-8"))
by_id = {case["id"]: case for case in manifest["cases"]}
print("\n".join(str(root / by_id[i]["path"]) for i in manifest["playlists"]["00-safe-master"]))
ORDER

# 4. 构建并运行实现（各 exit 0；CLI stdout 0 bytes、stderr 14111 bytes）
cargo --manifest-path "$WORKTREE/Cargo.toml" build \
  --locked --release -p macinmeter-cli
path_args=()
while IFS= read -r path; do
  path_args+=("$path")
done < "$PATHS"
"$WORKTREE/target/release/macinmeter" batch \
  --format json \
  --output "$WIRE" \
  "${path_args[@]}"

# 5. 比较（exit 0）
python3 \
  "$WORKTREE/reference/tools/compare_macinmeter_report_metrics_to_foo_dr_meter.py" \
  --reference \
    "$WORKTREE/reference/observations/obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/normalized/safe-master.json" \
  --implementation-output "$WIRE" \
  --implementation-binary "$WORKTREE/target/release/macinmeter" \
  --output "$COMPARISON"

# 6. comparator 单元测试（exit 0）
python3 -m unittest discover \
  -s "$WORKTREE/reference/tools/tests" \
  -p 'test_*.py'
```

第 4 步执行两次，两次输出逐 byte 相同、stderr 字节数相同。WireEnvelope 的两处
`displayPath` 保留了实际 `$CORPUS` 根路径；comparison 不保存路径文本，fixture
映射也只使用文件 stem，但其 `wireOutputSha256` 对原始 WireEnvelope 字节计算。因此
更换 corpus 根路径会改变 wire 和 comparison 的 artifact SHA-256，即使字段匹配摘要
不变。本记录的逐 byte 重跑复用了同一路径与环境，不构成跨路径或跨机器复现声明。

## 并行状态

本次运行时产品 `ConcurrencyPlan` 恒为 serial，因此这 39 项全部在串行引擎上完成。
即使 packet workers 将来默认启用，这批输入仍是 WAV，按 ADR-0014 的 route 判定不会
创建 worker；本记录因此是既有 PCM 路径的回归基线，不能被解读为 packet workers
已经过 reference 对照。

## 范围

本记录只覆盖上述七类可比较字段。它不建立：

- internal intermediate state、album length weighting 或 host metadata parity；
- footer 的 `bitsPerSampleToken`、`bitrateToken`、`codecToken`；
- 任何跨机器可复现构建声明；
- ALAC route 或 packet workers 与 reference 的对照——这批输入不经过该 route。
