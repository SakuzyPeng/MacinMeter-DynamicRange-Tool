# MacinMeter DR Tool

[English](README.md) | [中文](README_CN.md)

MacinMeter measures the **dynamic range (DR)** of audio files. It runs entirely
on your machine — nothing is uploaded, and no network connection is used.

It reads WAV, FLAC, AIFF, and Apple Lossless (`.m4a` / `.mp4`) files and reports
a DR value for each channel and one for the track as a whole. There is a
command-line tool and a desktop app; both give the same numbers.

## Download

Get the latest build from the [releases page](../../releases/latest).

| Your system | File |
| --- | --- |
| macOS on Apple Silicon (M1 or newer), macOS 11 or later | `macinmeter-gui-…-aarch64-apple-darwin.dmg` |
| Windows, 64-bit | `macinmeter-gui-…-x86_64-pc-windows-msvc-setup.exe` |

The `macinmeter-cli-…` archives contain the command-line tool on its own.

### Your system will warn you the first time

**This is expected.** The downloads are not code-signed, so:

- **macOS** may refuse to open the app. Right-click it and choose **Open**, then
  confirm. You only need to do this once.
- **Windows** may show a blue "Windows protected your PC" screen. Click **More
  info**, then **Run anyway**.

Signing would require a certificate issued to a named individual, and that name
would then be embedded in every file published here. This project is maintained
by one person who would rather not attach their legal name to every download, so
neither platform is signed. If you would rather not accept that trade, you can
[build from source](docs/INTERNALS.md#building-from-source) instead.

You can check that a download is intact using the `SHA256SUMS` file on the
release page.

## Using the desktop app

Drag files or a folder onto the window, or use the buttons to pick them, then
press Analyze. Results can be copied as Markdown or exported as JSON, PNG, or
SVG. The interface is available in English and Chinese.

## Using the command line

```bash
mdrmeter analyze "01 - Song.flac"
mdrmeter batch "My Album/" --recursive
```

`batch` always lists files in the order you gave them, whatever order they
finish in, and one unreadable file does not stop the rest. It reports each track
on its own and does not compute an album DR.

Real output from a test file included in this repository:

```text
MacinMeter
Source: tests/fixtures/edge_cases.wav
PCM: 44100 Hz, 2 channels, 308700 frames
Duration: 0:07

CH 1: DR2 (2.4300 dB), overall RMS -2.43 dBFS, selected DR peak 1.00000000
CH 2: DR2 (2.4300 dB), overall RMS -2.43 dBFS, selected DR peak 1.00000000

Track aggregate: DR2 (2.4300 dB; 2 contributing channels)
Report levels: peak 0.00 dBFS, RMS -2.43 dBFS

Elapsed: 0.002 s (2929.6x realtime)
```

That file is a synthetic test fixture, not an example of a real music release.

Useful options:

| Option | Effect |
| --- | --- |
| `--format json` | machine-readable output instead of text |
| `--output PATH` | write the report to a file instead of the screen |
| `--recursive` | with `batch`, also search inside subfolders |
| `--timing` | also show how long decoding and analysis each took |

Shell completions are available for bash, zsh, fish, PowerShell, and elvish:

```bash
mdrmeter completions zsh > "${fpath[1]}/_mdrmeter"
```

## Reading the result

- **`DR2`** is the track's DR value. A larger number means a larger gap between
  the peaks and the loud parts' average level. A high DR value does not by
  itself mean a recording sounds good, but a very low one often indicates heavy
  compression and a more likely compromised master. Genre and artistic intent
  still matter.
- Each **`CH`** line is one channel's own DR, its overall RMS, and the peak the
  DR calculation selected.
- **`Report levels`** are whole-track measurements. The peak reported there is a
  different quantity from the selected DR peak above.
- **dBFS** treats amplitude `1.0` as 0 dB. Floating-point audio can legitimately
  contain samples above that, so 0 dBFS is not always a clipping point.
- **Silent channels** stay visible and count as DR0. Channels with too little
  data to measure are excluded and said to be excluded.
- **`Elapsed`** and the realtime multiple describe this run on this machine, not
  the analysis itself, so they appear only in the text output. JSON stays
  identical between runs of the same file.

Some reports add warnings — for example when a file is shorter than one analysis
window, or when a multichannel file's speaker layout is unknown and the track
value may therefore include an LFE channel. These never change the numbers.

### Exit codes

| Code | Meaning |
| ---: | --- |
| `0` | everything analyzed successfully |
| `1` | failure, no input, every batch item failed, or the report could not be written |
| `2` | invalid command-line arguments |
| `3` | batch finished with both successes and failures |
| `130` | cancelled |

## Which files work

| Format | Supported |
| --- | --- |
| WAV | 8/16/24/32-bit integer, 32/64-bit floating point |
| FLAC | native FLAC files |
| AIFF | 8/16/24/32-bit integer |
| Apple Lossless in `.m4a` / `.mp4` | 16/24-bit, 1–8 channels |

Folder scans look for `.wav`, `.wave`, `.flac`, `.aif`, `.aiff`, `.m4a`, and
`.mp4`. A supported file with some other extension still works if you pass its
path directly, because files are identified by content rather than by name.

Not every file with these extensions will work. AAC, MP3, Ogg, Opus, and DSD are
not supported at all, and some less common variants of the formats above are not
either. MacinMeter says so plainly instead of guessing: it never converts,
resamples, or falls back to another decoder. The [format
guide](docs/SUPPORTED_FORMATS.md) lists the exact rules and every excluded
variant.

## How accurate is it

The DR algorithm was reconstructed from one specific program, `foo_dr_meter
1.0.8 x64`, with the original author's permission. On a fixed set of recorded
test inputs, MacinMeter's results match that program's exactly — 39 of 39 tracks
and 62 of 62 channels, with no differences.

That is a precise claim about a specific test set, not a promise about every
file in the world. What was compared, and what was not, is listed in the
[accuracy record](docs/INTERNALS.md#accuracy).

## More

- [Format guide](docs/SUPPORTED_FORMATS.md) — exactly which files work and why
- [Technical notes](docs/INTERNALS.md) — architecture, Rust API, accuracy
  evidence, performance measurements
- [Release and packaging status](docs/RELEASE.md)
- [Legal notes](docs/LEGAL.md) and [third-party notices](THIRD_PARTY_NOTICES.md)

MacinMeter is released under the [MIT License](LICENSE).

The reference target is Janne Hyvärinen's `foo_dr_meter 1.0.8 x64` component.
Reverse-engineering it was done with the author's permission; only a [minimal
public authorization summary](reference/authorization/README.md) is kept in this
repository.
