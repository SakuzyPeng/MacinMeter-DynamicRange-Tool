# Performance evidence

This directory contains reviewed, source-bound performance records. It is not a
leaderboard or a user-facing throughput guarantee.

- [`M6_SCALAR_BASELINE_REPORT.md`](M6_SCALAR_BASELINE_REPORT.md) interprets the
  initial scalar baseline and defines the profiling targets.
- [`M6_SAMPLING_PROFILE_REPORT.md`](M6_SAMPLING_PROFILE_REPORT.md) attributes
  the analyzer and FLAC scopes and selects the first bounded optimization
  candidate.
- [`M6_VALIDATION_TRAVERSAL_AB_REPORT.md`](M6_VALIDATION_TRAVERSAL_AB_REPORT.md)
  records the bit-exact, source-bound interleaved A/B that accepted the first
  analyzer candidate.
- [`M6_VALIDATION_POST_PROFILE_REPORT.md`](M6_VALIDATION_POST_PROFILE_REPORT.md)
  attributes the accepted high-channel path and bounds one final refinement.
- [`M6_PERFORMANCE_ENGINEERING_CLOSURE_REPORT.md`](M6_PERFORMANCE_ENGINEERING_CLOSURE_REPORT.md)
  records the refinement and scalar-to-final A/B runs and closes active M6
  optimization.
- [`ADR0014_ALAC_PACKET_WORKER_AB_REPORT.md`](ADR0014_ALAC_PACKET_WORKER_AB_REPORT.md)
  records the same-run 1/2/4/8 worker sweep over a long ALAC track. It is a
  measurement, not an enablement decision: packet workers remain off by default
  and the report states which graduation gates are still open.
- [`baselines/`](baselines/) contains the complete runner JSON, including every
  warm-up and measured sample. File names bind the suite, source prefix, and
  target; the JSON binds the full source commit, worker/corpus/suite hashes,
  environment, schedule, raw samples, and summaries.
- [`profiles/`](profiles/) contains source-bound Time Profiler records. The
  committed JSON retains every folded-stack count and binds each ignored raw
  trace/export by SHA-256 and size.
- [`comparisons/`](comparisons/) contains complete interleaved A/B runner
  records, including every warm-up, measured sample, variant identity, and
  cross-variant fingerprint.

Do not compare records by filename or median alone. A performance comparison is
valid only when it follows
[`ADR-0007`](../adr/0007-m6-reproducible-performance-baseline.md): identical
result/PCM fingerprints, compatible work units, and all variants interleaved in
one run on the same machine.

Accepted
[`ADR-0014`](../adr/0014-deterministic-decode-analysis-pipeline.md) makes
route-specific packet decode the first future parallel candidate, followed by
file- and window-level work. It does not turn the historical M6 records into a
parallelism claim: every candidate needs a new source-bound record under the
same ADR-0007 protocol, and the current production implementation remains
serial until graduation.

Generated media remain under ignored `target/performance-corpus`; only their
deterministic generator and manifest identity enter a record. Large Xcode trace
bundles remain under ignored `target/performance-profiles`; profile conclusions
must be reproducible from the committed folded-stack evidence.
