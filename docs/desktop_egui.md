# MacinMeter desktop GUI on egui (prototype)

`apps/macinmeter-desktop` is a prototype of the desktop GUI on the same egui
stack as the MacinDecode Spatial Player (`eframe`/`egui` 0.36.1 on `wgpu`,
`rfd` 0.17.2 for native dialogs). The release workflow packages the Tauri GUI.

## Decisions it is built on

| Question | Decision |
|---|---|
| Toolchain | The whole workspace moves to Rust 1.98.0, pinned by `rust-toolchain.toml`, the same compiler as the player. eframe 0.36.1 itself needs 1.95. |
| UI fonts | Two builds; see [Font builds](#font-builds). The default `bundled-fonts` build embeds the player's checksum-pinned Noto Sans CJK SC (16.4 MB) and Source Serif 4 Semibold 4.005 (253 KB). A build without that feature embeds neither and uses the host's fonts, as the Tauri WebView did. |
| Complex scripts | egui 0.36 shapes text with HarfRust but has no bidi and no system font fallback. Arabic or Hebrew file names render in logical (reversed) order, and scripts outside the embedded font render as boxes. This is accepted. |
| Windows installer | Stays NSIS when the Tauri bundler goes, so an install can replace a 0.3.x Tauri install in place. Not part of the prototype. |
| Image export | Drawn from report data as a real vector SVG, with PNG rasterised from that SVG; see [Image export](#image-export). |

### Font builds

`src/fonts.rs` is the one place faces come from. It resolves a sans fallback
chain and a serif chain once per process. The window's egui families, card
layout measurement and PNG rasterisation all read those chains, so in either
build the window and an exported image still draw one card.

| | `bundled-fonts` (default) | system fonts (`--no-default-features`) |
|---|---|---|
| Faces | Noto Sans CJK SC; Source Serif 4 Semibold, then Noto for CJK | The stylesheet's `--font-ui` and `--font-serif` stacks, looked up among the host's fonts with fontdb |
| CJK serif | No (CJK falls back to Noto Sans) | The host's Songti SC or SimSun, as in the Tauri GUI |
| Exported PNG | The same on every host | Depends on the host's fonts, as Tauri's did |
| Build | Fetches Noto once (the network exception) | Fetches nothing |
| Additional embedded font data | About 16.7 MB | 0 bytes |

The system build checks the full Unicode character map of each named face.
The proportional chain starts with the stylesheet's named faces and egui's
default faces. Additional host fonts supply missing characters. Proportional
upright faces take priority, and an installed monospace font can supply the
only available CJK glyphs. Private-use characters participate in the named
stacks. Each selected file has one backing allocation shared by all its
collection members. The monospace chain starts with Hack, and the default
Latin and emoji faces are available to both builds.

CI lints both builds on Linux, Windows and macOS and tests the system build on
Linux. The desktop tests use a small, fixed TTC to check member indices,
mixed-script glyphs, advances, shared data and monospace-only fallback.

### Font build input

The desktop crate embeds the unmodified Source Serif 4 Semibold OTF from
Adobe's 4.005R release. Its checksum and license are in
`apps/macinmeter-desktop/OFL-SourceSerif4.txt`.

In the `bundled-fonts` build, `build.rs` verifies the SHA-256 of Noto Sans
CJK SC and copies it into `OUT_DIR`; without the feature it does nothing. Set `MACINMETER_UI_FONT_PATH` to use a local copy. On a cache
miss it can download Noto from jsDelivr. `MACINMETER_UI_FONT_CACHE_DIR`
selects an absolute cache directory shared across profiles and checkouts.

jsDelivr sometimes answers the Noto request with a 403 that it caches for 60 s
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
  PNG/SVG image export, per-entry PNG copying, and a structured-result view.
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

### Result cards

The window and the image export draw one card. `report_image::lay_out_card`
turns an entry into rectangles and baseline-positioned text at a given width.
The window paints those shapes with egui (`report_image::paint`) and lays its
PNG/MD/Details buttons over the header; the export writes the same shapes as
SVG. This is how the Tauri GUI's window and its html-to-image capture shared
one DOM, and the two cannot drift apart. A batch row shows the same card
without the channel table, with its warning count in the warning band, until
its details are opened. Below 690 px the summary grid uses two columns, with
Track DR across the first row. When the channel table exceeds the available
width, each channel lists its labeled measurements vertically. Screen readers
receive text nodes for the painted labels, including full elided file names.

### Image export

`src/report_image.rs` lays out one card per visible entry at a fixed 900 px
width, from the report itself rather than from the window. The card follows
the Tauri card's stylesheet (`.directory-entry` and its children): a tinted
header band, a five-cell summary grid with dividers, a format band, the channel
table with role pills, and a warning band, clipped to a 15 px rounded border. It wraps text using
the selected faces' glyph advances, read with skrifa. Each page is serialised as
SVG made of `<rect>` and `<text>` elements. The Tauri export instead wrapped
cloned HTML in `<foreignObject>`, which only a browser renders, and its output
depended on the window width and the host's fonts.

- **Content**: strings come from the same formatters and locale tables as the
  window. The export follows the screen: visible entries in on-screen order,
  with the path and warning toggles applied. Every card carries its full
  channel table. A batch card reads it from `BatchDetailStore`. Hiding paths
  also hides free-form failure details that can contain filesystem paths. The
  error code and application message remain visible.
- **PNG** uses resvg at up to 2× within a 16384 px / 32 MP canvas. Each
  text run names the exact font and collection index used in the window.
  Adjacent graphemes that share a face are shaped together. Mixed Latin, CJK
  and emoji runs continue at the previous run's final horizontal position.
- **Typography** in a standalone SVG uses the stylesheet's font stacks and
  600–750 weights. Card baselines use each chain's primary face metrics.
  The window and PNG synthesise heavier sans text with a second paint pass
  and a thin stroke, respectively.
- **Pagination**: as Tauri did, at 4000 px of content per PNG page and
  12000 px per SVG page. One page goes to a chosen file; several go to a chosen
  folder as `name_01.png`, `name_02.png`, and so on.
- **Workers**. Before the destination dialog, a worker measures one card at a
  time and keeps the order and page boundaries. After selection, the writer
  renders and atomically saves one page at a time. Each saved page wakes the
  progress display. A cancelled dialog restores the previous status line.
- **Clipboard**. The PNG button renders the card and writes the image to the
  system clipboard on the image worker. The status line reports the result of
  the native write.
- **Determinism**: the same report, language and toggles give the same SVG
  text. PNG bytes are not claimed to be identical across hosts.

To look at sample pages:

```bash
MACINMETER_IMAGE_SAMPLE_DIR=/tmp/out cargo test -p macinmeter-desktop writes_sample_images -- --ignored
```

### Large batches

The page lays out the cards that intersect the viewport. A height index
locates the visible range and updates measured heights in logarithmic time.
Selecting a summary borrows one detail from the batch store. Complete batch
documents are materialized for explicit JSON operations.

## Not yet

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
