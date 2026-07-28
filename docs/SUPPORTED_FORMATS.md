[English](SUPPORTED_FORMATS.md) | [中文](SUPPORTED_FORMATS_CN.md)

# Stable audio formats

MacinMeter intentionally exposes a small, correctness-first decoder surface.
Availability here describes the current stable development surface; released
version records remain historical. Reference-analysis evidence is documented
separately from decoder support.

| Container | Accepted codec | PCM delivered to analysis |
|---|---|---|
| RIFF/WAVE, classic or accepted WAVE_FORMAT_EXTENSIBLE | 8/16/24/32-bit linear integer PCM | finite interleaved `f64` |
| RIFF/WAVE, classic or accepted WAVE_FORMAT_EXTENSIBLE | IEEE 32/64-bit float PCM | finite interleaved `f64` |
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

The committed
[`native-pcm-v1`](../tests/fixtures/native-pcm-v1/README.md) product corpus
locks every declared PCM bit depth to an independent raw-bit normalization
oracle. Its FLAC case is stereo and multi-block, and AIFF/FLAC also pass the
shared Rust API and CLI report boundary. These are product contract fixtures,
not reference-plugin goldens.

The separate
[`native-pcm-extensible-v1`](../tests/fixtures/native-pcm-extensible-v1/README.md)
corpus pairs every accepted Extensible shape with a classic WAV carrying the
same PCM. Extensible input requires an exact 40-byte `fmt` chunk, `cbSize=22`,
the complete PCM or IEEE-float sub-format GUID, and valid bits equal to the
container width. A zero channel mask is accepted as unspecified/direct-out; a
nonzero mask must use only the standard low 18 speaker bits and its population
must match the channel count. The stable Extensible route accepts 1–26
channels and keeps reported channel layout `unknown`.

## Deliberately unavailable

The following routes are not built into the current stable surface:

- padded or unspecified-valid-bit WAVE_FORMAT_EXTENSIBLE, Extensible streams
  above 26 channels, reserved channel-mask bits, AIFC, compressed WAV variants,
  and non-FLAC codecs in supported containers;
- MP1/MP2/MP3, AAC, ALAC, Vorbis, Opus, AC-3, E-AC-3, DTS, and DSD;
- MP4/M4A, Ogg, Matroska/WebM, DSF, and DFF containers;
- FFmpeg fallback or external decoder processes;
- resampling, gain, filters, edge trimming, and silence preprocessing;
- packet-level or file-level parallel decoding.

The stable AIFF route also requires a finite, positive, exactly integral
80-bit sample rate representable as `u32`, an exact 18-byte COMM chunk, plus
zero SSND offset and block-size fields. The stable FLAC route requires a
nonzero STREAMINFO total sample count: without it the end-of-stream frame
check is inert, and a stream whose MD5 signature is also absent could lose
whole tail frames undetectably. Unsupported container variants are rejected
rather than silently rounded or inherited from backend behavior.

Recognized but unavailable content returns the stable
`unsupported_format` error code. Detectable malformed content within a
supported format returns a probe or decode error; it is never converted to an
empty or partial successful report. Physical EOF can only be checked against
declared frame counts or codec integrity evidence: when an input omits both,
the decoder does not claim that every frame-boundary tail truncation is
detectable.

## Decoder contract

Each opened source has immutable PCM stream information. `read_block` returns
only a non-empty, finite, frame-aligned block, sticky EOF, or a structured
error. Empty waits and decoder failures are not reported as EOF. Expected and
decoded frame counts are tracked separately, and the stable route fails rather
than silently skipping a damaged packet.

Product analysis accepts at most 64 PCM channels. This is a resource contract,
not a claim that every current container/backend can represent 64 channels;
format-specific limits may be lower. A source declaring more than 64 channels
is rejected during probing with `unsupported_format`, before decoder creation.

Channel layout is never inferred from channel count. If the backend cannot
establish a trustworthy layout, the report uses `unknown`. Analysis produces
one `track` aggregate and, following the recorded numeric rule, includes LFE
rather than producing a separate `without_lfe` result.
Silent channels visibly remain `silent` and contribute DR0; only insufficient
data is excluded. If no channel can contribute, aggregate DR values are
explicit `null`.
