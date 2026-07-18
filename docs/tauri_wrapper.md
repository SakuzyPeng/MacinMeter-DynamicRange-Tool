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

The frontend creates each `jobId`. Tauri state maps that ID to an independent
`CancellationToken`; inserting a duplicate active ID is an error. Progress
events have the single shape `{ jobId, event }`, so simultaneous jobs cannot
cancel or overwrite each other.

Directory previews are jobs too. Reselecting, clearing, or starting analysis
cancels an outstanding preview before continuing, so it cannot race a batch
directory walk.

Analysis and batch responses use schema 2 of the same versioned `WireEnvelope`
as CLI JSON. The analysis payload exposes `analysis.aggregates.track`; silent
channels remain visibly silent while contributing DR0 to that track aggregate.
Rendering preferences stay in TypeScript and never enter the analysis request.

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
target and candidate revision; it is not a claim of reference parity.
