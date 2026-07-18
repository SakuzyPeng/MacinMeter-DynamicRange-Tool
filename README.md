# MacinMeter DR Tool

[English](README.md) | [中文](README_CN.md)

MacinMeter is an independent, local-first audio dynamic-range analysis project.
Version 0.2.0 rebuilds the project around one safe, streaming Rust core shared by
the library, CLI, and Tauri GUI.

> **Compatibility status: `ProvisionalV1 / Unverified`.** The current algorithm
> is a reproducible engineering baseline, not a verified implementation of the
> reference plugin. Its values must not be described as “official,” certified,
> or interchangeable with reference results.

## M0 scope

The 0.2.0 baseline deliberately keeps a small trusted surface:

- WAV: 8/16/24/32-bit integer PCM and IEEE 32/64-bit float
- FLAC
- AIFF: 8/16/24/32-bit integer PCM
- serial decoding and serial batch execution
- streaming, bounded-memory analysis
- one `ProvisionalV1` profile
- structured errors, cancellation, progress, and versioned JSON

Input is probed by content. File extensions are used only while discovering
files in directories. AIFC, MP3, AAC, ALAC, Vorbis, Opus, FFmpeg routes, DSD,
preprocessing, packet-level parallelism, and SIMD paths are not part of M0 and
return `unsupported_format` when encountered.

## Build and test

Rust 1.88 or later and Cargo are required.

```bash
cargo build --locked --workspace
cargo test --locked --workspace
cargo build --locked --release -p macinmeter-cli
```

The release CLI is written to `target/release/macinmeter`.

## CLI

The CLI has no implicit directory scan and never saves reports unless
`--output` is supplied.

```bash
macinmeter analyze FILE [--format human|json] [--output PATH]
macinmeter batch INPUT... [--recursive] [--format human|json] [--output PATH]
```

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

JSON and Tauri use the same envelope:

```json
{
  "schemaVersion": 1,
  "toolVersion": "0.2.0",
  "kind": "analysis",
  "data": {}
}
```

The payload contains no timestamp. Non-finite values are never emitted as JSON
numbers.

## Library

The public façade is the `macinmeter` crate:

```rust
use macinmeter::{AnalyzeRequest, Analyzer};

fn main() -> Result<(), macinmeter::AnalysisError> {
    let _report = Analyzer::new().analyze_file(AnalyzeRequest::new("track.flac"))?;
    Ok(())
}
```

Lower-level analysis is available through a frame-aligned streaming session:

```rust
use macinmeter::{AnalysisProfile, AnalyzerSession, ChannelLayout, StreamSpec};

fn main() -> Result<(), macinmeter::AnalysisError> {
    let spec = StreamSpec::new(48_000, 2, ChannelLayout::Unknown)?;
    let mut session = AnalyzerSession::new(spec, AnalysisProfile::ProvisionalV1)?;
    session.push_interleaved(&[0.25, -0.25, 0.5, -0.5])?;
    let _result = session.finish();
    Ok(())
}
```

`finish` consumes the session. Samples must be finite and frame-aligned.

## GUI

The Tauri 2 frontend uses exactly the same application façade and wire schema as
the CLI.

```bash
cd tauri-app
npm install
npm run tauri dev
```

Each GUI job owns an independent cancellation token. The GUI does not configure
FFmpeg, mutate process environment variables, or run a separate batch engine.

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
- [Architecture and reference-alignment roadmap](docs/ARCHITECTURE_AND_REFERENCE_ALIGNMENT.md)
- [Supported formats](docs/SUPPORTED_FORMATS.md)
- [`ProvisionalV1` specification](reference/specs/provisional-v1.md)
- [Reference-evidence policy](reference/README.md)

## Reference work and attribution

The project studies the behavior of foobar2000 DR Meter 1.0.3
(`foo_dr_meter`) by Janne Hyvärinen. Permission to reverse engineer the plugin
has been obtained from its author. Private permission correspondence is not
stored in this repository.

That permission and attribution do not establish numerical compatibility.
Reference binaries, hashes, experiments, observations, and conformance fixtures
will be recorded as evidence in later milestones; M0 only provides the boundary
that can accept them.

## License

MacinMeter is released under the [MIT License](LICENSE). See
[legal notes](docs/LEGAL.md) and [third-party notices](THIRD_PARTY_NOTICES.md).
