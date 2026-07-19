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


def protocol_request(channels: int = 1) -> dict[str, object]:
    return {
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
        "schemaVersion": 1,
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
        request = protocol_request(2)
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
        for mutation in ("request", "geometry", "runtime", "bits", "session"):
            with self.subTest(mutation=mutation):
                response = protocol_result(request)
                data = response["data"]
                assert isinstance(data, dict)
                if mutation == "request":
                    response["requestId"] = "9" * 64
                elif mutation == "geometry":
                    data["frames"] = 3
                elif mutation == "runtime":
                    artifacts = data["runtimeArtifacts"]
                    assert isinstance(artifacts, list)
                    artifacts.reverse()
                elif mutation == "bits":
                    data["trackDrBits"] = "7fc00000"
                else:
                    before = data["sessionBeforeFinish"]
                    assert isinstance(before, dict)
                    before["submittedFrames"] = 2
                with self.assertRaises(HARNESS.CoreHarnessError):
                    HARNESS.validate_worker_response(
                        json_line(response), exit_code=0, request=request
                    )

    def test_error_response_requires_failure_exit_and_surfaces_only_code(self) -> None:
        request = protocol_request()
        target = request["target"]
        assert isinstance(target, dict)
        response = {
            "schemaVersion": 1,
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
import argparse, json
p = argparse.ArgumentParser()
p.add_argument("--request", required=True)
a = p.parse_args()
r = json.load(open(a.request, encoding="utf-8"))
artifacts = [
    {"name": x["name"], "sha256": x["sha256"], "byteLength": x["byteLength"]}
    for x in r["target"]["runtimeArtifacts"]
]
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
    "fpEnvironment": {
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
    },
}
print(json.dumps({
    "schemaVersion": 1,
    "kind": "foo_dr_meter_108_core_result",
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
                )
            self.assertEqual(run_worker.call_count, 1)
            self.assertEqual(record["claims"]["foobarParity"], "not_assessed")
            self.assertEqual(record["execution"]["blockFrames"], 512)
            rendered = HARNESS.canonical_json_bytes(record).decode()
            self.assertNotIn(str(root), rendered)
            self.assertNotIn("dllPath", rendered)
            self.assertEqual(
                [item["name"] for item in record["target"]["runtimeArtifacts"]],
                list(HARNESS.RUNTIME_ARTIFACT_NAMES),
            )

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

    def test_request_id_ignores_transport_paths_but_binds_block_size(self) -> None:
        with tempfile.TemporaryDirectory() as first, tempfile.TemporaryDirectory() as second:
            ids = []
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
                request = HARNESS.build_worker_request(
                    prepared,
                    worker_identity=HARNESS.FileIdentity("1" * 64, 10),
                    target_identity=HARNESS.FileIdentity("2" * 64, 20),
                    runtime_artifacts=tuple(artifacts),
                    runtime_profile="fixed_foobar_2_25_10",
                    target_path=target,
                    pcm_path=pcm,
                    block_frames=512,
                )
                ids.append(request["requestId"])
            self.assertEqual(ids[0], ids[1])


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
