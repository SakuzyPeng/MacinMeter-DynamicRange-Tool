# MacinMeter contributor context

MacinMeter 0.2.0 is a breaking, correctness-first rebuild. The former 0.1.x
decoder, dual DR engines, packet/file parallelism, SIMD/unsafe conversion,
EdgeTrimmer, FFmpeg/DSD, Songbird, implicit CLI modes, and duplicate GUI DTOs
have been removed. Do not reintroduce them as compatibility helpers.

The current result profile is `FooDrMeter108CandidateV1` with compatibility
status `Unverified`. It follows the versioned candidate specification backed by
the recorded 1.0.8 target hashes, static analysis, fixed x86/x64 observations,
and scoped conformance records under `reference/`; it is not a claim of
accepted conformance. No generic “small floating-point tolerance” or
compatibility percentage is valid.

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
- `codecs` owns content probing and strict sequential PCM sources.
- The shared PCM contract is finite interleaved `f64`; do not narrow float64
  sources before analysis.
- `macinmeter::Application` is the only public file-analysis, batch, and
  controlled-discovery façade. Its clones share an M3 execution domain with
  one active job and at most 64 queued FIFO reservations.
- Tauri reserves an `ApplicationJob` before `spawn_blocking`; queued
  cancellation and RAII release are part of the application contract.
- CLI and GUI only parse, render, and adapt I/O.

All first-party Rust code forbids unsafe code. M0 supports only WAV linear
integer/IEEE float PCM, FLAC, and AIFF integer PCM. Unknown layout stays
unknown; it is never guessed from channel count.

## Verification

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace

cd tauri-app
npm run build
```

GitHub Actions is intentionally `workflow_dispatch` only during M3. Local
pre-commit performs format and workspace compile checks without network audit.

See `docs/adr/0001-m0-0.2.0-trusted-trunk-rebuild.md`,
`docs/adr/0004-m3-application-execution-budget.md`,
`docs/ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md`, and
`reference/specs/foo-dr-meter-1.0.8-candidate-v1.md` before changing architecture
or algorithm behavior.
