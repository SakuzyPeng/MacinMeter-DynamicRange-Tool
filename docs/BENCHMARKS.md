[English](BENCHMARKS.md) | [中文](BENCHMARKS_CN.md)

# Performance status

MacinMeter 0.2.0 has no published performance guarantee. M6 completed
reproducible local scalar-baseline, sampling-profile, and interleaved A/B
protocols plus one bounded analyzer optimization chain, but the results remain
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

On macOS with full Xcode, reproduce the three-case sampling profile from a
clean worktree with:

```bash
python3 scripts/run-performance-profile.py
```

The first clean profile attributes 39.48% of stereo analysis and 69.20% of
64-channel analysis to the independent finite-input scan plus transactional
numeric-safety shadow traversal. FLAC spends 79.07% in Symphonia's decoder;
product sample materialization and `PcmBlock` construction are much smaller.
Accordingly, M6 optimized analyzer validation traversal rather than file
parallelism, SIMD, checksum removal, or another decoder. After one bounded
post-profile refinement, the final clean interleaved A/B retained stereo
performance within noise while reducing median elapsed time by 12.92% at 8
channels and 26.72% at 64 channels, with identical cross-variant result
fingerprints.

The historical M6 evidence did not itself authorize packet/file/window
parallelism, SIMD, unsafe code, or external decoders. A later, separate
[ADR-0014](adr/0014-deterministic-decode-analysis-pipeline.md) now permits
bounded deterministic parallel candidates and makes route-specific packet
decode the first priority; the current product remains serial until each
candidate graduates. Every comparison and any future speed claim must still
follow [ADR-0007](adr/0007-m6-reproducible-performance-baseline.md). The initial
clean-source results and raw samples are recorded in the [M6 scalar-baseline
report](performance/M6_SCALAR_BASELINE_REPORT.md), [M6 sampling-profile
report](performance/M6_SAMPLING_PROFILE_REPORT.md), and [validation-traversal
A/B report](performance/M6_VALIDATION_TRAVERSAL_AB_REPORT.md). The complete
decision chain and final boundaries are in the [M6 closure
report](performance/M6_PERFORMANCE_ENGINEERING_CLOSURE_REPORT.md).
