# MacinMeter contributor context

MacinMeter 0.2.0 is a breaking, correctness-first rebuild. The former 0.1.x
decoder, dual DR engines, packet/file parallelism, SIMD/unsafe conversion,
EdgeTrimmer, FFmpeg/DSD, Songbird, implicit CLI modes, and duplicate GUI DTOs
have been removed. Do not reintroduce them as compatibility helpers.

The current result profile is `ProvisionalV1` with compatibility status
`Unverified`. Known reference-plugin deviations were systemic, so no generic
“small floating-point tolerance” or compatibility percentage is valid. Future
reference work must start from recorded target hashes, experiments, observations,
and conformance evidence under `reference/`.

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
- `macinmeter` composes decoding, analysis, discovery, cancellation, progress,
  serial batch execution, and the shared wire envelope.
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

GitHub Actions is intentionally `workflow_dispatch` only during M0. Local
pre-commit performs format and workspace compile checks without network audit.

See `docs/adr/0001-m0-0.2.0-trusted-trunk-rebuild.md`,
`docs/ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md`, and
`reference/specs/provisional-v1.md` before changing architecture or algorithm
behavior.
