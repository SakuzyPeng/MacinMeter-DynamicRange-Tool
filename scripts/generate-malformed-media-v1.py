#!/usr/bin/env python3
"""Generate the committed M2 malformed-media regression corpus.

Every case is a deterministic byte-level derivation of a committed
`native-pcm-v1` fixture (or a deterministic synthetic byte string), so the
corpus can be regenerated and audited without any external media. Expected
outcomes are recorded as structured product error codes and stages; they are
regression targets captured from the product, not a claim about all possible
byte inputs.
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

WAV_S16 = "wav-pcm-s16-stereo.wav"
WAV_F32 = "wav-float32-stereo.wav"
AIFF_S16 = "aiff-pcm-s16-stereo.aiff"
FLAC_S16 = "flac-pcm-s16-stereo-multiblock.flac"


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


def build_cases(sources: dict[str, bytes]) -> list[dict[str, object]]:
    wav = sources[WAV_S16]
    wav_f32 = sources[WAV_F32]
    aiff = sources[AIFF_S16]
    flac = sources[FLAC_S16]

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
            "expected": {"code": "unsupported_format", "stage": "probe"},
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


def generate(fixtures: Path, output: Path) -> None:
    sources = {
        name: (fixtures / name).read_bytes()
        for name in (WAV_S16, WAV_F32, AIFF_S16, FLAC_S16)
    }
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
        "sourceCorpus": SOURCE_CORPUS,
        "notes": (
            "Deterministic byte-level derivations of committed native-pcm-v1 "
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


def check_committed(fixtures: Path, destination: Path) -> None:
    with tempfile.TemporaryDirectory() as scratch:
        fresh = Path(scratch) / CORPUS_ID
        generate(fixtures, fresh)
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
        check_committed(arguments.fixtures, arguments.output)
    else:
        if arguments.output.exists():
            for stale in arguments.output.iterdir():
                if stale.name != "README.md":
                    if stale.is_dir():
                        shutil.rmtree(stale)
                    else:
                        stale.unlink()
        generate(arguments.fixtures, arguments.output)
        print(f"{CORPUS_ID}: wrote corpus to {arguments.output}")


if __name__ == "__main__":
    main()
