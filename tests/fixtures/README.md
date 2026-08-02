# Product test fixtures

`native-pcm-v1/` is the canonical M2 product codec corpus. Its committed files,
manifest, exact PCM definitions, hashes, provenance, and pinned regeneration
procedure are documented in [`native-pcm-v1/README.md`](native-pcm-v1/README.md).
These inputs test MacinMeter's decoder/application contracts; they are not
foo_dr_meter observations or numeric goldens.

`native-pcm-extensible-v1/` contains classic/Extensible WAV twins for the
constrained WAVE_FORMAT_EXTENSIBLE route. `native-alac-v1/` contains 16/24-bit,
1–8-channel MP4/M4A + ALAC fixtures and bit-identical WAV twins. Its committed
bytes are generated with pinned FFmpeg 8.0.1, while ordinary tests require no
FFmpeg installation. Both corpora record hashes, format geometry, PCM
fingerprints, provenance, and deterministic regeneration instructions in their
manifests and READMEs.

`malformed-media-v1/` is the fixed malformed/mutation regression corpus from
ADR-0003 §8: deterministic byte-level derivations of the three native fixture
corpora whose recorded structured failures gate the decoder against panics,
hangs, and partial success. See
[`malformed-media-v1/README.md`](malformed-media-v1/README.md).

The remaining older WAV files directly in this directory predate the product
manifest and are still used by application, CLI, or Tauri integration tests.
Their original generation and provenance are not retroactively claimed by
`native-pcm-v1`. Five unreferenced legacy WAVs were removed during M5; add a
new fixture only with an active test contract and record it explicitly before
using it as evidence for any codec capability.
