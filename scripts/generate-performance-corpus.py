#!/usr/bin/env python3
"""Generate the deterministic, untracked M6 performance corpus."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import struct
import subprocess
import tempfile
from pathlib import Path


CORPUS_ID = "m6-performance-baseline-v1"
SCHEMA_VERSION = 1
SAMPLE_RATE = 48_000
BLOCK_FRAMES = 4_096
DEFAULT_OUTPUT = Path("target/performance-corpus") / CORPUS_ID
STEREO_FRAMES = 60 * SAMPLE_RATE
SURROUND_FRAMES = 30 * SAMPLE_RATE
# The packet-worker A/B needs a track long enough for scheduling to matter.
ALAC_FRAMES = 240 * SAMPLE_RATE
ALAC_FFMPEG_VERSION = "8.0.1"
BATCH_TRACK_FRAMES = 15 * SAMPLE_RATE
BATCH_TRACKS = 8
DISCOVERY_SUPPORTED_FILES = 1_024
DISCOVERY_IGNORED_FILES = 256
STABLE_EXTENSIONS = ("wav", "wave", "flac", "aiff", "aif")


class CorpusError(RuntimeError):
    """A deterministic corpus generation or validation failure."""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def script_sha256() -> str:
    return sha256_file(Path(__file__).resolve())


def checked_u32(value: int, label: str) -> int:
    if not 0 <= value <= 0xFFFF_FFFF:
        raise CorpusError(f"{label} does not fit the format's u32 field: {value}")
    return value


def deterministic_integer_block(
    *,
    channels: int,
    bits: int,
    seed: int,
) -> tuple[list[int], bytes]:
    if not 1 <= channels <= 64:
        raise CorpusError(f"unsupported generated channel count: {channels}")
    if bits not in (16, 24):
        raise CorpusError(f"unsupported generated integer depth: {bits}")

    limit = (1 << (bits - 1)) - 1
    values: list[int] = []
    normalized = bytearray()
    for frame in range(BLOCK_FRAMES):
        for channel in range(channels):
            mixed = (
                (frame + seed * 17) * 1_103_515_245
                + (channel + 1) * 12_345
                + seed * 2_654_435_761
            ) & 0xFFFF_FFFF
            signed = int((mixed >> 1) % (2 * limit + 1)) - limit
            scale_numerator = 97 - (channel % 9) * 4
            value = signed * scale_numerator // 100
            values.append(value)
            normalized.extend(struct.pack("<d", value / float(1 << (bits - 1))))
    return values, bytes(normalized)


def _triangle(phase: int, period: int, amplitude: int) -> int:
    half = period // 2
    position = phase % period
    if position < half:
        return (2 * amplitude * position) // half - amplitude
    return amplitude - (2 * amplitude * (position - half)) // half


def deterministic_tonal_block(*, channels: int) -> tuple[list[int], bytes]:
    """Build a tonal, compressible 16-bit block.

    The pseudo-random block compresses to roughly 99.5%, which lets ALAC fall
    back to its uncompressed escape path, so a sweep run only on that signal
    cannot say whether the result holds for material the codec actually has to
    predict. Summed integer triangle waves plus a small dither land near 60%,
    the ordinary range for lossless music, while staying exactly reproducible
    without any floating point.
    """
    if channels < 1:
        raise CorpusError(f"unsupported tonal channel count: {channels}")

    limit = (1 << 15) - 1
    partials = ((218, 9_000), (173, 4_500), (411, 3_000), (1_021, 1_500))
    dither_span = 4_096
    values: list[int] = []
    normalized = bytearray()
    for frame in range(BLOCK_FRAMES):
        for channel in range(channels):
            phase = frame + channel * 97
            value = sum(
                _triangle(phase, period, amplitude) for period, amplitude in partials
            )
            mixed = (
                (phase + 1) * 1_103_515_245 + (channel + 1) * 2_654_435_761
            ) & 0xFFFF_FFFF
            value += (mixed >> 9) % dither_span - dither_span // 2
            value = max(-limit - 1, min(limit, value))
            values.append(value)
            normalized.extend(struct.pack("<d", value / float(1 << 15)))
    return values, bytes(normalized)


def pack_integer_block(values: list[int], bits: int, byteorder: str) -> bytes:
    width = bits // 8
    return b"".join(
        value.to_bytes(width, byteorder=byteorder, signed=True) for value in values
    )


def normalized_pcm_sha256(
    normalized_block: bytes,
    *,
    frames: int,
    channels: int,
) -> str:
    bytes_per_frame = channels * 8
    block_bytes = BLOCK_FRAMES * bytes_per_frame
    if len(normalized_block) != block_bytes:
        raise CorpusError("normalized block geometry is inconsistent")
    full_blocks, tail_frames = divmod(frames, BLOCK_FRAMES)
    digest = hashlib.sha256()
    for _ in range(full_blocks):
        digest.update(normalized_block)
    digest.update(normalized_block[: tail_frames * bytes_per_frame])
    return digest.hexdigest()


def wave_header(
    *,
    frames: int,
    channels: int,
    sample_rate: int,
    bits: int,
    format_tag: int,
) -> bytes:
    bytes_per_sample = bits // 8
    block_align = channels * bytes_per_sample
    data_size = checked_u32(frames * block_align, "WAVE data size")
    byte_rate = checked_u32(sample_rate * block_align, "WAVE byte rate")
    riff_size = checked_u32(36 + data_size, "WAVE RIFF size")
    return b"".join(
        (
            b"RIFF",
            struct.pack("<I", riff_size),
            b"WAVE",
            b"fmt ",
            struct.pack("<IHHIIHH", 16, format_tag, channels, sample_rate, byte_rate, block_align, bits),
            b"data",
            struct.pack("<I", data_size),
        )
    )


def aiff_header(*, frames: int, channels: int, sample_rate: int, bits: int) -> bytes:
    if sample_rate != 48_000:
        raise CorpusError("the M6 AIFF generator currently fixes exactly 48,000 Hz")
    bytes_per_frame = channels * (bits // 8)
    data_size = checked_u32(frames * bytes_per_frame, "AIFF sample data size")
    form_size = checked_u32(46 + data_size, "AIFF FORM size")
    extended_48_000 = bytes.fromhex("400ebb80000000000000")
    return b"".join(
        (
            b"FORM",
            struct.pack(">I", form_size),
            b"AIFF",
            b"COMM",
            struct.pack(">IHIH", 18, channels, frames, bits),
            extended_48_000,
            b"SSND",
            struct.pack(">III", 8 + data_size, 0, 0),
        )
    )


def write_repeated_payload(
    path: Path,
    header: bytes,
    block: bytes,
    *,
    frames: int,
    channels: int,
    bytes_per_sample: int,
) -> None:
    bytes_per_frame = channels * bytes_per_sample
    expected_block_bytes = BLOCK_FRAMES * bytes_per_frame
    if len(block) != expected_block_bytes:
        raise CorpusError(
            f"{path.name} block has {len(block)} bytes, expected {expected_block_bytes}"
        )
    full_blocks, tail_frames = divmod(frames, BLOCK_FRAMES)
    with path.open("wb") as output:
        output.write(header)
        for _ in range(full_blocks):
            output.write(block)
        output.write(block[: tail_frames * bytes_per_frame])


def media_entry(
    root: Path,
    path: Path,
    *,
    identifier: str,
    container: str,
    codec: str,
    frames: int,
    channels: int,
    bits: int,
    normalized_sha256: str,
    signal: str,
    encoder: dict[str, object] | None = None,
) -> dict[str, object]:
    entry: dict[str, object] = {
        "id": identifier,
        "path": path.relative_to(root).as_posix(),
        "container": container,
        "codec": codec,
        "sampleRate": SAMPLE_RATE,
        "channels": channels,
        "frames": frames,
        "audioSeconds": frames / SAMPLE_RATE,
        "bitsPerSample": bits,
        "signal": signal,
        "normalizedInterleavedF64LeSha256": normalized_sha256,
        "sha256": sha256_file(path),
        "sizeBytes": path.stat().st_size,
    }
    if encoder is not None:
        entry["encoder"] = encoder
    return entry


def write_stereo_routes(root: Path) -> list[dict[str, object]]:
    values, normalized = deterministic_integer_block(channels=2, bits=16, seed=1)
    normalized_sha = normalized_pcm_sha256(
        normalized, frames=STEREO_FRAMES, channels=2
    )
    little = pack_integer_block(values, 16, "little")
    big = pack_integer_block(values, 16, "big")
    signal = "deterministic_integer_v1_seed_1"
    entries: list[dict[str, object]] = []

    wave_path = root / "stereo-s16-60s.wav"
    write_repeated_payload(
        wave_path,
        wave_header(
            frames=STEREO_FRAMES,
            channels=2,
            sample_rate=SAMPLE_RATE,
            bits=16,
            format_tag=1,
        ),
        little,
        frames=STEREO_FRAMES,
        channels=2,
        bytes_per_sample=2,
    )
    entries.append(
        media_entry(
            root,
            wave_path,
            identifier="stereo-s16-wave-60s",
            container="wave",
            codec="pcm_integer",
            frames=STEREO_FRAMES,
            channels=2,
            bits=16,
            normalized_sha256=normalized_sha,
            signal=signal,
        )
    )

    aiff_path = root / "stereo-s16-60s.aiff"
    write_repeated_payload(
        aiff_path,
        aiff_header(
            frames=STEREO_FRAMES,
            channels=2,
            sample_rate=SAMPLE_RATE,
            bits=16,
        ),
        big,
        frames=STEREO_FRAMES,
        channels=2,
        bytes_per_sample=2,
    )
    entries.append(
        media_entry(
            root,
            aiff_path,
            identifier="stereo-s16-aiff-60s",
            container="aiff",
            codec="pcm_integer",
            frames=STEREO_FRAMES,
            channels=2,
            bits=16,
            normalized_sha256=normalized_sha,
            signal=signal,
        )
    )

    flac_path = root / "stereo-s16-60s.flac"
    flac = shutil.which("flac")
    if flac is None:
        raise CorpusError("reference flac executable is required to generate the M6 corpus")
    version = subprocess.run(
        (flac, "--version"),
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    subprocess.run(
        (
            flac,
            "--force",
            "--silent",
            "--verify",
            "--compression-level-5",
            "--no-padding",
            "--no-seektable",
            "--no-preserve-modtime",
            f"--output-name={flac_path}",
            str(wave_path),
        ),
        check=True,
    )
    entries.append(
        media_entry(
            root,
            flac_path,
            identifier="stereo-s16-flac-60s",
            container="flac",
            codec="flac",
            frames=STEREO_FRAMES,
            channels=2,
            bits=16,
            normalized_sha256=normalized_sha,
            signal=signal,
            encoder={
                "name": "reference libFLAC command line",
                "version": version,
                "compressionLevel": 5,
            },
        )
    )

    float_path = root / "stereo-f64-60s.wav"
    float_block = b"".join(normalized[index : index + 8] for index in range(0, len(normalized), 8))
    write_repeated_payload(
        float_path,
        wave_header(
            frames=STEREO_FRAMES,
            channels=2,
            sample_rate=SAMPLE_RATE,
            bits=64,
            format_tag=3,
        ),
        float_block,
        frames=STEREO_FRAMES,
        channels=2,
        bytes_per_sample=8,
    )
    entries.append(
        media_entry(
            root,
            float_path,
            identifier="stereo-f64-wave-60s",
            container="wave",
            codec="pcm_float",
            frames=STEREO_FRAMES,
            channels=2,
            bits=64,
            normalized_sha256=normalized_sha,
            signal=signal,
        )
    )
    return entries


def write_alac_routes(root: Path) -> list[dict[str, object]]:
    """Write the long ALAC tracks the packet-worker sweep needs.

    ADR-0014 requires a source-bound long ALAC input before any packet-worker
    speedup may be claimed: the committed correctness fixtures are far too short
    to expose scheduling behaviour. Two tracks of identical length carry the
    same geometry at opposite ends of the compression range, so the sweep can
    say whether its result depends on how hard the codec has to work. The
    intermediate WAVs are not kept, since only the compressed tracks and their
    normalized f64 oracles are part of the corpus.
    """
    pseudorandom_values, pseudorandom_normalized = deterministic_integer_block(
        channels=2, bits=16, seed=1
    )
    tonal_values, tonal_normalized = deterministic_tonal_block(channels=2)
    return [
        write_alac_track(
            root,
            values=pseudorandom_values,
            normalized_block=pseudorandom_normalized,
            identifier="stereo-s16-alac-240s",
            filename="stereo-s16-alac-240s.m4a",
            signal="deterministic_integer_v1_seed_1",
        ),
        write_alac_track(
            root,
            values=tonal_values,
            normalized_block=tonal_normalized,
            identifier="stereo-s16-alac-tonal-240s",
            filename="stereo-s16-alac-tonal-240s.m4a",
            signal="deterministic_tonal_v1",
        ),
    ]


def write_alac_track(
    root: Path,
    *,
    values: list[int],
    normalized_block: bytes,
    identifier: str,
    filename: str,
    signal: str,
) -> dict[str, object]:
    normalized_sha = normalized_pcm_sha256(
        normalized_block, frames=ALAC_FRAMES, channels=2
    )
    payload = pack_integer_block(values, 16, "little")

    ffmpeg = shutil.which("ffmpeg")
    if ffmpeg is None:
        raise CorpusError("ffmpeg is required to generate the M6 ALAC corpus track")
    version_line = subprocess.run(
        (ffmpeg, "-version"),
        check=True,
        capture_output=True,
        text=True,
    ).stdout.splitlines()
    version_line = version_line[0] if version_line else ""
    if not version_line.startswith(f"ffmpeg version {ALAC_FFMPEG_VERSION}"):
        raise CorpusError(
            f"the M6 ALAC corpus track requires ffmpeg {ALAC_FFMPEG_VERSION}; "
            f"observed: {version_line or '<no output>'}"
        )

    path = root / filename
    with tempfile.TemporaryDirectory() as scratch:
        source = Path(scratch) / "alac-source.wav"
        write_repeated_payload(
            source,
            wave_header(
                frames=ALAC_FRAMES,
                channels=2,
                sample_rate=SAMPLE_RATE,
                bits=16,
                format_tag=1,
            ),
            payload,
            frames=ALAC_FRAMES,
            channels=2,
            bytes_per_sample=2,
        )
        # The same bit-exact shape ADR-0013 fixed for native-alac-v1, so the
        # corpus track is byte-reproducible on the pinned encoder.
        subprocess.run(
            (
                ffmpeg,
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-fflags",
                "+bitexact",
                "-i",
                str(source),
                "-map_metadata",
                "-1",
                "-vn",
                "-c:a",
                "alac",
                "-compression_level",
                "2",
                "-flags:a",
                "+bitexact",
                "-channel_layout:a",
                "stereo",
                "-f",
                "ipod",
                str(path),
            ),
            check=True,
        )

    return media_entry(
        root,
        path,
        identifier=identifier,
        container="mp4",
        codec="alac",
        frames=ALAC_FRAMES,
        channels=2,
        bits=16,
        normalized_sha256=normalized_sha,
        signal=signal,
        encoder={
            "name": "ffmpeg alac",
            "version": ALAC_FFMPEG_VERSION,
            "versionLine": version_line,
            "compressionLevel": 2,
        },
    )


def write_surround_route(root: Path) -> dict[str, object]:
    values, normalized = deterministic_integer_block(channels=6, bits=24, seed=6)
    normalized_sha = normalized_pcm_sha256(
        normalized, frames=SURROUND_FRAMES, channels=6
    )
    payload = pack_integer_block(values, 24, "little")
    path = root / "surround-s24-6ch-30s.wav"
    write_repeated_payload(
        path,
        wave_header(
            frames=SURROUND_FRAMES,
            channels=6,
            sample_rate=SAMPLE_RATE,
            bits=24,
            format_tag=1,
        ),
        payload,
        frames=SURROUND_FRAMES,
        channels=6,
        bytes_per_sample=3,
    )
    return media_entry(
        root,
        path,
        identifier="surround-s24-wave-6ch-30s",
        container="wave",
        codec="pcm_integer",
        frames=SURROUND_FRAMES,
        channels=6,
        bits=24,
        normalized_sha256=normalized_sha,
        signal="deterministic_integer_v1_seed_6",
    )


def write_batch_routes(root: Path) -> list[dict[str, object]]:
    directory = root / "batch"
    directory.mkdir()
    entries: list[dict[str, object]] = []
    for index in range(BATCH_TRACKS):
        values, normalized = deterministic_integer_block(
            channels=2, bits=16, seed=100 + index
        )
        normalized_sha = normalized_pcm_sha256(
            normalized, frames=BATCH_TRACK_FRAMES, channels=2
        )
        payload = pack_integer_block(values, 16, "little")
        path = directory / f"track-{index:02d}.wav"
        write_repeated_payload(
            path,
            wave_header(
                frames=BATCH_TRACK_FRAMES,
                channels=2,
                sample_rate=SAMPLE_RATE,
                bits=16,
                format_tag=1,
            ),
            payload,
            frames=BATCH_TRACK_FRAMES,
            channels=2,
            bytes_per_sample=2,
        )
        entries.append(
            media_entry(
                root,
                path,
                identifier=f"batch-wave-track-{index:02d}",
                container="wave",
                codec="pcm_integer",
                frames=BATCH_TRACK_FRAMES,
                channels=2,
                bits=16,
                normalized_sha256=normalized_sha,
                signal=f"deterministic_integer_v1_seed_{100 + index}",
            )
        )
    return entries


def discovery_relative_paths() -> tuple[list[Path], list[Path]]:
    supported = [
        Path(f"group-{index % 16:02d}")
        / f"input-{index:04d}.{STABLE_EXTENSIONS[index % len(STABLE_EXTENSIONS)]}"
        for index in range(DISCOVERY_SUPPORTED_FILES)
    ]
    ignored = [
        Path(f"group-{index % 16:02d}") / f"ignored-{index:04d}.txt"
        for index in range(DISCOVERY_IGNORED_FILES)
    ]
    return supported, ignored


def write_discovery_tree(root: Path) -> dict[str, object]:
    directory = root / "discovery"
    supported, ignored = discovery_relative_paths()
    for relative in (*supported, *ignored):
        path = directory / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.touch()
    digest = hashlib.sha256()
    for relative in sorted((*supported, *ignored), key=lambda path: path.as_posix()):
        digest.update(relative.as_posix().encode("utf-8"))
        digest.update(b"\0")
    return {
        "path": directory.relative_to(root).as_posix(),
        "supportedFiles": len(supported),
        "ignoredFiles": len(ignored),
        "relativePathSetSha256": digest.hexdigest(),
    }


def generate_into(root: Path) -> dict[str, object]:
    root.mkdir(parents=True)
    media = write_stereo_routes(root)
    media.extend(write_alac_routes(root))
    media.append(write_surround_route(root))
    media.extend(write_batch_routes(root))
    media.sort(key=lambda entry: str(entry["id"]))
    discovery = write_discovery_tree(root)
    manifest: dict[str, object] = {
        "schemaVersion": SCHEMA_VERSION,
        "corpusId": CORPUS_ID,
        "generator": {
            "path": "scripts/generate-performance-corpus.py",
            "sha256": script_sha256(),
            "blockFrames": BLOCK_FRAMES,
        },
        "provenance": {
            "kind": "deterministically_generated",
            "copyrightedAudio": False,
            "license": "MIT",
        },
        "media": media,
        "discovery": discovery,
    }
    (root / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return manifest


def load_manifest(root: Path) -> dict[str, object]:
    path = root / "manifest.json"
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise CorpusError(f"cannot read {path}: {error}") from error
    if not isinstance(value, dict) or value.get("corpusId") != CORPUS_ID:
        raise CorpusError(f"{path} is not a {CORPUS_ID} manifest")
    return value


def validate_corpus(root: Path) -> dict[str, object]:
    manifest = load_manifest(root)
    if manifest.get("schemaVersion") != SCHEMA_VERSION:
        raise CorpusError("performance corpus schema version drifted")
    generator = manifest.get("generator")
    if not isinstance(generator, dict) or generator.get("sha256") != script_sha256():
        raise CorpusError("performance corpus was generated by different script bytes")
    media = manifest.get("media")
    if not isinstance(media, list):
        raise CorpusError("performance corpus media list is invalid")

    expected_files = {"manifest.json"}
    for entry in media:
        if not isinstance(entry, dict) or not isinstance(entry.get("path"), str):
            raise CorpusError("performance corpus media entry is invalid")
        relative = Path(entry["path"])
        if relative.is_absolute() or ".." in relative.parts:
            raise CorpusError(f"unsafe performance corpus path: {relative}")
        path = root / relative
        expected_files.add(relative.as_posix())
        if not path.is_file():
            raise CorpusError(f"performance corpus file is missing: {relative}")
        if path.stat().st_size != entry.get("sizeBytes"):
            raise CorpusError(f"performance corpus size drifted: {relative}")
        if sha256_file(path) != entry.get("sha256"):
            raise CorpusError(f"performance corpus hash drifted: {relative}")

    supported, ignored = discovery_relative_paths()
    for relative in (*supported, *ignored):
        expected_files.add((Path("discovery") / relative).as_posix())
    actual_files = {
        path.relative_to(root).as_posix()
        for path in root.rglob("*")
        if path.is_file()
    }
    if actual_files != expected_files:
        missing = sorted(expected_files - actual_files)
        extra = sorted(actual_files - expected_files)
        raise CorpusError(
            f"performance corpus file set drifted; missing={missing[:5]}, extra={extra[:5]}"
        )

    discovery = manifest.get("discovery")
    if not isinstance(discovery, dict):
        raise CorpusError("performance discovery manifest is invalid")
    if discovery.get("supportedFiles") != len(supported):
        raise CorpusError("performance discovery supported-file count drifted")
    if discovery.get("ignoredFiles") != len(ignored):
        raise CorpusError("performance discovery ignored-file count drifted")
    return manifest


def safe_remove_existing(root: Path) -> None:
    if root.is_symlink():
        raise CorpusError(f"refusing to replace symlinked corpus directory: {root}")
    load_manifest(root)
    shutil.rmtree(root)


def generate(root: Path, replace: bool) -> dict[str, object]:
    root = root.resolve()
    if root.exists():
        if not replace:
            return validate_corpus(root)
        safe_remove_existing(root)
    root.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix=f".{root.name}-", dir=root.parent
    ) as temporary:
        staging = Path(temporary) / root.name
        manifest = generate_into(staging)
        os.replace(staging, root)
    validate_corpus(root)
    return manifest


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=DEFAULT_OUTPUT,
        help=f"corpus directory (default: {DEFAULT_OUTPUT})",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="validate an existing corpus without changing it",
    )
    parser.add_argument(
        "--replace",
        action="store_true",
        help="replace only an existing directory carrying this corpus marker",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.check:
            if args.replace:
                raise CorpusError("--check and --replace cannot be combined")
            manifest = validate_corpus(args.output_dir.resolve())
            action = "verified"
        else:
            manifest = generate(args.output_dir, args.replace)
            action = "ready"
    except (CorpusError, OSError, subprocess.CalledProcessError) as error:
        print(f"performance corpus error: {error}", file=os.sys.stderr)
        return 1

    media = manifest["media"]
    total_bytes = sum(int(entry["sizeBytes"]) for entry in media)
    print(
        f"{CORPUS_ID} {action}: {len(media)} media files, "
        f"{total_bytes} bytes at {args.output_dir.resolve()}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
