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
  measurement, not the later enablement decision: the report preserves which
  graduation gates were still open when that run was recorded; current product
  status is fixed by ADR-0014.
- [`ADR0014_FLAC_PACKET_WORKER_AB_REPORT.md`](ADR0014_FLAC_PACKET_WORKER_AB_REPORT.md)
  records the same-run 1/2/4/8 worker sweep over three long FLAC tracks on a
  Windows x86_64 host, with the ALAC tracks of the same run as an in-suite
  control. It also records the direct measurement of what stays sequential on
  that route, and why the serial fraction implied by a speedup ratio is an upper
  bound rather than a measurement of it.
- [`ADR0014_FLAC_HASHER_AB_REPORT.md`](ADR0014_FLAC_HASHER_AB_REPORT.md)
  records the two source-bound A/B runs that moved FLAC stream hashing off the
  commit/analysis thread. The first run rejects allocating a hasher at two and
  four total permits; the second accepts the bounded 7-decoder + 1-hasher split
  only at the measured eight-permit product allocation.
- [`ADR0014_PACKET_PIPELINE_ATTRIBUTION_REPORT.md`](ADR0014_PACKET_PIPELINE_ATTRIBUTION_REPORT.md)
  directly attributes the scaling gap above the previously measured sequential
  floor. It records source-owned open, decoder, conversion, queue/caller and
  hasher phases, then accepts chunked ALAC sample-table validation and direct
  construction of the final PCM buffer through interleaved Windows A/B runs.
- [`ADR0014_FILE_LANE_WIDTH_REPORT.md`](ADR0014_FILE_LANE_WIDTH_REPORT.md)
  records the allocation-bound file-lane width sweep on macOS arm64 and Windows
  x86_64. Both raw records bind the clean source, worker, suite, corpus,
  environment, every sample and the actual post-discovery allocation.
- [`baselines/`](baselines/) contains the complete runner JSON, including every
  warm-up and measured sample. File names bind the suite, source prefix, and
  target; the JSON binds the full source commit, worker/corpus/suite hashes,
  environment, schedule, raw samples, and summaries.
- [`profiles/`](profiles/) contains source-bound Time Profiler records. The
  committed JSON retains every folded-stack count and binds each ignored raw
  trace/export by SHA-256 and size.
- [`equivalence/`](equivalence/) contains ADR-0014 decode allocation matrices.
  These are correctness records, not timings: every worker count crossed with
  the minimum, plan-derived, and maximum reorder permit, each carrying the
  decoded `f64`, `AnalysisResult` raw-bit, and wire-report fingerprints for one
  long ALAC input. The harness rejects other codecs, verifies the actual engine
  selected for every cell, normalizes the wire display path, and writes the
  canonical sorted four-space JSON itself. Regenerate an exact record with
  `cargo run --locked --release -p macinmeter --example adr0014_allocation_matrix -- PATH > OUTPUT.json`.
- [`probes/`](probes/) contains both sequential-floor records and explicit
  source-owned pipeline attribution records. The former measure sequential
  demux with no decoding and the cost of hashing an exact-size stream signature;
  regenerate one with `cargo run --locked --release -p macinmeter-codecs
  --example demux_cost_probe -- PATH`. The latter require the non-default
  `performance-probes` feature and bind each source/thread phase to the ordinary
  decode controls in the same suite.
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
route-specific packet decode the first parallel candidate, followed by file- and
window-level work. It does not turn the historical M6 records into a parallelism
claim: every candidate needs a new source-bound record under the same ADR-0007
protocol, and a route stays serial until it graduates on its own evidence. The
ALAC and FLAC packet routes, decode-analysis overlap, and file lanes have
graduated; window-level work has not.

Records are bound to one host. A speedup measured on one machine says nothing
about another, so a report compares only figures from within its own run — which
is why both packet-route reports carry the other route's tracks as an in-suite
control.

Generated media remain under ignored `target/performance-corpus`; only their
deterministic generator and manifest identity enter a record. Large Xcode trace
bundles remain under ignored `target/performance-profiles`; profile conclusions
must be reproducible from the committed folded-stack evidence.
