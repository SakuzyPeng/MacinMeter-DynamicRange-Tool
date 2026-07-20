# Performance evidence

This directory contains reviewed, source-bound performance records. It is not a
leaderboard or a user-facing throughput guarantee.

- [`M6_SCALAR_BASELINE_REPORT.md`](M6_SCALAR_BASELINE_REPORT.md) interprets the
  initial scalar baseline and defines the next profiling targets.
- [`baselines/`](baselines/) contains the complete runner JSON, including every
  warm-up and measured sample. File names bind the suite, source prefix, and
  target; the JSON binds the full source commit, worker/corpus/suite hashes,
  environment, schedule, raw samples, and summaries.

Do not compare records by filename or median alone. A performance comparison is
valid only when it follows
[`ADR-0007`](../adr/0007-m6-reproducible-performance-baseline.md): identical
result/PCM fingerprints, compatible work units, and all variants interleaved in
one run on the same machine.

Generated media remain under ignored `target/performance-corpus`; only their
deterministic generator and manifest identity enter a record.
