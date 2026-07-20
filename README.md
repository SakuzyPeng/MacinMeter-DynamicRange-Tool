# MacinMeter DR Tool

[English](README.md) | [中文](README_CN.md)

MacinMeter is an independent, local-first audio dynamic-range (DR) analysis
tool. It measures per-channel and per-track DR values from WAV, FLAC, and AIFF
files, following a candidate reconstruction of the foobar2000 DR Meter 1.0.8
algorithm, and ships one safe, streaming Rust core shared by the library, the
CLI, and the Tauri GUI.

> **Compatibility status: `foo_dr_meter 1.0.8 Candidate V1 / Unverified`.**
> The current profile implements a candidate interpretation of evidence
> gathered from foo_dr_meter 1.0.8 x64. The bounded M4 direct-PCM conformance
> milestone is complete, but this does not establish arbitrary-input or full
> foobar/component compatibility. Values must not be described as “official,”
> certified, or interchangeable with reference results.

## Highlights

- **One analysis core.** File analysis in the library, CLI, and GUI reaches the
  same `AnalyzerSession` through the `Application` façade; direct streaming
  callers use that same session type, so adapters cannot fork algorithm
  behavior.
- **Streaming and bounded.** Analysis is windowed and histogram-based; memory
  grows with channel count, not stream length.
- **Safe by construction.** Every first-party crate uses
  `#![forbid(unsafe_code)]`; success reports are built only through checked
  constructors that cannot represent non-finite values.
- **Evidence-first claims.** The reference profile, its specification, and
  conformance records live in-repo and bind fixed target, corpus, and artifact
  identities; claims never exceed the recorded evidence.
- **Measured, not promised, performance.** The scalar core has a reproducible
  local baseline, sampling attribution, and one bit-exact-gated optimization
  chain; no cross-machine throughput promise is made.

## Trusted surface (0.2.0)

| Container | Accepted encodings |
| --- | --- |
| classic RIFF/WAVE | 8/16/24/32-bit integer PCM; IEEE 32/64-bit float |
| FLAC (native container) | FLAC with a declared nonzero total sample count |
| AIFF | 8/16/24/32-bit integer PCM |

Everything is probed by content; extensions matter only for directory
discovery. Serial decoding, serial batch execution, and a 64-channel product
analysis limit are deliberate. WAVE_FORMAT_EXTENSIBLE, AIFC, MP3, AAC, ALAC,
Vorbis, Opus, and DSD are outside the 0.2.0 stable surface; recognized but
unavailable media returns `unsupported_format`. FFmpeg backends,
preprocessing, packet-level parallelism, and SIMD execution paths are absent
rather than configurable. See
[supported formats](docs/SUPPORTED_FORMATS.md) for exact route limits.

## Quick start

Rust 1.88 or later and Cargo are required.

```bash
cargo build --locked --release -p macinmeter-cli
target/release/macinmeter analyze track.flac
target/release/macinmeter batch Album/ --recursive --format json
```

### CLI

The CLI has no implicit modes: it never scans directories unless asked and
never writes report files unless `--output` is supplied.

```bash
macinmeter analyze FILE [--format human|json] [--output PATH]
macinmeter batch INPUT... [--recursive] [--format human|json] [--output PATH]
```

Standard output carries only the requested result; progress and diagnostics go
to standard error. Output files are replaced atomically. `batch` returns
independent per-track reports and performs no album aggregation.

| Exit code | Meaning |
|---:|---|
| `0` | all requested analyses succeeded |
| `1` | failure, no input, or output-write failure |
| `2` | invalid CLI arguments |
| `3` | batch completed with both successes and failures |
| `130` | cancelled |

### JSON

JSON and the Tauri GUI share the same versioned schema-v3 envelope. Abridged
analysis example:

```json
{
  "schemaVersion": 3,
  "toolVersion": "0.2.0",
  "kind": "analysis",
  "data": {
    "analysis": {
      "channels": [{
        "report": {
          "overallRmsLinear": 0.5,
          "overallRmsDbfs": -6.0206,
          "primaryPeakLinear": 1.0
        },
        "outcome": {
          "status": "measured",
          "measurement": {
            "loudWindowRms": 0.25,
            "drSelectedPeak": 0.5,
            "drPrimaryPeak": 1.0,
            "drSecondaryPeak": 0.5
          }
        }
      }],
      "report": {
        "overallRmsLinear": 0.5,
        "overallRmsDbfs": -6.0206,
        "primaryPeakLinear": 1.0,
        "primaryPeakDbfs": 0.0,
        "duration": { "decodedFrames": 48000, "sampleRate": 48000 }
      }
    }
  }
}
```

