# MacinMeter 0.2.0 Tauri adapter

`tauri-app/` is a thin Tauri 2 adapter over the `macinmeter` application crate.
Its Rust backend does not contain a second decoder, analyzer, batch scheduler,
or serialized report DTO. TypeScript declarations are a compile-time view of
the shared `WireEnvelope`; Rust contract tests lock its tags and field casing.

## Commands

The backend exposes:

| Command | Purpose |
|---|---|
| `run_analysis` | analyze one explicit file |
| `run_batch` | analyze discovered inputs serially |
| `discover_inputs` | expand files/directories in stable order under a cancellable `jobId` |
| `cancel_job` | cancel exactly one caller-generated `jobId` |
| `get_capabilities` | return the read-only native capability snapshot; the picker builds its extension filter from `stableDiscoveryExtensions` instead of a handwritten list |

The frontend creates each `jobId`. Tauri state maps that ID to an independent
`CancellationToken`; inserting a duplicate active ID is an error. Progress
events have the single shape `{ jobId, event }`, so simultaneous jobs cannot
cancel or overwrite each other.

Directory previews are jobs too. Reselecting, clearing, or starting analysis
cancels an outstanding preview before continuing, so it cannot race a batch
directory walk.

Analysis and batch responses use schema 3 of the same versioned `WireEnvelope`
as CLI JSON. The analysis payload exposes `analysis.aggregates.track`, separate
channel/track `report` metrics, exact decoded duration, and DR diagnostics named
`drSelectedPeak`, `drPrimaryPeak`, and `drSecondaryPeak`. Zero-level dBFS is
`null`; other public report values are finite by construction.

Silent channels remain visibly silent while contributing DR0 to the track
aggregate. A batch remains a list of independent reports: the library's
explicit `AlbumAggregator` is not invoked by Tauri commands. Rendering
preferences stay in TypeScript and never enter the analysis request.

## Development

```bash
cd tauri-app
npm install
npm run build
npm run tauri dev
```

Rust 1.88+, Node.js 18/20/22+ (prefer an active LTS), and the platform-specific
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) are required.
Workspace Rust builds place artifacts under the root `target/` directory.
The current bundle targets are macOS `.app` and `.dmg`; Windows/Linux packaging
is outside M0.

The backend performs blocking analysis through Tauri's blocking task facility,
leaving the UI event loop responsive. It does not modify environment variables,
look for FFmpeg, use a global cancel flag, or create a Rayon batch pool.

Current GUI results are always labelled
`foo_dr_meter 1.0.8 Candidate V1 / Unverified`. This identifies the evidence
target and candidate revision. The fixed schema-v3 safe-master comparison
matches the five exported DR/report field groups plus all 39 rendered duration
tokens. Its limited footer check covers track count, sample-rate/channel-count
sets, and the aggregate DR token. M1 separately retains the statically recovered
album arithmetic and renderer rounding rules, but host metadata, playlist
grouping, complete text rendering, production/reference internal-state parity,
and arbitrary audio are explicit non-goals. This is not a claim of full
reference parity.
