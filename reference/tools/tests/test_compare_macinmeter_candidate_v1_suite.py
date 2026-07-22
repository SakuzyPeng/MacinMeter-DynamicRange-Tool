#!/usr/bin/env python3

from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
import struct
import sys
import tempfile
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[3]
TOOL_PATH = (
    REPOSITORY_ROOT
    / "reference/tools/compare_macinmeter_candidate_v1_suite.py"
)
SPEC = importlib.util.spec_from_file_location(
    "macinmeter_candidate_v1_comparison", TOOL_PATH
)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot import {TOOL_PATH}")
COMPARATOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = COMPARATOR
SPEC.loader.exec_module(COMPARATOR)

REFERENCE_CORE_PATH = (
    REPOSITORY_ROOT
    / "reference/observations/"
    "obs-foo-dr-meter-108-x64-isolated-core-safe-master-run1-20260719/"
    "suite.json"
)
NORMALIZED_REPORT_PATH = (
    REPOSITORY_ROOT
    / "reference/observations/"
    "obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/"
    "normalized/safe-master.json"
)
HISTORICAL_WIRE_PATH = (
    REPOSITORY_ROOT
    / "reference/conformance/"
    "conf-foo-dr-meter-108-x64-complete-v2-safe-master-"
    "macinmeter-020-report-v3-clean-20260718/"
    "implementation/schema3-wire.json"
)


def f32_bits(value: float) -> str:
    raw = struct.pack("<f", value)
    return f"{struct.unpack('<I', raw)[0]:08x}"


def core_projection(analysis: dict[str, object]) -> dict[str, object]:
    aggregate = analysis["aggregates"]["track"]
    track_dr = aggregate["drDb"]
    channels = []
    for channel in analysis["channels"]:
        outcome = channel["outcome"]
        status = outcome["status"]
        if status == "measured":
            dr_bits = f32_bits(outcome["measurement"]["drDb"])
        elif status == "silent":
            dr_bits = "00000000"
        else:
            dr_bits = None
        channels.append(
            {
                "index": channel["channelIndex"],
                "outcome": status,
                "drBits": dr_bits,
                "rmsBits": f32_bits(channel["report"]["overallRmsLinear"]),
                "peakBits": f32_bits(channel["report"]["primaryPeakLinear"]),
            }
        )
    return {
        "trackDrBits": None if track_dr is None else f32_bits(track_dr),
        "channelResults": channels,
    }


def build_candidate_suite() -> dict[str, object]:
    reference = json.loads(REFERENCE_CORE_PATH.read_text(encoding="utf-8"))
    wire = json.loads(HISTORICAL_WIRE_PATH.read_text(encoding="utf-8"))
    source_commit = "1" * 40
    worker_sha256 = "2" * 64
    worker_byte_length = 123
    block_frames = 4096
    timeout_seconds = 5.0
    items = []
    for reference_item, wire_item in zip(
        reference["items"], wire["data"]["items"], strict=True
    ):
        analysis = copy.deepcopy(wire_item["outcome"]["report"]["analysis"])
        analysis["algorithm"].pop("profile", None)
        analysis["algorithm"].pop("profileVersion", None)
        input_value = copy.deepcopy(reference_item["input"])
        request_semantic = {
            "schemaVersion": 1,
            "inputId": reference_item["inputId"],
            "pcmSha256": input_value["pcmSha256"],
            "pcmByteLength": input_value["pcmByteLength"],
            "sampleRateHz": input_value["sampleRateHz"],
            "channels": input_value["channels"],
            "frames": input_value["frames"],
            "workerSha256": worker_sha256,
            "workerByteLength": worker_byte_length,
            "sourceCommit": source_commit,
            "blockFrames": block_frames,
        }
        request_id = hashlib.sha256(
            COMPARATOR.CORE.canonical_json_bytes(request_semantic)
        ).hexdigest()
        items.append(
            {
                "manifestOrder": reference_item["manifestOrder"],
                "inputId": reference_item["inputId"],
                "requestId": request_id,
                "input": input_value,
                "result": {
                    "kind": "success",
                    "data": {
                        "schemaVersion": 1,
                        "kind": (
                            "macinmeter_candidate_v1_conformance_result"
                        ),
                        "inputId": reference_item["inputId"],
                        "input": {
                            "sampleRateHz": input_value["sampleRateHz"],
                            "channels": input_value["channels"],
                            "frames": input_value["frames"],
                            "blockFrames": block_frames,
                            "sampleEncoding": "f64le-interleaved",
                        },
                        "algorithm": analysis["algorithm"],
                        "coreBits": core_projection(analysis),
                        "analysis": analysis,
                        "claims": {
                            "scope": "decoder-independent MacinMeter analysis",
                            "referenceParity": "not_assessed",
                        },
                    },
                },
            }
        )

    corpus = copy.deepcopy(reference["corpus"])
    identity = {
        "schemaVersion": 1,
        "manifestSha256": corpus["manifestSha256"],
        "corpusId": corpus["id"],
        "safeCaseIds": corpus["safeCaseIds"],
        "workerSha256": worker_sha256,
        "workerByteLength": worker_byte_length,
        "sourceCommit": source_commit,
        "blockFrames": block_frames,
        "timeoutSeconds": timeout_seconds,
    }
    return {
        "schemaVersion": 1,
        "kind": "macinmeter_candidate_v1_direct_pcm_suite",
        "suiteId": hashlib.sha256(
            COMPARATOR.CORE.canonical_json_bytes(identity)
        ).hexdigest(),
        "corpus": corpus,
        "implementation": {
            "sourceCommit": source_commit,
            "workerSha256": worker_sha256,
            "workerByteLength": worker_byte_length,
        },
        "execution": {
            "timeoutSeconds": timeout_seconds,
            "blockFrames": block_frames,
            "processModel": "one_worker_process_per_input",
            "inputBoundary": "finite_interleaved_f64le",
            "decoderUsed": False,
        },
        "items": items,
        "summary": {
            "status": "success",
            "total": 39,
            "succeeded": 39,
            "failed": 0,
        },
        "claims": {
            "scope": "decoder-independent analysis suite",
            "referenceParity": "not_assessed",
        },
        "limitations": ["Synthetic unit-test suite metadata."],
    }


