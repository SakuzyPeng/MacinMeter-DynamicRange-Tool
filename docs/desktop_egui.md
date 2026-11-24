# MacinMeter desktop GUI on egui (prototype)

`apps/macinmeter-desktop` is a prototype of the desktop GUI on the same egui
stack as the MacinDecode Spatial Player (`eframe`/`egui` 0.36.1 on `wgpu`,
`rfd` 0.17.2 for native dialogs). It sits beside the Tauri GUI and replaces
nothing yet: the Tauri app, its staging, and its release scope are unchanged.

## Decisions it is built on

| Question | Decision |
|---|---|
| Toolchain | The whole workspace moves to Rust 1.98.0, pinned by `rust-toolchain.toml`, the same compiler as the player. eframe 0.36.1 itself needs 1.95. |
| UI font | One checksum-pinned Noto Sans CJK SC (16.4 MB, the player's file) is fetched at build time and embedded. This is the repository's **only** network exception, and it belongs to this crate's `build.rs` alone. Tests never fetch anything, and building `-p macinmeter` or `-p macinmeter-cli` never runs the script. |
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

Everything the Tauri GUI does except image export:

- **Input**: file, directory and deep-scan selection, whole-window drops,
  command-line paths, and a cancellable discovery preview.
- **Running**: single-file and batch analysis, cancellation, and live progress
  over interleaved file lanes.
- **Results**: streamed batch results; hide path, warnings or failures; the
  timing split; precise-DR sorting; search and jump.
- **Output**: Markdown copying per entry and for everything shown, JSON export,
  and a structured-result view.
- **Settings**: Chinese/English, persisted between launches.

### Application contract

The application contract is the one ADR-0004 defines and the Tauri adapter
honours:

- **Reservation**: `Application::reserve` is called on the UI thread before any
  worker thread is spawned. A refused reservation becomes an error envelope.
- **Cancellation**: the GUI owns each job's `CancellationToken` directly. There
  is no string job registry, because no JavaScript caller needs to name a job.
  Dropping a job cancels it, and the worker releases its reservation on the way
  out. Replacing a selection drops, and so cancels, its preview.
- **Execution**: no scheduler or pool is owned by the adapter. Each job is one
  named thread (`macinmeter-analysis-job` or `macinmeter-discovery`).
- **Progress**: events update shared state and ask for a repaint at most every
  33 ms. Nothing is serialised per event. The 50 ms sampling, 8-item chunking
  and 12-cards-per-frame rendering in the Tauri adapter existed only to protect
  the WebView IPC channel and DOM.

### Exactness

- **JSON export**: `Export JSON` writes `serde_json::to_string_pretty` of the
  `WireEnvelope`. Those are the bytes `mdrmeter --json --output` writes, and the
  export is written atomically. The Tauri frontend instead re-serialised the
  envelope in JavaScript after a JSON round trip, which rewrites integral
  floats: a full-scale `primaryPeakLinear` of `1.0` became `1`.
- **Displayed values**: dBFS, precise DR and duration follow the CLI renderer's
  rules, not JavaScript `toFixed` on re-parsed numbers. The rules are copied
  into `src/format.rs` with the CLI's own test vectors. Moving them to one
  shared place is follow-up work.

### Large batches

The page is one scroll area. Selection, tools and status sit at its top, and
the result cards follow. Only cards that intersect the viewport are laid out.
A card's height is measured the first time it is drawn and estimated from its
channel and warning counts until then.

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
