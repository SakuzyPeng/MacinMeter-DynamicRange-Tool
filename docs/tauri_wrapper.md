# MacinMeter 0.3.0 Tauri adapter

`tauri-app/` is a thin Tauri 2 adapter over the `macinmeter` application crate.
Its Rust backend does not contain a second decoder, analyzer, batch scheduler,
or serialized report DTO. TypeScript declarations are a compile-time view of
the shared `WireEnvelope`; Rust contract tests lock its tags and field casing.

## Commands

The backend exposes:

| Command | Purpose |
|---|---|
| `run_analysis` | analyze one explicit file |
| `run_batch` | analyze discovered inputs across bounded file lanes and report them in stable order |
| `discover_inputs` | expand files/directories in stable order under a cancellable `jobId` |
| `cancel_job` | cancel exactly one caller-generated `jobId` |
| `get_capabilities` | return the read-only native capability snapshot; the picker builds its extension filter from `stableDiscoveryExtensions` instead of a handwritten list |

`run_batch` uses the file-lane width the shared application plan derives after
discovery. Packet workers, decode-analysis overlap, and file lanes have
graduated under accepted
[ADR-0014](adr/0014-deterministic-decode-analysis-pipeline.md); window-level
parallelism has not been implemented. All internal work remains inside the
shared `Application` execution domain, so Tauri remains an adapter and does not
become a scheduler or own a pool.

Progress events may interleave across file lanes. Every event carries its item
index and path, the report remains in stable discovery order, and the frontend
aggregates progress from the latest state of every item instead of treating an
event index as a completed serial prefix.

The frontend creates each `jobId`. Tauri state maps that ID to an independent
`CancellationToken`; inserting a duplicate active ID is an error. Progress
events have the single shape `{ jobId, event }`, so simultaneous jobs cannot
cancel or overwrite each other.

Before submitting blocking work, each command reserves an `ApplicationJob`
from the single managed `Application`. The budget established in M3 admits one
active job and at most 64 waiting jobs in FIFO order. No file/discovery
progress is emitted until a reservation becomes active; cancelling a queued
job removes only that reservation.

Directory previews are jobs too. Reselecting, clearing, or starting analysis
cancels an outstanding preview before continuing, so it cannot race a batch
directory walk.

Analysis and batch responses use schema 4 of the same versioned `WireEnvelope`
as CLI JSON. The analysis payload exposes `analysis.aggregates.track`, separate
channel/track `report` metrics, exact decoded duration, and DR diagnostics named
`drSelectedPeak`, `drPrimaryPeak`, and `drSecondaryPeak`. Zero-level dBFS is
`null`; other public report values are finite by construction.

Silent channels remain visibly silent while contributing DR0 to the track
aggregate. A batch remains a list of independent reports: the library's
explicit `AlbumAggregator` is not invoked by Tauri commands. Rendering
preferences stay in TypeScript and never enter the analysis request.

The product frontend accepts native whole-window drops of files, directories,
or mixed inputs. Dropped and picker-selected paths share the same cancellable
discovery and batch commands. Chinese/English presentation, result search,
precise-DR sorting, path hiding, Markdown copying, and PNG/SVG rendering are
frontend-only features. JSON export writes the exact backend `WireEnvelope`
instead of constructing another transport model.

## Development

```bash
cd tauri-app
npm install
npm run build
npm run tauri dev
```

Rust 1.88+, Node.js 18/20/22+ (prefer an active LTS), and the platform-specific
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) are required.
Workspace Rust builds place artifacts under the root `target/` directory. The
0.3.0 bundle target is Apple Silicon macOS 11.0+ `.app`/`.dmg`; Intel,
universal, Windows, and Linux GUI packaging is outside the release contract.

`python3 scripts/stage-release.py stage --include-gui` builds the current-host
DMG and verifies its image integrity, mounted bundle version/identifier,
executable, and exact architecture before adding it to the SHA-256 release
manifest. The bounded `main` macOS 26 arm64 CI job runs the same clean contract
and discards its artifacts. Manual dispatch creates the stricter unsigned
Apple Silicon candidate and retains it for 14 days. Neither path launches,
Developer-ID-signs, notarizes, tags, or publishes the app. A structural smoke
pass is not a public-distribution or Gatekeeper claim.
See [`RELEASE.md`](RELEASE.md).

The backend performs admitted blocking analysis through Tauri's blocking task
facility, leaving the UI event loop responsive. Admission happens first so the
runtime is not used as an unbounded hidden queue. The backend does not modify
environment variables, look for FFmpeg, use a global cancel flag, or create an
adapter-owned Rayon batch pool.

The structured result records fixed numeric parameters for reproducibility,
but does not expose an internal profile name or project status. The fixed
schema-v3 safe-master comparison
matches the five exported DR/report field groups plus all 39 rendered duration
tokens; its exact scope and exclusions remain in the reference records.