def write_json(path: Path, value: object) -> None:
    path.write_bytes(COMPARATOR.CORE.canonical_json_bytes(value))


class CandidateComparisonTests(unittest.TestCase):
    def test_matching_public_bits_and_tokens_are_all_exact(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            candidate_path = Path(directory) / "candidate.json"
            write_json(candidate_path, build_candidate_suite())

            comparison = COMPARATOR.compare(
                candidate_path,
                REFERENCE_CORE_PATH,
                NORMALIZED_REPORT_PATH,
            )

            self.assertEqual(comparison["summary"]["status"], "match")
            self.assertEqual(comparison["summary"]["differenceCount"], 0)
            self.assertEqual(comparison["summary"]["trackDrBitsMatched"], 39)
            self.assertEqual(comparison["summary"]["channelDrBitsMatched"], 62)
            self.assertEqual(comparison["summary"]["channelRmsBitsMatched"], 62)
            self.assertEqual(comparison["summary"]["channelPeakBitsMatched"], 62)
            self.assertEqual(
                comparison["policy"]["numericToleranceDb"],
                0.0,
            )
            self.assertFalse(
                comparison["policy"]["intermediateStateCompared"]
            )

    def test_one_public_bit_change_is_reported_without_a_tolerance(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            candidate = build_candidate_suite()
            result = candidate["items"][0]["result"]["data"]
            analysis = result["analysis"]
            analysis["aggregates"]["track"]["drDb"] = 12.05
            result["coreBits"]["trackDrBits"] = f32_bits(12.05)
            candidate_path = Path(directory) / "candidate.json"
            write_json(candidate_path, candidate)

            comparison = COMPARATOR.compare(
                candidate_path,
                REFERENCE_CORE_PATH,
                NORMALIZED_REPORT_PATH,
            )

            self.assertEqual(
                comparison["summary"]["status"],
                "systematic_difference",
            )
            self.assertEqual(comparison["summary"]["trackDrBitsMatched"], 38)
            self.assertTrue(
                any(
                    difference["field"] == "trackDrBits"
                    for difference in comparison["differences"]
                )
            )

    def test_decoder_backed_suite_is_rejected_before_comparison(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            candidate = build_candidate_suite()
            candidate["execution"]["decoderUsed"] = True
            candidate_path = Path(directory) / "candidate.json"
            write_json(candidate_path, candidate)

            with self.assertRaises(COMPARATOR.CandidateComparisonError):
                COMPARATOR.compare(
                    candidate_path,
                    REFERENCE_CORE_PATH,
                    NORMALIZED_REPORT_PATH,
                )


if __name__ == "__main__":
    unittest.main()
