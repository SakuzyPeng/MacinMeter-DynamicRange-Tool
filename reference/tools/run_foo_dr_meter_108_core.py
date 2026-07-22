#!/usr/bin/env python3
"""Drive the isolated foo_dr_meter 1.0.8 x64 core worker.

This parent harness does not start foobar2000 and does not load the target DLL
itself.  It validates and stages one finite interleaved f64 PCM stream, starts
one worker process for that one stream, and emits a path-free record.  The
record is evidence about the isolated core only; it does not establish foobar
decoder, component, renderer, or compatibility parity.
"""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import math
import os
import re
import stat
import struct
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Any, Iterable


SCHEMA_VERSION = 2
PROTOCOL_KIND_REQUEST = "foo_dr_meter_108_core_request"
PROTOCOL_KIND_RESULT = "foo_dr_meter_108_core_result"
PROTOCOL_KIND_ERROR = "foo_dr_meter_108_core_error"
DURATION_PROTOCOL_KIND_REQUEST = "foo_dr_meter_108_duration_request"
DURATION_PROTOCOL_KIND_RESULT = "foo_dr_meter_108_duration_result"
RECORD_KIND = "foo_dr_meter_108_core_harness_record"
DURATION_RECORD_KIND = "foo_dr_meter_108_duration_harness_record"
EXPECTED_TARGET_SHA256 = (
    "ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489"
)
EXPECTED_TARGET_BYTE_LENGTH = 424448
INIT_RVA = 0x8410
PUSH_RVA = 0x89F0
FINISH_RVA = 0x8DF0
DURATION_FORMAT_RVA = 0x38540
DEFAULT_BLOCK_FRAMES = 512
HISTOGRAM_BINS_PER_CHANNEL = 10001
HISTOGRAM_LAYOUT = "channel_major"
HISTOGRAM_ELEMENT_ENCODING = "u32le"
MAX_CHANNELS = 64
MAX_BLOCK_FRAMES = 1_048_576
MAX_WORKER_STDOUT_BYTES = 1_048_576
MAX_WORKER_STDERR_BYTES = 1_048_576
RUNTIME_ARTIFACT_NAMES = (
    "shared.dll",
    "msvcp140.dll",
    "vcruntime140.dll",
    "vcruntime140_1.dll",
)
RUNTIME_PROFILES = ("fixed_foobar_2_25_10",)
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
IDENTIFIER_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$")
BITS_RE = re.compile(r"^[0-9a-f]{8}$")
BITS64_RE = re.compile(r"^[0-9a-f]{16}$")
DURATION_TEXT_RE = re.compile(
    r"^(?:"
    r"\d+:[0-5]\d"
    r"|(?:[1-9]\d*):[0-5]\d:[0-5]\d"
    r"|(?:[1-6])d (?:\d|1\d|2[0-3]):[0-5]\d:[0-5]\d"
    r"|(?:[1-9]\d*)wk [0-6]d (?:\d|1\d|2[0-3]):[0-5]\d:[0-5]\d"
    r")$"
)
WINDOWS_DRIVE_RE = re.compile(r"(?i)(?:^|[^A-Za-z0-9_])[A-Z]:[\\/]")
UNC_RE = re.compile(r"\\\\[^\\\s]+\\")
POSIX_ABSOLUTE_RE = re.compile(r"(?:^|[\s\"'=({])/(?!/)[^\s\"']+")


class CoreHarnessError(ValueError):
    """A local input or worker response violates the harness contract."""


class WorkerReportedError(CoreHarnessError):
    """The worker returned one valid, path-free protocol error."""

    def __init__(self, code: str) -> None:
        super().__init__(f"worker reported {code}")
        self.code = code


@dataclass(frozen=True)
class FileIdentity:
    sha256: str
    byte_length: int


@dataclass(frozen=True)
class RuntimeArtifact:
    name: str
    source_path: Path
    identity: FileIdentity


@dataclass(frozen=True)
class PreparedPcm:
    input_id: str
    source_kind: str
    source_encoding: str
    conversion: str
    source_identity: FileIdentity
    pcm: bytes
    sample_rate: int
    channels: int
    frames: int
    manifest_sha256: str | None = None

    @property
    def pcm_identity(self) -> FileIdentity:
        return FileIdentity(sha256_bytes(self.pcm), len(self.pcm))


@dataclass(frozen=True)
class WorkerProcessResult:
    returncode: int
    stdout: bytes
    stderr: bytes


@dataclass(frozen=True)
class PreparedExecution:
    worker_raw: bytes
    worker_identity: FileIdentity
    target_identity: FileIdentity
    runtime_artifacts: tuple[RuntimeArtifact, ...]


@dataclass
class _BoundedPipeCapture:
    data: bytearray
    exceeded: bool = False
    error: OSError | ValueError | None = None


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def canonical_json_bytes(value: Any) -> bytes:
    try:
        rendered = json.dumps(
            value,
            ensure_ascii=False,
            sort_keys=True,
            indent=2,
            allow_nan=False,
        )
    except (TypeError, ValueError) as error:
        raise CoreHarnessError("value is not finite canonical JSON") from error
    return (rendered + "\n").encode("utf-8")


def load_json_object_bytes(raw: bytes, context: str) -> dict[str, Any]:
    def reject_constant(value: str) -> None:
        raise CoreHarnessError(f"{context} contains non-finite JSON number {value}")

    def reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in pairs:
            if key in result:
                raise CoreHarnessError(
                    f"{context} contains duplicate object key {key!r}"
                )
            result[key] = value
        return result

    try:
        value = json.loads(
            raw.decode("utf-8"),
            parse_constant=reject_constant,
            object_pairs_hook=reject_duplicate_keys,
        )
    except (UnicodeError, json.JSONDecodeError) as error:
        raise CoreHarnessError(f"{context} is not strict UTF-8 JSON") from error
    if not isinstance(value, dict):
        raise CoreHarnessError(f"{context} must contain one JSON object")
    return value


def require_exact_keys(
    value: dict[str, Any],
    required: Iterable[str],
    context: str,
) -> None:
    expected = set(required)
    missing = sorted(expected - value.keys())
    extra = sorted(value.keys() - expected)
    if missing or extra:
        raise CoreHarnessError(
            f"{context} keys differ: missing={missing}, extra={extra}"
        )


def require_int(
    value: Any,
    context: str,
    *,
    minimum: int = 0,
    maximum: int | None = None,
) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < minimum:
        raise CoreHarnessError(f"{context} must be an integer >= {minimum}")
    if maximum is not None and value > maximum:
        raise CoreHarnessError(f"{context} must be <= {maximum}")
    return value


def require_bool(value: Any, context: str) -> bool:
    if not isinstance(value, bool):
        raise CoreHarnessError(f"{context} must be a boolean")
    return value


def require_sha256(value: Any, context: str) -> str:
    if not isinstance(value, str) or SHA256_RE.fullmatch(value) is None:
        raise CoreHarnessError(f"{context} must be one lowercase SHA-256")
    return value


def require_identifier(value: Any, context: str) -> str:
    if not isinstance(value, str) or IDENTIFIER_RE.fullmatch(value) is None:
        raise CoreHarnessError(f"{context} must be a path-free identifier")
    return value


def require_regular_file_bytes(
    path: Path,
    context: str,
    *,
    maximum_bytes: int | None = None,
) -> bytes:
    if path.is_symlink():
        raise CoreHarnessError(f"{context} must not be a symbolic link")
    try:
        before = path.stat()
    except OSError as error:
        raise CoreHarnessError(f"cannot stat {context}") from error
    if not stat.S_ISREG(before.st_mode):
        raise CoreHarnessError(f"{context} must be a regular file")
    if maximum_bytes is not None and before.st_size > maximum_bytes:
        raise CoreHarnessError(f"{context} exceeds its byte limit")
    try:
        with path.open("rb") as source:
            raw = source.read()
            after_handle = os.fstat(source.fileno())
        after_path = path.stat()
    except OSError as error:
        raise CoreHarnessError(f"cannot read {context}") from error
    identities = (
        (before.st_dev, before.st_ino, before.st_size),
        (after_handle.st_dev, after_handle.st_ino, after_handle.st_size),
        (after_path.st_dev, after_path.st_ino, after_path.st_size),
    )
    if identities[0] != identities[1] or identities[1] != identities[2]:
        raise CoreHarnessError(f"{context} changed while it was read")
    if len(raw) != before.st_size:
        raise CoreHarnessError(f"{context} length changed while it was read")
    return raw


def assert_expected_identity(
    raw: bytes,
    expected_sha256: str,
    expected_byte_length: int | None,
    context: str,
) -> FileIdentity:
    expected = require_sha256(expected_sha256, f"{context} expected SHA-256")
    actual = FileIdentity(sha256_bytes(raw), len(raw))
    if actual.sha256 != expected:
        raise CoreHarnessError(f"{context} SHA-256 mismatch")
    if expected_byte_length is not None and actual.byte_length != expected_byte_length:
        raise CoreHarnessError(f"{context} byte length mismatch")
    return actual


