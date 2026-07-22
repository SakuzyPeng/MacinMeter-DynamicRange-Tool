# Repository Guidelines

## Architecture

This repository is a virtual Cargo workspace targeting version 0.2.0:

- `crates/macinmeter-domain` — valid domain types, reports, and stable errors
- `crates/macinmeter-analysis` — the sole fixed-rule streaming analyzer
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

## Trusted trunk and post-M3 constraints

- Every first-party Rust crate uses `#![forbid(unsafe_code)]`.
- Production analysis has one `AnalyzerSession`; do not add a compatibility
  engine or a selectable profile.
- Valid PCM blocks and the analyzer boundary use finite interleaved `f64`;
  source float64 samples must not be narrowed before analysis.
- Stable product analysis accepts at most 64 channels. Preserve broader source
  metadata types, but reject over-limit media before decoder creation and
  over-limit direct sessions before allocation.
- Decoding remains serial. WAV integer/float PCM, FLAC, and AIFF integer PCM
  remain the only stable routes until another in-process Symphonia route
  satisfies ADR-0003's capability graduation contract.
- `Application` is the only public file-analysis, batch, and controlled
  discovery façade. Keep `Analyzer`/`BatchRunner` crate-private; adapters must
  not bypass the shared execution domain.
- The product budget established in M3 remains one active top-level job with at
  most 64 queued reservations. Tauri must reserve `ApplicationJob` before
  `spawn_blocking`; queued cancellation and RAII release are contract behavior.
- Completed M3 did not add a second backend, FFmpeg, DSD, Songbird/Opus,
  Tokio/Rayon scheduling, SIMD, trimming, silence preprocessing, or file-level
  parallelism. Any such change requires a separate evidence-backed decision.
- Analysis reports contain the fixed numeric parameters needed for
  reproducibility. Internal profile names and compatibility status are not
  report fields; do not serialize or render them, and do not claim reference
  parity beyond the recorded scope.
- M4 validates the fixed x64 numeric rules through finite interleaved `f64`
  and `AnalyzerSession`, independently of product decoder support. Do not
  graduate WAVE_FORMAT_EXTENSIBLE or another codec route merely to replay the
  reference corpus.
- Add a new reference observation only for an in-scope unexplained final-output
  residual that static analysis and existing isolated evidence cannot decide.
- Extensions are discovery hints only. Decoder errors must not become EOF or
  partial successful reports.
- Library/application code does not print or write files. CLI/Tauri are
  adapters over the shared façade and `WireEnvelope`.
- M5 centralizes every direct third-party Rust dependency in the root
  `[workspace.dependencies]`; member manifests use `.workspace = true`.
  Package identity, the two lockfiles, GUI version mirrors, and the bounded
  pull-request/main/manual workflow triggers are enforced by
  `scripts/check-repository-contract.py`.
- Ordinary GUI build/dev commands only check version mirrors. Version changes
  are written only by the explicit `npm run sync-version` command.
- Release staging must start clean unless it is explicitly marked dirty. It
  verifies the extracted CLI and current-host DMG bytes plus SHA-256. The
  macOS arm64 main CI gate may run the same contract as ephemeral validation.
  Manual dispatch may retain one clean unsigned Apple Silicon candidate for
  14 days, but it never signs, notarizes, creates a tag/Release, or implies
  Gatekeeper readiness.
- The 0.2.0 packaged GUI targets only `aarch64-apple-darwin` with macOS 11.0 as
  its minimum system version. Do not add Intel, universal, Windows, or Linux
  GUI artifacts without a separate target-bound decision.
- M6 performance evidence uses the release `m6_baseline_worker` and
  `scripts/run-performance-baseline.py`. Formal runs start clean, bind source,
  binary, suite, corpus, toolchain, environment, and raw samples, and require
  exact result/PCM fingerprints before comparison. Benchmarks are explicit
  local tasks, never ordinary test/CI gates or cross-host performance claims.
- Completed M6 keeps validation geometry-sensitive: 1–4 channels use the
  channel-major path; 5–64 channels use frame-major transactional shadows and
  replay the immutable channel-major inspector only for numeric-error
  precedence. Do not merge validation with commit or add SIMD, unsafe,
  parallelism, or a second backend without a new source-bound decision.

## Commands

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --release -p macinmeter-cli
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
python3 -m unittest discover -s reference/tools/tests -p 'test_*.py'

cd tauri-app
npm install
npm run build
npm run tauri dev
```

Remote CI runs bounded Ubuntu 24.04, Windows Server 2025 x64, and macOS 26
arm64 jobs for pull requests and pushes to `main`. Main/manual runs add the
Windows release CLI smoke; main runs ephemeral macOS staging, while manual
dispatch from `main` retains the unsigned Apple Silicon candidate for 14 days
and adds the Linux release build. No CI path creates a tag or GitHub Release.
Do not trigger, rerun, or wait for remote CI as part of ordinary development
unless the user requests it or its result is required for the current GitHub
operation.

Local artifact staging is separate from ordinary verification:

```bash
python3 scripts/stage-release.py stage
# macOS current-host GUI, still unsigned/unnotarized:
python3 scripts/stage-release.py stage --include-gui
```

Performance measurement is also explicit and separate:

```bash
python3 scripts/generate-performance-corpus.py
python3 scripts/run-performance-baseline.py
```

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
