# Native PCM Extensible v1

This immutable capability corpus pairs each accepted `WAVE_FORMAT_EXTENSIBLE`
shape with a classic WAV carrying the exact same PCM bytes. It covers integer
8/16/24/32-bit and IEEE float32/float64 PCM, standard and zero channel masks,
mono, stereo, six-channel, and the 26-channel backend boundary.

Regenerate or audit it without network access or external media tools:

```bash
python3 scripts/generate-native-pcm-extensible-v1.py
python3 scripts/generate-native-pcm-extensible-v1.py --check
```

`manifest.json` binds every file's bytes, format geometry, twin identity, and
normalized finite interleaved little-endian `f64` SHA-256 oracle. All audio is
deterministically generated under the repository's MIT license.