def require_portable_relative_path(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value:
        raise CoreHarnessError(f"{context} must be a non-empty relative path")
    path = PurePosixPath(value)
    if (
        path.is_absolute()
        or PureWindowsPath(value).is_absolute()
        or path.as_posix() != value
        or "\\" in value
        or ":" in value
        or any(part in {"", ".", ".."} for part in path.parts)
    ):
        raise CoreHarnessError(f"{context} must be a canonical POSIX relative path")
    return value


def _parse_wave(raw: bytes) -> tuple[dict[str, int | str], bytes]:
    if len(raw) < 12 or raw[:4] != b"RIFF" or raw[8:12] != b"WAVE":
        raise CoreHarnessError("fixture is not a RIFF/WAVE file")
    if struct.unpack_from("<I", raw, 4)[0] + 8 != len(raw):
        raise CoreHarnessError("fixture RIFF length is inconsistent")
    cursor = 12
    fmt: bytes | None = None
    data: bytes | None = None
    while cursor < len(raw):
        if cursor + 8 > len(raw):
            raise CoreHarnessError("fixture has a truncated WAVE chunk header")
        chunk_id = raw[cursor : cursor + 4]
        size = struct.unpack_from("<I", raw, cursor + 4)[0]
        payload_start = cursor + 8
        payload_end = payload_start + size
        padded_end = payload_end + (size & 1)
        if payload_end > len(raw) or padded_end > len(raw):
            raise CoreHarnessError("fixture has a truncated WAVE chunk")
        payload = raw[payload_start:payload_end]
        if chunk_id == b"fmt ":
            if fmt is not None:
                raise CoreHarnessError("fixture repeats the WAVE fmt chunk")
            fmt = payload
        elif chunk_id == b"data":
            if data is not None:
                raise CoreHarnessError("fixture repeats the WAVE data chunk")
            data = payload
        cursor = padded_end
    if cursor != len(raw) or fmt is None or data is None or len(fmt) < 16:
        raise CoreHarnessError("fixture lacks a complete WAVE fmt/data pair")

    tag, channels, rate, byte_rate, block_align, bits = struct.unpack_from(
        "<HHIIHH", fmt
    )
    valid_bits = bits
    if tag == 0xFFFE:
        if len(fmt) < 40:
            raise CoreHarnessError("fixture has a truncated extensible fmt chunk")
        extension_size, valid_bits = struct.unpack_from("<HH", fmt, 16)
        if extension_size < 22 or len(fmt) < 18 + extension_size:
            raise CoreHarnessError("fixture has an invalid extensible fmt chunk")
        subformat = fmt[24:40]
        pcm_guid = bytes.fromhex("0100000000001000800000aa00389b71")
        float_guid = bytes.fromhex("0300000000001000800000aa00389b71")
        if subformat == pcm_guid:
            tag = 1
        elif subformat == float_guid:
            tag = 3
        else:
            raise CoreHarnessError("fixture has an unsupported WAVE subformat")
    elif len(fmt) != 16:
        raise CoreHarnessError("fixture has a non-canonical WAVE fmt chunk")

    require_int(channels, "fixture channels", minimum=1, maximum=MAX_CHANNELS)
    require_int(rate, "fixture sample rate", minimum=1)
    if bits % 8 != 0 or valid_bits != bits:
        raise CoreHarnessError("fixture has unsupported WAVE sample width")
    bytes_per_sample = bits // 8
    expected_align = channels * bytes_per_sample
    if (
        block_align != expected_align
        or byte_rate != rate * block_align
        or block_align == 0
        or len(data) % block_align != 0
    ):
        raise CoreHarnessError("fixture WAVE stream geometry is inconsistent")
    encoding_by_shape = {
        (1, 8): "wav-pcm-u8",
        (1, 16): "wav-pcm-s16le",
        (1, 24): "wav-pcm-s24le",
        (1, 32): "wav-pcm-s32le",
        (3, 32): "wav-ieee-float32le",
        (3, 64): "wav-ieee-float64le",
    }
    try:
        encoding = encoding_by_shape[(tag, bits)]
    except KeyError as error:
        raise CoreHarnessError("fixture WAVE encoding is unsupported") from error
    return (
        {
            "sampleRate": rate,
            "channels": channels,
            "frames": len(data) // block_align,
            "bits": bits,
            "tag": tag,
            "encoding": encoding,
        },
        data,
    )


def _wave_data_to_f64le(info: dict[str, int | str], data: bytes) -> bytes:
    tag = int(info["tag"])
    bits = int(info["bits"])
    output = bytearray()
    if tag == 3:
        code = "<f" if bits == 32 else "<d"
        width = bits // 8
        for offset in range(0, len(data), width):
            value = struct.unpack_from(code, data, offset)[0]
            if not math.isfinite(value):
                raise CoreHarnessError("fixture contains non-finite PCM")
            output.extend(struct.pack("<d", value))
        return bytes(output)

    width = bits // 8
    denominator = float(1 << (bits - 1))
    for offset in range(0, len(data), width):
        if bits == 8:
            integer = data[offset] - 128
        else:
            integer = int.from_bytes(
                data[offset : offset + width], "little", signed=True
            )
        output.extend(struct.pack("<d", integer / denominator))
    return bytes(output)


def _validate_f64le(
    raw: bytes,
    *,
    channels: int,
    frames: int,
    context: str,
) -> None:
    require_int(channels, f"{context} channels", minimum=1, maximum=MAX_CHANNELS)
    require_int(frames, f"{context} frames", minimum=0)
    expected_length = channels * frames * 8
    if len(raw) != expected_length:
        raise CoreHarnessError(f"{context} byte length is not stream-aligned")
    for (value,) in struct.iter_unpack("<d", raw):
        if not math.isfinite(value):
            raise CoreHarnessError(f"{context} contains non-finite PCM")


def prepare_manifest_fixture(
    manifest_path: Path,
    corpus_root: Path,
    fixture_id: str,
) -> PreparedPcm:
    fixture_id = require_identifier(fixture_id, "fixture ID")
    manifest_raw = require_regular_file_bytes(
        manifest_path, "fixture manifest", maximum_bytes=16 * 1024 * 1024
    )
    manifest = load_json_object_bytes(manifest_raw, "fixture manifest")
    cases = manifest.get("cases")
    if not isinstance(cases, list):
        raise CoreHarnessError("fixture manifest cases must be an array")
    matches = [
        case
        for case in cases
        if isinstance(case, dict) and case.get("id") == fixture_id
    ]
    if len(matches) != 1:
        raise CoreHarnessError("fixture ID must match exactly one manifest case")
    case = matches[0]
    relative = require_portable_relative_path(
        case.get("path"), "fixture manifest case path"
    )
    try:
        root = corpus_root.resolve(strict=True)
        fixture_path = (root / relative).resolve(strict=True)
        fixture_path.relative_to(root)
    except (OSError, ValueError) as error:
        raise CoreHarnessError("fixture path escapes or is absent from corpus") from error
    raw = require_regular_file_bytes(
        fixture_path, "fixture file", maximum_bytes=512 * 1024 * 1024
    )
    source_identity = assert_expected_identity(
        raw,
        require_sha256(case.get("fileSha256"), "fixture manifest file SHA-256"),
        require_int(case.get("byteLength"), "fixture manifest byte length"),
        "fixture file",
    )
    info, data = _parse_wave(raw)
    expected_fields = {
        "encoding": info["encoding"],
        "sampleRateHz": info["sampleRate"],
        "channels": info["channels"],
        "frames": info["frames"],
    }
    for key, actual in expected_fields.items():
        if case.get(key) != actual:
            raise CoreHarnessError(f"fixture manifest {key} differs from WAVE")
    if require_sha256(
        case.get("dataSha256"), "fixture manifest data SHA-256"
    ) != sha256_bytes(data):
        raise CoreHarnessError("fixture WAVE data SHA-256 differs from manifest")
    pcm = _wave_data_to_f64le(info, data)
    _validate_f64le(
        pcm,
        channels=int(info["channels"]),
        frames=int(info["frames"]),
        context="converted fixture PCM",
    )
    return PreparedPcm(
        input_id=fixture_id,
        source_kind="manifest_wav_fixture",
        source_encoding=str(info["encoding"]),
        conversion="strict_wav_sample_to_binary64",
        source_identity=source_identity,
        pcm=pcm,
        sample_rate=int(info["sampleRate"]),
        channels=int(info["channels"]),
        frames=int(info["frames"]),
        manifest_sha256=sha256_bytes(manifest_raw),
    )


def prepare_explicit_pcm(
    pcm_path: Path,
    *,
    input_id: str,
    expected_sha256: str,
    sample_rate: int,
    channels: int,
    frames: int,
) -> PreparedPcm:
    input_id = require_identifier(input_id, "input ID")
    require_int(sample_rate, "PCM sample rate", minimum=1)
    require_int(channels, "PCM channels", minimum=1, maximum=MAX_CHANNELS)
    require_int(frames, "PCM frames", minimum=0)
    raw = require_regular_file_bytes(
        pcm_path, "explicit PCM", maximum_bytes=1024 * 1024 * 1024
    )
    identity = assert_expected_identity(
        raw, expected_sha256, channels * frames * 8, "explicit PCM"
    )
    _validate_f64le(
        raw, channels=channels, frames=frames, context="explicit PCM"
    )
    return PreparedPcm(
        input_id=input_id,
        source_kind="explicit_f64le_pcm",
        source_encoding="f64le-interleaved",
        conversion="identity",
        source_identity=identity,
        pcm=raw,
        sample_rate=sample_rate,
        channels=channels,
        frames=frames,
    )


def _request_identity(
    prepared: PreparedPcm,
    worker: FileIdentity,
    target: FileIdentity,
    runtime_artifacts: tuple[RuntimeArtifact, ...],
    runtime_profile: str,
    block_frames: int,
    multichannel_loudness_weighting: bool = False,
) -> str:
    multichannel_loudness_weighting = require_bool(
        multichannel_loudness_weighting,
        "multichannel loudness weighting",
    )
    semantic = {
        "schemaVersion": SCHEMA_VERSION,
        "protocolKind": PROTOCOL_KIND_REQUEST,
        "input": {
            "inputId": prepared.input_id,
            "sourceKind": prepared.source_kind,
            "sourceSha256": prepared.source_identity.sha256,
            "sourceByteLength": prepared.source_identity.byte_length,
            "manifestSha256": prepared.manifest_sha256,
            "pcmSha256": prepared.pcm_identity.sha256,
            "pcmByteLength": prepared.pcm_identity.byte_length,
            "sampleRate": prepared.sample_rate,
            "channels": prepared.channels,
            "frames": prepared.frames,
        },
        "worker": {
            "sha256": worker.sha256,
            "byteLength": worker.byte_length,
        },
        "target": {
            "sha256": target.sha256,
            "byteLength": target.byte_length,
            "runtimeArtifacts": [
                {
                    "name": artifact.name,
                    "sha256": artifact.identity.sha256,
                    "byteLength": artifact.identity.byte_length,
                }
                for artifact in runtime_artifacts
            ],
            "runtimeProfile": runtime_profile,
            "initRva": INIT_RVA,
            "pushRva": PUSH_RVA,
            "finishRva": FINISH_RVA,
        },
        "options": {
            "multichannelLoudnessWeighting": multichannel_loudness_weighting,
            "blockFrames": block_frames,
        },
    }
    return sha256_bytes(canonical_json_bytes(semantic))


def build_worker_request(
    prepared: PreparedPcm,
    *,
    worker_identity: FileIdentity,
    target_identity: FileIdentity,
    runtime_artifacts: tuple[RuntimeArtifact, ...],
    runtime_profile: str,
    target_path: Path,
    pcm_path: Path,
    block_frames: int = DEFAULT_BLOCK_FRAMES,
    multichannel_loudness_weighting: bool = False,
) -> dict[str, Any]:
    block_frames = require_int(
        block_frames, "block frames", minimum=1, maximum=MAX_BLOCK_FRAMES
    )
    multichannel_loudness_weighting = require_bool(
        multichannel_loudness_weighting,
        "multichannel loudness weighting",
    )
    if runtime_profile not in RUNTIME_PROFILES:
        raise CoreHarnessError("runtime profile is unsupported")
    request_id = _request_identity(
        prepared,
        worker_identity,
        target_identity,
        runtime_artifacts,
        runtime_profile,
        block_frames,
        multichannel_loudness_weighting,
    )
    return {
        "schemaVersion": SCHEMA_VERSION,
        "kind": PROTOCOL_KIND_REQUEST,
        "requestId": request_id,
        "target": {
            "dllPath": str(target_path.resolve(strict=True)),
            "sha256": target_identity.sha256,
            "byteLength": target_identity.byte_length,
            "initRva": INIT_RVA,
            "pushRva": PUSH_RVA,
            "finishRva": FINISH_RVA,
            "runtimeProfile": runtime_profile,
            "runtimeArtifacts": [
                {
                    "name": artifact.name,
                    "sourcePath": str(artifact.source_path.resolve(strict=True)),
                    "sha256": artifact.identity.sha256,
                    "byteLength": artifact.identity.byte_length,
                }
                for artifact in runtime_artifacts
            ],
        },
        "stream": {
            "sampleRate": prepared.sample_rate,
            "channels": prepared.channels,
            "frames": prepared.frames,
            "sampleEncoding": "f64le-interleaved",
        },
        "pcm": {
            "path": str(pcm_path.resolve(strict=True)),
            "sha256": prepared.pcm_identity.sha256,
            "byteLength": prepared.pcm_identity.byte_length,
        },
        "options": {
            "multichannelLoudnessWeighting": multichannel_loudness_weighting,
            "blockFrames": block_frames,
        },
    }


def _duration_request_identity(
    worker: FileIdentity,
    target: FileIdentity,
    runtime_artifacts: tuple[RuntimeArtifact, ...],
    runtime_profile: str,
    decoded_frames: int,
    sample_rate_hz: int,
    fractional_digits: int,
) -> str:
    semantic = {
        "schemaVersion": SCHEMA_VERSION,
        "protocolKind": DURATION_PROTOCOL_KIND_REQUEST,
        "worker": {
            "sha256": worker.sha256,
            "byteLength": worker.byte_length,
        },
        "target": {
            "sha256": target.sha256,
            "byteLength": target.byte_length,
            "runtimeArtifacts": [
                {
                    "name": artifact.name,
                    "sha256": artifact.identity.sha256,
                    "byteLength": artifact.identity.byte_length,
                }
                for artifact in runtime_artifacts
            ],
            "runtimeProfile": runtime_profile,
            "durationFormatRva": DURATION_FORMAT_RVA,
        },
        "duration": {
            "decodedFrames": decoded_frames,
            "sampleRateHz": sample_rate_hz,
            "fractionalDigits": fractional_digits,
        },
    }
    return sha256_bytes(canonical_json_bytes(semantic))


def build_duration_worker_request(
    *,
    worker_identity: FileIdentity,
    target_identity: FileIdentity,
    runtime_artifacts: tuple[RuntimeArtifact, ...],
    runtime_profile: str,
    target_path: Path,
    decoded_frames: int,
    sample_rate_hz: int,
    fractional_digits: int = 0,
) -> dict[str, Any]:
    decoded_frames = require_int(
        decoded_frames,
        "duration decoded frames",
        minimum=0,
        maximum=0xFFFF_FFFF_FFFF_FFFF,
    )
    sample_rate_hz = require_int(
        sample_rate_hz,
        "duration sample rate",
        minimum=1,
        maximum=0xFFFF_FFFF,
    )
    fractional_digits = require_int(
        fractional_digits,
        "duration fractional digits",
        minimum=0,
        maximum=0,
    )
    if runtime_profile not in RUNTIME_PROFILES:
        raise CoreHarnessError("runtime profile is unsupported")
    request_id = _duration_request_identity(
        worker_identity,
        target_identity,
        runtime_artifacts,
        runtime_profile,
        decoded_frames,
        sample_rate_hz,
        fractional_digits,
    )
    return {
        "schemaVersion": SCHEMA_VERSION,
        "kind": DURATION_PROTOCOL_KIND_REQUEST,
        "requestId": request_id,
        "target": {
            "dllPath": str(target_path.resolve(strict=True)),
            "sha256": target_identity.sha256,
            "byteLength": target_identity.byte_length,
            "durationFormatRva": DURATION_FORMAT_RVA,
            "runtimeProfile": runtime_profile,
            "runtimeArtifacts": [
                {
                    "name": artifact.name,
                    "sourcePath": str(artifact.source_path.resolve(strict=True)),
                    "sha256": artifact.identity.sha256,
                    "byteLength": artifact.identity.byte_length,
                }
                for artifact in runtime_artifacts
            ],
        },
        "duration": {
            "decodedFrames": decoded_frames,
            "sampleRateHz": sample_rate_hz,
            "fractionalDigits": fractional_digits,
        },
    }


def _looks_absolute_or_private_path(value: str) -> bool:
    if "file://" in value.casefold() or WINDOWS_DRIVE_RE.search(value):
        return True
    if (
        UNC_RE.search(value)
        or POSIX_ABSOLUTE_RE.search(value)
        or value.startswith(("/", "~"))
    ):
        return True
    return False


def assert_path_free(value: Any, context: str = "worker response") -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            assert_path_free(child, f"{context}.{key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            assert_path_free(child, f"{context}[{index}]")
    elif isinstance(value, str) and _looks_absolute_or_private_path(value):
        raise CoreHarnessError(f"{context} contains an absolute/private path")


def _require_finite_f32_bits(value: Any, context: str) -> str:
    if not isinstance(value, str) or BITS_RE.fullmatch(value) is None:
        raise CoreHarnessError(f"{context} must be eight lowercase hex digits")
    number = struct.unpack("<f", struct.pack("<I", int(value, 16)))[0]
    if not math.isfinite(number):
        raise CoreHarnessError(f"{context} encodes a non-finite binary32 value")
    return value


def _require_finite_f64_bits(value: Any, context: str) -> str:
    if not isinstance(value, str) or BITS64_RE.fullmatch(value) is None:
        raise CoreHarnessError(f"{context} must be sixteen lowercase hex digits")
    number = struct.unpack("<d", struct.pack("<Q", int(value, 16)))[0]
    if not math.isfinite(number):
        raise CoreHarnessError(f"{context} encodes a non-finite binary64 value")
    return value


def _validate_session_state(
    value: Any,
    *,
    expected_frames: int,
    finalized: bool,
    context: str,
) -> tuple[int, int, int]:
    if not isinstance(value, dict):
        raise CoreHarnessError(f"{context} must be an object")
    require_exact_keys(
        value,
        {"currentWindowFrames", "windowCount", "submittedFrames"},
        context,
    )
    current = require_int(
        value["currentWindowFrames"],
        f"{context} currentWindowFrames",
        minimum=0,
        maximum=0xFFFF_FFFF,
    )
    windows = require_int(
        value["windowCount"],
        f"{context} windowCount",
        minimum=0,
        maximum=0xFFFF_FFFF_FFFF_FFFF,
    )
    submitted = require_int(
        value["submittedFrames"],
        f"{context} submittedFrames",
        minimum=0,
        maximum=0xFFFF_FFFF_FFFF_FFFF,
    )
    if finalized:
        if current != 0 or submitted != expected_frames:
            raise CoreHarnessError(f"{context} finalized frame accounting mismatch")
    elif current + submitted != expected_frames:
        raise CoreHarnessError(f"{context} pre-finish frame accounting mismatch")
    return current, windows, submitted


def _validate_fp_control_pair(value: Any, context: str) -> tuple[int, int]:
    if not isinstance(value, dict):
        raise CoreHarnessError(f"{context} must be an object")
    require_exact_keys(
        value, {"x87ControlWordBits", "mxcsrBits"}, context
    )
    x87 = value["x87ControlWordBits"]
    mxcsr = value["mxcsrBits"]
    if not isinstance(x87, str) or re.fullmatch(r"[0-9a-f]{4}", x87) is None:
        raise CoreHarnessError(f"{context} x87 bits must be four lowercase hex digits")
    if not isinstance(mxcsr, str) or BITS_RE.fullmatch(mxcsr) is None:
        raise CoreHarnessError(f"{context} MXCSR bits must be eight lowercase hex digits")
    return int(x87, 16), int(mxcsr, 16)


def _validate_fp_environment(value: Any) -> None:
    if not isinstance(value, dict):
        raise CoreHarnessError("worker FP environment must be an object")
    require_exact_keys(
        value, {"before", "applied", "after", "restored"}, "worker FP environment"
    )
    before = _validate_fp_control_pair(value["before"], "worker FP before")
    after = _validate_fp_control_pair(value["after"], "worker FP after")
    restored = _validate_fp_control_pair(value["restored"], "worker FP restored")
    applied = value["applied"]
    if not isinstance(applied, dict):
        raise CoreHarnessError("worker FP applied must be an object")
    require_exact_keys(
        applied,
        {
            "x87ControlWordBits",
            "mxcsrBits",
            "rounding",
            "ftz",
            "daz",
            "exceptionsMasked",
        },
        "worker FP applied",
    )
    applied_bits = _validate_fp_control_pair(
        {
            "x87ControlWordBits": applied["x87ControlWordBits"],
            "mxcsrBits": applied["mxcsrBits"],
        },
        "worker FP applied controls",
    )
    if (
        applied["rounding"] != "nearest"
        or applied["ftz"] is not False
        or applied["daz"] is not False
        or applied["exceptionsMasked"] is not True
    ):
        raise CoreHarnessError("worker FP applied semantic policy mismatch")
    x87_policy_mask = 0x0C3F
    x87_expected = 0x003F
    mxcsr_policy_mask = 0xFFC0
    mxcsr_expected = 0x1F80
    if (
        applied_bits[0] & x87_policy_mask != x87_expected
        or applied_bits[1] & mxcsr_policy_mask != mxcsr_expected
        or applied_bits[1] & 0x3F != 0
    ):
        raise CoreHarnessError("worker FP applied control bits violate policy")
    if (
        after[0] & x87_policy_mask != x87_expected
        or after[1] & mxcsr_policy_mask != mxcsr_expected
    ):
        raise CoreHarnessError("worker FP policy changed during core execution")
    if restored != before:
        raise CoreHarnessError("worker FP environment was not exactly restored")


def _validate_result_options(
    value: Any,
    *,
    request: dict[str, Any],
) -> None:
    if not isinstance(value, dict):
        raise CoreHarnessError("worker result options must be an object")
    require_exact_keys(
        value,
        {"multichannelLoudnessWeighting", "blockFrames"},
        "worker result options",
    )
    require_bool(
        value["multichannelLoudnessWeighting"],
        "worker result multichannel loudness weighting",
    )
    require_int(
        value["blockFrames"],
        "worker result block frames",
        minimum=1,
        maximum=MAX_BLOCK_FRAMES,
    )
    if value != request["options"]:
        raise CoreHarnessError("worker result options mismatch")


def _validate_histogram_after_finish(
    value: Any,
    *,
    expected_channels: int,
    window_count: int,
) -> None:
    if not isinstance(value, dict):
        raise CoreHarnessError("worker histogram must be an object")
    require_exact_keys(
        value,
        {"layout", "elementEncoding", "binsPerChannel", "channels"},
        "worker histogram",
    )
    if (
        value["layout"] != HISTOGRAM_LAYOUT
        or value["elementEncoding"] != HISTOGRAM_ELEMENT_ENCODING
        or value["binsPerChannel"] != HISTOGRAM_BINS_PER_CHANNEL
    ):
        raise CoreHarnessError("worker histogram geometry mismatch")
    channels = value["channels"]
    if not isinstance(channels, list) or len(channels) != expected_channels:
        raise CoreHarnessError("worker histogram channel count mismatch")
    for index, item in enumerate(channels):
        if not isinstance(item, dict):
            raise CoreHarnessError("worker histogram channel must be an object")
        require_exact_keys(
            item,
            {
                "index",
                "totalCount",
                "nonzeroBinCount",
                "minus100DbCount",
                "zeroDbCount",
                "sha256",
            },
            "worker histogram channel",
        )
        if item["index"] != index:
            raise CoreHarnessError(
                "worker histogram channel indexes must be ordered"
            )
        total = require_int(
            item["totalCount"],
            f"worker histogram channel {index} total count",
            minimum=0,
            maximum=0xFFFF_FFFF_FFFF_FFFF,
        )
        nonzero = require_int(
            item["nonzeroBinCount"],
            f"worker histogram channel {index} nonzero bin count",
            minimum=0,
            maximum=HISTOGRAM_BINS_PER_CHANNEL,
        )
        minus_100 = require_int(
            item["minus100DbCount"],
            f"worker histogram channel {index} minus-100 dB count",
            minimum=0,
            maximum=0xFFFF_FFFF_FFFF_FFFF,
        )
        zero = require_int(
            item["zeroDbCount"],
            f"worker histogram channel {index} zero dB count",
            minimum=0,
            maximum=0xFFFF_FFFF_FFFF_FFFF,
        )
        require_sha256(
            item["sha256"],
            f"worker histogram channel {index} SHA-256",
        )
        if total > window_count:
            raise CoreHarnessError(
                "worker histogram total exceeds finalized window count"
            )
        if nonzero > total:
            raise CoreHarnessError(
                "worker histogram nonzero bin count exceeds total count"
            )
        if minus_100 > total or zero > total or minus_100 + zero > total:
            raise CoreHarnessError(
                "worker histogram distinguished-bin counts exceed total count"
            )


def validate_worker_response(
    raw_stdout: bytes,
    *,
    exit_code: int,
    request: dict[str, Any],
) -> dict[str, Any]:
    if len(raw_stdout) > MAX_WORKER_STDOUT_BYTES:
        raise CoreHarnessError("worker stdout exceeds its byte limit")
    try:
        text = raw_stdout.decode("utf-8")
    except UnicodeError as error:
        raise CoreHarnessError("worker stdout is not UTF-8") from error
    lines = text.splitlines()
    if len(lines) != 1 or not lines[0].strip():
        raise CoreHarnessError("worker stdout must contain exactly one JSON line")
    response = load_json_object_bytes(lines[0].encode("utf-8"), "worker response")
    assert_path_free(response)
    kind = response.get("kind")
    common_values = {
        "schemaVersion": SCHEMA_VERSION,
        "requestId": request["requestId"],
        "targetSha256": request["target"]["sha256"],
    }
    for key, expected in common_values.items():
        if response.get(key) != expected:
            raise CoreHarnessError(f"worker response {key} mismatch")

    if kind == PROTOCOL_KIND_ERROR:
        require_exact_keys(
            response,
            {"schemaVersion", "kind", "requestId", "targetSha256", "error"},
            "worker error response",
        )
        if exit_code == 0:
            raise CoreHarnessError("worker error response used a success exit code")
        error = response["error"]
        if not isinstance(error, dict):
            raise CoreHarnessError("worker error must be an object")
        require_exact_keys(error, {"code", "message"}, "worker error")
        code = require_identifier(error.get("code"), "worker error code")
        message = error.get("message")
        if not isinstance(message, str) or not message or len(message) > 512:
            raise CoreHarnessError("worker error message is invalid")
        raise WorkerReportedError(code)

    if kind != PROTOCOL_KIND_RESULT:
        raise CoreHarnessError("worker response kind is unknown")
    require_exact_keys(
        response,
        {
            "schemaVersion",
            "kind",
            "requestId",
            "targetSha256",
            "data",
        },
        "worker result response",
    )
    if exit_code != 0:
        raise CoreHarnessError("worker result response used a failure exit code")
    data = response["data"]
    if not isinstance(data, dict):
        raise CoreHarnessError("worker result data must be an object")
    require_exact_keys(
        data,
        {
            "sampleRateHz",
            "channels",
            "frames",
            "trackDrBits",
            "channelResults",
            "runtimeArtifacts",
            "loaderMode",
            "sharedServiceBoundary",
            "sessionBeforeFinish",
            "sessionAfterFinish",
            "channelStateAfterFinish",
            "fpEnvironment",
            "options",
            "histogramAfterFinish",
        },
        "worker result data",
    )
    expected_stream = request["stream"]
    if (
        data["sampleRateHz"] != expected_stream["sampleRate"]
        or data["channels"] != expected_stream["channels"]
        or data["frames"] != expected_stream["frames"]
    ):
        raise CoreHarnessError("worker result stream geometry mismatch")
    _require_finite_f32_bits(data["trackDrBits"], "worker track DR bits")
    if data["loaderMode"] != "private_staging_dll_load_dir_system32":
        raise CoreHarnessError("worker loader mode mismatch")
    _validate_result_options(data["options"], request=request)
    shared_boundary = data["sharedServiceBoundary"]
    if not isinstance(shared_boundary, dict):
        raise CoreHarnessError("worker shared-service boundary must be an object")
    require_exact_keys(
        shared_boundary,
        {"loadLifecycle", "coreExecution", "armedImportCount"},
        "worker shared-service boundary",
    )
    if shared_boundary != {
        "loadLifecycle": "real_shared",
        "coreExecution": "fail_fast_iat_tripwire",
        "armedImportCount": 13,
    }:
        raise CoreHarnessError("worker shared-service boundary mismatch")
    response_artifacts = data["runtimeArtifacts"]
    expected_artifacts = [
        {
            "name": item["name"],
            "sha256": item["sha256"],
            "byteLength": item["byteLength"],
        }
        for item in request["target"]["runtimeArtifacts"]
    ]
    if response_artifacts != expected_artifacts:
        raise CoreHarnessError("worker runtime artifact identities mismatch")
    before_session = _validate_session_state(
        data["sessionBeforeFinish"],
        expected_frames=expected_stream["frames"],
        finalized=False,
        context="worker session before finish",
    )
    after_session = _validate_session_state(
        data["sessionAfterFinish"],
        expected_frames=expected_stream["frames"],
        finalized=True,
        context="worker session after finish",
    )
    expected_window_delta = 1 if before_session[0] > 0 else 0
    if after_session[1] != before_session[1] + expected_window_delta:
        raise CoreHarnessError("worker finish window-count transition mismatch")
    _validate_histogram_after_finish(
        data["histogramAfterFinish"],
        expected_channels=expected_stream["channels"],
        window_count=after_session[1],
    )
    _validate_fp_environment(data["fpEnvironment"])
    channel_state = data["channelStateAfterFinish"]
    if (
        not isinstance(channel_state, list)
        or len(channel_state) != expected_stream["channels"]
    ):
        raise CoreHarnessError("worker channel state count mismatch")
    state_bit_keys = (
        "rmsSquareSumBits",
        "primaryPeakBits",
        "secondaryPeakBits",
        "primaryPeakKeyBits",
        "secondaryPeakKeyBits",
    )
    for index, item in enumerate(channel_state):
        if not isinstance(item, dict):
            raise CoreHarnessError("worker channel state must be an object")
        require_exact_keys(
            item,
            {"index", *state_bit_keys},
            "worker channel state",
        )
        if item["index"] != index:
            raise CoreHarnessError("worker channel state indexes must be ordered")
        for key in state_bit_keys:
            _require_finite_f64_bits(
                item[key], f"worker channel state {index} {key}"
            )
    channels = data["channelResults"]
    if not isinstance(channels, list) or len(channels) != expected_stream["channels"]:
        raise CoreHarnessError("worker channel result count mismatch")
    for index, item in enumerate(channels):
        if not isinstance(item, dict):
            raise CoreHarnessError("worker channel result must be an object")
        require_exact_keys(
            item, {"index", "drBits", "peakBits", "rmsBits"}, "worker channel result"
        )
        if item["index"] != index:
            raise CoreHarnessError("worker channel indexes must be contiguous and ordered")
        for key in ("drBits", "peakBits", "rmsBits"):
            _require_finite_f32_bits(
                item[key], f"worker channel {index} {key}"
            )
    return response


def _duration_seconds_bits(decoded_frames: int, sample_rate_hz: int) -> str:
    seconds = float(decoded_frames) / float(sample_rate_hz)
    if not math.isfinite(seconds):
        raise CoreHarnessError("duration seconds are not finite")
    bits = struct.unpack("<Q", struct.pack("<d", seconds))[0]
    return f"{bits:016x}"


def _validate_duration_text(value: Any) -> str:
    if not isinstance(value, str) or not value or len(value) > 128:
        raise CoreHarnessError("worker duration text is invalid")
    try:
        value.encode("ascii")
    except UnicodeEncodeError as error:
        raise CoreHarnessError("worker duration text must be ASCII") from error
    if DURATION_TEXT_RE.fullmatch(value) is None:
        raise CoreHarnessError("worker duration text is not canonical")
    return value


def validate_duration_worker_response(
    raw_stdout: bytes,
    *,
    exit_code: int,
    request: dict[str, Any],
) -> dict[str, Any]:
    if len(raw_stdout) > MAX_WORKER_STDOUT_BYTES:
        raise CoreHarnessError("worker stdout exceeds its byte limit")
    try:
        text = raw_stdout.decode("utf-8")
    except UnicodeError as error:
        raise CoreHarnessError("worker stdout is not UTF-8") from error
    lines = text.splitlines()
    if len(lines) != 1 or not lines[0].strip():
        raise CoreHarnessError("worker stdout must contain exactly one JSON line")
    response = load_json_object_bytes(
        lines[0].encode("utf-8"),
        "duration worker response",
    )
    assert_path_free(response)
    common_values = {
        "schemaVersion": SCHEMA_VERSION,
        "requestId": request["requestId"],
        "targetSha256": request["target"]["sha256"],
    }
    for key, expected in common_values.items():
        if response.get(key) != expected:
            raise CoreHarnessError(f"worker response {key} mismatch")

    kind = response.get("kind")
    if kind == PROTOCOL_KIND_ERROR:
        require_exact_keys(
            response,
            {"schemaVersion", "kind", "requestId", "targetSha256", "error"},
            "worker error response",
        )
        if exit_code == 0:
            raise CoreHarnessError(
                "worker error response used a success exit code"
            )
        error = response["error"]
        if not isinstance(error, dict):
            raise CoreHarnessError("worker error must be an object")
        require_exact_keys(error, {"code", "message"}, "worker error")
        code = require_identifier(error.get("code"), "worker error code")
        message = error.get("message")
        if not isinstance(message, str) or not message or len(message) > 512:
            raise CoreHarnessError("worker error message is invalid")
        raise WorkerReportedError(code)

    if kind != DURATION_PROTOCOL_KIND_RESULT:
        raise CoreHarnessError("worker response kind is unknown")
    require_exact_keys(
        response,
        {
            "schemaVersion",
            "kind",
            "requestId",
            "targetSha256",
            "data",
        },
        "duration worker result response",
    )
    if exit_code != 0:
        raise CoreHarnessError(
            "duration worker result response used a failure exit code"
        )
    data = response["data"]
    if not isinstance(data, dict):
        raise CoreHarnessError("duration worker result data must be an object")
    require_exact_keys(
        data,
        {
            "geometry",
            "secondsBits",
            "text",
            "runtimeArtifacts",
            "loaderMode",
            "sharedServiceBoundary",
            "fpEnvironment",
        },
        "duration worker result data",
    )
    geometry = data["geometry"]
    if not isinstance(geometry, dict):
        raise CoreHarnessError("duration worker geometry must be an object")
    require_exact_keys(
        geometry,
        {"decodedFrames", "sampleRateHz", "fractionalDigits"},
        "duration worker geometry",
    )
    decoded_frames = require_int(
        geometry["decodedFrames"],
        "duration worker decoded frames",
        minimum=0,
        maximum=0xFFFF_FFFF_FFFF_FFFF,
    )
    sample_rate_hz = require_int(
        geometry["sampleRateHz"],
        "duration worker sample rate",
        minimum=1,
        maximum=0xFFFF_FFFF,
    )
    require_int(
        geometry["fractionalDigits"],
        "duration worker fractional digits",
        minimum=0,
        maximum=0,
    )
    if geometry != request["duration"]:
        raise CoreHarnessError("duration worker geometry mismatch")
    seconds_bits = _require_finite_f64_bits(
        data["secondsBits"],
        "duration worker seconds bits",
    )
    expected_seconds_bits = _duration_seconds_bits(
        decoded_frames,
        sample_rate_hz,
    )
    if seconds_bits != expected_seconds_bits:
        raise CoreHarnessError("duration worker seconds bits mismatch")
    _validate_duration_text(data["text"])
    if data["loaderMode"] != "private_staging_dll_load_dir_system32":
        raise CoreHarnessError("worker loader mode mismatch")
    shared_boundary = data["sharedServiceBoundary"]
    if not isinstance(shared_boundary, dict):
        raise CoreHarnessError(
            "duration worker shared-service boundary must be an object"
        )
    require_exact_keys(
        shared_boundary,
        {"loadLifecycle", "numericLeafExecution", "armedImportCount"},
        "duration worker shared-service boundary",
    )
    if shared_boundary != {
        "loadLifecycle": "real_shared",
        "numericLeafExecution": "fail_fast_iat_tripwire",
        "armedImportCount": 13,
    }:
        raise CoreHarnessError(
            "duration worker shared-service boundary mismatch"
        )
    expected_artifacts = [
        {
            "name": item["name"],
            "sha256": item["sha256"],
            "byteLength": item["byteLength"],
        }
        for item in request["target"]["runtimeArtifacts"]
    ]
    if data["runtimeArtifacts"] != expected_artifacts:
        raise CoreHarnessError(
            "duration worker runtime artifact identities mismatch"
        )
    _validate_fp_environment(data["fpEnvironment"])
    return response


def _stage_worker(worker_raw: bytes, original: Path, stage: Path) -> Path:
    suffix = original.suffix.casefold()
    name = "core-worker.exe" if suffix == ".exe" else "core-worker.py"
    target = stage / name
    target.write_bytes(worker_raw)
    target.chmod(0o700)
    return target


def _worker_command(worker: Path, request: Path) -> list[str]:
    if worker.suffix.casefold() == ".py":
        return [sys.executable, str(worker), "--request", str(request)]
    return [str(worker), "--request", str(request)]


def _is_windows() -> bool:
    return os.name == "nt"


def _open_windows_no_write_delete_handle(path: Path, *, directory: bool) -> int:
    import ctypes
    from ctypes import wintypes

    file_read_attributes = 0x00000080
    file_share_read = 0x00000001
    open_existing = 3
    file_flag_backup_semantics = 0x02000000
    file_flag_open_reparse_point = 0x00200000
    flags = file_flag_open_reparse_point
    if directory:
        flags |= file_flag_backup_semantics

    create_file = ctypes.WinDLL("kernel32", use_last_error=True).CreateFileW
    create_file.argtypes = (
        wintypes.LPCWSTR,
        wintypes.DWORD,
        wintypes.DWORD,
        wintypes.LPVOID,
        wintypes.DWORD,
        wintypes.DWORD,
        wintypes.HANDLE,
    )
    create_file.restype = wintypes.HANDLE
    handle = create_file(
        str(path.resolve(strict=True)),
        file_read_attributes,
        file_share_read,
        None,
        open_existing,
        flags,
        None,
    )
    invalid_handle = ctypes.c_void_p(-1).value
    handle_value = ctypes.cast(handle, ctypes.c_void_p).value
    if handle_value is None or handle_value == invalid_handle:
        error_code = ctypes.get_last_error()
        label = "staging directory" if directory else "staged worker"
        raise CoreHarnessError(
            f"cannot guard {label} against write/delete"
        ) from OSError(error_code, ctypes.FormatError(error_code))
    return handle_value


def _close_windows_handle(handle: int) -> None:
    import ctypes
    from ctypes import wintypes

    close_handle = ctypes.WinDLL("kernel32", use_last_error=True).CloseHandle
    close_handle.argtypes = (wintypes.HANDLE,)
    close_handle.restype = wintypes.BOOL
    if not close_handle(wintypes.HANDLE(handle)):
        error_code = ctypes.get_last_error()
        raise OSError(error_code, ctypes.FormatError(error_code))


@contextlib.contextmanager
def _hold_staged_worker_launch_guards(
    stage: Path,
    staged_worker: Path,
    staged_pcm: Path | None,
    request_path: Path,
) -> Iterable[None]:
    if not _is_windows():
        yield
        return

    handles: list[int] = []
    staged_files = tuple(
        path
        for path in (staged_worker, staged_pcm, request_path)
        if path is not None
    )
    try:
        handles.append(
            _open_windows_no_write_delete_handle(stage, directory=True)
        )
        for path in staged_files:
            handles.append(
                _open_windows_no_write_delete_handle(path, directory=False)
            )
        if stage.is_symlink() or any(
            path.is_symlink() for path in staged_files
        ):
            raise CoreHarnessError("staged execution path became a symbolic link")
        yield
    finally:
        close_error: OSError | None = None
        for handle in reversed(handles):
            try:
                _close_windows_handle(handle)
            except OSError as error:
                close_error = error
        if close_error is not None and sys.exc_info()[0] is None:
            raise CoreHarnessError("cannot release staged worker guards") from close_error


def _drain_pipe_bounded(
    stream: Any,
    *,
    byte_limit: int,
    capture: _BoundedPipeCapture,
    state_changed: threading.Event,
) -> None:
    capture_limit = byte_limit + 1
    try:
        while True:
            chunk = stream.read(64 * 1024)
            if not chunk:
                return
            remaining = capture_limit - len(capture.data)
            if remaining > 0:
                capture.data.extend(chunk[:remaining])
            if len(chunk) > remaining or len(capture.data) > byte_limit:
                capture.exceeded = True
                state_changed.set()
    except (OSError, ValueError) as error:
        capture.error = error
        state_changed.set()


def _terminate_direct_process(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    try:
        process.terminate()
    except OSError:
        pass
    try:
        process.wait(timeout=1.0)
        return
    except subprocess.TimeoutExpired:
        pass
    try:
        process.kill()
    except OSError:
        pass
    try:
        process.wait(timeout=2.0)
    except subprocess.TimeoutExpired as error:
        raise CoreHarnessError("cannot terminate core worker") from error


def _run_worker_bounded(
    command: list[str],
    *,
    cwd: Path,
    timeout_seconds: float,
) -> WorkerProcessResult:
    try:
        process = subprocess.Popen(
            command,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=cwd,
            bufsize=0,
        )
    except OSError as error:
        raise CoreHarnessError("cannot start core worker") from error
    if process.stdout is None or process.stderr is None:
        _terminate_direct_process(process)
        raise CoreHarnessError("core worker pipes were not created")

    stdout_capture = _BoundedPipeCapture(bytearray())
    stderr_capture = _BoundedPipeCapture(bytearray())
    state_changed = threading.Event()
    readers = (
        threading.Thread(
            target=_drain_pipe_bounded,
            kwargs={
                "stream": process.stdout,
                "byte_limit": MAX_WORKER_STDOUT_BYTES,
                "capture": stdout_capture,
                "state_changed": state_changed,
            },
            name="core-worker-stdout",
            daemon=True,
        ),
        threading.Thread(
            target=_drain_pipe_bounded,
            kwargs={
                "stream": process.stderr,
                "byte_limit": MAX_WORKER_STDERR_BYTES,
                "capture": stderr_capture,
                "state_changed": state_changed,
            },
            name="core-worker-stderr",
            daemon=True,
        ),
    )
    for reader in readers:
        reader.start()

    deadline = time.monotonic() + timeout_seconds
    failure: str | None = None
    while process.poll() is None:
        remaining = deadline - time.monotonic()
        if remaining <= 0.0:
            failure = "core worker timed out"
            break
        state_changed.wait(min(remaining, 0.05))
        if stdout_capture.exceeded:
            failure = "worker stdout exceeds its byte limit"
            break
        if stderr_capture.exceeded:
            failure = "worker stderr exceeds its byte limit"
            break
        if stdout_capture.error is not None or stderr_capture.error is not None:
            failure = "cannot capture core worker output"
            break
        state_changed.clear()

    if failure is None and (
        stdout_capture.exceeded or stderr_capture.exceeded
    ):
        failure = (
            "worker stdout exceeds its byte limit"
            if stdout_capture.exceeded
            else "worker stderr exceeds its byte limit"
        )
    if failure is not None:
        _terminate_direct_process(process)
    else:
        process.wait()

    # This bounds and terminates the direct worker. A descendant that inherits
    # either pipe is outside this process contract and is detected below when
    # the pipe fails to reach EOF; no cross-platform process-tree kill is used.
    for reader in readers:
        reader.join(timeout=1.0)
    readers_stopped = all(not reader.is_alive() for reader in readers)
    if readers_stopped:
        process.stdout.close()
        process.stderr.close()
    if not readers_stopped:
        raise CoreHarnessError(
            "core worker output pipes remained open after process exit"
        )
    if stdout_capture.error is not None or stderr_capture.error is not None:
        raise CoreHarnessError("cannot capture core worker output")
    if failure is not None:
        raise CoreHarnessError(failure)
    if stdout_capture.exceeded:
        raise CoreHarnessError("worker stdout exceeds its byte limit")
    if stderr_capture.exceeded:
        raise CoreHarnessError("worker stderr exceeds its byte limit")
    return WorkerProcessResult(
        returncode=process.returncode,
        stdout=bytes(stdout_capture.data),
        stderr=bytes(stderr_capture.data),
    )


def _validate_timeout_seconds(timeout_seconds: float) -> float:
    if (
        isinstance(timeout_seconds, bool)
        or not isinstance(timeout_seconds, (int, float))
        or not math.isfinite(timeout_seconds)
        or timeout_seconds <= 0.0
        or timeout_seconds > 300.0
    ):
        raise CoreHarnessError(
            "timeout must be finite and within (0, 300] seconds"
        )
    return float(timeout_seconds)


def _prepare_execution(
    *,
    worker_path: Path,
    worker_sha256: str,
    target_path: Path,
    runtime_artifact_sources: dict[str, tuple[Path, str]],
) -> PreparedExecution:
    worker_raw = require_regular_file_bytes(
        worker_path,
        "core worker",
        maximum_bytes=256 * 1024 * 1024,
    )
    worker_identity = assert_expected_identity(
        worker_raw,
        worker_sha256,
        None,
        "core worker",
    )
    target_raw = require_regular_file_bytes(
        target_path,
        "fixed target DLL",
        maximum_bytes=16 * 1024 * 1024,
    )
    target_identity = assert_expected_identity(
        target_raw,
        EXPECTED_TARGET_SHA256,
        EXPECTED_TARGET_BYTE_LENGTH,
        "fixed target DLL",
    )
    if set(runtime_artifact_sources) != set(RUNTIME_ARTIFACT_NAMES):
        raise CoreHarnessError(
            "runtime artifact allowlist is incomplete or has extras"
        )
    runtime_artifacts: list[RuntimeArtifact] = []
    resolved_paths: set[Path] = set()
    for name in RUNTIME_ARTIFACT_NAMES:
        source_path, expected_sha256 = runtime_artifact_sources[name]
        if source_path.name.casefold() != name:
            raise CoreHarnessError(
                "runtime artifact basename differs from allowlist"
            )
        try:
            resolved = source_path.resolve(strict=True)
        except OSError as error:
            raise CoreHarnessError("runtime artifact is absent") from error
        if resolved in resolved_paths:
            raise CoreHarnessError(
                "runtime artifacts must have unique source files"
            )
        resolved_paths.add(resolved)
        artifact_raw = require_regular_file_bytes(
            source_path,
            f"runtime artifact {name}",
            maximum_bytes=64 * 1024 * 1024,
        )
        artifact_identity = assert_expected_identity(
            artifact_raw,
            expected_sha256,
            None,
            f"runtime artifact {name}",
        )
        runtime_artifacts.append(
            RuntimeArtifact(name, source_path, artifact_identity)
        )
    return PreparedExecution(
        worker_raw=worker_raw,
        worker_identity=worker_identity,
        target_identity=target_identity,
        runtime_artifacts=tuple(runtime_artifacts),
    )


def run_core_worker(
    prepared: PreparedPcm,
    *,
    worker_path: Path,
    worker_sha256: str,
    target_path: Path,
    runtime_artifact_sources: dict[str, tuple[Path, str]],
    runtime_profile: str = "fixed_foobar_2_25_10",
    timeout_seconds: float = 30.0,
    block_frames: int = DEFAULT_BLOCK_FRAMES,
    multichannel_loudness_weighting: bool = False,
) -> dict[str, Any]:
    timeout_seconds = _validate_timeout_seconds(timeout_seconds)
    multichannel_loudness_weighting = require_bool(
        multichannel_loudness_weighting,
        "multichannel loudness weighting",
    )
    execution = _prepare_execution(
        worker_path=worker_path,
        worker_sha256=worker_sha256,
        target_path=target_path,
        runtime_artifact_sources=runtime_artifact_sources,
    )
    worker_raw = execution.worker_raw
    worker_identity = execution.worker_identity
    target_identity = execution.target_identity
    runtime_artifacts = execution.runtime_artifacts
    _validate_f64le(
        prepared.pcm,
        channels=prepared.channels,
        frames=prepared.frames,
        context="prepared PCM",
    )

    with tempfile.TemporaryDirectory(prefix="foo-dr-meter-108-core-") as directory:
        stage = Path(directory)
        staged_worker = _stage_worker(worker_raw, worker_path, stage)
        staged_pcm = stage / "input.f64le"
        staged_pcm.write_bytes(prepared.pcm)
        request_path = stage / "request.json"
        request = build_worker_request(
            prepared,
            worker_identity=worker_identity,
            target_identity=target_identity,
            runtime_artifacts=runtime_artifacts,
            runtime_profile=runtime_profile,
            target_path=target_path,
            pcm_path=staged_pcm,
            block_frames=block_frames,
            multichannel_loudness_weighting=(
                multichannel_loudness_weighting
            ),
        )
        request_path.write_bytes(canonical_json_bytes(request))
        with _hold_staged_worker_launch_guards(
            stage,
            staged_worker,
            staged_pcm,
            request_path,
        ):
            staged_worker_raw = require_regular_file_bytes(
                staged_worker,
                "staged core worker",
                maximum_bytes=256 * 1024 * 1024,
            )
            assert_expected_identity(
                staged_worker_raw,
                worker_identity.sha256,
                worker_identity.byte_length,
                "staged core worker",
            )
            staged_pcm_raw = require_regular_file_bytes(
                staged_pcm,
                "staged PCM",
                maximum_bytes=1024 * 1024 * 1024,
            )
            assert_expected_identity(
                staged_pcm_raw,
                prepared.pcm_identity.sha256,
                prepared.pcm_identity.byte_length,
                "staged PCM",
            )
            staged_request_raw = require_regular_file_bytes(
                request_path,
                "staged worker request",
                maximum_bytes=16 * 1024 * 1024,
            )
            if staged_request_raw != canonical_json_bytes(request):
                raise CoreHarnessError(
                    "staged worker request differs from canonical request"
                )
            completed = _run_worker_bounded(
                _worker_command(staged_worker, request_path),
                cwd=stage,
                timeout_seconds=timeout_seconds,
            )
        response = validate_worker_response(
            completed.stdout,
            exit_code=completed.returncode,
            request=request,
        )

    record: dict[str, Any] = {
        "schemaVersion": SCHEMA_VERSION,
        "kind": RECORD_KIND,
        "requestId": request["requestId"],
        "input": {
            "inputId": prepared.input_id,
            "sourceKind": prepared.source_kind,
            "sourceEncoding": prepared.source_encoding,
            "conversion": prepared.conversion,
            "sourceSha256": prepared.source_identity.sha256,
            "sourceByteLength": prepared.source_identity.byte_length,
            "manifestSha256": prepared.manifest_sha256,
            "pcmSha256": prepared.pcm_identity.sha256,
            "pcmByteLength": prepared.pcm_identity.byte_length,
            "sampleRateHz": prepared.sample_rate,
            "channels": prepared.channels,
            "frames": prepared.frames,
        },
        "execution": {
            "workerSha256": worker_identity.sha256,
            "workerByteLength": worker_identity.byte_length,
            "blockFrames": block_frames,
            "multichannelLoudnessWeighting": (
                multichannel_loudness_weighting
            ),
            "processModel": "one_worker_process_per_input",
        },
        "target": {
            "id": "TARGET-foo-dr-meter-1.0.8-x64-static-ff3556ad",
            "sha256": target_identity.sha256,
            "byteLength": target_identity.byte_length,
            "runtimeArtifacts": [
                {
                    "name": artifact.name,
                    "sha256": artifact.identity.sha256,
                    "byteLength": artifact.identity.byte_length,
                }
                for artifact in runtime_artifacts
            ],
            "runtimeProfile": runtime_profile,
        },
        "result": response["data"],
        "claims": {
            "scope": "isolated foo_dr_meter 1.0.8 x64 analyzer core",
            "foobarParity": "not_assessed",
        },
        "limitations": [
            "No foobar decoder, component lifecycle, metadata, album grouping, or renderer was exercised.",
            "Manifest WAV conversion is a deterministic harness conversion, not an observation of foobar decoding.",
        ],
    }
    assert_path_free(record, "harness record")
    canonical_json_bytes(record)
    return record


def run_duration_worker(
    *,
    decoded_frames: int,
    sample_rate_hz: int,
    worker_path: Path,
    worker_sha256: str,
    target_path: Path,
    runtime_artifact_sources: dict[str, tuple[Path, str]],
    fractional_digits: int = 0,
    runtime_profile: str = "fixed_foobar_2_25_10",
    timeout_seconds: float = 30.0,
) -> dict[str, Any]:
    timeout_seconds = _validate_timeout_seconds(timeout_seconds)
    decoded_frames = require_int(
        decoded_frames,
        "duration decoded frames",
        minimum=0,
        maximum=0xFFFF_FFFF_FFFF_FFFF,
    )
    sample_rate_hz = require_int(
        sample_rate_hz,
        "duration sample rate",
        minimum=1,
        maximum=0xFFFF_FFFF,
    )
    fractional_digits = require_int(
        fractional_digits,
        "duration fractional digits",
        minimum=0,
        maximum=0,
    )
    if runtime_profile not in RUNTIME_PROFILES:
        raise CoreHarnessError("runtime profile is unsupported")
    execution = _prepare_execution(
        worker_path=worker_path,
        worker_sha256=worker_sha256,
        target_path=target_path,
        runtime_artifact_sources=runtime_artifact_sources,
    )
    worker_raw = execution.worker_raw
    worker_identity = execution.worker_identity
    target_identity = execution.target_identity
    runtime_artifacts = execution.runtime_artifacts

    with tempfile.TemporaryDirectory(
        prefix="foo-dr-meter-108-duration-"
    ) as directory:
        stage = Path(directory)
        staged_worker = _stage_worker(worker_raw, worker_path, stage)
        request_path = stage / "request.json"
        request = build_duration_worker_request(
            worker_identity=worker_identity,
            target_identity=target_identity,
            runtime_artifacts=runtime_artifacts,
            runtime_profile=runtime_profile,
            target_path=target_path,
            decoded_frames=decoded_frames,
            sample_rate_hz=sample_rate_hz,
            fractional_digits=fractional_digits,
        )
        request_path.write_bytes(canonical_json_bytes(request))
        with _hold_staged_worker_launch_guards(
            stage,
            staged_worker,
            None,
            request_path,
        ):
            staged_worker_raw = require_regular_file_bytes(
                staged_worker,
                "staged core worker",
                maximum_bytes=256 * 1024 * 1024,
            )
            assert_expected_identity(
                staged_worker_raw,
                worker_identity.sha256,
                worker_identity.byte_length,
                "staged core worker",
            )
            staged_request_raw = require_regular_file_bytes(
                request_path,
                "staged worker request",
                maximum_bytes=16 * 1024 * 1024,
            )
            if staged_request_raw != canonical_json_bytes(request):
                raise CoreHarnessError(
                    "staged worker request differs from canonical request"
                )
            completed = _run_worker_bounded(
                _worker_command(staged_worker, request_path),
                cwd=stage,
                timeout_seconds=timeout_seconds,
            )
        response = validate_duration_worker_response(
            completed.stdout,
            exit_code=completed.returncode,
            request=request,
        )

    record: dict[str, Any] = {
        "schemaVersion": SCHEMA_VERSION,
        "kind": DURATION_RECORD_KIND,
        "requestId": request["requestId"],
        "duration": {
            "decodedFrames": decoded_frames,
            "sampleRateHz": sample_rate_hz,
            "fractionalDigits": fractional_digits,
        },
        "execution": {
            "workerSha256": worker_identity.sha256,
            "workerByteLength": worker_identity.byte_length,
            "processModel": "one_worker_process_per_request",
        },
        "target": {
            "id": "TARGET-foo-dr-meter-1.0.8-x64-static-ff3556ad",
            "sha256": target_identity.sha256,
            "byteLength": target_identity.byte_length,
            "durationFormatRva": DURATION_FORMAT_RVA,
            "runtimeArtifacts": [
                {
                    "name": artifact.name,
                    "sha256": artifact.identity.sha256,
                    "byteLength": artifact.identity.byte_length,
                }
                for artifact in runtime_artifacts
            ],
            "runtimeProfile": runtime_profile,
        },
        "result": response["data"],
        "claims": {
            "scope": (
                "isolated foo_dr_meter 1.0.8 x64 duration numeric leaf"
            ),
            "foobarParity": "not_assessed",
        },
        "limitations": [
            "No foobar decoder, component lifecycle, report assembly, or renderer lifecycle was exercised.",
            "The input frame count and sample rate are explicit harness values, not observations of host decoding.",
        ],
    }
    assert_path_free(record, "duration harness record")
    canonical_json_bytes(record)
    return record


def _write_record(record: dict[str, Any], output: Path | None) -> None:
    raw = canonical_json_bytes(record)
    if output is None:
        sys.stdout.buffer.write(raw)
        return
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        dir=output.parent, prefix=f".{output.name}.", delete=False
    ) as temporary:
        temporary.write(raw)
        temporary.flush()
        os.fsync(temporary.fileno())
        temporary_path = Path(temporary.name)
    try:
        os.replace(temporary_path, output)
    finally:
        if temporary_path.exists():
            temporary_path.unlink()


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", required=True, type=Path)
    parser.add_argument("--worker-sha256", required=True)
    parser.add_argument("--target-dll", required=True, type=Path)
    parser.add_argument("--shared-dll", required=True, type=Path)
    parser.add_argument("--shared-sha256", required=True)
    parser.add_argument("--msvcp140-dll", required=True, type=Path)
    parser.add_argument("--msvcp140-sha256", required=True)
    parser.add_argument("--vcruntime140-dll", required=True, type=Path)
    parser.add_argument("--vcruntime140-sha256", required=True)
    parser.add_argument("--vcruntime140-1-dll", required=True, type=Path)
    parser.add_argument("--vcruntime140-1-sha256", required=True)
    parser.add_argument(
        "--runtime-profile",
        choices=RUNTIME_PROFILES,
        default="fixed_foobar_2_25_10",
    )
    parser.add_argument("--timeout-seconds", type=float, default=30.0)
    parser.add_argument("--block-frames", type=int, default=DEFAULT_BLOCK_FRAMES)
    parser.add_argument(
        "--multichannel-loudness-weighting",
        action="store_true",
        help="pass the optional multichannel weighting flag to finish()",
    )
    parser.add_argument("--output", type=Path)
    source = parser.add_subparsers(dest="source", required=True)

    fixture = source.add_parser("fixture", help="convert one manifest-bound WAV")
    fixture.add_argument("--manifest", required=True, type=Path)
    fixture.add_argument("--corpus-root", required=True, type=Path)
    fixture.add_argument("--fixture-id", required=True)

    pcm = source.add_parser("pcm", help="use explicit finite interleaved f64le PCM")
    pcm.add_argument("--pcm", required=True, type=Path)
    pcm.add_argument("--pcm-sha256", required=True)
    pcm.add_argument("--input-id", required=True)
    pcm.add_argument("--sample-rate", required=True, type=int)
    pcm.add_argument("--channels", required=True, type=int)
    pcm.add_argument("--frames", required=True, type=int)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    try:
        args = parse_args(argv)
        if args.source == "fixture":
            prepared = prepare_manifest_fixture(
                args.manifest, args.corpus_root, args.fixture_id
            )
        else:
            prepared = prepare_explicit_pcm(
                args.pcm,
                input_id=args.input_id,
                expected_sha256=args.pcm_sha256,
                sample_rate=args.sample_rate,
                channels=args.channels,
                frames=args.frames,
            )
        record = run_core_worker(
            prepared,
            worker_path=args.worker,
            worker_sha256=args.worker_sha256,
            target_path=args.target_dll,
            runtime_artifact_sources={
                "shared.dll": (args.shared_dll, args.shared_sha256),
                "msvcp140.dll": (args.msvcp140_dll, args.msvcp140_sha256),
                "vcruntime140.dll": (
                    args.vcruntime140_dll,
                    args.vcruntime140_sha256,
                ),
                "vcruntime140_1.dll": (
                    args.vcruntime140_1_dll,
                    args.vcruntime140_1_sha256,
                ),
            },
            runtime_profile=args.runtime_profile,
            timeout_seconds=args.timeout_seconds,
            block_frames=args.block_frames,
            multichannel_loudness_weighting=(
                args.multichannel_loudness_weighting
            ),
        )
        _write_record(record, args.output)
        return 0
    except WorkerReportedError as error:
        print(f"core harness error: worker_{error.code}", file=sys.stderr)
        return 1
    except CoreHarnessError:
        print("core harness error: contract_violation", file=sys.stderr)
        return 1
    except OSError:
        print("core harness error: io_failure", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
