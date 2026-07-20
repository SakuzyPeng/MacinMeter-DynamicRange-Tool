# MacinMeter DR Tool

[English](README.md) | [中文](README_CN.md)

MacinMeter is an independent, local-first audio dynamic-range analysis project.
Version 0.2.0 rebuilds the project around one safe, streaming Rust core shared by
the library, CLI, and Tauri GUI.

> **Compatibility status: `foo_dr_meter 1.0.8 Candidate V1 / Unverified`.**
> The current profile implements a candidate interpretation of evidence gathered
> from foo_dr_meter 1.0.8. The bounded M4 direct-PCM conformance milestone is
> complete, but this does not establish arbitrary-input or full
> foobar/component compatibility.
> Values must not be described as “official,” certified, or interchangeable with
> reference results.

## 0.2.0 trusted surface

The 0.2.0 baseline deliberately keeps a small trusted surface:

- classic RIFF/WAVE: 8/16/24/32-bit integer PCM and IEEE 32/64-bit float
- FLAC
- AIFF: 8/16/24/32-bit integer PCM
- serial decoding and serial batch execution
- streaming, bounded-memory analysis
- a 64-channel product analysis limit (individual formats/backends may be lower)
- one `FooDrMeter108CandidateV1` profile
- structured errors, cancellation, progress, and versioned JSON

Input is probed by content. File extensions are used only while discovering
files in directories. WAVE_FORMAT_EXTENSIBLE, AIFC, MP3, AAC, ALAC, Vorbis,
Opus, FFmpeg routes, DSD, preprocessing, packet-level parallelism, and SIMD
paths are not part of the 0.2.0 stable surface and return `unsupported_format`
when encountered.

## Build and test

Rust 1.88 or later and Cargo are required.

```bash
cargo build --locked --workspace
cargo test --locked --workspace
cargo build --locked --release -p macinmeter-cli
```

The release CLI is written to `target/release/macinmeter`.

Verified local staging packages the extracted CLI smoke test and SHA-256
manifests without uploading or signing:

```bash
python3 scripts/stage-release.py stage
```

On macOS, `--include-gui` additionally builds and structurally verifies the
current-host DMG. Current GUI staging is local-only and
unsigned/unnotarized. See [the release artifact contract](docs/RELEASE.md).

## CLI

The CLI has no implicit directory scan and never saves reports unless
`--output` is supplied.

```bash
macinmeter analyze FILE [--format human|json] [--output PATH]
macinmeter batch INPUT... [--recursive] [--format human|json] [--output PATH]
```

Human `analyze` output includes channel overall RMS plus track report peak/RMS.
`batch` returns independent per-track reports and does not perform album
aggregation.

Standard output contains only the requested result. Progress and diagnostics go
to standard error. Output files are replaced atomically through a temporary
file in the destination directory.

Exit codes:

| Code | Meaning |
|---:|---|
| `0` | all requested analyses succeeded |
| `1` | failure, no input, or output-write failure |
| `2` | invalid CLI arguments |
| `3` | batch completed with both successes and failures |
| `130` | cancelled |

JSON and Tauri use the same schema-v3 envelope. This abridged analysis example
shows the report/diagnostic split:

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

The payload contains no timestamp. `FiniteF32`/`FiniteF64` wrappers make
non-finite report values unrepresentable; zero-amplitude dBFS values are
explicit `null`. Each channel has independent public-f32 overall RMS and
primary-peak report metrics. Track RMS follows the reference public-f32-square
then f64-accumulation path, while track peak is the maximum public primary peak.
`DecodedDuration` preserves the exact decoded-frame/sample-rate pair instead of
storing a rounded seconds value.

DR calculation diagnostics remain separate: `loudWindowRms`,
`drSelectedPeak`, `drPrimaryPeak`, and nullable `drSecondaryPeak` describe the
values used by the DR state machine. They must not be substituted for report
metrics.

Decoders normalize supported inputs to finite interleaved `f64`, matching the
fixed x64 core's PCM width. On the fixed 39-track schema-v3 safe-master run,
track DR matched 39/39, channel DR 62/62, overall peak 39/39, overall RMS
39/39, channel RMS 62/62, and rendered duration 39/39. The reference footer's
track count, sample-rate set, channel-count set, and `DR12` token are also
consistent with the implementation reports; excluding the three numeric DR0
tracks would instead produce DR13. This partial footer check does not establish
host metadata, precise album-internal arithmetic, duration weighting, or full
text parity. Internal implementation-state parity is intentionally not a target.
The M1 numeric scope includes the statically recovered album arithmetic and
renderer numeric rules. A separate 38-vector isolated run now also
cross-checks duration half-second/carry behavior, optional multichannel
loudness weighting, and both RMS-histogram clamp endpoints. Host behavior,
playlist grouping, metadata provenance, complete text parity, and arbitrary
audio remain outside the claim. The profile therefore remains `Unverified`.

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

Clones of one `Application` share a bounded FIFO execution domain. The product
policy established in M3 admits one active top-level analyze, batch, or
discovery job and at most 64 queued jobs. This keeps CLI/Tauri execution serial
without a hidden process-global singleton or a second scheduler.

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

