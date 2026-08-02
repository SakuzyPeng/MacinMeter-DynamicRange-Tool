# MacinMeter contributor context

MacinMeter 0.3.0 continues the correctness-first trunk rebuilt in 0.2.0. The former 0.1.x
decoder, dual DR engines, untrusted packet/file parallel implementations,
SIMD/unsafe conversion, EdgeTrimmer, FFmpeg/DSD, Songbird, implicit CLI modes,
and duplicate GUI DTOs have been removed. Do not reintroduce them as
compatibility helpers. New parallel work must follow the clean-room,
route-specific ADR-0014 contract.

The product has one fixed analysis algorithm backed by the versioned
specification, recorded 1.0.8 target hashes, static analysis, fixed x86/x64
observations, and scoped conformance records under `reference/`. A report
contains its fixed numeric parameters, not an internal profile name or
compatibility status. Claims must instead name their evidence scope. No generic
“small floating-point tolerance” or compatibility percentage is valid.

## Workspace

```text
domain
├── analysis
├── codecs
└── macinmeter application façade
    ├── CLI
    └── Tauri GUI
```

- `domain` owns valid stream/source/report/error types.
- `analysis` owns the only frame-streaming `AnalyzerSession`.
- `codecs` owns content probing and the strict `PcmSource` contract. The current
  implementation reads serially; future route-specific packet workers remain
  private behind the same `Data / Eof / Error` boundary.
- The shared PCM contract is finite interleaved `f64`; do not narrow float64
  sources before analysis.
- `macinmeter::Application` is the only public file-analysis, batch, and
  controlled-discovery façade. Its clones share the execution domain
  established in M3, with one active job and at most 64 queued FIFO
  reservations.
- Current decoding and batch execution remain serial. ADR-0014 has accepted
  bounded packet-, file-, and window-level parallelism as future work, with
  packet decode as P0 and constrained ALAC as the first slice. Do not describe
  this decision as an implemented 0.3.0 capability.
- All internal concurrency axes must consume one application-owned worker and
  memory plan; nested file × packet × window pools are forbidden. FLAC packet
  workers may not graduate by disabling or weakening ordered full-stream MD5.
- Tauri reserves an `ApplicationJob` before `spawn_blocking`; queued
  cancellation and RAII release are part of the application contract.
- CLI and GUI only parse, render, and adapt I/O.
- M4 conformance drives `AnalyzerSession` from controlled finite interleaved
  `f64` independently of product codec support. Do not restore a codec route to
  make a reference fixture pass.

All first-party Rust code forbids unsafe code. The 0.3.0 stable surface supports
WAV linear integer/IEEE float PCM, FLAC, AIFF integer PCM, and the ADR-0013
constrained MP4/M4A + ALAC route. Unknown layout stays unknown; it is never
guessed from channel count.

## Verification

```bash
python3 scripts/check-repository-contract.py
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets
cargo build --locked --release -p macinmeter-cli
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
python3 -m unittest discover -s reference/tools/tests -p 'test_*.py'

cd tauri-app
npm run build
```

GitHub Actions runs bounded validation for pull requests and `main`, with an
explicit manual path for the retained unsigned candidate. Local pre-commit
performs the repository contract, format, and workspace compile checks without
network audit.

Release staging is a separate local operation:

```bash
python3 scripts/stage-release.py stage
# current-host macOS DMG, explicitly local and unsigned/unnotarized
python3 scripts/stage-release.py stage --include-gui
```

M6 performance measurement is another explicit local operation. It uses a
generated, untracked corpus and refuses dirty formal runs:

```bash
python3 scripts/generate-performance-corpus.py
python3 scripts/run-performance-baseline.py
```

Do not turn elapsed time or RSS into ordinary test/CI thresholds. Optimization
claims require exact result/PCM fingerprints and a same-run interleaved A/B as
defined by ADR-0007.

See `docs/adr/0001-m0-0.2.0-trusted-trunk-rebuild.md`,
`docs/adr/0004-m3-application-execution-budget.md`,
`docs/adr/0006-m5-product-repository-convergence.md`,
`docs/adr/0007-m6-reproducible-performance-baseline.md`,
`docs/adr/0013-mp4-m4a-alac-stable-route.md`,
`docs/adr/0014-deterministic-decode-analysis-pipeline.md`,
`docs/ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md`, and
`reference/specs/foo-dr-meter-1.0.8-candidate-v1.md` before changing architecture
or algorithm behavior.
