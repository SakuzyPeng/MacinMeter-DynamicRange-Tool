#!/usr/bin/env python3
"""Generate the committed M2 malformed-media regression corpus.

Every case is a deterministic byte-level derivation of a committed native PCM
fixture (or a deterministic synthetic byte string), so the corpus can be
regenerated and audited without any external media. Expected outcomes are
recorded as structured product error codes and stages; they are regression
targets captured from the product, not a claim about all possible byte inputs.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import struct
import tempfile
from pathlib import Path

CORPUS_ID = "malformed-media-v1"
SOURCE_CORPUS = "native-pcm-v1"
EXTENSIBLE_SOURCE_CORPUS = "native-pcm-extensible-v1"
ALAC_SOURCE_CORPUS = "native-alac-v1"

WAV_S16 = "wav-pcm-s16-stereo.wav"
WAV_F32 = "wav-float32-stereo.wav"
AIFF_S16 = "aiff-pcm-s16-stereo.aiff"
FLAC_S16 = "flac-pcm-s16-stereo-multiblock.flac"
WAV_EXTENSIBLE_S16 = "pcm-s16-stereo-mask-extensible.wav"
ALAC_S16 = "alac16-stereo-48000-multipacket.m4a"
ALAC_AAC = "unsupported-aac.m4a"
ALAC_FRAGMENTED = "unsupported-fragmented-alac.mp4"
ALAC_MULTITRACK = "unsupported-multitrack-alac.mp4"
ALAC_VIDEO = "unsupported-video-alac.mp4"


def xorshift64(seed: int) -> int:
    """One fixed xorshift64 step used for all seeded masks."""
    value = seed & 0xFFFF_FFFF_FFFF_FFFF
    if value == 0:
        raise ValueError("xorshift64 seed must be nonzero")
    value ^= (value << 13) & 0xFFFF_FFFF_FFFF_FFFF
    value ^= value >> 7
    value ^= (value << 17) & 0xFFFF_FFFF_FFFF_FFFF
    return value


def seeded_u32_mask(seed: int) -> int:
    mask = xorshift64(seed) & 0xFFFF_FFFF
    if mask == 0:
        mask = 1
    return mask


def truncate(data: bytes, length: int) -> bytes:
    if not 0 <= length < len(data):
        raise ValueError(f"truncation length {length} is not a strict prefix")
    return data[:length]


def patch(data: bytes, offset: int, replacement: bytes) -> bytes:
    if offset + len(replacement) > len(data):
        raise ValueError("patch exceeds the source bytes")
    return data[:offset] + replacement + data[offset + len(replacement) :]


def insert(data: bytes, offset: int, addition: bytes) -> bytes:
    if not 0 <= offset <= len(data):
        raise ValueError("insert offset is outside the source bytes")
    return data[:offset] + addition + data[offset:]


def xor_at(data: bytes, offset: int, mask: bytes) -> bytes:
    original = data[offset : offset + len(mask)]
    if len(original) != len(mask):
        raise ValueError("xor mask exceeds the source bytes")
    mutated = bytes(a ^ b for a, b in zip(original, mask))
    if mutated == original:
        raise ValueError("xor mask must change the source bytes")
    return patch(data, offset, mutated)


def extended80(sign_exponent: int, significand: int) -> bytes:
    return struct.pack(">HQ", sign_exponent, significand)


def zero_flac_verification(flac: bytes) -> bytes:
    """Zero the STREAMINFO 36-bit total-sample count and the MD5 signature.

    With both fields absent, a stream truncated exactly on a frame boundary is
    undetectable in principle, which is why the stable FLAC route rejects such
    streams at probe time.
    """
    mutated = bytearray(flac)
    mutated[21] &= 0xF0
    mutated[22:26] = b"\x00" * 4
    mutated[26:42] = b"\x00" * 16
    return bytes(mutated)


def deterministic_noise(seed: int, length: int) -> bytes:
    out = bytearray()
    state = seed
    while len(out) < length:
        state = xorshift64(state)
        out.extend(state.to_bytes(8, "little"))
    return bytes(out[:length])


def find_fourcc(data: bytes, kind: bytes, occurrence: int = 0) -> int:
    start = 0
    for _ in range(occurrence + 1):
        found = data.find(kind, start)
        if found < 0:
            raise ValueError(f"missing {kind!r} occurrence {occurrence}")
        start = found + len(kind)
    return found


def find_alac_config_type(data: bytes) -> int:
    start = 0
    marker = b"alac\x00\x00\x00\x00\x00\x00\x10\x00"
    found = data.find(marker, start)
    if found < 4:
        raise ValueError("missing nested ALAC configuration box")
    size = struct.unpack(">I", data[found - 4 : found])[0]
    if size not in (36, 60):
        raise ValueError(f"unexpected nested ALAC configuration size {size}")
    return found


def find_outer_alac_type(data: bytes) -> int:
    config = find_alac_config_type(data)
    found = data.rfind(b"alac", 0, config)
    if found < 4:
        raise ValueError("missing outer ALAC sample entry")
    return found


def add_alac_explicit_layout(data: bytes, layout_tag: int) -> bytes:
    config = find_alac_config_type(data)
    outer = find_outer_alac_type(data)
    config_start = config - 4
    config_size = struct.unpack(">I", data[config_start:config])[0]
    if config_size != 36:
        raise ValueError("explicit-layout mutation requires a 24-byte ALAC cookie")
    insertion = config_start + config_size
    layout = struct.pack(">I4sIIII", 24, b"chan", 0, layout_tag, 0, 0)

    updated = data
    for box_type in (b"moov", b"trak", b"mdia", b"minf", b"stbl", b"stsd"):
        type_offset = find_fourcc(updated, box_type)
        size_offset = type_offset - 4
        size = struct.unpack(">I", updated[size_offset:type_offset])[0]
        updated = patch(updated, size_offset, struct.pack(">I", size + len(layout)))
    outer_size = struct.unpack(">I", updated[outer - 4 : outer])[0]
    updated = patch(updated, outer - 4, struct.pack(">I", outer_size + len(layout)))
    updated = patch(
        updated,
        config_start,
        struct.pack(">I", config_size + len(layout)),
    )
    return insert(updated, insertion, layout)


def build_cases(sources: dict[str, bytes]) -> list[dict[str, object]]:
    wav = sources[WAV_S16]
    wav_f32 = sources[WAV_F32]
    aiff = sources[AIFF_S16]
    flac = sources[FLAC_S16]
    wav_extensible = sources[WAV_EXTENSIBLE_S16]
    alac = sources[ALAC_S16]

    alac_config = find_alac_config_type(alac)
    alac_cookie = alac_config + 8
    alac_outer = find_outer_alac_type(alac)
    alac_elst = find_fourcc(alac, b"elst")
    alac_stts = find_fourcc(alac, b"stts")
    alac_stsz = find_fourcc(alac, b"stsz")
    alac_mdhd = find_fourcc(alac, b"mdhd")
    alac_moov = find_fourcc(alac, b"moov")
    alac_mdat = find_fourcc(alac, b"mdat")
    alac_unknown_layout = add_alac_explicit_layout(alac, 0x1234_0002)
    alac_mismatched_layout = add_alac_explicit_layout(alac, 0x0064_0001)
    alac_zero_frames = patch(alac, alac_mdhd + 20, struct.pack(">I", 0))
    alac_stts_entry_count = struct.unpack(">I", alac[alac_stts + 8 : alac_stts + 12])[0]
    for entry in range(alac_stts_entry_count):
        alac_zero_frames = patch(
            alac_zero_frames,
            alac_stts + 16 + entry * 8,
            struct.pack(">I", 0),
        )

    extensible_with_extra = insert(wav_extensible, 60, b"\0\0")
    extensible_with_extra = patch(
        extensible_with_extra,
        4,
        struct.pack("<I", struct.unpack("<I", wav_extensible[4:8])[0] + 2),
    )
    extensible_with_extra = patch(extensible_with_extra, 16, struct.pack("<I", 42))
    extensible_with_extra = patch(extensible_with_extra, 36, struct.pack("<H", 24))

    # wav-pcm-s16-stereo.wav layout: RIFF size at 4, fmt chunk at 12
    # (format tag 20, channels 22, bits 34), data chunk header at 36.
    # aiff-pcm-s16-stereo.aiff layout: FORM size at 4, COMM at 12 (size 16,
    # channels 20, frames 22, bits 26, rate 28), SSND at 38 (size 42,
    # offset 46, block size 50).
    cases: list[dict[str, object]] = [
        {
            "id": "wav-truncated-signature",
            "source": WAV_S16,
            "operation": "truncate to the first 8 bytes",
            "bytes": truncate(wav, 8),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "wav-truncated-mid-fmt",
            "source": WAV_S16,
            "operation": "truncate to the first 30 bytes",
            "bytes": truncate(wav, 30),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "wav-riff-size-overrun",
            "source": WAV_S16,
            "operation": "set the RIFF size field to 60 (u32le at offset 4)",
            "bytes": patch(wav, 4, struct.pack("<I", 60)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "wav-riff-size-underrun",
            "source": WAV_S16,
            "operation": "set the RIFF size field to 28 (u32le at offset 4)",
            "bytes": patch(wav, 4, struct.pack("<I", 28)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "wav-data-length-overrun",
            "source": WAV_S16,
            "operation": "set the data chunk length to 0xFFFFFF00 (u32le at offset 40)",
            "bytes": patch(wav, 40, struct.pack("<I", 0xFFFF_FF00)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "wav-zero-channels",
            "source": WAV_S16,
            "operation": "set the fmt channel count to 0 (u16le at offset 22)",
            "bytes": patch(wav, 22, struct.pack("<H", 0)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "wav-mp3-format-tag",
            "source": WAV_S16,
            "operation": "set the fmt format tag to 0x0055 (u16le at offset 20)",
            "bytes": patch(wav, 20, struct.pack("<H", 0x0055)),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "wav-extensible-format-tag",
            "source": WAV_S16,
            "operation": "set the fmt format tag to 0xFFFE (u16le at offset 20)",
            "bytes": patch(wav, 20, struct.pack("<H", 0xFFFE)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "wav-extensible-unsupported-guid",
            "source": f"{EXTENSIBLE_SOURCE_CORPUS}/{WAV_EXTENSIBLE_S16}",
            "operation": "replace the sub-format GUID tag with 0x00000002 (u32le at offset 44)",
            "bytes": patch(wav_extensible, 44, struct.pack("<I", 2)),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "wav-extensible-guid-tail-mismatch",
            "source": f"{EXTENSIBLE_SOURCE_CORPUS}/{WAV_EXTENSIBLE_S16}",
            "operation": "xor the final sub-format GUID byte at offset 59 with 0x01",
            "bytes": xor_at(wav_extensible, 59, b"\x01"),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "wav-extensible-fmt-size-39",
            "source": f"{EXTENSIBLE_SOURCE_CORPUS}/{WAV_EXTENSIBLE_S16}",
            "operation": "set fmt_size to 39 (u32le at offset 16)",
            "bytes": patch(wav_extensible, 16, struct.pack("<I", 39)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "wav-extensible-coherent-extra-extension",
            "source": f"{EXTENSIBLE_SOURCE_CORPUS}/{WAV_EXTENSIBLE_S16}",
            "operation": "grow fmt_size/cbSize to coherent 42/24 and insert two extension bytes",
            "bytes": extensible_with_extra,
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "wav-extensible-incoherent-cbsize",
            "source": f"{EXTENSIBLE_SOURCE_CORPUS}/{WAV_EXTENSIBLE_S16}",
            "operation": "set cbSize to 23 while fmt_size remains 40 (u16le at offset 36)",
            "bytes": patch(wav_extensible, 36, struct.pack("<H", 23)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "wav-extensible-zero-valid-bits",
            "source": f"{EXTENSIBLE_SOURCE_CORPUS}/{WAV_EXTENSIBLE_S16}",
            "operation": "set valid bits to 0 (u16le at offset 38)",
            "bytes": patch(wav_extensible, 38, struct.pack("<H", 0)),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "wav-extensible-valid-bits-over-container",
            "source": f"{EXTENSIBLE_SOURCE_CORPUS}/{WAV_EXTENSIBLE_S16}",
            "operation": "set valid bits to 17 for a 16-bit container (u16le at offset 38)",
            "bytes": patch(wav_extensible, 38, struct.pack("<H", 17)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "wav-extensible-padded-valid-bits",
            "source": f"{EXTENSIBLE_SOURCE_CORPUS}/{WAV_EXTENSIBLE_S16}",
            "operation": "set valid bits to 15 for a 16-bit container (u16le at offset 38)",
            "bytes": patch(wav_extensible, 38, struct.pack("<H", 15)),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "wav-extensible-reserved-channel-mask",
            "source": f"{EXTENSIBLE_SOURCE_CORPUS}/{WAV_EXTENSIBLE_S16}",
            "operation": "set channel mask to 0x00040001 with a reserved speaker bit",
            "bytes": patch(wav_extensible, 40, struct.pack("<I", 0x0004_0001)),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "wav-extensible-channel-mask-popcount-mismatch",
            "source": f"{EXTENSIBLE_SOURCE_CORPUS}/{WAV_EXTENSIBLE_S16}",
            "operation": "set the stereo channel mask to one speaker bit (u32le at offset 40)",
            "bytes": patch(wav_extensible, 40, struct.pack("<I", 1)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "wav-unaligned-data-length",
            "source": WAV_S16,
            "operation": "set the data chunk length to 15 (u32le at offset 40)",
            "bytes": patch(wav, 40, struct.pack("<I", 15)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "wav-12bit-depth",
            "source": WAV_S16,
            "operation": "set the fmt bits per sample to 12 (u16le at offset 34)",
            "bytes": patch(wav, 34, struct.pack("<H", 12)),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "wav-float-48bit-depth",
            "source": WAV_F32,
            "operation": "set the fmt bits per sample to 48 (u16le at offset 34)",
            "bytes": patch(wav_f32, 34, struct.pack("<H", 48)),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "wav-duplicate-fmt",
            "source": WAV_S16,
            "operation": 'rename the data chunk id to "fmt " (offset 36)',
            "bytes": patch(wav, 36, b"fmt "),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "aiff-truncated-comm",
            "source": AIFF_S16,
            "operation": "truncate to the first 20 bytes",
            "bytes": truncate(aiff, 20),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "aiff-comm-size-19",
            "source": AIFF_S16,
            "operation": "set the COMM chunk length to 19 (u32be at offset 16)",
            "bytes": patch(aiff, 16, struct.pack(">I", 19)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "aiff-negative-channels",
            "source": AIFF_S16,
            "operation": "set the COMM channel count to -2 (i16be at offset 20)",
            "bytes": patch(aiff, 20, struct.pack(">h", -2)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "aiff-frame-count-mismatch",
            "source": AIFF_S16,
            "operation": "set the COMM frame count to 5 (u32be at offset 22)",
            "bytes": patch(aiff, 22, struct.pack(">I", 5)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "aiff-nan-sample-rate",
            "source": AIFF_S16,
            "operation": "set the 80-bit sample rate exponent to 0x7FFF (offset 28)",
            "bytes": patch(aiff, 28, extended80(0x7FFF, 0x8000_0000_0000_0000)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "aiff-fractional-sample-rate",
            "source": AIFF_S16,
            "operation": "set the 80-bit sample rate to 44100.5 (offset 28)",
            "bytes": patch(aiff, 28, extended80(16_398, 88_201 << 47)),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "aiff-nonzero-ssnd-offset",
            "source": AIFF_S16,
            "operation": "set the SSND offset to 2 (u32be at offset 46)",
            "bytes": patch(aiff, 46, struct.pack(">I", 2)),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "aiff-duplicate-ssnd",
            "source": AIFF_S16,
            "operation": "append a copy of the SSND chunk and grow the FORM size by 32",
            "bytes": patch(aiff, 4, struct.pack(">I", 62 + 32)) + aiff[38:70],
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "aifc-signature",
            "source": AIFF_S16,
            "operation": 'replace the form type with "AIFC" (offset 8)',
            "bytes": patch(aiff, 8, b"AIFC"),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "flac-bad-magic",
            "source": FLAC_S16,
            "operation": 'replace the stream marker with "fLaX" (offset 0)',
            "bytes": patch(flac, 0, b"fLaX"),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "flac-truncated-streaminfo",
            "source": FLAC_S16,
            "operation": "truncate to the first 20 bytes",
            "bytes": truncate(flac, 20),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "flac-frame-byte-flip",
            "source": FLAC_S16,
            "operation": "xor the byte at offset 641 with 0x01",
            "bytes": xor_at(flac, 641, b"\x01"),
            "expected": {"code": "decode_failed", "stage": "decode"},
        },
        {
            "id": "flac-truncated-mid-frame",
            "source": FLAC_S16,
            "operation": "truncate to the first 600 bytes",
            "bytes": truncate(flac, 600),
            "expected": {"code": "decode_failed", "stage": "decode"},
        },
        {
            "id": "flac-terminal-byte-corruption",
            "source": FLAC_S16,
            "operation": "xor the final byte with 0xFF",
            "bytes": xor_at(flac, len(flac) - 1, b"\xff"),
            "expected": {"code": "decode_failed", "stage": "decode"},
        },
        {
            "id": "flac-huge-metadata-length",
            "source": FLAC_S16,
            "operation": "set the VORBIS_COMMENT block length to 0xFFFFFF (u24be at offset 43)",
            "bytes": patch(flac, 43, b"\xff\xff\xff"),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        # The outer 24-bit block length is bounded by the file, but the inner
        # Vorbis-comment lengths are independent 32-bit little-endian fields.
        # The 4 GiB declarations run under the verifier's 2 GiB RLIMIT_AS, so a
        # clean structured failure also proves no allocation proportional to
        # the declared inner length.
        {
            "id": "flac-vorbis-vendor-length-overrun",
            "source": FLAC_S16,
            "operation": "set the inner vendor string length to 0xFFFFFFFF (u32le at offset 46)",
            "bytes": patch(flac, 46, struct.pack("<I", 0xFFFF_FFFF)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "flac-vorbis-vendor-length-inner-overrun",
            "source": FLAC_S16,
            "operation": (
                "set the inner vendor string length to 0x00FFFFF0, larger than "
                "the 40-byte block but far below the file-bounded outer length"
            ),
            "bytes": patch(flac, 46, struct.pack("<I", 0x00FF_FFF0)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "flac-vorbis-comment-count-overrun",
            "source": FLAC_S16,
            "operation": "set the inner comment count to 0xFFFFFFFF (u32le at offset 82)",
            "bytes": patch(flac, 82, struct.pack("<I", 0xFFFF_FFFF)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "flac-unknown-total-samples",
            "source": FLAC_S16,
            "operation": "zero the STREAMINFO total-sample count and MD5 signature",
            "bytes": zero_flac_verification(flac),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "flac-unknown-total-samples-boundary-truncation",
            "source": FLAC_S16,
            "operation": (
                "zero the STREAMINFO total-sample count and MD5 signature, "
                "then truncate at the final frame boundary (offset 916)"
            ),
            "bytes": truncate(zero_flac_verification(flac), 916),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "flac-frame-boundary-truncation",
            "source": FLAC_S16,
            "operation": "truncate at the final frame boundary (offset 916)",
            "bytes": truncate(flac, 916),
            "expected": {"code": "decode_failed", "stage": "decode"},
        },
        {
            "id": "alac-truncated-ftyp",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "truncate inside the first ftyp payload",
            "bytes": truncate(alac, 10),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "alac-ftyp-size-overrun",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "set the first box size beyond the file length",
            "bytes": patch(alac, 0, struct.pack(">I", len(alac) + 1)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "alac-zero-box-size",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "set the ftyp box size to zero",
            "bytes": patch(alac, 0, struct.pack(">I", 0)),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "alac-missing-moov",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "rename the top-level moov box to free",
            "bytes": patch(alac, alac_moov, b"free"),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "alac-missing-mdat",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "rename the top-level mdat box to free",
            "bytes": patch(alac, alac_mdat, b"free"),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "alac-nonidentity-edit",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "set the sole edit-list media_time to one",
            "bytes": patch(alac, alac_elst + 16, struct.pack(">I", 1)),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "alac-cropped-edit-duration",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "shorten the identity edit-list segment duration by one movie tick",
            "bytes": patch(
                alac,
                alac_elst + 12,
                struct.pack(
                    ">I",
                    struct.unpack(">I", alac[alac_elst + 12 : alac_elst + 16])[0] - 1,
                ),
            ),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "alac-compatible-version-1",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "set ALAC compatibleVersion to one",
            "bytes": patch(alac, alac_cookie + 4, b"\x01"),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "alac-20bit",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "set the ALAC cookie bit depth to 20",
            "bytes": patch(alac, alac_cookie + 5, b"\x14"),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "alac-32bit",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "set the ALAC cookie bit depth to 32",
            "bytes": patch(alac, alac_cookie + 5, b"\x20"),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "alac-9channels",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "set the ALAC cookie channel count to nine",
            "bytes": patch(alac, alac_cookie + 9, b"\x09"),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "alac-nonzero-config-flags",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "set one ALAC configuration full-box flag",
            "bytes": patch(alac, alac_config + 7, b"\x01"),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "alac-nonstandard-explicit-layout",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "expand the cookie to 48 bytes with a nonstandard channel-layout tag",
            "bytes": alac_unknown_layout,
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "alac-explicit-layout-channel-mismatch",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "expand the stereo cookie with the standard mono channel-layout tag",
            "bytes": alac_mismatched_layout,
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "alac-channel-declaration-mismatch",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "set the sample-entry channel count to one while the cookie remains stereo",
            "bytes": patch(alac, alac_outer + 20, struct.pack(">H", 1)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "alac-sample-rate-mismatch",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "set the sample-entry 16.16 sample rate to 44100",
            "bytes": patch(alac, alac_outer + 28, struct.pack(">I", 44_100 << 16)),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "alac-stts-duration-mismatch",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "increment the first stts sample duration",
            "bytes": patch(
                alac,
                alac_stts + 16,
                struct.pack(">I", struct.unpack(">I", alac[alac_stts + 16 : alac_stts + 20])[0] + 1),
            ),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "alac-stsz-count-mismatch",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "increment the stsz packet count without adding a size entry",
            "bytes": patch(
                alac,
                alac_stsz + 12,
                struct.pack(">I", struct.unpack(">I", alac[alac_stsz + 12 : alac_stsz + 16])[0] + 1),
            ),
            "expected": {"code": "malformed_media", "stage": "probe"},
        },
        {
            "id": "alac-zero-frames",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "set mdhd duration and every stts packet duration to zero",
            "bytes": alac_zero_frames,
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "alac-corrupt-first-packet",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "xor the first byte of the first mdat packet with 0xff",
            "bytes": xor_at(alac, alac_mdat + 4, b"\xff"),
            "expected": {"code": "decode_failed", "stage": "decode"},
        },
        {
            "id": "alac-non-alac-sample-entry",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_S16}",
            "operation": "rewrite the outer sample entry type from alac to mp4a",
            "bytes": patch(alac, alac_outer, b"mp4a"),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "alac-aac-track",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_AAC}",
            "operation": "use a valid ISO BMFF file containing one AAC audio track",
            "bytes": sources[ALAC_AAC],
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "alac-fragmented",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_FRAGMENTED}",
            "operation": "use a valid fragmented ISO BMFF file containing ALAC",
            "bytes": sources[ALAC_FRAGMENTED],
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "alac-multiple-audio-tracks",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_MULTITRACK}",
            "operation": "use a valid ISO BMFF file containing two ALAC audio tracks",
            "bytes": sources[ALAC_MULTITRACK],
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "alac-video-track",
            "source": f"{ALAC_SOURCE_CORPUS}/{ALAC_VIDEO}",
            "operation": "use a valid ISO BMFF file containing video plus ALAC audio",
            "bytes": sources[ALAC_VIDEO],
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "unknown-content",
            "source": None,
            "operation": "64 deterministic xorshift64 bytes from seed 0x4D4D_0001",
            "bytes": deterministic_noise(0x4D4D_0001, 64),
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
        {
            "id": "empty-file",
            "source": None,
            "operation": "zero bytes",
            "bytes": b"",
            "expected": {"code": "unsupported_format", "stage": "probe"},
        },
    ]

    for channels in (27, 32, 64):
        cases.append(
            {
                "id": f"wav-extensible-{channels}-channels",
                "source": f"{EXTENSIBLE_SOURCE_CORPUS}/{WAV_EXTENSIBLE_S16}",
                "operation": f"set channel count to {channels} and channel mask to zero",
                "bytes": patch(
                    patch(wav_extensible, 22, struct.pack("<H", channels)),
                    40,
                    struct.pack("<I", 0),
                ),
                "expected": {"code": "unsupported_format", "stage": "probe"},
            }
        )

    for seed in (1, 2, 3):
        mask = seeded_u32_mask(seed)
        cases.append(
            {
                "id": f"wav-seeded-riff-flip-{seed}",
                "source": WAV_S16,
                "operation": f"xor the RIFF size field with xorshift64({seed}) mask 0x{mask:08x}",
                "seed": seed,
                "bytes": xor_at(wav, 4, struct.pack("<I", mask)),
                "expected": {"code": "malformed_media", "stage": "probe"},
            }
        )
        cases.append(
            {
                "id": f"aiff-seeded-form-flip-{seed}",
                "source": AIFF_S16,
                "operation": f"xor the FORM size field with xorshift64({seed}) mask 0x{mask:08x}",
                "seed": seed,
                "bytes": xor_at(aiff, 4, struct.pack(">I", mask)),
                "expected": {"code": "malformed_media", "stage": "probe"},
            }
        )

    identifiers = [case["id"] for case in cases]
    if len(identifiers) != len(set(identifiers)):
        raise ValueError("corpus case ids must be unique")
    return cases


def case_file_name(case: dict[str, object]) -> str:
    source = case["source"]
    suffix = Path(source).suffix if source else ".bin"
    return f"{case['id']}{suffix}"


def generate(
    fixtures: Path, extensible_fixtures: Path, alac_fixtures: Path, output: Path
) -> None:
    sources = {
        name: (fixtures / name).read_bytes()
        for name in (WAV_S16, WAV_F32, AIFF_S16, FLAC_S16)
    }
    sources[WAV_EXTENSIBLE_S16] = (extensible_fixtures / WAV_EXTENSIBLE_S16).read_bytes()
    for name in (ALAC_S16, ALAC_AAC, ALAC_FRAGMENTED, ALAC_MULTITRACK, ALAC_VIDEO):
        sources[name] = (alac_fixtures / name).read_bytes()
    output.mkdir(parents=True, exist_ok=True)

    cases = build_cases(sources)
    manifest_cases = []
    for case in cases:
        payload: bytes = case["bytes"]  # type: ignore[assignment]
        file_name = case_file_name(case)
        (output / file_name).write_bytes(payload)
        entry = {
            "id": case["id"],
            "path": file_name,
            "source": case["source"],
            "operation": case["operation"],
            "expected": case["expected"],
            "sha256": hashlib.sha256(payload).hexdigest(),
            "sizeBytes": len(payload),
        }
        if "seed" in case:
            entry["seed"] = case["seed"]
        manifest_cases.append(entry)

    manifest = {
        "corpusId": CORPUS_ID,
        "sourceCorpora": [
            SOURCE_CORPUS,
            EXTENSIBLE_SOURCE_CORPUS,
            ALAC_SOURCE_CORPUS,
        ],
        "notes": (
            "Deterministic byte-level derivations of committed native PCM "
            "fixtures. Expected codes/stages are product regression targets "
            "for exactly these files; they do not claim behavior for all "
            "byte inputs."
        ),
        "cases": manifest_cases,
    }
    (output / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def check_committed(
    fixtures: Path,
    extensible_fixtures: Path,
    alac_fixtures: Path,
    destination: Path,
) -> None:
    with tempfile.TemporaryDirectory() as scratch:
        fresh = Path(scratch) / CORPUS_ID
        generate(fixtures, extensible_fixtures, alac_fixtures, fresh)
        for candidate in sorted(fresh.iterdir()):
            committed = destination / candidate.name
            if not committed.exists():
                raise SystemExit(f"missing committed corpus file: {committed}")
            if committed.read_bytes() != candidate.read_bytes():
                raise SystemExit(f"committed corpus file differs: {committed}")
        extra = {p.name for p in destination.iterdir() if p.name != "README.md"} - {
            p.name for p in fresh.iterdir()
        }
        if extra:
            raise SystemExit(f"unexpected files in committed corpus: {sorted(extra)}")
    print(f"{CORPUS_ID}: committed corpus matches deterministic regeneration")


def main() -> None:
    repo_root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--fixtures",
        type=Path,
        default=repo_root / "tests" / "fixtures" / SOURCE_CORPUS,
        help="directory containing the committed native-pcm-v1 fixtures",
    )
    parser.add_argument(
        "--alac-fixtures",
        type=Path,
        default=repo_root / "tests" / "fixtures" / ALAC_SOURCE_CORPUS,
        help="directory containing the committed native-alac-v1 fixtures",
    )
    parser.add_argument(
        "--extensible-fixtures",
        type=Path,
        default=repo_root / "tests" / "fixtures" / EXTENSIBLE_SOURCE_CORPUS,
        help="directory containing the committed native-pcm-extensible-v1 fixtures",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=repo_root / "tests" / "fixtures" / CORPUS_ID,
        help="directory to write the corpus into",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="verify the committed corpus matches deterministic regeneration",
    )
    arguments = parser.parse_args()
    if arguments.check:
        check_committed(
            arguments.fixtures,
            arguments.extensible_fixtures,
            arguments.alac_fixtures,
            arguments.output,
        )
    else:
        if arguments.output.exists():
            for stale in arguments.output.iterdir():
                if stale.name != "README.md":
                    if stale.is_dir():
                        shutil.rmtree(stale)
                    else:
                        stale.unlink()
        generate(
            arguments.fixtures,
            arguments.extensible_fixtures,
            arguments.alac_fixtures,
            arguments.output,
        )
        print(f"{CORPUS_ID}: wrote corpus to {arguments.output}")


if __name__ == "__main__":
    main()
