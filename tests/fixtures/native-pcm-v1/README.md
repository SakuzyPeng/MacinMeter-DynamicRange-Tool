# native-pcm-v1

This is the committed M2 product codec corpus for the 0.2.0 native matrix:

- classic RIFF/WAVE integer PCM at 8/16/24/32 bits;
- classic RIFF/WAVE IEEE float PCM at 32/64 bits;
- uncompressed AIFF signed integer PCM at 8/16/24/32 bits;
- stereo 16-bit FLAC with 400 frames and a pinned 192-frame encoder block size.

Every PCM fixture uses four stereo frames chosen to distinguish sign,
endianness, interleaving, full scale, and normalization. The float fixtures
also cover signed zero, normal/subnormal boundaries, values outside `[-1, 1]`,
and binary64 values that cannot round-trip through `f32`. The FLAC fixture uses
two distinct deterministic channel sequences and produces multiple decoder
blocks without making exact packet boundaries part of the public contract.

[`manifest.json`](manifest.json) records byte and normalized-interleaved-`f64`
SHA-256 hashes, geometry, encoding, the independent PCM oracle, and provenance
for every file. The inputs are project-authored numerical signals, contain no
third-party audio, and are distributed under the repository's MIT license.
They are product fixtures, not reference observations or goldens.

Regenerate with:

```bash
python3 scripts/generate-native-pcm-v1.py
python3 scripts/generate-native-pcm-v1.py --check
```

WAV and AIFF generation uses only the Python standard library. Reproducing the
committed FLAC bytes is pinned to reference `flac 1.5.0`; ordinary builds and
tests consume the committed file and never invoke an external encoder or the
network. A damaged-FLAC regression is derived in memory using the mutation
registered in the manifest rather than committing a second binary.
