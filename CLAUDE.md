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
- `codecs` owns content probing and the strict `PcmSource` contract. The
  graduated ALAC and FLAC routes may decode with bounded packet workers; every
  other route reads serially. All stay private behind the same
  `Data / Eof / Error` boundary and commit in input packet order.
- The shared PCM contract is finite interleaved `f64`; do not narrow float64
  sources before analysis.
- `macinmeter::Application` is the only public file-analysis, batch, and
  controlled-discovery façade. Its clones share the execution domain
  established in M3, with one active job and at most 64 queued FIFO
  reservations.
- ADR-0014 packet-level decoding is enabled for the ADR-0013 ALAC route and for
  FLAC streams whose STREAMINFO geometry proves the whole reorder window fits
  the granted reservation; a stream that cannot prove that bound degrades to the
  serial oracle before any thread is created. Decode-analysis overlap graduated
  and is enabled by default; it spends only the permits a route left unspent, so
  a route that spent them all and a one-worker plan behave exactly as before.
  File lanes (P1) graduated at the width the plan derives for itself — the
  widest split that still grants each lane a packet pool, three on an eight-worker
  plan. A single file and a one-item batch still take the whole decoder, because
  the plan is split per operation once the item count is known rather than at
  admission. Window-level analysis (P2) remains unimplemented; do not describe
  it as shipped. Results never depend on worker count or lane count, so a report
  is not evidence of which engine produced it.
- All internal concurrency axes must consume one application-owned worker and
  memory plan; nested file × packet × window pools are forbidden. FLAC graduated
  with ordered full-stream MD5 intact; it may not later be disabled or weakened.
  At the full eight-worker allocation a signed FLAC stream subdivides that same
  plan into seven decoder permits and one ordered signature hasher, which is a
  split of the granted permits, never an extra thread.
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

`performance-probes` is a non-default feature. It adds per-phase timing owned by
the source and worker threads themselves and must stay out of product builds;
the default build carries no probe storage, atomic update, or per-packet timing.
Phase timings are wall intervals that may overlap, not a CPU partition that sums
to elapsed, so they may not be added together or inverted into a serial fraction.

Product builds do carry one coarse decode/analysis split, reached only through
`Application::analyze_file_timed` / `run_batch_timed`, the CLI's `--timing`, and
the GUI's timing toggle. Each measured interval reads the clock at its start and
stop, so an ordinary data block costs two reads per role and four in total. It
is off unless a caller asks; an ordinary run and every ADR-0007 measurement
still read no phase clock. `PhaseTimings` never enters `AnalysisReport` or the
wire envelope, which stay a pure function of the input. For one file, each role
also reports its own span, and `active + other` partitions exactly that one span
— including after presentation rounding. `Other` is unclassified wall time
outside that role's measured calls and must not be presented as a causal
bottleneck attribution. The two spans may overlap each other by an amount that
is **not** recoverable from them, because the total length of two interval sets
does not determine their intersection; recovering it would need a shared atomic
state machine on both hot paths, which is refused. A batch reports sums of
item-level spans, not batch-global first-to-last windows; same-role item spans
may overlap across file lanes. These totals therefore never partition batch
elapsed time. This caller-visible split is not the `performance-probes`
decomposition and does not license reintroducing that decomposition into
product builds.

See `docs/adr/0001-m0-0.2.0-trusted-trunk-rebuild.md`,
`docs/adr/0004-m3-application-execution-budget.md`,
`docs/adr/0006-m5-product-repository-convergence.md`,
`docs/adr/0007-m6-reproducible-performance-baseline.md`,
`docs/adr/0013-mp4-m4a-alac-stable-route.md`,
`docs/adr/0014-deterministic-decode-analysis-pipeline.md`,
`docs/ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md`, and
`reference/specs/foo-dr-meter-1.0.8-candidate-v1.md` before changing architecture
or algorithm behavior.
