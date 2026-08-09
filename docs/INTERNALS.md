# MacinMeter technical notes

[English](INTERNALS.md) | [中文](INTERNALS_CN.md)

Material for people building against MacinMeter, reproducing its measurements,
or reading its sources. The [README](../README.md) covers everyday use.

## Building from source

Rust 1.88 or newer, with Cargo:

```bash
cargo build --locked --release -p macinmeter-cli
```

The CLI is written to `target/release/mdrmeter`, or `target/release/mdrmeter.exe`
on Windows.

The desktop frontend is a Tauri 2 app:

```bash
cd tauri-app
npm install
npm run tauri dev
```

It calls the same `Application` façade and consumes the same wire schema as the
CLI. Each job has its own cancellation token, while the shared application
budget keeps top-level work bounded, with one active job at a time.

## JSON output

```bash
mdrmeter analyze track.flac --format json
mdrmeter analyze track.flac --format json --output track.json
```

Without `--output` the result stays on stdout and no report file is created.
With an output path, the completed report atomically replaces that file.

The CLI and the GUI emit the same versioned schema-v4 `WireEnvelope`, containing
`schemaVersion`, `toolVersion`, `kind`, and `data`, with no timestamp.
Successful numeric fields are finite; values such as zero-amplitude dBFS are
represented explicitly as `null` where appropriate. Stdout carries only the
requested result; progress and diagnostics go to stderr.

The envelope is a pure function of the input: two runs over one file serialize
identically. Wall-clock timing therefore never enters it, and appears only in
human-readable output.

## Rust API

The workspace's public façade is the `macinmeter` crate. It is not on crates.io
yet, so depend on it by tag:

```toml
[dependencies]
macinmeter = { git = "https://github.com/SakuzyPeng/MacinMeter-DynamicRange-Tool", tag = "v0.3.0" }
```

The manifests are prepared for a registry release — the four library crates
carry descriptions and pin their siblings to the workspace version — but
publishing is deliberately deferred while the public surface is still settling.
Releasing means uploading `macinmeter-domain`, `macinmeter-analysis`,
`macinmeter-codecs`, then `macinmeter`, in that order: each one's dependencies
must already be on the registry, so no dry run can verify the chain in advance.

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

Clones of one `Application` share a bounded top-level execution queue, which
currently admits one active job; separately constructed `Application` values
remain independent. Queue sizing is available through
`Application::with_budget`. Internal workers stay inside that
application-owned execution domain.

`AnalyzerSession` is available for callers that already have finite,
frame-aligned, interleaved `f64` PCM. `AlbumAggregator` is a separate numeric
operation over track reports, with unweighted and decoded-duration weighting.
Playlist grouping, metadata, footer rendering, and the rest of the album
subsystem sit outside that numeric API.

## Workspace

MacinMeter 0.3.0 is a virtual Cargo workspace with one-way dependencies:

```text
macinmeter-domain
├── macinmeter-analysis
├── macinmeter-codecs
└── macinmeter
    ├── macinmeter-cli
    └── macinmeter-gui
```

Every first-party Rust crate uses `#![forbid(unsafe_code)]`.

## Internal parallelism

The product has one analyzer implementation. Under one application-owned worker
and memory plan it decodes the ALAC route and provably bounded FLAC streams with
packet workers, overlaps decoding with analysis on a permit the route leaves
unspent, and runs batch items across file lanes derived from that same plan. A
single file receives the whole decoder, and window-level parallelism is not
implemented.

Results are identical whichever path runs. Each axis is bounded and
deterministic, and is enabled one route and one axis at a time, only after that
axis has been shown not to change a result. There is no public thread,
batch-size, or queue control, and no throughput figure is published.

## Accuracy

The reference target is the fixed `foo_dr_meter 1.0.8 x64` binary identified by
the `ff3556ad…` hash prefix. Against fixed recorded inputs, the repository
contains the following bounded results:

| Evidence | Recorded result |
| --- | --- |
| schema-v3 safe-master track DR, overall peak, overall RMS, and rendered duration | 39/39 each |
| same run: channel DR and channel RMS | 62/62 each |
| decoder-independent direct-PCM final-field projection | 0 differences on 39 fixed inputs |
| isolated x64 analyzer-core run | all preregistered assertions met on 39 inputs |
| numeric-boundary vectors for duration, weighting, and histogram endpoints | 24/24, 8/8, and 6/6 |

The table describes one named target, corpus, set of fields, and runtime
boundary. Arbitrary audio, x86 and other plugin versions, foobar2000 decoding,
host and playlist behavior, metadata provenance, complete text rendering, and
internal implementation-state identity all remain outside those observations.

The supporting records are the
[M4 evidence matrix](M4_X64_NUMERIC_CLAIM_MATRIX.md),
[M4 numeric-alignment report](M4_X64_NUMERIC_ALIGNMENT_REPORT.md), and
[algorithm specification](../reference/specs/foo-dr-meter-1.0.8-candidate-v1.md).

Permission and attribution provide the legal and historical context for the
research; the numerical claims come from the bounded records above. Historical
M0/1.0.3 material is kept as a superseded archive, separate from the current
target. Target identities, experiments, observations, specifications, and their
limits are indexed under [`reference/`](../reference/README.md).

## Performance

Analysis is streaming: its analysis-state memory grows with channel count, not
track duration. The published performance material is a set of local
measurements rather than a universal throughput or memory figure.

Recorded release-worker measurements come from one fixed Apple M4 Pro host,
toolchain, generated corpus, and synthetic workload. The audio cases ran faster
than real time on that host, and the accepted optimization reduced fixed
8-/64-channel analyzer medians while preserving result fingerprints. Actual
speed and memory use still vary with the machine, format, channel count, and
input; the recorded figures are a local baseline rather than a prediction for
another environment.

The reproducible measurement scripts and their interpretation are documented in
the [performance notes](BENCHMARKS.md) and [records](performance/README.md).

## Packaging

0.3.0 packages the CLI and the GUI for two platforms: Apple Silicon Macs running
macOS 11.0 or newer, and Windows x64. Each platform is staged on its own host — a
DMG on macOS, an NSIS installer on Windows — and both local staging and the
bounded CI gate build and structurally verify the final artifact by opening it:
the DMG is mounted and its `.app` inspected, and the installer is extracted
outside the candidate directory and its `macinmeter-gui.exe` checked for an
observed x86_64 PE machine, matching version resource, and unsigned Authenticode
state. The outer installer must also be an unsigned PE.

Intel/universal macOS, ARM64 Windows, and Linux GUI packages are not part of the
0.3.0 release. The current packaging picture is summarized in [release and
artifact status](RELEASE.md).

## Design history

- [architecture and reference-alignment roadmap](ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md)
- [architecture decision records](adr/)
- [format guide](SUPPORTED_FORMATS.md)
