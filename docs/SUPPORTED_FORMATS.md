[English](SUPPORTED_FORMATS.md) | [中文](SUPPORTED_FORMATS_CN.md)

# M0 supported audio formats

MacinMeter 0.2.0 intentionally exposes a small, correctness-first decoder
surface. Availability means that the route is part of the M0 contract; it does
not imply compatibility with the reference DR plugin.

| Container | Accepted codec | PCM delivered to analysis |
|---|---|---|
| WAV / WAVE | 8/16/24/32-bit linear integer PCM | finite interleaved `f32` |
| WAV / WAVE | IEEE 32/64-bit float PCM | finite interleaved `f32` |
| FLAC | FLAC | finite interleaved `f32` |
| AIFF | 8/16/24/32-bit linear integer PCM | finite interleaved `f32` |

The decoder probes file contents with no extension hint. Extensions
`.wav`, `.wave`, `.flac`, `.aif`, and `.aiff` are used only to discover files
inside directories. An explicitly supplied file may have any extension if its
content is supported.

## Deliberately unavailable in M0

The following routes are not built into 0.2.0:

- AIFC, compressed WAV variants, and non-FLAC codecs in supported containers;
- MP1/MP2/MP3, AAC, ALAC, Vorbis, Opus, AC-3, E-AC-3, DTS, and DSD;
- MP4/M4A, Ogg, Matroska/WebM, DSF, and DFF containers;
- FFmpeg fallback or external decoder processes;
- resampling, gain, filters, edge trimming, and silence preprocessing;
- packet-level or file-level parallel decoding.

Recognized but unavailable content returns the stable
`unsupported_format` error code. Malformed content within a supported format
returns a probe or decode error; it is never converted to an empty or partial
successful report.

## Decoder contract

Each opened source has immutable PCM stream information. `read_block` returns
only a non-empty, finite, frame-aligned block, sticky EOF, or a structured
error. Empty waits and decoder failures are not reported as EOF. Expected and
decoded frame counts are tracked separately, and M0 fails rather than silently
skipping a damaged packet.

Channel layout is never inferred from channel count. If the backend cannot
establish a trustworthy layout, the report uses `unknown`; consequently no
`without_lfe` aggregate is produced. Aggregate objects preserve every exclusion
reason even when no channel is measurable; their DR values are then explicit
`null`.