`finish` consumes the session and is fallible so numeric/resource failures
cannot leak non-finite output. Samples must be finite and frame-aligned.
Successful `AnalysisResult` and `AnalysisReport` roots are immutable and can
only be created through checked constructors; callers inspect them through
read-only getters or `AnalysisResult::view()`. Result/report and shared
batch/event/wire types that are not product inputs support serialization, not
deserialization. These Rust API constraints do not alter the schema-v3 JSON
keys, tags, or numeric representation.

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
values, including numeric DR0 tracks. Optional duration weighting uses each
track's exact decoded duration. This numeric API does not claim playlist
grouping, footer, or other album-subsystem parity. Batch and GUI results remain
collections of independent track reports unless a caller explicitly invokes it.

## GUI

The Tauri 2 frontend uses exactly the same application façade and wire schema as
the CLI.

```bash
cd tauri-app
npm install
npm run tauri dev
```

Each GUI job owns an independent cancellation token. Jobs reserve the shared
application budget before entering the blocking runtime; queued cancellation
does not affect the active job. The GUI does not configure FFmpeg, mutate
process environment variables, or run a separate batch engine.

## Architecture

The repository is a virtual Cargo workspace:

```text
macinmeter-domain
├── macinmeter-analysis
├── macinmeter-codecs
└── macinmeter (application façade)
    ├── macinmeter-cli
    └── macinmeter-gui
```

All first-party crates use `#![forbid(unsafe_code)]`. The application layer is
the only place that composes decoding and analysis, so frontends cannot silently
fork algorithm behavior.

See:

- [M0 architecture decision](docs/adr/0001-m0-0.2.0-trusted-trunk-rebuild.md)
- [M1 reference-numeric scope decision](docs/adr/0002-m1-reference-numeric-scope.md)
- [M2 native-decoder contract decision](docs/adr/0003-m2-native-decoder-contract-hardening.md)
- [M3 application execution-budget decision](docs/adr/0004-m3-application-execution-budget.md)
- [M4 bounded x64 numeric-claim decision](docs/adr/0005-m4-bounded-x64-numeric-claim.md)
- [M5 product/repository convergence decision](docs/adr/0006-m5-product-repository-convergence.md)
- [Local release artifact contract](docs/RELEASE.md)
- [M4 x64 numeric-claim evidence matrix](docs/M4_X64_NUMERIC_CLAIM_MATRIX.md)
- [M4 bounded x64 numeric-conformance report](docs/M4_X64_NUMERIC_COMPATIBILITY_REPORT.md)
- [Architecture and reference-alignment roadmap](docs/ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md)
- [Supported formats](docs/SUPPORTED_FORMATS.md)
- [`foo_dr_meter 1.0.8 Candidate V1` specification](reference/specs/foo-dr-meter-1.0.8-candidate-v1.md)
- [Reference-evidence policy](reference/README.md)
- [Isolated x64 analyzer-core harness](reference/observations/CORE_HARNESS.md)
- [Isolated x64 numeric-boundary observation](reference/observations/obs-foo-dr-meter-108-x64-numeric-boundaries-v1-run1-20260719/record.md)

## Reference work and attribution

The current reference target is foobar2000 DR Meter 1.0.8
(`foo_dr_meter`) by Janne Hyvärinen. Permission to reverse engineer the plugin
has been obtained from its author. Private permission correspondence is not
stored in this repository; only a
[minimal public scope summary](reference/authorization/README.md) is retained.

That permission and attribution do not establish numerical compatibility.
Target hashes, experiments, observations, and the candidate specification are
recorded under `reference/`. The current
[clean-commit schema-v3 x64 safe-master conformance record](reference/conformance/conf-foo-dr-meter-108-x64-complete-v2-safe-master-macinmeter-020-report-v3-clean-20260718/record.md)
documents its exact scope and declared limits. The profile remains `Unverified`
unless a later, separately reviewed compatibility claim justifies a stronger
statement.

An accepted
[39-input isolated x64 analyzer-core observation](reference/observations/obs-foo-dr-meter-108-x64-isolated-core-safe-master-run1-20260719/record.md)
now exercises the fixed target directly without starting foobar2000. It verifies
that boundary with one fresh worker per input and fail-fast tripwires on all 13
ordinary `shared.dll` IAT entries during core calls. It does not verify foobar
decoding, registration, metadata, album grouping, or complete rendering; those
are explicit non-goals rather than unfinished M1 evidence. Its claims remain
`compatibility: none` and `foobarParity: not_assessed`.

The same hardened boundary has also completed an accepted
[38-vector numeric observation](reference/observations/obs-foo-dr-meter-108-x64-numeric-boundaries-v1-run1-20260719/record.md):
24 duration, 8 multichannel-weighting, and 6 histogram-endpoint workers all
matched their preregistered assertions. It closes those output-affecting
per-track evidence gaps without running the full renderer or broadening the
compatibility claim.

## License

MacinMeter is released under the [MIT License](LICENSE). See
[legal notes](docs/LEGAL.md) and [third-party notices](THIRD_PARTY_NOTICES.md).
