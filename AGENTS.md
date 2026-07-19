# Repository Guidelines

## Architecture

This repository is a virtual Cargo workspace targeting version 0.2.0:

- `crates/macinmeter-domain` — valid domain types, reports, and stable errors
- `crates/macinmeter-analysis` — the sole `FooDrMeter108CandidateV1` streaming analyzer
- `crates/macinmeter-codecs` — strict in-process PCM sources; WAV/FLAC/AIFF
  are currently the only stable routes
- `crates/macinmeter` — application façade, discovery, batch, control, wire DTO
- `apps/macinmeter-cli` — CLI adapter and renderers
- `tauri-app/src-tauri` — Tauri adapter; frontend is under `tauri-app/src`
- `reference` — fixed targets, observations, static analysis, specifications,
  and conformance records

Dependencies flow from adapters through `macinmeter` to `analysis`/`codecs`,
which both depend on `domain`. Do not introduce frontend, filesystem, or codec
dependencies into lower layers.

## Trusted trunk and M2 constraints

- Every first-party Rust crate uses `#![forbid(unsafe_code)]`.
- Production analysis has one `AnalyzerSession`; do not add a compatibility
  engine or legacy profile.
- Valid PCM blocks and the analyzer boundary use finite interleaved `f64`;
  source float64 samples must not be narrowed before analysis.
- Stable product analysis accepts at most 64 channels. Preserve broader source
  metadata types, but reject over-limit media before decoder creation and
  over-limit direct sessions before allocation.
- Decoding remains serial. WAV integer/float PCM, FLAC, and AIFF integer PCM
  remain the only stable routes until another in-process Symphonia route
  satisfies ADR-0003's capability graduation contract.
- M2 does not add a second backend, FFmpeg, DSD, Songbird/Opus, Tokio/Rayon
  scheduling, SIMD, trimming, or silence preprocessing.
- Results are always `FooDrMeter108CandidateV1 / Unverified`; never claim
  reference parity.
- Extensions are discovery hints only. Decoder errors must not become EOF or
  partial successful reports.
- Library/application code does not print or write files. CLI/Tauri are
  adapters over the shared façade and `WireEnvelope`.

## Commands

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --release -p macinmeter-cli

cd tauri-app
npm install
npm run build
npm run tauri dev
```

Remote CI remains manual-only during M2. Do not trigger or wait for it as part of
ordinary development.

## Style and tests

- Rust 2024, MSRV 1.88, rustfmt defaults, zero Clippy warnings.
- Prefer valid constructors and structured `AnalysisError` over panics outside
  tests.
- Keep tests deterministic, local, and independent of FFmpeg or network access.
- Algorithm changes require chunk-boundary, window-boundary, multichannel, and
  finite-JSON coverage plus an update to
  `reference/specs/foo-dr-meter-1.0.8-candidate-v1.md`.
- Codec claims require the codec-level ADR-0003 contract matrix: content probe,
  immutable stream info, block/spec geometry, sticky EOF, frame-count/progress,
  route-specific malformed inputs, and a finite normalization oracle. Each
  concrete `PcmSource` implementation separately requires a deterministic
  fault-injection test for sticky terminal errors. Application and adapters
  require separate integration coverage at their own boundaries.
- CLI changes require black-box stdout/stderr, JSON, output-file, and exit-code
  tests.

Use Conventional Commit prefixes such as `feat:`, `fix:`, `refactor:`, `test:`,
and `docs:`.
