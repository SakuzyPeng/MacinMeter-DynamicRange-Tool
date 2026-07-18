[English](SUPPORTED_FORMATS.md) | [中文](SUPPORTED_FORMATS_CN.md)

# M0 supported audio formats

MacinMeter 0.2.0 intentionally exposes a small, correctness-first decoder
surface. Availability means that the route is part of the M0 contract; it does
not imply compatibility with foo_dr_meter 1.0.8. The current analysis profile
is a candidate and remains `Unverified`.

| Container | Accepted codec | PCM delivered to analysis |
|---|---|---|
| WAV / WAVE | 8/16/24/32-bit linear integer PCM | finite interleaved `f64` |
| WAV / WAVE | IEEE 32/64-bit float PCM | finite interleaved `f64` |
| FLAC | FLAC | finite interleaved `f64` |
| AIFF | 8/16/24/32-bit linear integer PCM | finite interleaved `f64` |

The fixed x64 1.0.8 reference core also consumes `f64`. Moving the product PCM
path to `f64` closed the two source-f64 differences exposed by the current
safe-master corpus, whose comparable core fields now match at 39/39 track DR
and 62/62 channel DR tokens. This does not establish bit-identical decoder
normalization for every host, container, runtime boundary, or input.

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
establish a trustworthy layout, the report uses `unknown`. Candidate V1
produces one `track` aggregate and, following the evidence-backed candidate
rule, includes LFE rather than producing a separate `without_lfe` result.
Silent channels visibly remain `silent` and contribute DR0; only insufficient
data is excluded. If no channel can contribute, aggregate DR values are
explicit `null`.
