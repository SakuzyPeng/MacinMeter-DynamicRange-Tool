#!/usr/bin/env python3
"""Generate the committed WAVE_FORMAT_EXTENSIBLE capability corpus."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import shutil
import struct
import tempfile
from dataclasses import dataclass
from pathlib import Path


CORPUS_ID = "native-pcm-extensible-v1"
SAMPLE_RATE = 48_000
FRAMES = 8
PCM_GUID_BYTES = bytes.fromhex("0100000000001000800000aa00389b71")
FLOAT_GUID_BYTES = bytes.fromhex("0300000000001000800000aa00389b71")
PCM_GUID = "00000001-0000-0010-8000-00aa00389b71"
FLOAT_GUID = "00000003-0000-0010-8000-00aa00389b71"


@dataclass(frozen=True)
class PairSpec:
    twin_id: str
    codec: str
    bits: int
    channels: int
    channel_mask: int


PAIR_SPECS = (
    PairSpec("pcm-u8-stereo-mask", "pcm_integer", 8, 2, 0x0000_0003),
    PairSpec("pcm-s16-stereo-mask", "pcm_integer", 16, 2, 0x0000_0003),
    PairSpec("pcm-s24-stereo-mask", "pcm_integer", 24, 2, 0x0000_0003),
    PairSpec("pcm-s32-stereo-mask", "pcm_integer", 32, 2, 0x0000_0003),
    PairSpec("float32-stereo-mask", "pcm_float", 32, 2, 0x0000_0003),
    PairSpec("float64-stereo-mask", "pcm_float", 64, 2, 0x0000_0003),
    PairSpec("pcm-s16-mono-center-mask", "pcm_integer", 16, 1, 0x0000_0004),
    PairSpec("pcm-s24-6ch-mask", "pcm_integer", 24, 6, 0x0000_003F),
    PairSpec("pcm-s16-stereo-zero-mask", "pcm_integer", 16, 2, 0),
    PairSpec("pcm-s16-26ch-zero-mask", "pcm_integer", 16, 26, 0),
)


def checked_u32(value: int, label: str) -> int:
    if not 0 <= value <= 0xFFFF_FFFF:
        raise ValueError(f"{label} does not fit u32: {value}")
    return value


def integer_values(bits: int, count: int) -> list[int]:
    minimum = -(1 << (bits - 1))
    maximum = (1 << (bits - 1)) - 1
    anchors = (minimum, maximum, -(1 << (bits - 2)), 1 << (bits - 2), -1, 1, 0, 3)
    return [anchors[index % len(anchors)] for index in range(count)]


def float_values(bits: int, count: int) -> list[float]:
    if bits == 32:
        anchors = (
            0.0,
            -0.0,
            float.fromhex("0x1p-126"),
            float.fromhex("0x1p-149"),
            0.25,
            -0.5,
            1.5,
            -2.0,
        )
        return [
            struct.unpack("<f", struct.pack("<f", anchors[index % len(anchors)]))[0]
            for index in range(count)
        ]
    epsilon = math.ulp(1.0)
    anchors = (
        0.0,
        -0.0,
        float.fromhex("0x1p-1022"),
        float.fromhex("0x0.0000000000001p-1022"),
        0.125 + epsilon,
        -0.75 + epsilon,
        1.0 - epsilon,
        -1.0 + epsilon,
    )
    return [anchors[index % len(anchors)] for index in range(count)]


def pcm_payload(spec: PairSpec) -> tuple[bytes, list[float], dict[str, object]]:
    count = spec.channels * FRAMES
    if spec.codec == "pcm_integer":
        values = integer_values(spec.bits, count)
        if spec.bits == 8:
            payload = bytes(value + 128 for value in values)
            encoding = "unsigned offset-binary"
        else:
            payload = b"".join(
                value.to_bytes(spec.bits // 8, "little", signed=True) for value in values
            )
            encoding = "signed little-endian"
        divisor = 1 << (spec.bits - 1)
        normalized = [value / float(divisor) for value in values]
        oracle = {
            "kind": "integer_normalization",
            "interleavedValues": values,
            "divisor": divisor,
            "sampleEncoding": encoding,
        }
        return payload, normalized, oracle

    values = float_values(spec.bits, count)
    pack_format = "<f" if spec.bits == 32 else "<d"
    payload = b"".join(struct.pack(pack_format, value) for value in values)
    oracle = {
        "kind": "explicit_f64_bits",
        "interleavedValues": [
            f"{struct.unpack('<Q', struct.pack('<d', value))[0]:016x}" for value in values
        ],
        "sampleEncoding": f"IEEE binary{spec.bits} little-endian",
    }
    return payload, values, oracle


def wave_bytes(spec: PairSpec, payload: bytes, *, extensible: bool) -> bytes:
    bytes_per_sample = spec.bits // 8
    block_align = spec.channels * bytes_per_sample
    byte_rate = SAMPLE_RATE * block_align
    format_tag = 1 if spec.codec == "pcm_integer" else 3
    fmt_payload = struct.pack(
        "<HHIIHH",
        0xFFFE if extensible else format_tag,
        spec.channels,
        SAMPLE_RATE,
        byte_rate,
        block_align,
        spec.bits,
    )
    if extensible:
        guid = PCM_GUID_BYTES if spec.codec == "pcm_integer" else FLOAT_GUID_BYTES
        fmt_payload += struct.pack("<HHI", 22, spec.bits, spec.channel_mask) + guid
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


def fixture_entry(
    path: Path,
    spec: PairSpec,
    *,
    extensible: bool,
    normalized: list[float],
    oracle: dict[str, object],
) -> dict[str, object]:
    payload = path.read_bytes()
    normalized_bytes = b"".join(struct.pack("<d", sample) for sample in normalized)
    return {
        "id": path.stem,
        "path": path.name,
        "sha256": hashlib.sha256(payload).hexdigest(),
        "sizeBytes": len(payload),
        "container": "wave",
        "codec": spec.codec,
        "encapsulation": "wave_format_extensible" if extensible else "classic_wave_format",
        "sampleRate": SAMPLE_RATE,
        "channels": spec.channels,
        "frames": FRAMES,
        "containerBits": spec.bits,
        "validBits": spec.bits,
        "channelMask": spec.channel_mask if extensible else None,
        "subFormat": (
            PCM_GUID if spec.codec == "pcm_integer" else FLOAT_GUID
        ) if extensible else ("WAVE_FORMAT_PCM" if spec.codec == "pcm_integer" else "WAVE_FORMAT_IEEE_FLOAT"),
        "twinId": spec.twin_id,
        "minimumDataBlocks": 1,
        "pcmOracle": oracle,
        "normalizedInterleavedF64LeSha256": hashlib.sha256(normalized_bytes).hexdigest(),
        "provenance": {
            "kind": "deterministically_generated",
            "copyrightedAudio": False,
            "license": "MIT",
        },
    }


def generate(output: Path) -> None:
    output.mkdir(parents=True, exist_ok=True)
    entries: list[dict[str, object]] = []
    for spec in PAIR_SPECS:
        payload, normalized, oracle = pcm_payload(spec)
        for extensible in (False, True):
            suffix = "extensible" if extensible else "classic"
            path = output / f"{spec.twin_id}-{suffix}.wav"
            path.write_bytes(wave_bytes(spec, payload, extensible=extensible))
            entries.append(
                fixture_entry(
                    path,
                    spec,
                    extensible=extensible,
                    normalized=normalized,
                    oracle=oracle,
                )
            )

    entries.sort(key=lambda entry: str(entry["path"]))
    manifest = {
        "schemaVersion": 1,
        "corpusId": CORPUS_ID,
        "generator": {
            "path": "scripts/generate-native-pcm-extensible-v1.py",
            "externalToolsRequired": False,
            "networkRequired": False,
        },
        "fixtures": entries,
    }
    (output / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def check_committed(destination: Path) -> None:
    with tempfile.TemporaryDirectory(prefix="macinmeter-extensible-pcm-check-") as temporary:
        generated = Path(temporary)
        generate(generated)
        expected = {path.name for path in generated.iterdir()}
        committed = {
            path.name for path in destination.iterdir() if path.name != "README.md"
        }
        mismatches = sorted(
            name
            for name in expected
            if name not in committed
            or (destination / name).read_bytes() != (generated / name).read_bytes()
        )
        extras = sorted(committed - expected)
    if mismatches or extras:
        raise SystemExit(
            "committed Extensible PCM fixtures differ from the generator: "
            f"mismatches={mismatches}; extras={extras}"
        )
    print(f"{CORPUS_ID}: {len(expected)} generated files match")


def main() -> None:
    repository = Path(__file__).resolve().parents[1]
    destination = repository / "tests" / "fixtures" / CORPUS_ID
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=destination)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if args.check:
        check_committed(args.output.resolve())
    else:
        if args.output.exists():
            for stale in args.output.iterdir():
                if stale.name != "README.md":
                    if stale.is_dir():
                        shutil.rmtree(stale)
                    else:
                        stale.unlink()
        generate(args.output.resolve())
        print(f"generated {CORPUS_ID} under {args.output.resolve()}")


if __name__ == "__main__":
    main()
