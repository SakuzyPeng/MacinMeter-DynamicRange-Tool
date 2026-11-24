# MacinMeter desktop GUI on egui (prototype)

`apps/macinmeter-desktop` is a prototype of the desktop GUI on the same egui
stack as the MacinDecode Spatial Player (`eframe`/`egui` 0.36.1 on `wgpu`,
`rfd` 0.17.2 for native dialogs). The release workflow packages the Tauri GUI.

## Decisions it is built on

| Question | Decision |
|---|---|
| Toolchain | The whole workspace moves to Rust 1.98.0, pinned by `rust-toolchain.toml`, the same compiler as the player. eframe 0.36.1 itself needs 1.95. |
| UI font | The desktop crate embeds the player's checksum-pinned Noto Sans CJK SC (16.4 MB). Its build script downloads the font on a cache miss, including when compiling the test binary. Set `MACINMETER_UI_FONT_PATH` to build with a local copy. |
| Complex scripts | egui 0.36 shapes text with HarfRust but has no bidi and no system font fallback. Arabic or Hebrew file names render in logical (reversed) order, and scripts outside the embedded font render as boxes. This is accepted. |
| Windows installer | Stays NSIS when the Tauri bundler goes, so an install can replace a 0.3.x Tauri install in place. Not part of the prototype. |
| Image export | Will be rebuilt from report data (a real vector SVG, PNG rasterised from it), not from a screenshot of the UI. Not part of the prototype. |

### Font build input

`build.rs` downloads the font once, verifies its SHA-256, and copies it into
`OUT_DIR`. Two variables control it:

- `MACINMETER_UI_FONT_PATH`: a local copy of the exact file, for offline
  builds. The checksum is still enforced.
- `MACINMETER_UI_FONT_CACHE_DIR`: an absolute directory shared by profiles and
  checkouts. CI sets one per job.

jsDelivr sometimes answers this file with a 403 that it caches for 60 s
(`s-maxage=60`). That was observed while building this prototype. Each retry
therefore waits 61 s rather than hitting the same cached refusal.

## What the prototype covers

- **Input**: file, directory and deep-scan selection, whole-window drops,
  command-line paths, and a cancellable discovery preview. A single ambiguous
  dropped or command-line path is classified before analysis starts.
- **Running**: single-file and batch analysis, cancellation, and live progress
  over interleaved file lanes.
- **Results**: streamed batch summaries, one expandable channel detail,
  path, warning and failure visibility, timing, precise-DR sorting, and search.
- **Output**: Markdown copying per entry and for everything shown, JSON export,
  and a structured-result view.
- **Settings**: Chinese/English, persisted between launches.

### Application contract

The application contract is the one ADR-0004 defines and the Tauri adapter
honours:

- **Reservation**. The UI reserves an `ApplicationJob` before spawning one
  named worker. Reservation, spawn and worker failures become structured errors.
- **Cancellation**. The GUI owns each job's `CancellationToken`. Dropping a job
  cancels it, and the worker releases its reservation on exit. Replacing a
  selection cancels its preview.
- **Progress**. Events update shared state and request a deferred repaint. The
  batch sink transfers each owned chunk into `BatchDetailStore` and publishes
  lightweight rows by discovery index. The UI drains the rows together with
  the terminal state. Cancellation or a task error clears any partial rows.

### Exactness

- **JSON export**. After the user chooses a path, a worker writes the schema-4
  `WireEnvelope` with `serde_json::to_writer_pretty`. The bytes match
  `mdrmeter --format json --output` for the same envelope. The worker flushes
  a unique temporary file in the destination directory before replacing the
  chosen path. Opening the structured-result view also prepares JSON on a worker.
- **Displayed values**: dBFS, precise DR and duration follow the CLI renderer's
  rules, not JavaScript `toFixed` on re-parsed numbers. The rules are copied
  into `src/format.rs` with the CLI's own test vectors. Moving them to one
  shared place is follow-up work.

### Large batches

The page lays out the cards that intersect the viewport. A height index
locates the visible range and updates measured heights in logarithmic time.
Selecting a summary borrows one detail from the batch store. Complete batch
documents are materialized for explicit JSON operations.

## Not yet

- Image export (PNG/SVG), as decided above.
- Packaging. This needs:
  - an `.app` bundle and DMG for macOS 11.0+ arm64;
  - a hand-written NSIS installer for Windows x64;
  - the icon and version resources;
  - `stage-release.py` verification of both.
- An embedded Windows application manifest. Until there is one, `rfd` runs
  without `common-controls-v6` (that feature imports `TaskDialogIndirect`, which
  only Common Controls v6 exports) and uses plain `MessageBoxW`.
- Retiring the Tauri GUI, with an ADR that supersedes the GUI-adapter parts of
  ADR-0004, ADR-0006, ADR-0010, ADR-0011 and ADR-0015.
- The locale files still live under `tauri-app/src/i18n/locales/`, so both GUIs
  read the same strings.

## Running

```bash
cargo run -p macinmeter-desktop
cargo run -p macinmeter-desktop -- path/to/album path/to/track.flac
cargo test -p macinmeter-desktop
```

Linux needs a desktop session (X11 or Wayland) and a Vulkan or GL driver to
open the window. The tests need neither: they drive the egui passes headlessly.
