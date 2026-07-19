#!/usr/bin/env python3
"""Generate the committed M2 native PCM/FLAC product fixture corpus."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import shutil
import struct
import subprocess
import tempfile
from pathlib import Path


CORPUS_ID = "native-pcm-v1"
EXPECTED_FLAC_VERSION = "flac 1.5.0"
SAMPLE_RATE = 48_000
AIFF_SAMPLE_RATE = 44_100
CHANNELS = 2
FLAC_SAMPLE_RATE = 8_000
FLAC_FRAMES = 400
FLAC_BLOCK_SIZE = 192
GENERATED_FILES = (
    "wav-pcm-u8-stereo.wav",
    "wav-pcm-s16-stereo.wav",
    "wav-pcm-s24-stereo.wav",
    "wav-pcm-s32-stereo.wav",
    "wav-float32-stereo.wav",
    "wav-float64-stereo.wav",
    "aiff-pcm-s8-stereo.aiff",
    "aiff-pcm-s16-stereo.aiff",
    "aiff-pcm-s24-stereo.aiff",
    "aiff-pcm-s32-stereo.aiff",
    "flac-pcm-s16-stereo-multiblock.flac",
    "manifest.json",
)


def checked_u32(value: int, label: str) -> int:
    if not 0 <= value <= 0xFFFF_FFFF:
        raise ValueError(f"{label} does not fit u32: {value}")
    return value


def pcm_integer_values(bits: int) -> list[int]:
    minimum = -(1 << (bits - 1))
    maximum = (1 << (bits - 1)) - 1
    half = 1 << (bits - 2)
    marker = (1 << (bits - 3)) + 3
    return [minimum, maximum, -half, half, -1, 1, 0, marker]


def pack_signed(value: int, bits: int, byteorder: str) -> bytes:
    if bits == 8:
        return struct.pack("b", value)
    return value.to_bytes(bits // 8, byteorder=byteorder, signed=True)


def wave_bytes(format_tag: int, bits: int, payload: bytes, frames: int) -> bytes:
    bytes_per_sample = bits // 8
    block_align = CHANNELS * bytes_per_sample
    byte_rate = SAMPLE_RATE * block_align
    fmt_payload = struct.pack(
        "<HHIIHH",
        format_tag,
        CHANNELS,
        SAMPLE_RATE,
        byte_rate,
        block_align,
        bits,
    )
    padded_payload = payload + (b"\0" if len(payload) % 2 else b"")
    riff_size = checked_u32(
        4 + 8 + len(fmt_payload) + 8 + len(padded_payload), "RIFF size"
    )
    return b"".join(
        (
            b"RIFF",
            struct.pack("<I", riff_size),
            b"WAVE",
            b"fmt ",
            struct.pack("<I", len(fmt_payload)),
            fmt_payload,
            b"data",
            struct.pack("<I", len(payload)),
            padded_payload,
        )
    )


def aiff_chunk(chunk_id: bytes, payload: bytes) -> bytes:
    return (
        chunk_id
        + struct.pack(">I", checked_u32(len(payload), f"{chunk_id!r} size"))
        + payload
        + (b"\0" if len(payload) % 2 else b"")
    )


def aiff_bytes(bits: int, payload: bytes, frames: int) -> bytes:
    # Canonical 80-bit extended representation of exactly 44,100 Hz.
    extended_44_100 = bytes((0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0))
    comm_payload = b"".join(
        (
            struct.pack(">H", CHANNELS),
            struct.pack(">I", checked_u32(frames, "AIFF frame count")),
            struct.pack(">H", bits),
            extended_44_100,
        )
    )
    ssnd_payload = struct.pack(">II", 0, 0) + payload
    chunks = aiff_chunk(b"COMM", comm_payload) + aiff_chunk(b"SSND", ssnd_payload)
    form_size = checked_u32(4 + len(chunks), "AIFF FORM size")
    return b"FORM" + struct.pack(">I", form_size) + b"AIFF" + chunks


def write_integer_fixtures(output: Path) -> list[dict[str, object]]:
    entries: list[dict[str, object]] = []
    for bits in (8, 16, 24, 32):
        values = pcm_integer_values(bits)
        wave_payload = (
            bytes(value + 128 for value in values)
            if bits == 8
            else b"".join(pack_signed(value, bits, "little") for value in values)
        )
        wave_name = f"wav-pcm-{'u8' if bits == 8 else f's{bits}'}-stereo.wav"
        wave_path = output / wave_name
        wave_path.write_bytes(wave_bytes(1, bits, wave_payload, len(values) // CHANNELS))
        entries.append(
            fixture_entry(
                wave_path,
                container="wave",
                codec="pcm_integer",
                sample_rate=SAMPLE_RATE,
                channels=CHANNELS,
                frames=len(values) // CHANNELS,
                bits_per_sample=bits,
                sample_encoding=(
                    "unsigned offset-binary" if bits == 8 else "signed little-endian"
                ),
                oracle={
                    "kind": "integer_normalization",
                    "interleavedValues": values,
                    "divisor": 1 << (bits - 1),
                },
                normalized_pcm=[
                    value / float(1 << (bits - 1)) for value in values
                ],
            )
        )

        aiff_payload = b"".join(pack_signed(value, bits, "big") for value in values)
        aiff_name = f"aiff-pcm-s{bits}-stereo.aiff"
        aiff_path = output / aiff_name
        aiff_path.write_bytes(
            aiff_bytes(bits, aiff_payload, len(values) // CHANNELS)
        )
        entries.append(
            fixture_entry(
                aiff_path,
                container="aiff",
                codec="pcm_integer",
                sample_rate=AIFF_SAMPLE_RATE,
                channels=CHANNELS,
                frames=len(values) // CHANNELS,
                bits_per_sample=bits,
                sample_encoding="signed big-endian",
                oracle={
                    "kind": "integer_normalization",
                    "interleavedValues": values,
                    "divisor": 1 << (bits - 1),
                },
                normalized_pcm=[
                    value / float(1 << (bits - 1)) for value in values
                ],
            )
        )
    return entries


def f32_values() -> list[float]:
    return [
        0.0,
        -0.0,
        float.fromhex("0x1p-126"),
        float.fromhex("0x1p-149"),
        0.25,
        -0.5,
        1.5,
        -2.0,
    ]


def f64_values() -> list[float]:
    epsilon = math.ulp(1.0)
    return [
        0.0,
        -0.0,
        float.fromhex("0x1p-1022"),
        float.fromhex("0x0.0000000000001p-1022"),
        0.125 + epsilon,
        -0.75 + epsilon,
        1.0 - epsilon,
        -1.0 + epsilon,
    ]


def f64_bits(value: float) -> str:
    return f"{struct.unpack('<Q', struct.pack('<d', value))[0]:016x}"


def write_float_fixtures(output: Path) -> list[dict[str, object]]:
    entries: list[dict[str, object]] = []
    for bits, values, pack_format in (
        (32, f32_values(), "<f"),
        (64, f64_values(), "<d"),
    ):
        payload = b"".join(struct.pack(pack_format, value) for value in values)
        path = output / f"wav-float{bits}-stereo.wav"
        path.write_bytes(wave_bytes(3, bits, payload, len(values) // CHANNELS))
        normalized = (
            [float(struct.unpack("<f", struct.pack("<f", value))[0]) for value in values]
            if bits == 32
            else values
        )
        entries.append(
            fixture_entry(
                path,
                container="wave",
                codec="pcm_float",
                sample_rate=SAMPLE_RATE,
                channels=CHANNELS,
                frames=len(values) // CHANNELS,
                bits_per_sample=bits,
                sample_encoding=f"IEEE binary{bits} little-endian",
                oracle={
                    "kind": "explicit_f64_bits",
                    "interleavedValues": [f64_bits(value) for value in normalized],
                },
                normalized_pcm=normalized,
            )
        )
    return entries


def flac_pcm_frames() -> list[tuple[int, int]]:
    frames = [
        (-(1 << 15), (1 << 15) - 1),
        (0, 0),
        (-1, 1),
        (-16_384, 16_384),
    ]
    for index in range(len(frames), FLAC_FRAMES):
        left = ((index * 257 + 17) % 40_001) - 20_000
        right = ((index * 509 + 1_234) % 30_001) - 15_000
        frames.append((left, right))
    return frames


def flac_version() -> str:
    executable = shutil.which("flac")
    if executable is None:
        raise RuntimeError("flac is required only to regenerate the committed FLAC fixture")
    result = subprocess.run(
        (executable, "--version"),
        check=True,
        capture_output=True,
        text=True,
    )
    version = result.stdout.strip()
    if version != EXPECTED_FLAC_VERSION:
        raise RuntimeError(
            f"fixture bytes are pinned to {EXPECTED_FLAC_VERSION}, found {version}"
        )
    return version


def write_flac_fixture(output: Path) -> tuple[dict[str, object], str]:
    version = flac_version()
    frames = flac_pcm_frames()
    raw = b"".join(
        struct.pack("<hh", left, right) for left, right in frames
    )
    path = output / "flac-pcm-s16-stereo-multiblock.flac"
    with tempfile.TemporaryDirectory(prefix="macinmeter-native-pcm-") as temporary:
        raw_path = Path(temporary) / "source.raw"
        raw_path.write_bytes(raw)
        subprocess.run(
            (
                "flac",
                "--force",
                "--silent",
                "--force-raw-format",
                "--verify",
                "--endian=little",
                "--sign=signed",
                f"--channels={CHANNELS}",
                "--bps=16",
                f"--sample-rate={FLAC_SAMPLE_RATE}",
                f"--blocksize={FLAC_BLOCK_SIZE}",
                "--compression-level-0",
                "--no-padding",
                "--no-seektable",
                "--no-preserve-modtime",
                f"--output-name={path}",
                str(raw_path),
            ),
            check=True,
        )
    entry = fixture_entry(
        path,
        container="flac",
        codec="flac",
        sample_rate=FLAC_SAMPLE_RATE,
        channels=CHANNELS,
        frames=FLAC_FRAMES,
        bits_per_sample=16,
        sample_encoding="signed 16-bit source PCM encoded by reference libFLAC",
        oracle={
            "kind": "stereo_integer_formula",
            "divisor": 32_768,
            "firstFrames": [list(frame) for frame in frames[:4]],
            "remainingFrames": {
                "startIndex": 4,
                "left": "((index * 257 + 17) % 40001) - 20000",
                "right": "((index * 509 + 1234) % 30001) - 15000",
            },
        },
        normalized_pcm=[
            sample / 32_768.0 for frame in frames for sample in frame
        ],
        minimum_data_blocks=2,
    )
    entry["encoder"] = {
        "name": "reference libFLAC",
        "version": version,
        "blockSize": FLAC_BLOCK_SIZE,
        "compressionLevel": 0,
    }
    return entry, version


def fixture_entry(
    path: Path,
    *,
    container: str,
    codec: str,
    sample_rate: int,
    channels: int,
    frames: int,
    bits_per_sample: int,
    sample_encoding: str,
    oracle: dict[str, object],
    normalized_pcm: list[float],
    minimum_data_blocks: int = 1,
) -> dict[str, object]:
    payload = path.read_bytes()
    normalized_bytes = b"".join(
        struct.pack("<d", sample) for sample in normalized_pcm
    )
    return {
        "id": path.stem,
        "path": path.name,
        "sha256": hashlib.sha256(payload).hexdigest(),
        "sizeBytes": len(payload),
        "container": container,
        "codec": codec,
        "sampleRate": sample_rate,
        "channels": channels,
        "frames": frames,
        "bitsPerSample": bits_per_sample,
        "minimumDataBlocks": minimum_data_blocks,
        "sampleEncoding": sample_encoding,
        "pcmOracle": oracle,
        "normalizedInterleavedF64LeSha256": hashlib.sha256(
            normalized_bytes
        ).hexdigest(),
        "provenance": {
            "kind": "deterministically_generated",
            "copyrightedAudio": False,
            "license": "MIT",
        },
    }


def generate(output: Path) -> None:
    output.mkdir(parents=True, exist_ok=True)
    entries = write_integer_fixtures(output)
    entries.extend(write_float_fixtures(output))
    flac_entry, version = write_flac_fixture(output)
    entries.append(flac_entry)
    entries.sort(key=lambda entry: str(entry["path"]))
    manifest = {
        "schemaVersion": 1,
        "corpusId": CORPUS_ID,
        "generator": {
            "path": "scripts/generate-native-pcm-v1.py",
            "requiresForFlacRegeneration": version,
            "networkRequired": False,
        },
        "fixtures": entries,
        "derivedMutations": [
            {
                "id": "flac-terminal-frame-crc-corruption",
                "source": "flac-pcm-s16-stereo-multiblock.flac",
                "operation": "xor the final byte with 0xff",
                "expected": "sticky decode_failed; never EOF or a successful report",
            }
        ],
    }
    (output / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def check_committed(destination: Path) -> None:
    with tempfile.TemporaryDirectory(prefix="macinmeter-native-pcm-check-") as temporary:
        generated = Path(temporary)
        generate(generated)
        mismatches = [
            name
            for name in GENERATED_FILES
            if not (destination / name).is_file()
            or (destination / name).read_bytes() != (generated / name).read_bytes()
        ]
    if mismatches:
        raise SystemExit(
            "committed native PCM fixtures differ from the pinned generator: "
            + ", ".join(mismatches)
        )
    print(f"{CORPUS_ID}: {len(GENERATED_FILES)} generated files match")


def main() -> None:
    repository = Path(__file__).resolve().parents[1]
    destination = repository / "tests" / "fixtures" / CORPUS_ID
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--output",
        type=Path,
        default=destination,
        help="output directory (defaults to the committed product corpus)",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="regenerate in a temporary directory and compare with --output",
    )
    args = parser.parse_args()
    if args.check:
        check_committed(args.output.resolve())
    else:
        generate(args.output.resolve())
        print(f"generated {CORPUS_ID} under {args.output.resolve()}")


if __name__ == "__main__":
    main()
