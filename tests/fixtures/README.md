# Product test fixtures

`native-pcm-v1/` is the canonical M2 product codec corpus. Its committed files,
manifest, exact PCM definitions, hashes, provenance, and pinned regeneration
procedure are documented in [`native-pcm-v1/README.md`](native-pcm-v1/README.md).
These inputs test MacinMeter's decoder/application contracts; they are not
foo_dr_meter observations or numeric goldens.

`malformed-media-v1/` is the fixed malformed/mutation regression corpus from
ADR-0003 §8: deterministic byte-level derivations of `native-pcm-v1` fixtures
whose recorded structured failures gate the decoder against panics, hangs, and
partial success. See
[`malformed-media-v1/README.md`](malformed-media-v1/README.md).

The older WAV files directly in this directory predate the product manifest.
They remain deterministic regression inputs used by existing application and
CLI tests, but their original generation and provenance are not retroactively
claimed by `native-pcm-v1`. Replace or register them explicitly before using
them as evidence for any new codec capability.
