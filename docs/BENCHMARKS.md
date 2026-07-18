[English](BENCHMARKS.md) | [中文](BENCHMARKS_CN.md)

# Performance status

MacinMeter 0.2.0 has no published performance claim yet.

The benchmark numbers previously stored here measured the removed 0.1.x
packet-parallel, file-parallel, SIMD, and FFmpeg/DSD paths. They do not describe
the M0 architecture and cannot be used as a correctness or throughput
commitment.

M0 intentionally establishes a safe scalar, serial baseline. A new benchmark
suite will be introduced only after:

- the workspace contracts and reference-facing result semantics are stable;
- datasets, commands, environment, and binary hashes are recorded;
- timing distinguishes discovery, decoding, analysis, and rendering;
- peak memory includes the complete process tree where applicable;
- comparisons use randomized/interleaved runs rather than a fixed warm-up order.

Future optimization will first consider one application-owned file-level
parallelism axis with a shared resource budget. Packet-level parallelism, SIMD,
and external decoder processes are not implied by this document.
