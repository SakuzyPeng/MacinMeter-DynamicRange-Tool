#!/usr/bin/env python3

from __future__ import annotations

import contextlib
import hashlib
import importlib.util
import json
import math
import struct
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


TOOL_PATH = Path(__file__).resolve().parents[1] / "run_foo_dr_meter_108_core.py"
SPEC = importlib.util.spec_from_file_location("foo_dr_meter_core_parent", TOOL_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot import {TOOL_PATH}")
HARNESS = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = HARNESS
SPEC.loader.exec_module(HARNESS)


def sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def json_line(value: object) -> bytes:
    return (json.dumps(value, separators=(",", ":")) + "\n").encode()


def chunk(name: bytes, payload: bytes) -> bytes:
    return name + struct.pack("<I", len(payload)) + payload + (b"\0" if len(payload) & 1 else b"")


def wave_file(
    data: bytes,
    *,
    tag: int,
    bits: int,
    channels: int = 1,
    sample_rate: int = 8000,
    extensible: bool = False,
) -> bytes:
    align = channels * (bits // 8)
    if extensible:
        subformat = bytes.fromhex(
            "0300000000001000800000aa00389b71"
            if tag == 3
            else "0100000000001000800000aa00389b71"
        )
        fmt = struct.pack(
            "<HHIIHHHHI16s",
            0xFFFE,
            channels,
            sample_rate,
            sample_rate * align,
            align,
            bits,
            22,
            bits,
            (1 << channels) - 1,
            subformat,
        )
    else:
        fmt = struct.pack(
            "<HHIIHH",
            tag,
            channels,
            sample_rate,
            sample_rate * align,
            align,
            bits,
        )
    body = b"WAVE" + chunk(b"fmt ", fmt) + chunk(b"data", data)
    return b"RIFF" + struct.pack("<I", len(body)) + body


def make_manifest_fixture(
    root: Path,
    data: bytes,
    *,
    encoding: str,
    tag: int,
    bits: int,
    channels: int = 1,
    extensible: bool = False,
) -> tuple[Path, Path]:
    corpus = root / "corpus"
    fixture = corpus / "core" / "input.wav"
    fixture.parent.mkdir(parents=True)
    raw = wave_file(
        data,
        tag=tag,
        bits=bits,
        channels=channels,
        extensible=extensible,
    )
    fixture.write_bytes(raw)
    frames = len(data) // (channels * (bits // 8))
    manifest = {
        "schemaVersion": 2,
        "corpusId": "synthetic",
        "cases": [
            {
                "id": "case-1",
                "path": "core/input.wav",
                "fileSha256": sha256(raw),
                "byteLength": len(raw),
                "dataSha256": sha256(data),
                "encoding": encoding,
                "sampleRateHz": 8000,
                "channels": channels,
                "frames": frames,
            }
        ],
    }
    manifest_path = root / "manifest.json"
    manifest_path.write_text(
        json.dumps(manifest, sort_keys=True, indent=2) + "\n",
        encoding="utf-8",
    )
    return manifest_path, corpus


def histogram_sha256(total_count: int) -> str:
    bins = [0] * HARNESS.HISTOGRAM_BINS_PER_CHANNEL
    if total_count:
        bins[1] = total_count
    return sha256(struct.pack(f"<{len(bins)}I", *bins))


def protocol_request(
    channels: int = 1,
    *,
    weighting: bool = False,
    block_frames: int = 512,
) -> dict[str, object]:
    return {
        "schemaVersion": HARNESS.SCHEMA_VERSION,
        "kind": HARNESS.PROTOCOL_KIND_REQUEST,
        "requestId": "1" * 64,
        "target": {
            "sha256": "2" * 64,
            "runtimeProfile": "fixed_foobar_2_25_10",
            "runtimeArtifacts": [
                {"name": name, "sha256": str(index) * 64, "byteLength": index}
                for index, name in enumerate(HARNESS.RUNTIME_ARTIFACT_NAMES, 3)
            ],
        },
        "stream": {
            "sampleRate": 8000,
            "channels": channels,
            "frames": 2,
        },
        "options": {
            "multichannelLoudnessWeighting": weighting,
            "blockFrames": block_frames,
        },
    }


def protocol_result(
    request: dict[str, object],
    *,
    channels: int = 1,
) -> dict[str, object]:
    target = request["target"]
    assert isinstance(target, dict)
    runtime = target["runtimeArtifacts"]
    assert isinstance(runtime, list)
    return {
        "schemaVersion": HARNESS.SCHEMA_VERSION,
        "kind": HARNESS.PROTOCOL_KIND_RESULT,
        "requestId": request["requestId"],
        "targetSha256": target["sha256"],
        "data": {
            "sampleRateHz": 8000,
            "channels": channels,
            "frames": 2,
            "trackDrBits": "41200000",
            "channelResults": [
                {
                    "index": index,
                    "drBits": "41200000",
                    "peakBits": "3f800000",
                    "rmsBits": "3dcccccd",
                }
                for index in range(channels)
            ],
            "runtimeArtifacts": [
                {
                    "name": item["name"],
                    "sha256": item["sha256"],
                    "byteLength": item["byteLength"],
                }
                for item in runtime
                if isinstance(item, dict)
            ],
            "loaderMode": "private_staging_dll_load_dir_system32",
            "sharedServiceBoundary": {
                "loadLifecycle": "real_shared",
                "coreExecution": "fail_fast_iat_tripwire",
                "armedImportCount": 13,
            },
            "sessionBeforeFinish": {
                "currentWindowFrames": 2,
                "windowCount": 0,
                "submittedFrames": 0,
            },
            "sessionAfterFinish": {
                "currentWindowFrames": 0,
                "windowCount": 1,
                "submittedFrames": 2,
            },
            "channelStateAfterFinish": [
                {
                    "index": index,
                    "rmsSquareSumBits": "0000000000000000",
                    "primaryPeakBits": "3ff0000000000000",
                    "secondaryPeakBits": "0000000000000000",
                    "primaryPeakKeyBits": "0000000000000000",
                    "secondaryPeakKeyBits": "0000000000000000",
                }
                for index in range(channels)
            ],
            "options": dict(request["options"]),
            "histogramAfterFinish": {
                "layout": HARNESS.HISTOGRAM_LAYOUT,
                "elementEncoding": HARNESS.HISTOGRAM_ELEMENT_ENCODING,
                "binsPerChannel": HARNESS.HISTOGRAM_BINS_PER_CHANNEL,
                "channels": [
                    {
                        "index": index,
                        "totalCount": 1,
                        "nonzeroBinCount": 1,
                        "minus100DbCount": 0,
                        "zeroDbCount": 0,
                        "sha256": histogram_sha256(1),
                    }
                    for index in range(channels)
                ],
            },
            "fpEnvironment": {
                "before": {
                    "x87ControlWordBits": "037f",
                    "mxcsrBits": "00001f80",
                },
                "applied": {
                    "x87ControlWordBits": "037f",
                    "mxcsrBits": "00001f80",
                    "rounding": "nearest",
                    "ftz": False,
                    "daz": False,
                    "exceptionsMasked": True,
                },
                "after": {
                    "x87ControlWordBits": "037f",
                    "mxcsrBits": "00001f80",
                },
                "restored": {
                    "x87ControlWordBits": "037f",
                    "mxcsrBits": "00001f80",
                },
            },
        },
    }


def duration_protocol_request(
    *,
    decoded_frames: int = 1,
    sample_rate_hz: int = 2,
    fractional_digits: int = 0,
) -> dict[str, object]:
    return {
        "schemaVersion": HARNESS.SCHEMA_VERSION,
        "kind": HARNESS.DURATION_PROTOCOL_KIND_REQUEST,
        "requestId": "a" * 64,
        "target": {
            "dllPath": "C:\\staged\\foo_dr_meter.dll",
            "sha256": "2" * 64,
            "byteLength": 20,
            "durationFormatRva": HARNESS.DURATION_FORMAT_RVA,
            "runtimeProfile": "fixed_foobar_2_25_10",
            "runtimeArtifacts": [
                {
                    "name": name,
                    "sourcePath": f"C:\\runtime\\{name}",
                    "sha256": str(index) * 64,
                    "byteLength": index,
                }
                for index, name in enumerate(HARNESS.RUNTIME_ARTIFACT_NAMES, 3)
            ],
        },
        "duration": {
            "decodedFrames": decoded_frames,
            "sampleRateHz": sample_rate_hz,
            "fractionalDigits": fractional_digits,
        },
    }


def duration_protocol_result(
    request: dict[str, object],
    *,
    text: str = "0:01",
) -> dict[str, object]:
    target = request["target"]
    duration = request["duration"]
    assert isinstance(target, dict)
    assert isinstance(duration, dict)
    runtime = target["runtimeArtifacts"]
    assert isinstance(runtime, list)
    decoded_frames = duration["decodedFrames"]
    sample_rate_hz = duration["sampleRateHz"]
    assert isinstance(decoded_frames, int)
    assert isinstance(sample_rate_hz, int)
    return {
        "schemaVersion": HARNESS.SCHEMA_VERSION,
        "kind": HARNESS.DURATION_PROTOCOL_KIND_RESULT,
        "requestId": request["requestId"],
        "targetSha256": target["sha256"],
        "data": {
            "geometry": dict(duration),
            "secondsBits": HARNESS._duration_seconds_bits(
                decoded_frames,
                sample_rate_hz,
            ),
            "text": text,
            "runtimeArtifacts": [
                {
                    "name": item["name"],
                    "sha256": item["sha256"],
                    "byteLength": item["byteLength"],
                }
                for item in runtime
                if isinstance(item, dict)
            ],
            "loaderMode": "private_staging_dll_load_dir_system32",
            "sharedServiceBoundary": {
                "loadLifecycle": "real_shared",
                "numericLeafExecution": "fail_fast_iat_tripwire",
                "armedImportCount": 13,
            },
            "fpEnvironment": {
                "before": {
                    "x87ControlWordBits": "037f",
                    "mxcsrBits": "00001f80",
                },
                "applied": {
                    "x87ControlWordBits": "037f",
                    "mxcsrBits": "00001f80",
                    "rounding": "nearest",
                    "ftz": False,
                    "daz": False,
                    "exceptionsMasked": True,
                },
                "after": {
                    "x87ControlWordBits": "037f",
                    "mxcsrBits": "00001f80",
                },
                "restored": {
                    "x87ControlWordBits": "037f",
                    "mxcsrBits": "00001f80",
                },
            },
        },
    }


class PcmPreparationTests(unittest.TestCase):
    def test_manifest_wave_encodings_convert_deterministically_to_f64le(self) -> None:
        cases = [
            ("wav-pcm-u8", 1, 8, bytes([0, 128, 255]), [-1.0, 0.0, 127 / 128]),
            (
                "wav-pcm-s16le",
                1,
                16,
                struct.pack("<hhh", -32768, 0, 32767),
                [-1.0, 0.0, 32767 / 32768],
            ),
            (
                "wav-pcm-s24le",
                1,
                24,
                (-8388608).to_bytes(3, "little", signed=True)
                + (0).to_bytes(3, "little", signed=True)
                + (8388607).to_bytes(3, "little", signed=True),
                [-1.0, 0.0, 8388607 / 8388608],
            ),
            (
                "wav-pcm-s32le",
                1,
                32,
                struct.pack("<iii", -2147483648, 0, 2147483647),
                [-1.0, 0.0, 2147483647 / 2147483648],
            ),
            (
                "wav-ieee-float32le",
                3,
                32,
                struct.pack("<fff", -0.25, -0.0, 1.25),
                [-0.25, -0.0, 1.25],
            ),
            (
                "wav-ieee-float64le",
                3,
                64,
                struct.pack("<ddd", -0.25, -0.0, 1.25),
                [-0.25, -0.0, 1.25],
            ),
        ]
        for encoding, tag, bits, data, expected in cases:
            with self.subTest(encoding=encoding), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                manifest, corpus = make_manifest_fixture(
                    root, data, encoding=encoding, tag=tag, bits=bits
                )
                prepared = HARNESS.prepare_manifest_fixture(
                    manifest, corpus, "case-1"
                )
                actual = [
                    value[0] for value in struct.iter_unpack("<d", prepared.pcm)
                ]
                self.assertEqual(actual, expected)
                self.assertEqual(prepared.conversion, "strict_wav_sample_to_binary64")

    def test_extensible_multichannel_float_is_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            data = struct.pack("<ffffff", 0.1, 0.2, 0.3, -0.1, -0.2, -0.3)
            manifest, corpus = make_manifest_fixture(
                root,
                data,
                encoding="wav-ieee-float32le",
                tag=3,
                bits=32,
                channels=3,
                extensible=True,
            )
            prepared = HARNESS.prepare_manifest_fixture(manifest, corpus, "case-1")
            self.assertEqual((prepared.channels, prepared.frames), (3, 2))
            self.assertEqual(len(prepared.pcm), 3 * 2 * 8)

    def test_manifest_hash_or_geometry_mismatch_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            data = struct.pack("<f", 0.5)
            manifest, corpus = make_manifest_fixture(
                root,
                data,
                encoding="wav-ieee-float32le",
                tag=3,
                bits=32,
            )
            value = json.loads(manifest.read_text())
            value["cases"][0]["frames"] = 2
            manifest.write_text(json.dumps(value))
            with self.assertRaisesRegex(HARNESS.CoreHarnessError, "frames differs"):
                HARNESS.prepare_manifest_fixture(manifest, corpus, "case-1")

    def test_manifest_recursively_rejects_duplicate_object_keys(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            data = struct.pack("<f", 0.5)
            manifest, corpus = make_manifest_fixture(
                root,
                data,
                encoding="wav-ieee-float32le",
                tag=3,
                bits=32,
            )
            raw = manifest.read_text(encoding="utf-8")
            raw = raw.replace(
                '"frames": 1',
                '"frames": 1,\n      "frames": 1',
                1,
            )
            manifest.write_text(raw, encoding="utf-8")
            with self.assertRaisesRegex(
                HARNESS.CoreHarnessError, "duplicate object key 'frames'"
            ):
                HARNESS.prepare_manifest_fixture(manifest, corpus, "case-1")

    def test_explicit_pcm_requires_exact_hash_geometry_and_finite_values(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pcm = root / "input.f64le"
            raw = struct.pack("<dd", 0.25, -0.5)
            pcm.write_bytes(raw)
            prepared = HARNESS.prepare_explicit_pcm(
                pcm,
                input_id="explicit-1",
                expected_sha256=sha256(raw),
                sample_rate=44100,
                channels=1,
                frames=2,
            )
            self.assertEqual(prepared.pcm, raw)
            with self.assertRaises(HARNESS.CoreHarnessError):
                HARNESS.prepare_explicit_pcm(
                    pcm,
                    input_id="explicit-1",
                    expected_sha256="0" * 64,
                    sample_rate=44100,
                    channels=1,
                    frames=2,
                )
            pcm.write_bytes(struct.pack("<d", math.nan))
            with self.assertRaisesRegex(HARNESS.CoreHarnessError, "non-finite"):
                HARNESS.prepare_explicit_pcm(
                    pcm,
                    input_id="explicit-1",
                    expected_sha256=sha256(pcm.read_bytes()),
                    sample_rate=44100,
                    channels=1,
                    frames=1,
                )


class WorkerProtocolTests(unittest.TestCase):
    def test_valid_result_is_strictly_accepted(self) -> None:
        request = protocol_request(2, weighting=True)
        response = protocol_result(request, channels=2)
        parsed = HARNESS.validate_worker_response(
            json_line(response), exit_code=0, request=request
        )
        self.assertEqual(parsed, response)

    def test_extra_stdout_unknown_fields_and_path_leaks_are_rejected(self) -> None:
        request = protocol_request()
        response = protocol_result(request)
        with self.assertRaisesRegex(HARNESS.CoreHarnessError, "exactly one"):
            HARNESS.validate_worker_response(
                json_line(response) + b"diagnostic\n",
                exit_code=0,
                request=request,
            )
        response["extra"] = True
        with self.assertRaisesRegex(HARNESS.CoreHarnessError, "keys differ"):
            HARNESS.validate_worker_response(
                json_line(response), exit_code=0, request=request
            )
        del response["extra"]
        data = response["data"]
        assert isinstance(data, dict)
        data["loaderMode"] = "C:\\private\\loader"
        with self.assertRaisesRegex(HARNESS.CoreHarnessError, "path"):
            HARNESS.validate_worker_response(
                json_line(response), exit_code=0, request=request
            )

    def test_worker_response_recursively_rejects_duplicate_object_keys(self) -> None:
        request = protocol_request()
        response = json_line(protocol_result(request))
        response = response.replace(
            b'"armedImportCount":13',
            b'"armedImportCount":13,"armedImportCount":13',
            1,
        )
        with self.assertRaisesRegex(
            HARNESS.CoreHarnessError,
            "duplicate object key 'armedImportCount'",
        ):
            HARNESS.validate_worker_response(
                response,
                exit_code=0,
                request=request,
            )

    def test_response_must_echo_identity_geometry_runtime_and_finite_bits(self) -> None:
        request = protocol_request()
        for mutation in (
            "schema",
            "request",
            "geometry",
            "runtime",
            "bits",
            "session",
            "options",
            "histogram",
        ):
            with self.subTest(mutation=mutation):
                response = protocol_result(request)
                data = response["data"]
                assert isinstance(data, dict)
                if mutation == "schema":
                    response["schemaVersion"] = 1
                elif mutation == "request":
                    response["requestId"] = "9" * 64
                elif mutation == "geometry":
                    data["frames"] = 3
                elif mutation == "runtime":
                    artifacts = data["runtimeArtifacts"]
                    assert isinstance(artifacts, list)
                    artifacts.reverse()
                elif mutation == "bits":
                    data["trackDrBits"] = "7fc00000"
                elif mutation == "session":
                    before = data["sessionBeforeFinish"]
                    assert isinstance(before, dict)
                    before["submittedFrames"] = 2
                elif mutation == "options":
                    options = data["options"]
                    assert isinstance(options, dict)
                    options["multichannelLoudnessWeighting"] = True
                else:
                    histogram = data["histogramAfterFinish"]
                    assert isinstance(histogram, dict)
                    histogram["binsPerChannel"] = 10000
                with self.assertRaises(HARNESS.CoreHarnessError):
                    HARNESS.validate_worker_response(
                        json_line(response), exit_code=0, request=request
                    )

    def test_error_response_requires_failure_exit_and_surfaces_only_code(self) -> None:
        request = protocol_request()
        target = request["target"]
        assert isinstance(target, dict)
        response = {
            "schemaVersion": HARNESS.SCHEMA_VERSION,
            "kind": HARNESS.PROTOCOL_KIND_ERROR,
            "requestId": request["requestId"],
            "targetSha256": target["sha256"],
            "error": {"code": "load_failed", "message": "target load failed"},
        }
        with self.assertRaisesRegex(HARNESS.WorkerReportedError, "load_failed"):
            HARNESS.validate_worker_response(
                json_line(response), exit_code=10, request=request
            )
        with self.assertRaisesRegex(HARNESS.CoreHarnessError, "success exit"):
            HARNESS.validate_worker_response(
                json_line(response), exit_code=0, request=request
            )

    def test_histogram_geometry_counts_and_hash_are_strict(self) -> None:
        request = protocol_request()
        mutations = {
            "index": 1,
            "totalCount": 2,
            "nonzeroBinCount": 2,
            "minus100DbCount": 2,
            "zeroDbCount": 2,
            "sha256": "A" * 64,
        }
        for key, invalid in mutations.items():
            with self.subTest(key=key):
                response = protocol_result(request)
                data = response["data"]
                assert isinstance(data, dict)
                histogram = data["histogramAfterFinish"]
                assert isinstance(histogram, dict)
                channels = histogram["channels"]
                assert isinstance(channels, list)
                item = channels[0]
                assert isinstance(item, dict)
                item[key] = invalid
                with self.assertRaises(HARNESS.CoreHarnessError):
                    HARNESS.validate_worker_response(
                        json_line(response),
                        exit_code=0,
                        request=request,
                    )

    def test_core_request_rejects_non_boolean_weighting(self) -> None:
        prepared = HARNESS.PreparedPcm(
            input_id="empty",
            source_kind="explicit_f64le_pcm",
            source_encoding="f64le-interleaved",
            conversion="identity",
            source_identity=HARNESS.FileIdentity(sha256(b""), 0),
            pcm=b"",
            sample_rate=8000,
            channels=1,
            frames=0,
        )
        for value in (0, 1, None, "false"):
            with self.subTest(value=value):
                with self.assertRaises(HARNESS.CoreHarnessError):
                    HARNESS.build_worker_request(
                        prepared,
                        worker_identity=HARNESS.FileIdentity("1" * 64, 10),
                        target_identity=HARNESS.FileIdentity("2" * 64, 20),
                        runtime_artifacts=(),
                        runtime_profile="fixed_foobar_2_25_10",
                        target_path=Path("missing-target.dll"),
                        pcm_path=Path("missing-input.f64le"),
                        multichannel_loudness_weighting=value,
                    )


class DurationProtocolTests(unittest.TestCase):
    def test_valid_duration_result_is_strictly_accepted(self) -> None:
        request = duration_protocol_request()
        response = duration_protocol_result(request)
        parsed = HARNESS.validate_duration_worker_response(
            json_line(response),
            exit_code=0,
            request=request,
        )
        self.assertEqual(parsed, response)
        data = parsed["data"]
        assert isinstance(data, dict)
        self.assertEqual(data["secondsBits"], "3fe0000000000000")

    def test_duration_text_accepts_only_safe_canonical_shapes(self) -> None:
        valid = (
            "0:01",
            "1:02:03",
            "2d 3:04:05",
            "1wk 0d 0:00:00",
        )
        invalid = (
            "",
            "1:2",
            "1:60",
            "1:60:00",
            "duration",
            "C:\\private\\0:01",
            "一分钟",
            "1" * 129,
        )
        request = duration_protocol_request()
        for text in valid:
            with self.subTest(valid=text):
                response = duration_protocol_result(request, text=text)
                HARNESS.validate_duration_worker_response(
                    json_line(response),
                    exit_code=0,
                    request=request,
                )
        for text in invalid:
            with self.subTest(invalid=text):
                response = duration_protocol_result(request, text=text)
                with self.assertRaises(HARNESS.CoreHarnessError):
                    HARNESS.validate_duration_worker_response(
                        json_line(response),
                        exit_code=0,
                        request=request,
                    )

    def test_duration_response_requires_exact_echo_and_runtime_boundary(self) -> None:
        request = duration_protocol_request()
        for mutation in (
            "schema",
            "request",
            "target",
            "geometry",
            "seconds_nonfinite",
            "seconds_mismatch",
            "runtime",
            "boundary",
            "fp",
            "extra",
        ):
            with self.subTest(mutation=mutation):
                response = duration_protocol_result(request)
                data = response["data"]
                assert isinstance(data, dict)
                if mutation == "schema":
                    response["schemaVersion"] = 1
                elif mutation == "request":
                    response["requestId"] = "9" * 64
                elif mutation == "target":
                    response["targetSha256"] = "9" * 64
                elif mutation == "geometry":
                    geometry = data["geometry"]
                    assert isinstance(geometry, dict)
                    geometry["decodedFrames"] = 2
                elif mutation == "seconds_nonfinite":
                    data["secondsBits"] = "7ff8000000000000"
                elif mutation == "seconds_mismatch":
                    data["secondsBits"] = "3ff0000000000000"
                elif mutation == "runtime":
                    runtime = data["runtimeArtifacts"]
                    assert isinstance(runtime, list)
                    runtime.reverse()
                elif mutation == "boundary":
                    boundary = data["sharedServiceBoundary"]
                    assert isinstance(boundary, dict)
                    boundary["numericLeafExecution"] = "uncontrolled"
                elif mutation == "fp":
                    fp = data["fpEnvironment"]
                    assert isinstance(fp, dict)
                    applied = fp["applied"]
                    assert isinstance(applied, dict)
                    applied["rounding"] = "down"
                else:
                    data["unexpected"] = True
                with self.assertRaises(HARNESS.CoreHarnessError):
                    HARNESS.validate_duration_worker_response(
                        json_line(response),
                        exit_code=0,
                        request=request,
                    )

    def test_duration_request_rejects_invalid_geometry(self) -> None:
        base = {
            "worker_identity": HARNESS.FileIdentity("1" * 64, 10),
            "target_identity": HARNESS.FileIdentity("2" * 64, 20),
            "runtime_artifacts": (),
            "runtime_profile": "fixed_foobar_2_25_10",
            "target_path": Path("missing-target.dll"),
            "decoded_frames": 1,
            "sample_rate_hz": 2,
            "fractional_digits": 0,
        }
        cases = (
            ("decoded_frames", True),
            ("decoded_frames", -1),
            ("decoded_frames", 1 << 64),
            ("sample_rate_hz", True),
            ("sample_rate_hz", 0),
            ("sample_rate_hz", 1 << 32),
            ("fractional_digits", True),
            ("fractional_digits", 1),
        )
        for key, value in cases:
            with self.subTest(key=key, value=value):
                arguments = dict(base)
                arguments[key] = value
                with self.assertRaises(HARNESS.CoreHarnessError):
                    HARNESS.build_duration_worker_request(**arguments)

    def test_duration_uses_the_shared_protocol_error_kind(self) -> None:
        request = duration_protocol_request()
        target = request["target"]
        assert isinstance(target, dict)
        response = {
            "schemaVersion": HARNESS.SCHEMA_VERSION,
            "kind": HARNESS.PROTOCOL_KIND_ERROR,
            "requestId": request["requestId"],
            "targetSha256": target["sha256"],
            "error": {
                "code": "duration_contract_mismatch",
                "message": "duration contract mismatch",
            },
        }
        with self.assertRaisesRegex(
            HARNESS.WorkerReportedError,
            "duration_contract_mismatch",
        ):
            HARNESS.validate_duration_worker_response(
                json_line(response),
                exit_code=2,
                request=request,
            )


class EndToEndParentTests(unittest.TestCase):
    def _runtime_sources(
        self, root: Path
    ) -> dict[str, tuple[Path, str]]:
        sources: dict[str, tuple[Path, str]] = {}
        runtime = root / "runtime"
        runtime.mkdir()
        for index, name in enumerate(HARNESS.RUNTIME_ARTIFACT_NAMES):
            path = runtime / name
            raw = f"runtime-{index}".encode()
            path.write_bytes(raw)
            sources[name] = (path, sha256(raw))
        return sources

    def _fake_worker(self, root: Path, mode: str = "success") -> tuple[Path, str]:
        path = root / "fake_worker.py"
        if mode == "timeout":
            source = "import time\ntime.sleep(5)\n"
        elif mode == "stdout_overflow":
            source = "import sys\nsys.stdout.buffer.write(b'x' * 1048576)\n"
        elif mode == "stderr_overflow":
            source = "import sys\nsys.stderr.buffer.write(b'x' * 1048576)\n"
        else:
            source = r'''
import argparse, hashlib, json, math, struct
p = argparse.ArgumentParser()
p.add_argument("--request", required=True)
a = p.parse_args()
r = json.load(open(a.request, encoding="utf-8"))
artifacts = [
    {"name": x["name"], "sha256": x["sha256"], "byteLength": x["byteLength"]}
    for x in r["target"]["runtimeArtifacts"]
]
fp = {
    "before": {"x87ControlWordBits": "037f", "mxcsrBits": "00001f80"},
    "applied": {
        "x87ControlWordBits": "037f",
        "mxcsrBits": "00001f80",
        "rounding": "nearest",
        "ftz": False,
        "daz": False,
        "exceptionsMasked": True,
    },
    "after": {"x87ControlWordBits": "037f", "mxcsrBits": "00001f80"},
    "restored": {"x87ControlWordBits": "037f", "mxcsrBits": "00001f80"},
}
if r["kind"] == "foo_dr_meter_108_duration_request":
    geometry = r["duration"]
    seconds = float(geometry["decodedFrames"]) / float(geometry["sampleRateHz"])
    seconds_bits = f"{struct.unpack('<Q', struct.pack('<d', seconds))[0]:016x}"
    rounded = math.floor(seconds + 0.5)
    weeks, remainder = divmod(rounded, 7 * 24 * 60 * 60)
    days, remainder = divmod(remainder, 24 * 60 * 60)
    hours, remainder = divmod(remainder, 60 * 60)
    minutes, whole_seconds = divmod(remainder, 60)
    prefix = f"{weeks}wk " if weeks else ""
    if days or weeks:
        prefix += f"{days}d "
    if hours or days or weeks:
        text = f"{prefix}{hours}:{minutes:02d}:{whole_seconds:02d}"
    else:
        text = f"{minutes}:{whole_seconds:02d}"
    data = {
        "geometry": geometry,
        "secondsBits": seconds_bits,
        "text": text,
        "runtimeArtifacts": artifacts,
        "loaderMode": "private_staging_dll_load_dir_system32",
        "sharedServiceBoundary": {
            "loadLifecycle": "real_shared",
            "numericLeafExecution": "fail_fast_iat_tripwire",
            "armedImportCount": 13,
        },
        "fpEnvironment": fp,
    }
    kind = "foo_dr_meter_108_duration_result"
else:
    histogram = bytearray(10001 * 4)
    struct.pack_into("<I", histogram, 4, 1)
    histogram_sha = hashlib.sha256(histogram).hexdigest()
    data = {
        "sampleRateHz": r["stream"]["sampleRate"],
        "channels": r["stream"]["channels"],
        "frames": r["stream"]["frames"],
        "trackDrBits": "41200000",
        "channelResults": [
            {"index": i, "drBits": "41200000", "peakBits": "3f800000", "rmsBits": "3dcccccd"}
            for i in range(r["stream"]["channels"])
        ],
        "runtimeArtifacts": artifacts,
        "loaderMode": "private_staging_dll_load_dir_system32",
        "sharedServiceBoundary": {
            "loadLifecycle": "real_shared",
            "coreExecution": "fail_fast_iat_tripwire",
            "armedImportCount": 13,
        },
        "sessionBeforeFinish": {
            "currentWindowFrames": r["stream"]["frames"],
            "windowCount": 0,
            "submittedFrames": 0,
        },
        "sessionAfterFinish": {
            "currentWindowFrames": 0,
            "windowCount": 1,
            "submittedFrames": r["stream"]["frames"],
        },
        "channelStateAfterFinish": [
            {
                "index": i,
                "rmsSquareSumBits": "0000000000000000",
                "primaryPeakBits": "3ff0000000000000",
                "secondaryPeakBits": "0000000000000000",
                "primaryPeakKeyBits": "0000000000000000",
                "secondaryPeakKeyBits": "0000000000000000",
            }
            for i in range(r["stream"]["channels"])
        ],
        "options": r["options"],
        "histogramAfterFinish": {
            "layout": "channel_major",
            "elementEncoding": "u32le",
            "binsPerChannel": 10001,
            "channels": [
                {
                    "index": i,
                    "totalCount": 1,
                    "nonzeroBinCount": 1,
                    "minus100DbCount": 0,
                    "zeroDbCount": 0,
                    "sha256": histogram_sha,
                }
                for i in range(r["stream"]["channels"])
            ],
        },
        "fpEnvironment": fp,
    }
    kind = "foo_dr_meter_108_core_result"
print(json.dumps({
    "schemaVersion": 2,
    "kind": kind,
    "requestId": r["requestId"],
    "targetSha256": r["target"]["sha256"],
    "data": data,
}, separators=(",", ":")))
'''
        path.write_text(source, encoding="utf-8")
        return path, sha256(path.read_bytes())

    def _prepared(self, root: Path) -> HARNESS.PreparedPcm:
        pcm = root / "source.f64le"
        raw = struct.pack("<dddd", 0.25, -0.25, 0.5, -0.5)
        pcm.write_bytes(raw)
        return HARNESS.prepare_explicit_pcm(
            pcm,
            input_id="pure-core-1",
            expected_sha256=sha256(raw),
            sample_rate=8000,
            channels=2,
            frames=2,
        )

    def test_one_fake_worker_process_produces_path_free_stable_record(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            prepared = self._prepared(root)
            worker, worker_hash = self._fake_worker(root)
            target = root / "foo_dr_meter.dll"
            target_raw = b"synthetic-target"
            target.write_bytes(target_raw)
            runtimes = self._runtime_sources(root)
            with (
                mock.patch.object(
                    HARNESS, "EXPECTED_TARGET_SHA256", sha256(target_raw)
                ),
                mock.patch.object(
                    HARNESS, "EXPECTED_TARGET_BYTE_LENGTH", len(target_raw)
                ),
                mock.patch.object(
                    HARNESS,
                    "_run_worker_bounded",
                    wraps=HARNESS._run_worker_bounded,
                ) as run_worker,
            ):
                record = HARNESS.run_core_worker(
                    prepared,
                    worker_path=worker,
                    worker_sha256=worker_hash,
                    target_path=target,
                    runtime_artifact_sources=runtimes,
                    timeout_seconds=5,
                    multichannel_loudness_weighting=True,
                )
            self.assertEqual(run_worker.call_count, 1)
            self.assertEqual(record["claims"]["foobarParity"], "not_assessed")
            self.assertEqual(record["execution"]["blockFrames"], 512)
            self.assertIs(
                record["execution"]["multichannelLoudnessWeighting"],
                True,
            )
            self.assertEqual(
                record["result"]["options"],
                {
                    "multichannelLoudnessWeighting": True,
                    "blockFrames": 512,
                },
            )
            self.assertIn("histogramAfterFinish", record["result"])
            rendered = HARNESS.canonical_json_bytes(record).decode()
            self.assertNotIn(str(root), rendered)
            self.assertNotIn("dllPath", rendered)
            self.assertEqual(
                [item["name"] for item in record["target"]["runtimeArtifacts"]],
                list(HARNESS.RUNTIME_ARTIFACT_NAMES),
            )

    def test_duration_worker_is_isolated_path_free_and_stages_no_pcm(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            worker, worker_hash = self._fake_worker(root)
            target = root / "foo_dr_meter.dll"
            target_raw = b"synthetic-target"
            target.write_bytes(target_raw)
            runtimes = self._runtime_sources(root)
            seen_requests: list[dict[str, object]] = []
            original_guard = HARNESS._hold_staged_worker_launch_guards

            @contextlib.contextmanager
            def inspecting_guard(
                stage: Path,
                staged_worker: Path,
                staged_pcm: Path | None,
                request_path: Path,
            ):
                self.assertIsNone(staged_pcm)
                seen_requests.append(
                    json.loads(request_path.read_text(encoding="utf-8"))
                )
                with original_guard(
                    stage,
                    staged_worker,
                    staged_pcm,
                    request_path,
                ):
                    yield

            with (
                mock.patch.object(
                    HARNESS,
                    "EXPECTED_TARGET_SHA256",
                    sha256(target_raw),
                ),
                mock.patch.object(
                    HARNESS,
                    "EXPECTED_TARGET_BYTE_LENGTH",
                    len(target_raw),
                ),
                mock.patch.object(
                    HARNESS,
                    "_hold_staged_worker_launch_guards",
                    inspecting_guard,
                ),
                mock.patch.object(
                    HARNESS,
                    "_run_worker_bounded",
                    wraps=HARNESS._run_worker_bounded,
                ) as run_worker,
            ):
                record = HARNESS.run_duration_worker(
                    decoded_frames=1,
                    sample_rate_hz=2,
                    worker_path=worker,
                    worker_sha256=worker_hash,
                    target_path=target,
                    runtime_artifact_sources=runtimes,
                    timeout_seconds=5,
                )
            self.assertEqual(run_worker.call_count, 1)
            self.assertEqual(len(seen_requests), 1)
            self.assertEqual(
                set(seen_requests[0]),
                {"schemaVersion", "kind", "requestId", "target", "duration"},
            )
            self.assertEqual(record["result"]["secondsBits"], "3fe0000000000000")
            self.assertEqual(record["result"]["text"], "0:01")
            rendered = HARNESS.canonical_json_bytes(record).decode()
            self.assertNotIn(str(root), rendered)
            self.assertNotIn("dllPath", rendered)

    def test_duration_staged_request_is_revalidated_before_launch(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            worker, worker_hash = self._fake_worker(root)
            target = root / "foo_dr_meter.dll"
            target_raw = b"synthetic-target"
            target.write_bytes(target_raw)
            runtimes = self._runtime_sources(root)

            @contextlib.contextmanager
            def tampering_guard(
                stage: Path,
                staged_worker: Path,
                staged_pcm: Path | None,
                request_path: Path,
            ):
                del stage, staged_worker
                self.assertIsNone(staged_pcm)
                request = json.loads(
                    request_path.read_text(encoding="utf-8")
                )
                request["duration"]["decodedFrames"] = 2
                request_path.write_bytes(
                    HARNESS.canonical_json_bytes(request)
                )
                yield

            with (
                mock.patch.object(
                    HARNESS,
                    "EXPECTED_TARGET_SHA256",
                    sha256(target_raw),
                ),
                mock.patch.object(
                    HARNESS,
                    "EXPECTED_TARGET_BYTE_LENGTH",
                    len(target_raw),
                ),
                mock.patch.object(
                    HARNESS,
                    "_hold_staged_worker_launch_guards",
                    tampering_guard,
                ),
                mock.patch.object(
                    HARNESS,
                    "_run_worker_bounded",
                ) as run_worker,
                self.assertRaisesRegex(
                    HARNESS.CoreHarnessError,
                    "differs from canonical request",
                ),
            ):
                HARNESS.run_duration_worker(
                    decoded_frames=1,
                    sample_rate_hz=2,
                    worker_path=worker,
                    worker_sha256=worker_hash,
                    target_path=target,
                    runtime_artifact_sources=runtimes,
                    timeout_seconds=5,
                )
            run_worker.assert_not_called()

    def test_worker_timeout_and_prelaunch_hash_gate_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            prepared = self._prepared(root)
            worker, worker_hash = self._fake_worker(root, "timeout")
            target = root / "foo_dr_meter.dll"
            target_raw = b"synthetic-target"
            target.write_bytes(target_raw)
            runtimes = self._runtime_sources(root)
            patches = (
                mock.patch.object(
                    HARNESS, "EXPECTED_TARGET_SHA256", sha256(target_raw)
                ),
                mock.patch.object(
                    HARNESS, "EXPECTED_TARGET_BYTE_LENGTH", len(target_raw)
                ),
            )
            with patches[0], patches[1]:
                with self.assertRaisesRegex(HARNESS.CoreHarnessError, "timed out"):
                    HARNESS.run_core_worker(
                        prepared,
                        worker_path=worker,
                        worker_sha256=worker_hash,
                        target_path=target,
                        runtime_artifact_sources=runtimes,
                        timeout_seconds=0.05,
                    )
                with (
                    mock.patch.object(HARNESS, "_run_worker_bounded") as run_worker,
                    self.assertRaisesRegex(HARNESS.CoreHarnessError, "SHA-256"),
                ):
                    HARNESS.run_core_worker(
                        prepared,
                        worker_path=worker,
                        worker_sha256="0" * 64,
                        target_path=target,
                        runtime_artifact_sources=runtimes,
                    )
                run_worker.assert_not_called()

    def test_worker_output_is_bounded_while_the_process_is_running(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            prepared = self._prepared(root)
            target = root / "foo_dr_meter.dll"
            target_raw = b"synthetic-target"
            target.write_bytes(target_raw)
            runtimes = self._runtime_sources(root)
            patches = (
                mock.patch.object(
                    HARNESS, "EXPECTED_TARGET_SHA256", sha256(target_raw)
                ),
                mock.patch.object(
                    HARNESS, "EXPECTED_TARGET_BYTE_LENGTH", len(target_raw)
                ),
                mock.patch.object(HARNESS, "MAX_WORKER_STDOUT_BYTES", 1024),
                mock.patch.object(HARNESS, "MAX_WORKER_STDERR_BYTES", 1024),
            )
            with patches[0], patches[1], patches[2], patches[3]:
                for mode, stream in (
                    ("stdout_overflow", "stdout"),
                    ("stderr_overflow", "stderr"),
                ):
                    with self.subTest(stream=stream):
                        worker, worker_hash = self._fake_worker(root, mode)
                        with self.assertRaisesRegex(
                            HARNESS.CoreHarnessError,
                            f"worker {stream} exceeds its byte limit",
                        ):
                            HARNESS.run_core_worker(
                                prepared,
                                worker_path=worker,
                                worker_sha256=worker_hash,
                                target_path=target,
                                runtime_artifact_sources=runtimes,
                                timeout_seconds=5,
                            )

    def test_staged_request_and_pcm_are_revalidated_before_launch(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            prepared = self._prepared(root)
            worker, worker_hash = self._fake_worker(root)
            target = root / "foo_dr_meter.dll"
            target_raw = b"synthetic-target"
            target.write_bytes(target_raw)
            runtimes = self._runtime_sources(root)

            for mutation, expected_error in (
                ("request", "differs from canonical request"),
                ("pcm", "staged PCM SHA-256 mismatch"),
            ):
                with self.subTest(mutation=mutation):
                    @contextlib.contextmanager
                    def tampering_guard(
                        stage: Path,
                        staged_worker: Path,
                        staged_pcm: Path,
                        request_path: Path,
                    ):
                        del stage, staged_worker
                        if mutation == "request":
                            request = json.loads(
                                request_path.read_text(encoding="utf-8")
                            )
                            request["options"]["blockFrames"] = 1
                            request_path.write_bytes(
                                HARNESS.canonical_json_bytes(request)
                            )
                        else:
                            raw = bytearray(staged_pcm.read_bytes())
                            raw[0] ^= 1
                            staged_pcm.write_bytes(raw)
                        yield

                    with (
                        mock.patch.object(
                            HARNESS,
                            "EXPECTED_TARGET_SHA256",
                            sha256(target_raw),
                        ),
                        mock.patch.object(
                            HARNESS,
                            "EXPECTED_TARGET_BYTE_LENGTH",
                            len(target_raw),
                        ),
                        mock.patch.object(
                            HARNESS,
                            "_hold_staged_worker_launch_guards",
                            tampering_guard,
                        ),
                        mock.patch.object(
                            HARNESS, "_run_worker_bounded"
                        ) as run_worker,
                        self.assertRaisesRegex(
                            HARNESS.CoreHarnessError,
                            expected_error,
                        ),
                    ):
                        HARNESS.run_core_worker(
                            prepared,
                            worker_path=worker,
                            worker_sha256=worker_hash,
                            target_path=target,
                            runtime_artifact_sources=runtimes,
                            timeout_seconds=5,
                        )
                    run_worker.assert_not_called()

    def test_request_id_ignores_paths_but_binds_options_and_duration(self) -> None:
        with tempfile.TemporaryDirectory() as first, tempfile.TemporaryDirectory() as second:
            core_ids = []
            duration_ids = []
            for directory in (first, second):
                root = Path(directory)
                prepared = self._prepared(root)
                target = root / "target.dll"
                target.write_bytes(b"target")
                pcm = root / "staged.f64le"
                pcm.write_bytes(prepared.pcm)
                artifacts = []
                for index, name in enumerate(HARNESS.RUNTIME_ARTIFACT_NAMES):
                    path = root / name
                    path.write_bytes(bytes([index]))
                    artifacts.append(
                        HARNESS.RuntimeArtifact(
                            name,
                            path,
                            HARNESS.FileIdentity(str(index + 3) * 64, 1),
                        )
                    )
                common = {
                    "worker_identity": HARNESS.FileIdentity("1" * 64, 10),
                    "target_identity": HARNESS.FileIdentity("2" * 64, 20),
                    "runtime_artifacts": tuple(artifacts),
                    "runtime_profile": "fixed_foobar_2_25_10",
                    "target_path": target,
                }
                core_requests = [
                    HARNESS.build_worker_request(
                        prepared,
                        **common,
                        pcm_path=pcm,
                        block_frames=block_frames,
                        multichannel_loudness_weighting=weighting,
                    )
                    for block_frames, weighting in (
                        (512, False),
                        (512, True),
                        (511, False),
                    )
                ]
                core_ids.append(
                    tuple(request["requestId"] for request in core_requests)
                )
                self.assertEqual(
                    core_requests[1]["options"],
                    {
                        "multichannelLoudnessWeighting": True,
                        "blockFrames": 512,
                    },
                )
                duration_requests = [
                    HARNESS.build_duration_worker_request(
                        **common,
                        decoded_frames=frames,
                        sample_rate_hz=rate,
                    )
                    for frames, rate in ((1, 2), (2, 2), (1, 3))
                ]
                duration_ids.append(
                    tuple(
                        request["requestId"]
                        for request in duration_requests
                    )
                )
            self.assertEqual(core_ids[0], core_ids[1])
            self.assertEqual(duration_ids[0], duration_ids[1])
            self.assertEqual(len(set(core_ids[0])), 3)
            self.assertEqual(len(set(duration_ids[0])), 3)


class CliTests(unittest.TestCase):
    def _arguments(self) -> list[str]:
        return [
            "--worker",
            "worker.exe",
            "--worker-sha256",
            "1" * 64,
            "--target-dll",
            "foo_dr_meter.dll",
            "--shared-dll",
            "shared.dll",
            "--shared-sha256",
            "2" * 64,
            "--msvcp140-dll",
            "msvcp140.dll",
            "--msvcp140-sha256",
            "3" * 64,
            "--vcruntime140-dll",
            "vcruntime140.dll",
            "--vcruntime140-sha256",
            "4" * 64,
            "--vcruntime140-1-dll",
            "vcruntime140_1.dll",
            "--vcruntime140-1-sha256",
            "5" * 64,
            "pcm",
            "--pcm",
            "input.f64le",
            "--pcm-sha256",
            "6" * 64,
            "--input-id",
            "input",
            "--sample-rate",
            "8000",
            "--channels",
            "3",
            "--frames",
            "2",
        ]

    def test_weighting_cli_flag_defaults_false_and_can_be_enabled(self) -> None:
        arguments = self._arguments()
        source_index = arguments.index("pcm")
        default = HARNESS.parse_args(arguments)
        enabled = HARNESS.parse_args(
            [
                *arguments[:source_index],
                "--multichannel-loudness-weighting",
                *arguments[source_index:],
            ]
        )
        self.assertIs(default.multichannel_loudness_weighting, False)
        self.assertIs(enabled.multichannel_loudness_weighting, True)


class StagedWorkerGuardTests(unittest.TestCase):
    def test_non_windows_guard_is_a_noop(self) -> None:
        with (
            mock.patch.object(HARNESS, "_is_windows", return_value=False),
            mock.patch.object(
                HARNESS, "_open_windows_no_write_delete_handle"
            ) as open_handle,
            HARNESS._hold_staged_worker_launch_guards(
                Path("stage"),
                Path("stage/worker"),
                Path("stage/input.f64le"),
                Path("stage/request.json"),
            ),
        ):
            pass
        open_handle.assert_not_called()

    def test_windows_guard_holds_all_staged_inputs_until_launch_finishes(
        self,
    ) -> None:
        with (
            tempfile.TemporaryDirectory() as directory,
            mock.patch.object(HARNESS, "_is_windows", return_value=True),
            mock.patch.object(
                HARNESS,
                "_open_windows_no_write_delete_handle",
                side_effect=(101, 102, 103, 104),
            ) as open_handle,
            mock.patch.object(HARNESS, "_close_windows_handle") as close_handle,
        ):
            stage = Path(directory)
            worker = stage / "worker.exe"
            pcm = stage / "input.f64le"
            request = stage / "request.json"
            worker.write_bytes(b"worker")
            pcm.write_bytes(b"pcm")
            request.write_bytes(b"request")
            with HARNESS._hold_staged_worker_launch_guards(
                stage,
                worker,
                pcm,
                request,
            ):
                close_handle.assert_not_called()
            self.assertEqual(
                open_handle.call_args_list,
                [
                    mock.call(stage, directory=True),
                    mock.call(worker, directory=False),
                    mock.call(pcm, directory=False),
                    mock.call(request, directory=False),
                ],
            )
            self.assertEqual(
                close_handle.call_args_list,
                [
                    mock.call(104),
                    mock.call(103),
                    mock.call(102),
                    mock.call(101),
                ],
            )


if __name__ == "__main__":
    unittest.main()
