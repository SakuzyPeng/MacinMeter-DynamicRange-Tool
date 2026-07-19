#!/usr/bin/env python3

from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import struct
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


TOOL_PATH = (
    Path(__file__).resolve().parents[1] / "run_foo_dr_meter_108_core_suite.py"
)
SPEC = importlib.util.spec_from_file_location("foo_dr_meter_core_suite", TOOL_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot import {TOOL_PATH}")
SUITE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = SUITE
SPEC.loader.exec_module(SUITE)


def sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def riff_f64(samples: list[float], sample_rate: int = 8000) -> tuple[bytes, bytes]:
    data = b"".join(struct.pack("<d", sample) for sample in samples)
    fmt = struct.pack(
        "<HHIIHH",
        3,
        1,
        sample_rate,
        sample_rate * 8,
        8,
        64,
    )

    def chunk(name: bytes, payload: bytes) -> bytes:
        return name + struct.pack("<I", len(payload)) + payload

    body = b"WAVE" + chunk(b"fmt ", fmt) + chunk(b"data", data)
    return b"RIFF" + struct.pack("<I", len(body)) + body, data


class SyntheticSuite:
    def __init__(
        self,
        root: Path,
        samples: list[tuple[str, float, str]] | None = None,
    ) -> None:
        self.root = root
        self.corpus = root / "corpus"
        self.corpus.mkdir()
        definitions = samples or [
            ("safe-a", 0.25, "safe"),
            ("safe-b", 0.5, "safe"),
            ("isolated-c", 2.0, "isolated"),
            ("safe-d", 0.75, "safe"),
        ]
        cases = []
        for order, (case_id, sample, execution_class) in enumerate(definitions, 1):
            raw, data = riff_f64([sample, sample / 2])
            relative = f"core/{order:03d}_{case_id}.wav"
            path = self.corpus / relative
            path.parent.mkdir(exist_ok=True)
            path.write_bytes(raw)
            cases.append(
                {
                    "id": case_id,
                    "order": order,
                    "path": relative,
                    "executionClass": execution_class,
                    "fileSha256": sha256(raw),
                    "byteLength": len(raw),
                    "dataSha256": sha256(data),
                    "encoding": "wav-ieee-float64le",
                    "sampleRateHz": 8000,
                    "channels": 1,
                    "frames": 2,
                }
            )
        self.manifest_value = {
            "schemaVersion": 2,
            "corpusId": "synthetic-safe-suite",
            "budgets": {
                "expectedSafeMasterEntries": sum(
                    case["executionClass"] == "safe" for case in cases
                )
            },
            "cases": cases,
        }
        self.manifest = root / "manifest.json"
        self.manifest.write_text(
            json.dumps(self.manifest_value, sort_keys=True, indent=2) + "\n",
            encoding="utf-8",
        )

        self.worker = root / "fake_worker.py"
        self.worker.write_text(
            r'''
import argparse, json, os, struct, sys
p = argparse.ArgumentParser()
p.add_argument("--request", required=True)
a = p.parse_args()
with open(a.request, encoding="utf-8") as source:
    r = json.load(source)
with open(r["pcm"]["path"], "rb") as source:
    first = struct.unpack("<d", source.read(8))[0]
log = os.environ.get("CORE_SUITE_FAKE_LOG")
if log:
    with open(log, "a", encoding="ascii") as output:
        output.write(f"{os.getpid()} {first.hex()}\n")
if first < 0:
    print(json.dumps({
        "schemaVersion": 1,
        "kind": "foo_dr_meter_108_core_error",
        "requestId": r["requestId"],
        "targetSha256": r["target"]["sha256"],
        "error": {"code": "synthetic_failure", "message": "synthetic failure"},
    }, separators=(",", ":")))
    sys.exit(9)
artifacts = [
    {"name": x["name"], "sha256": x["sha256"], "byteLength": x["byteLength"]}
    for x in r["target"]["runtimeArtifacts"]
]
state = [
    {
        "index": i,
        "rmsSquareSumBits": "0000000000000000",
        "primaryPeakBits": "3ff0000000000000",
        "secondaryPeakBits": "0000000000000000",
        "primaryPeakKeyBits": "0000000000000000",
        "secondaryPeakKeyBits": "0000000000000000",
    }
    for i in range(r["stream"]["channels"])
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
    "channelStateAfterFinish": state,
    "fpEnvironment": fp,
}
print(json.dumps({
    "schemaVersion": 1,
    "kind": "foo_dr_meter_108_core_result",
    "requestId": r["requestId"],
    "targetSha256": r["target"]["sha256"],
    "data": data,
}, separators=(",", ":")))
''',
            encoding="utf-8",
        )
        self.worker_sha256 = sha256(self.worker.read_bytes())
        self.target = root / "foo_dr_meter.dll"
        self.target.write_bytes(b"synthetic-fixed-target")
        self.runtime_sources: dict[str, tuple[Path, str]] = {}
        runtime = root / "runtime"
        runtime.mkdir()
        for index, name in enumerate(SUITE.PARENT.RUNTIME_ARTIFACT_NAMES):
            path = runtime / name
            raw = f"runtime-{index}".encode()
            path.write_bytes(raw)
            self.runtime_sources[name] = (path, sha256(raw))

    def run(self) -> dict[str, object]:
        with (
            mock.patch.object(
                SUITE.PARENT,
                "EXPECTED_TARGET_SHA256",
                sha256(self.target.read_bytes()),
            ),
            mock.patch.object(
                SUITE.PARENT,
                "EXPECTED_TARGET_BYTE_LENGTH",
                len(self.target.read_bytes()),
            ),
        ):
            return SUITE.run_suite(
                manifest_path=self.manifest,
                corpus_root=self.corpus,
                worker_path=self.worker,
                worker_sha256=self.worker_sha256,
                target_path=self.target,
                runtime_artifact_sources=self.runtime_sources,
                timeout_seconds=5,
            )


class CoreSuiteTests(unittest.TestCase):
    def test_safe_cases_run_once_each_in_canonical_order(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            synthetic = SyntheticSuite(Path(directory))
            log = synthetic.root / "worker-processes.log"
            with mock.patch.dict(os.environ, {"CORE_SUITE_FAKE_LOG": str(log)}):
                record = synthetic.run()
            self.assertEqual(record["summary"], {
                "status": "success",
                "total": 3,
                "succeeded": 3,
                "failed": 0,
            })
            self.assertEqual(
                [item["inputId"] for item in record["items"]],
                ["safe-a", "safe-b", "safe-d"],
            )
            self.assertEqual(
                [item["manifestOrder"] for item in record["items"]],
                [1, 2, 4],
            )
            processes = [line.split()[0] for line in log.read_text().splitlines()]
            self.assertEqual(len(processes), 3)
            self.assertEqual(len(set(processes)), 3)
            for item in record["items"]:
                self.assertEqual(item["result"]["kind"], "success")
                self.assertRegex(item["requestId"], r"^[0-9a-f]{64}$")
                self.assertEqual(item["claims"]["foobarParity"], "not_assessed")
            rendered = SUITE.PARENT.canonical_json_bytes(record).decode()
            self.assertNotIn(str(synthetic.root), rendered)
            self.assertNotIn("sourcePath", rendered)
            self.assertEqual(record["target"]["runtimeProfile"], SUITE.RUNTIME_PROFILE)

    def test_worker_error_is_tagged_and_later_case_still_runs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            synthetic = SyntheticSuite(
                Path(directory),
                [
                    ("first", 0.25, "safe"),
                    ("fails", -0.5, "safe"),
                    ("last", 0.75, "safe"),
                ],
            )
            log = synthetic.root / "worker-processes.log"
            with mock.patch.dict(os.environ, {"CORE_SUITE_FAKE_LOG": str(log)}):
                record = synthetic.run()
            self.assertEqual(
                [item["result"]["kind"] for item in record["items"]],
                ["success", "error", "success"],
            )
            failed = record["items"][1]
            self.assertEqual(failed["result"]["stage"], "worker")
            self.assertEqual(
                failed["result"]["workerCode"], "synthetic_failure"
            )
            self.assertRegex(failed["requestId"], r"^[0-9a-f]{64}$")
            self.assertEqual(record["summary"]["status"], "partial")
            self.assertEqual(record["summary"]["failed"], 1)
            self.assertEqual(len(log.read_text().splitlines()), 3)

    def test_input_error_is_tagged_and_does_not_stop_remaining_cases(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            synthetic = SyntheticSuite(Path(directory))
            missing = synthetic.corpus / synthetic.manifest_value["cases"][1]["path"]
            missing.unlink()
            record = synthetic.run()
            self.assertEqual(
                [item["result"]["kind"] for item in record["items"]],
                ["success", "error", "success"],
            )
            failed = record["items"][1]
            self.assertEqual(failed["result"]["stage"], "input")
            self.assertIsNone(failed["requestId"])
            self.assertEqual(failed["input"]["inputId"], "safe-b")

    def test_manifest_replacement_cannot_mix_case_snapshots(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            synthetic = SyntheticSuite(Path(directory))
            original_manifest_raw = synthetic.manifest.read_bytes()
            original_manifest_sha256 = sha256(original_manifest_raw)
            replacement = json.loads(original_manifest_raw)
            replacement_samples = {
                "safe-b": 0.125,
                "safe-d": 0.625,
            }
            for case in replacement["cases"]:
                sample = replacement_samples.get(case["id"])
                if sample is None:
                    continue
                raw, data = riff_f64([sample, sample / 2])
                relative = f"replacement/{case['id']}.wav"
                path = synthetic.corpus / relative
                path.parent.mkdir(exist_ok=True)
                path.write_bytes(raw)
                case.update(
                    {
                        "path": relative,
                        "fileSha256": sha256(raw),
                        "byteLength": len(raw),
                        "dataSha256": sha256(data),
                    }
                )
            replacement_raw = (
                json.dumps(replacement, sort_keys=True, indent=2) + "\n"
            ).encode()
            log = synthetic.root / "worker-processes.log"
            original_run_core_worker = SUITE.PARENT.run_core_worker
            worker_calls = 0

            def run_and_replace_manifest(*args: object, **kwargs: object) -> object:
                nonlocal worker_calls
                result = original_run_core_worker(*args, **kwargs)
                worker_calls += 1
                if worker_calls == 1:
                    synthetic.manifest.write_bytes(replacement_raw)
                return result

            with (
                mock.patch.dict(
                    os.environ, {"CORE_SUITE_FAKE_LOG": str(log)}
                ),
                mock.patch.object(
                    SUITE.PARENT,
                    "run_core_worker",
                    side_effect=run_and_replace_manifest,
                ),
            ):
                record = synthetic.run()

            self.assertEqual(record["summary"]["status"], "success")
            self.assertEqual(
                record["corpus"]["manifestSha256"], original_manifest_sha256
            )
            self.assertEqual(
                {
                    item["input"]["manifestSha256"]
                    for item in record["items"]
                },
                {original_manifest_sha256},
            )
            first_samples = [
                float.fromhex(line.split()[1])
                for line in log.read_text().splitlines()
            ]
            self.assertEqual(first_samples, [0.25, 0.5, 0.75])
            self.assertEqual(synthetic.manifest.read_bytes(), replacement_raw)

    def test_noncanonical_manifest_order_fails_before_worker_launch(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            synthetic = SyntheticSuite(Path(directory))
            value = json.loads(synthetic.manifest.read_text())
            value["cases"][0]["order"] = 2
            value["cases"][1]["order"] = 1
            synthetic.manifest.write_text(json.dumps(value))
            with (
                mock.patch.object(SUITE.PARENT, "run_core_worker") as run,
                self.assertRaisesRegex(
                    SUITE.PARENT.CoreHarnessError, "canonical contiguous"
                ),
            ):
                SUITE.run_suite(
                    manifest_path=synthetic.manifest,
                    corpus_root=synthetic.corpus,
                    worker_path=synthetic.worker,
                    worker_sha256=synthetic.worker_sha256,
                    target_path=synthetic.target,
                    runtime_artifact_sources=synthetic.runtime_sources,
                )
            run.assert_not_called()

    def test_main_returns_nonzero_for_partial_suite(self) -> None:
        record = {"summary": {"failed": 1}}
        with (
            mock.patch.object(SUITE, "parse_args", return_value=mock.Mock()),
            mock.patch.object(SUITE, "run_suite", return_value=record),
            mock.patch.object(SUITE.PARENT, "_write_record") as write,
        ):
            self.assertEqual(SUITE.main([]), 1)
        write.assert_called_once()


if __name__ == "__main__":
    unittest.main()