Report metrics and DR-state diagnostics are deliberately separate:
`loudWindowRms`, `drSelectedPeak`, `drPrimaryPeak`, and the nullable
`drSecondaryPeak` describe the DR state machine and must not be substituted
for report metrics. `FiniteF32`/`FiniteF64` wrappers make non-finite report
values unrepresentable; zero-amplitude dBFS values are explicit `null`;
`DecodedDuration` preserves the exact decoded-frame/sample-rate pair instead
of a rounded seconds value.

### GUI

The Tauri 2 frontend uses exactly the same application façade and wire schema
as the CLI:

```bash
cd tauri-app
npm install
npm run tauri dev
```

Each GUI job owns an independent cancellation token and reserves the shared
application budget before entering the blocking runtime; queued cancellation
does not affect the active job. The GUI never configures FFmpeg, mutates
process environment variables, or runs a separate batch engine.

## Library

The public façade is the `macinmeter` crate:

```rust
use macinmeter::{AnalyzeRequest, Application};

fn main() -> Result<(), macinmeter::AnalysisError> {
    let application = Application::new();
    let _report = application.analyze_file(AnalyzeRequest::new("track.flac"))?;
    Ok(())
}
```

Clones of one `Application` share a bounded FIFO execution domain: one active
top-level analyze, batch, or discovery job and at most 64 queued jobs. This
keeps CLI/Tauri execution serial without a hidden process-global singleton.

Lower-level analysis is available through a frame-aligned streaming session:

```rust
use macinmeter::{AnalysisProfile, AnalyzerSession, ChannelLayout, StreamSpec};

fn main() -> Result<(), macinmeter::AnalysisError> {
    let spec = StreamSpec::new(48_000, 2, ChannelLayout::Unknown)?;
    let mut session =
        AnalyzerSession::new(spec, AnalysisProfile::FooDrMeter108CandidateV1)?;
    session.push_interleaved(&[0.25, -0.25, 0.5, -0.5])?;
    let _result = session.finish()?;
    Ok(())
}
```

Samples must be finite interleaved `f64`; `finish` consumes the session and is
fallible so numeric or resource failures cannot leak non-finite output.
Successful `AnalysisResult`/`AnalysisReport` roots are immutable and inspected
through read-only getters; result, report, and shared batch/event/wire types
that are not product inputs serialize but do not deserialize.

Album aggregation is an explicit library operation, never an implicit property
of a batch:

```rust
use macinmeter::{
    AlbumAggregator, AlbumTrackMetrics, AlbumWeighting, AnalyzeRequest, Application,
};

fn main() -> Result<(), macinmeter::AnalysisError> {
    let report = Application::new().analyze_file(AnalyzeRequest::new("track.flac"))?;
    let track = AlbumTrackMetrics::try_from(&report)?;
    let _album = AlbumAggregator::aggregate(&[track], AlbumWeighting::Unweighted)?;
    Ok(())
}
```

The unweighted album value is the arithmetic mean of public-f32 track DR
values, including numeric DR0 tracks; optional duration weighting uses each
track's exact decoded duration. This numeric API does not claim playlist
grouping, footer, or other album-subsystem parity.

## Conformance evidence

Decoders normalize supported inputs to finite interleaved `f64`, matching the
fixed x64 core's PCM width. Against the fixed
`foo_dr_meter 1.0.8 x64` target (`ff3556ad…`), the recorded evidence is:

| Evidence | Result |
| --- | --- |
| 39-track schema-v3 safe-master run: track DR / overall peak / overall RMS / rendered duration | 39/39 each |
| same run: channel DR / channel RMS | 62/62 each |
| M4 decoder-independent direct-PCM Candidate conformance | 0 differences on the fixed 39-input final-field projection |
| 39-input isolated x64 analyzer-core observation (no foobar2000 started) | all preregistered assertions met |
| 38-vector isolated numeric boundaries: duration half-second/carry, optional multichannel loudness weighting, histogram clamp endpoints | 24/24, 8/8, 6/6 |

The reference footer's track count, sample-rate set, channel-count set, and
`DR12` token are also consistent with the implementation reports. Host
behavior, playlist grouping, metadata provenance, complete text parity,
internal implementation-state parity, and arbitrary audio remain outside the
claim, which is why the profile stays `Unverified`. The exact scope and limits
live in the
[M4 evidence matrix](docs/M4_X64_NUMERIC_CLAIM_MATRIX.md),
the [M4 conformance report](docs/M4_X64_NUMERIC_COMPATIBILITY_REPORT.md), and
the [candidate specification](reference/specs/foo-dr-meter-1.0.8-candidate-v1.md).

