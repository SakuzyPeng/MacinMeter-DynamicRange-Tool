[English](BENCHMARKS.md) | [中文](BENCHMARKS_CN.md)

# Performance status

MacinMeter 0.2.0 has no published performance guarantee. M6 now has a
reproducible local scalar-baseline protocol, but its results remain
source/binary/corpus/environment-specific evidence rather than a user-facing
throughput promise.

The benchmark numbers previously stored here measured the removed 0.1.x
packet-parallel, file-parallel, SIMD, and FFmpeg/DSD paths. They do not describe
the current architecture and cannot be used as a correctness or throughput
commitment.

Version 0.2.0 intentionally retains a safe scalar, serial product path. The M6
suite measures these scopes separately:

- direct finite-f64 analysis at 2, 8, and 64 channels;
- native WAV, AIFF, FLAC, and WAV-float64 decoding;
- the shared `Application` path and current serial eight-track batch;
- recursive discovery and wire-v3 pretty-JSON rendering.

The generated corpus contains no private audio and stays under ignored
`target/`. Its manifest pins media bytes, geometry, normalized decoded-f64
oracles, and generator identity.

Generate or verify it with:

```bash
python3 scripts/generate-performance-corpus.py
python3 scripts/generate-performance-corpus.py --check
```

From a clean worktree, build the release worker and run the full baseline:

```bash
python3 scripts/run-performance-baseline.py
```

The runner records the source commit, release binary hash, corpus/suite hashes,
toolchain, machine/OS/power identity, native resource counters, sampled
descendant-process RSS, all raw samples, and summaries. The default protocol is
one warm-up plus seven measured runs per case, with a seeded fully interleaved
schedule and no outlier deletion.

For a future same-machine A/B, pass all protocol-compatible prebuilt workers in
one invocation:

```bash
python3 scripts/run-performance-baseline.py \
  --variant scalar=/path/to/scalar-worker \
  --variant-source scalar=SCALAR_SOURCE_COMMIT \
  --variant candidate=/path/to/candidate-worker \
  --variant-source candidate=CANDIDATE_SOURCE_COMMIT
```

The run fails before summarization if result fingerprints, decoded PCM oracles,
work units, or identical-PCM application results differ.

Future optimization will first consider one application-owned file-level
parallelism axis with a shared resource budget. Packet-level parallelism, SIMD,
and external decoder processes are not implied by this document. See
[ADR-0007](adr/0007-m6-reproducible-performance-baseline.md) for the exact
measurement and claim boundary. The initial clean-source results and raw
samples are recorded in the
[M6 scalar-baseline report](performance/M6_SCALAR_BASELINE_REPORT.md).
