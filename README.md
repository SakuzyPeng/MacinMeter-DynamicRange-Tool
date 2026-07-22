# MacinMeter DR Tool

[English](README.md) | [中文](README_CN.md)

MacinMeter is an offline, local-first audio dynamic-range (DR) analyzer. It
reports per-channel and per-track DR values for supported WAV, FLAC, and AIFF
files. The command-line tool, Tauri desktop frontend, and Rust API all use the
same streaming analysis engine.

> **About reference results.** MacinMeter labels its current profile
> `foo_dr_meter 1.0.8 Candidate V1 / Unverified`. It is a candidate
> interpretation of evidence recovered from one fixed `foo_dr_meter 1.0.8
> x64` target. Recorded projections have zero differences on the fixed
> conformance corpus. The project presents that as bounded evidence, rather
> than as official certification or a promise of identical results for every
> file and every foobar2000 environment.

## Supported files

| Container | Available in 0.2.0 |
| --- | --- |
| classic RIFF/WAVE | 8/16/24/32-bit integer PCM; IEEE 32/64-bit float |
| native FLAC | FLAC with a declared nonzero total sample count |
| AIFF | 8/16/24/32-bit integer PCM |

An explicit file path is probed by content and may use any extension. Folder
scans look for `.wav`, `.wave`, `.flac`, `.aif`, and `.aiff`; a supported file
with another extension can still be analyzed by passing its path directly.
Current file analysis covers up to 64 channels.

Some files with familiar extensions use variants that are not available yet,
including WAVE_FORMAT_EXTENSIBLE, AIFC, and Ogg FLAC. MP3, AAC, ALAC, Vorbis,
Opus, and DSD are also unavailable. MacinMeter reports these as unsupported;
it does not invoke FFmpeg or silently resample or preprocess them. The
[format guide](docs/SUPPORTED_FORMATS.md) contains the exact route details.

## Installation

Building from source currently uses Rust 1.88 or later and Cargo:

```bash
cargo build --locked --release -p macinmeter-cli
```

The CLI is written to `target/release/macinmeter` on Unix-like hosts and
`target/release/macinmeter.exe` on Windows.

## Command-line use

The CLI is organized around two explicit commands:

```text
macinmeter analyze FILE [--format human|json] [--output PATH]
macinmeter batch INPUT... [--recursive] [--format human|json] [--output PATH]
```

For example:

```bash
macinmeter analyze "01 - Song.flac"
macinmeter batch "My Album/" --recursive
```

`batch` processes files serially in stable input order. A failed item does not
prevent later items from running. It produces independent track reports and
does not implicitly calculate an album DR.

The following is real stdout generated from a committed synthetic fixture:

```bash
target/release/macinmeter analyze tests/fixtures/edge_cases.wav
```

```text
MacinMeter — foo_dr_meter 1.0.8 Candidate V1 / Unverified
Source: tests/fixtures/edge_cases.wav
PCM: 44100 Hz, 2 channels, 308700 frames
Duration: 0:07

CH 1: DR2 (2.4300 dB), overall RMS -2.43 dBFS, selected DR peak 1.00000000
CH 2: DR2 (2.4300 dB), overall RMS -2.43 dBFS, selected DR peak 1.00000000

Track aggregate: DR2 (2.4300 dB; 2 contributing channels)
Report levels: peak 0.00 dBFS, RMS -2.43 dBFS
```

Progress for that command is written separately to stderr.

### Reading the result

- `DR2` is the rounded track aggregate produced by the current candidate
  profile. Within this metric, a larger value represents a larger ratio
  between the selected peak and loud-window RMS. A high DR value does not by
  itself mean that a recording sounds good. A very low value, however, is
  often a warning sign of aggressive compression and is more likely to go
  with a compromised master, even though genre and artistic intent still
  matter.
- Each `CH` line contains that channel's DR result, overall RMS, and the peak
  selected by the DR state machine.
- `Report levels` are whole-track report metrics. The report peak is distinct
  from the selected DR peak.
- dBFS uses normalized amplitude `1.0` as the 0 dB reference. Supported IEEE
  float PCM may contain finite samples above that reference, so 0 dBFS is not a
  universal clipping boundary.
- Silent channels remain visible and contribute numeric DR0 under Candidate
  V1; channels with insufficient data are explicitly excluded.

The fixture above is designed for deterministic automated tests, not as an
example of a typical music release.

### Exit codes

| Code | Meaning |
| ---: | --- |
| `0` | all requested analyses succeeded |
| `1` | failure, no input, all batch items failed, or output-write failure |
| `2` | invalid CLI arguments |
| `3` | batch completed with both successes and failures |
| `130` | cancelled |

### Saving and JSON

Without `--output`, the result stays on stdout and no report file is created.
With an output path, the completed report atomically replaces that file.

```bash
macinmeter analyze track.flac --format json
macinmeter analyze track.flac --format json --output track.json
```

JSON and Tauri use the same versioned schema-v3 `WireEnvelope`. The envelope
contains `schemaVersion`, `toolVersion`, `kind`, and `data`, with no timestamp.
Successful numeric fields are finite; values such as zero-amplitude dBFS are
represented explicitly as `null` where appropriate. Stdout contains only the
requested result while progress and diagnostics go to stderr.