## Performance

0.2.0 publishes no performance guarantee. M6 instead established a
reproducible local measurement protocol: a deterministic generated corpus, a
15-case scalar baseline, sampling attribution, and same-run interleaved A/B
comparisons in which every variant must first reproduce bit-identical results.
One analyzer-validation optimization chain passed that gate and is in the
product; on the fixed baseline host the stereo difference remained within
measurement noise, while 8-/64-channel median elapsed time fell by roughly 13%
and 27%. Those numbers describe one fixed machine, toolchain, and synthetic
workload — they are evidence for engineering decisions, not user-facing
throughput claims.

Reproduce or extend the measurements with:

```bash
python3 scripts/generate-performance-corpus.py
python3 scripts/run-performance-baseline.py
```

See the [performance measurement contract](docs/BENCHMARKS.md) and the
[M6 reports](docs/performance/README.md).

## Verification

Local gates, in increasing resource risk:

```bash
python3 scripts/check-repository-contract.py
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
python3 -m unittest discover -s reference/tools/tests -p 'test_*.py'
cd tauri-app && npm run build
```

Hostile-input verification is deliberately isolated: the committed
41-case malformed-media corpus runs per-case in subprocesses with a wall-clock
timeout and a Linux address-space limit
(`python3 scripts/verify-malformed-corpus.py`), and refuses to decode hostile
bytes when the limit cannot be enforced. Remote CI is intentionally
`workflow_dispatch` only. Verified local release staging (checksums, CLI smoke
test, optional unsigned macOS DMG) is documented in the
[release artifact contract](docs/RELEASE.md).

## Architecture

The repository is a virtual Cargo workspace with one-way dependencies:

```text
macinmeter-domain
├── macinmeter-analysis
├── macinmeter-codecs
└── macinmeter (application façade)
    ├── macinmeter-cli
    └── macinmeter-gui
```

`domain` owns valid types and errors; `analysis` owns the only streaming
analyzer; `codecs` owns probing and strict PCM sources plus the single native
capability catalog; the application layer is the only place that composes
decoding and analysis; CLI and GUI only parse, render, and adapt I/O.

The 0.2.0 rebuild was executed as seven reviewed milestones, each closed by an
architecture decision record:

| Milestone | Decision record |
| --- | --- |
| M0 — trusted-trunk rebuild | [ADR-0001](docs/adr/0001-m0-0.2.0-trusted-trunk-rebuild.md) |
| M1 — reference numeric scope | [ADR-0002](docs/adr/0002-m1-reference-numeric-scope.md) |
| M2 — native decoder contract hardening | [ADR-0003](docs/adr/0003-m2-native-decoder-contract-hardening.md) |
| M3 — application execution budget | [ADR-0004](docs/adr/0004-m3-application-execution-budget.md) |
| M4 — bounded x64 numeric claim | [ADR-0005](docs/adr/0005-m4-bounded-x64-numeric-claim.md) |
| M5 — product/repository convergence | [ADR-0006](docs/adr/0006-m5-product-repository-convergence.md) |
| M6 — reproducible performance baseline | [ADR-0007](docs/adr/0007-m6-reproducible-performance-baseline.md) |

The living overview is the
[architecture and reference-alignment roadmap](docs/ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md).

## Reference work and attribution

The current reference target is foobar2000 DR Meter 1.0.8 (`foo_dr_meter`) by
Janne Hyvärinen. Permission to reverse engineer the plugin has been obtained
from its author. Private permission correspondence is not stored in this
repository; only a
[minimal public scope summary](reference/authorization/README.md) is retained.

That permission and attribution do not establish numerical compatibility.
Target hashes, experiments, observations, the candidate specification, and all
conformance records are kept under [`reference/`](reference/README.md), each
with its declared scope and limits — including the
[isolated x64 analyzer-core harness](reference/observations/CORE_HARNESS.md)
that exercises the fixed target without starting foobar2000. The profile
remains `Unverified` unless a later, separately reviewed compatibility claim
justifies a stronger statement.

## License

MacinMeter is released under the [MIT License](LICENSE). See
[legal notes](docs/LEGAL.md) and [third-party notices](THIRD_PARTY_NOTICES.md).
