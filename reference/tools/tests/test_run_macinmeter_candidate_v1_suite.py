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
    Path(__file__).resolve().parents[1]
    / "run_macinmeter_candidate_v1_suite.py"
)
SPEC = importlib.util.spec_from_file_location(
    "macinmeter_candidate_v1_suite", TOOL_PATH
)
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


class SyntheticCandidateSuite:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.corpus = root / "corpus"
        self.corpus.mkdir()
        cases = []
        for order, (case_id, sample) in enumerate(
            (("safe-a", 0.25), ("safe-b", 0.5)),
            1,
        ):
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
                    "executionClass": "safe",
                    "fileSha256": sha256(raw),
                    "byteLength": len(raw),
                    "dataSha256": sha256(data),
                    "encoding": "wav-ieee-float64le",
                    "sampleRateHz": 8000,
                    "channels": 1,
                    "frames": 2,
                }
            )
        self.manifest = root / "manifest.json"
        self.manifest.write_text(
            json.dumps(
                {
                    "schemaVersion": 2,
                    "corpusId": "synthetic-candidate-suite",
                    "budgets": {"expectedSafeMasterEntries": len(cases)},
                    "cases": cases,
                },
                sort_keys=True,
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )

        self.worker = root / "fake_candidate_worker.py"
        self.worker.write_text(
            r'''#!/usr/bin/env python3
import json, os, sys
input_id, sample_rate, channels, frames, block_frames = sys.argv[1:]
pcm = sys.stdin.buffer.read()
if len(pcm) != int(channels) * int(frames) * 8:
    sys.exit(4)
log = os.environ.get("CANDIDATE_SUITE_FAKE_LOG")
if log:
    with open(log, "a", encoding="ascii") as output:
        output.write(f"{os.getpid()} {input_id}\n")
algorithm = {
    "profile": "foo_dr_meter_1_0_8_candidate_v1",
    "profileVersion": 1,
    "compatibility": "unverified",
}
channel_values = [
    {
        "channelIndex": index,
        "report": {"overallRmsLinear": 0.1, "primaryPeakLinear": 1.0},
        "outcome": {
            "status": "measured",
            "measurement": {"drDb": 10.0},
        },
    }
    for index in range(int(channels))
]
bits = [
    {
        "index": index,
        "outcome": "measured",
        "drBits": "41200000",
        "rmsBits": "3dcccccd",
        "peakBits": "3f800000",
    }
    for index in range(int(channels))
]
value = {
    "schemaVersion": 1,
    "kind": "macinmeter_candidate_v1_conformance_result",
    "inputId": input_id,
    "input": {
        "sampleRateHz": int(sample_rate),
        "channels": int(channels),
        "frames": int(frames),
        "blockFrames": int(block_frames),
        "sampleEncoding": "f64le-interleaved",
    },
    "algorithm": algorithm,
    "coreBits": {
        "trackDrBits": "41200000",
        "channelResults": bits,
    },
    "analysis": {
        "algorithm": algorithm,
        "stream": {
            "sampleRate": int(sample_rate),
            "channels": int(channels),
        },
        "framesSeen": int(frames),
        "channels": channel_values,
    },
    "claims": {
        "scope": "synthetic decoder-independent Candidate result",
        "compatibility": "unverified",
        "referenceParity": "not_assessed",
    },
}
print(json.dumps(value, separators=(",", ":")))
if os.environ.get("CANDIDATE_SUITE_FAKE_STDERR"):
    print("unexpected diagnostic", file=sys.stderr)
''',
            encoding="utf-8",
        )
        self.worker.chmod(0o755)
        self.worker_sha256 = sha256(self.worker.read_bytes())

    def run(self) -> dict[str, object]:
        return SUITE.run_suite(
            manifest_path=self.manifest,
            corpus_root=self.corpus,
            worker_path=self.worker,
            worker_sha256=self.worker_sha256,
            source_commit="1" * 40,
            timeout_seconds=5,
            block_frames=7,
        )


class CandidateSuiteTests(unittest.TestCase):
    def test_safe_cases_use_distinct_workers_and_path_free_records(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            synthetic = SyntheticCandidateSuite(Path(directory))
            log = synthetic.root / "worker-processes.log"
            with mock.patch.dict(
                os.environ, {"CANDIDATE_SUITE_FAKE_LOG": str(log)}
            ):
                record = synthetic.run()

            self.assertEqual(
                record["summary"],
                {
                    "status": "success",
                    "total": 2,
                    "succeeded": 2,
                    "failed": 0,
                },
            )
            self.assertEqual(
                [item["inputId"] for item in record["items"]],
                ["safe-a", "safe-b"],
            )
            process_ids = [
                line.split()[0]
                for line in log.read_text(encoding="ascii").splitlines()
            ]
            self.assertEqual(len(process_ids), 2)
            self.assertEqual(len(set(process_ids)), 2)
            SUITE.PARENT.assert_path_free(record, "synthetic record")

    def test_success_with_stderr_becomes_a_structured_suite_failure(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            synthetic = SyntheticCandidateSuite(Path(directory))
            with mock.patch.dict(
                os.environ, {"CANDIDATE_SUITE_FAKE_STDERR": "1"}
            ):
                record = synthetic.run()

            self.assertEqual(record["summary"]["status"], "failed")
            self.assertEqual(record["summary"]["failed"], 2)
            self.assertTrue(
                all(
                    item["result"]
                    == {
                        "kind": "error",
                        "stage": "worker",
                        "code": "contract_violation",
                    }
                    for item in record["items"]
                )
            )

    def test_worker_identity_must_match_before_any_process_starts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            synthetic = SyntheticCandidateSuite(Path(directory))
            with self.assertRaises(SUITE.PARENT.CoreHarnessError):
                SUITE.run_suite(
                    manifest_path=synthetic.manifest,
                    corpus_root=synthetic.corpus,
                    worker_path=synthetic.worker,
                    worker_sha256="0" * 64,
                    source_commit="1" * 40,
                )


if __name__ == "__main__":
    unittest.main()