## Desktop GUI

The repository includes a Tauri 2 desktop frontend:

```bash
cd tauri-app
npm install
npm run tauri dev
```

It calls the same `Application` façade and consumes the same wire schema as the
CLI. Each job has its own cancellation token, while the shared application
budget keeps top-level work bounded and serial.

The GUI is available as source. Local staging and the bounded `main`/manual
macOS 26 arm64 CI gate both build and structurally verify the current-host DMG.
Those builds remain unsigned and unnotarized, CI does not retain them, and
Windows/Linux GUI packages have not been verified on their target hosts. The
current packaging picture is summarized in
[release and artifact status](docs/RELEASE.md).

## Rust API

The workspace's public façade is the `macinmeter` crate:

```rust
use macinmeter::{AnalyzeRequest, Application};

fn main() -> Result<(), macinmeter::AnalysisError> {
    let application = Application::new();
    let report = application.analyze_file(AnalyzeRequest::new("track.flac"))?;

    if let Some(dr) = report.analysis().aggregates().track.rounded_dr {
        println!("DR{dr}");
    }
    Ok(())
}
```

Clones of one `Application` share a bounded serial execution queue, while
separately constructed `Application` values remain independent. Queue sizing
is available through `Application::with_budget`.

`AnalyzerSession` is available for callers that already have finite,
frame-aligned, interleaved `f64` PCM. `AlbumAggregator` is a separate numeric
operation over track reports, with unweighted and decoded-duration weighting.
Playlist grouping, metadata, footer rendering, and the rest of the album
subsystem sit outside that numeric API.

## Accuracy

The current target is the fixed `foo_dr_meter 1.0.8 x64` binary identified by
the `ff3556ad…` hash prefix. Against fixed recorded inputs, the repository
contains the following bounded results:

| Evidence | Recorded result |
| --- | --- |
| schema-v3 safe-master track DR, overall peak, overall RMS, and rendered duration | 39/39 each |
| same run: channel DR and channel RMS | 62/62 each |
| decoder-independent Candidate direct-PCM final-field projection | 0 differences on 39 fixed inputs |
| isolated x64 analyzer-core run | all preregistered assertions met on 39 inputs |
| numeric-boundary vectors for duration, weighting, and histogram endpoints | 24/24, 8/8, and 6/6 |

The table describes one named target, corpus, set of fields, and runtime
boundary. Arbitrary audio, x86 and other plugin versions, foobar2000 decoding,
host and playlist behavior, metadata provenance, complete text rendering, and
internal implementation-state identity all remain outside those observations.
That is the scope represented by `Candidate V1 / Unverified`.

The supporting records are the
[M4 evidence matrix](docs/M4_X64_NUMERIC_CLAIM_MATRIX.md),
[M4 compatibility report](docs/M4_X64_NUMERIC_COMPATIBILITY_REPORT.md), and
[Candidate V1 specification](reference/specs/foo-dr-meter-1.0.8-candidate-v1.md).

## Performance

Analysis is streaming: its analysis-state memory grows with channel count, not
track duration. The published performance material is a set of local
measurements rather than a universal throughput or memory figure.

M6 recorded release-worker measurements on one fixed Apple M4 Pro host,
toolchain, generated corpus, and synthetic workload. The audio cases ran faster
than real time on that host, and the accepted optimization reduced fixed
8-/64-channel analyzer medians while preserving result fingerprints. Actual
speed and memory use still vary with the machine, format, channel count, and
input; the recorded figures are a local baseline rather than a prediction for
another environment.

The reproducible measurement scripts and their interpretation are documented
in the [performance notes](docs/BENCHMARKS.md) and
[M6 records](docs/performance/README.md).

## Under the hood

MacinMeter 0.2.0 is a virtual Cargo workspace with one-way dependencies:

```text
macinmeter-domain
├── macinmeter-analysis
├── macinmeter-codecs
└── macinmeter
    ├── macinmeter-cli
    └── macinmeter-gui
```

Every first-party Rust crate uses `#![forbid(unsafe_code)]`. The product has
one analyzer, serial native decoding, and serial batch execution. Its design
history and deeper technical material live in:

- [architecture and reference-alignment roadmap](docs/ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md)
- [architecture decision records](docs/adr/)
- [supported formats](docs/SUPPORTED_FORMATS.md)
- [performance notes](docs/BENCHMARKS.md)
- [release and packaging notes](docs/RELEASE.md)

## Reference work and attribution

The sole current candidate target is Janne Hyvärinen's `foo_dr_meter 1.0.8
x64` component. Reverse-engineering that fixed target was performed with the
author's permission. Private correspondence is not stored in the repository;
only a [minimal public authorization summary](reference/authorization/README.md)
is retained.

Permission and attribution provide the legal and historical context for the
research. The numerical status comes from the bounded records above and remains
`Candidate V1 / Unverified`. Historical M0/1.0.3 material is kept as a
superseded archive, separate from the current target. Target identities,
experiments, observations, specifications, and their limits are indexed under
[`reference/`](reference/README.md).

## License

MacinMeter is released under the [MIT License](LICENSE). Related material is
collected in the [legal notes](docs/LEGAL.md) and
[third-party notices](THIRD_PARTY_NOTICES.md).
