#!/usr/bin/env python3

from __future__ import annotations

import hashlib
import json
import unittest
from pathlib import Path
from typing import Any


REPOSITORY_ROOT = Path(__file__).resolve().parents[3]
TARGET_SHA256 = "ff3556add231859c2f3ddfa111312720c8d4969270416229a7bd26f73ba22489"
MANIFEST_SHA256 = "479e535a7196487fdb67a54f0c4de681f925920453e8092bac9eeb04eec4bbf8"
CORE_SUITE_SHA256 = "a511b9f46d6624d957bcd8afc7ff4e36525a06fd4772c35f7708ae4379e19d93"
BOUNDARY_SUITE_SHA256 = (
    "28416daabebfb0291305b80328a5b2003b10606830051c370f90c78070f2901b"
)
NORMALIZED_REPORT_SHA256 = (
    "50205960b9850addb7f18bdb5f3c2c3c59897a5a2c5efc8e408870d5a3a2ffce"
)
CLEAN_COMPARISON_SHA256 = (
    "6e890323ca5d2338344695e5ad1129703dbcb32d20e11c483deea5af811d1f1f"
)
CLEAN_WIRE_SHA256 = "7e1bb26d2c4d88d39e87ec438507e4817dc6e599a4c2e2a79cb0a9311626214b"
GENERATOR_SHA256 = "f83fdcd0b88f2f414c53f8aa52a5b03f4fd4c8ee25024c4dce603df9a2179054"
DIRECT_SOURCE_COMMIT = "76d0f2eab5cdfce9de6a9d76ab971c333eab8e71"
DIRECT_WORKER_SHA256 = (
    "ae42263881d6a76f6bfc675fb9e52e1141a03a87dd0d91363616e14e9c4b669d"
)
DIRECT_SUITE_4096_SHA256 = (
    "93bfea94098035853b8630231d8e6c833a192cc2455093860f5dcb174ba7bec4"
)
DIRECT_COMPARISON_4096_SHA256 = (
    "cb2f6ea43f4c46d7cb6164f6124e720192c144012a1cecec0d4535dbc8b395fd"
)
DIRECT_SUITE_997_SHA256 = (
    "1506b76b61452111fdaced4c2075eb6919d64bf52a06e2a3ed18742ac740af6c"
)
DIRECT_COMPARISON_997_SHA256 = (
    "822ec149d28369c856ef4a01f9656ac8e9383746dc4feab8e177c23bb8356c1e"
)

MANIFEST_PATH = (
    REPOSITORY_ROOT
    / "reference/fixtures/foo-dr-meter-108-complete-v2.manifest.json"
)
CORE_SUITE_PATH = (
    REPOSITORY_ROOT
    / "reference/observations/"
    "obs-foo-dr-meter-108-x64-isolated-core-safe-master-run1-20260719/"
    "suite.json"
)
BOUNDARY_SUITE_PATH = (
    REPOSITORY_ROOT
    / "reference/observations/"
    "obs-foo-dr-meter-108-x64-numeric-boundaries-v1-run1-20260719/"
    "suite.json"
)
NORMALIZED_REPORT_PATH = (
    REPOSITORY_ROOT
    / "reference/observations/"
    "obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/"
    "normalized/safe-master.json"
)
CLEAN_CONFORMANCE_ROOT = (
    REPOSITORY_ROOT
    / "reference/conformance/"
    "conf-foo-dr-meter-108-x64-complete-v2-safe-master-"
    "macinmeter-020-report-v3-clean-20260718"
)
CLEAN_COMPARISON_PATH = CLEAN_CONFORMANCE_ROOT / "comparison.json"
CLEAN_WIRE_PATH = CLEAN_CONFORMANCE_ROOT / "implementation/schema3-wire.json"
GENERATOR_PATH = (
    REPOSITORY_ROOT
    / "reference/tools/generate_foo_dr_meter_108_complete_v2.py"
)
DIRECT_CONFORMANCE_ROOT = (
    REPOSITORY_ROOT
    / "reference/conformance/"
    "conf-foo-dr-meter-108-x64-candidate-v1-direct-pcm-run1-20260720"
)
DIRECT_SUITE_4096_PATH = DIRECT_CONFORMANCE_ROOT / "suite-block-4096.json"
DIRECT_COMPARISON_4096_PATH = (
    DIRECT_CONFORMANCE_ROOT / "comparison-block-4096.json"
)
DIRECT_SUITE_997_PATH = DIRECT_CONFORMANCE_ROOT / "suite-block-997.json"
DIRECT_COMPARISON_997_PATH = (
    DIRECT_CONFORMANCE_ROOT / "comparison-block-997.json"
)

EXTENSIBLE_SAFE_IDS = {
    "three-channel-arithmetic",
    "six-channel-lfe",
    "eight-channel-report-map",
    "aggregate-narrow-low",
    "aggregate-narrow-high",
}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_object(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise AssertionError(f"{path} must contain one JSON object")
    return value


class M4EvidenceContractTests(unittest.TestCase):
    def test_fixed_artifact_digests_do_not_drift(self) -> None:
        expected = {
            MANIFEST_PATH: MANIFEST_SHA256,
            CORE_SUITE_PATH: CORE_SUITE_SHA256,
            BOUNDARY_SUITE_PATH: BOUNDARY_SUITE_SHA256,
            NORMALIZED_REPORT_PATH: NORMALIZED_REPORT_SHA256,
            CLEAN_COMPARISON_PATH: CLEAN_COMPARISON_SHA256,
            CLEAN_WIRE_PATH: CLEAN_WIRE_SHA256,
            GENERATOR_PATH: GENERATOR_SHA256,
            DIRECT_SUITE_4096_PATH: DIRECT_SUITE_4096_SHA256,
            DIRECT_COMPARISON_4096_PATH: DIRECT_COMPARISON_4096_SHA256,
            DIRECT_SUITE_997_PATH: DIRECT_SUITE_997_SHA256,
            DIRECT_COMPARISON_997_PATH: DIRECT_COMPARISON_997_SHA256,
        }
        for path, digest in expected.items():
            with self.subTest(path=path.relative_to(REPOSITORY_ROOT)):
                self.assertEqual(sha256(path), digest)

    def test_safe_manifest_and_isolated_core_record_share_one_fixed_scope(self) -> None:
        manifest = load_object(MANIFEST_PATH)
        core = load_object(CORE_SUITE_PATH)

        self.assertEqual(manifest["schemaVersion"], 2)
        self.assertEqual(manifest["corpusId"], "foo-dr-meter-108-complete-v2")
        self.assertEqual(len(manifest["cases"]), 42)
        safe = [
            case
            for case in manifest["cases"]
            if case["executionClass"] == "safe"
        ]
        self.assertEqual(len(safe), 39)
        self.assertEqual(manifest["budgets"]["expectedSafeMasterEntries"], 39)
        self.assertEqual(len(manifest["playlists"]["00-safe-master"]), 39)

        self.assertEqual(core["schemaVersion"], 1)
        self.assertEqual(core["kind"], "foo_dr_meter_108_core_suite_record")
        self.assertEqual(core["target"]["sha256"], TARGET_SHA256)
        self.assertEqual(
            core["summary"],
            {"failed": 0, "status": "success", "succeeded": 39, "total": 39},
        )
        self.assertEqual(len(core["items"]), 39)
        self.assertEqual(
            [item["inputId"] for item in core["items"]],
            [case["id"] for case in safe],
        )
        self.assertEqual(core["claims"]["compatibility"], "none")
        self.assertEqual(core["claims"]["foobarParity"], "not_assessed")

    def test_numeric_boundary_record_keeps_every_registered_match(self) -> None:
        boundary = load_object(BOUNDARY_SUITE_PATH)

        self.assertEqual(
            boundary["kind"], "foo_dr_meter_108_numeric_boundaries_record"
        )
        self.assertEqual(boundary["target"]["sha256"], TARGET_SHA256)
        self.assertFalse(boundary["execution"]["foobarStarted"])
        self.assertEqual(
            boundary["execution"]["processModel"],
            "one_worker_process_per_vector",
        )
        self.assertEqual(
            boundary["summary"],
            {
                "allMatched": True,
                "durationMatched": 24,
                "durationTotal": 24,
                "histogramMatched": 6,
                "histogramTotal": 6,
                "weightingChannelPreconditionsMatched": 8,
                "weightingChannelPreconditionsTotal": 8,
                "weightingPairInvariantsMatched": 4,
                "weightingPairInvariantsTotal": 4,
                "weightingTrackBitsMatched": 8,
                "weightingTrackBitsTotal": 8,
            },
        )
        self.assertTrue(all(item["matched"] for item in boundary["duration"]))
        self.assertTrue(
            all(item["matched"] for item in boundary["multichannelWeighting"])
        )
        self.assertTrue(
            all(item["matched"] for item in boundary["histogramClamp"])
        )
        self.assertEqual(boundary["claims"]["compatibility"], "none")
        self.assertEqual(boundary["claims"]["foobarParity"], "not_assessed")

    def test_historical_clean_comparison_is_exact_and_stays_unverified(self) -> None:
        comparison = load_object(CLEAN_COMPARISON_PATH)

        self.assertEqual(
            comparison["kind"],
            "reference_report_metrics_conformance_comparison",
        )
        self.assertEqual(
            comparison["reference"]["manifestSha256"], MANIFEST_SHA256
        )
        self.assertEqual(
            comparison["reference"]["normalizationSha256"],
            NORMALIZED_REPORT_SHA256,
        )
        self.assertEqual(comparison["implementation"]["wireSchemaVersion"], 3)
        self.assertEqual(
            comparison["implementation"]["profile"],
            "foo_dr_meter_1_0_8_candidate_v1",
        )
        self.assertEqual(
            comparison["implementation"]["compatibility"], "unverified"
        )
        self.assertEqual(
            comparison["summary"],
            {
                "status": "match",
                "trackDrMatched": 39,
                "trackDrTotal": 39,
                "channelDrMatched": 62,
                "channelDrTotal": 62,
                "overallPeakMatched": 39,
                "overallPeakTotal": 39,
                "overallRmsMatched": 39,
                "overallRmsTotal": 39,
                "channelRmsMatched": 62,
                "channelRmsTotal": 62,
                "durationMatched": 39,
                "durationTotal": 39,
                "footerConsistencyMatched": 4,
                "footerConsistencyTotal": 4,
                "differenceCount": 0,
                "fixtureSetExact": True,
                "implementationOrderMatchesReference": True,
            },
        )
        self.assertEqual(comparison["differences"], [])
        self.assertEqual(comparison["policy"]["numericToleranceDb"], 0.0)

    def test_current_file_replay_split_is_an_explicit_decoder_boundary(self) -> None:
        manifest = load_object(MANIFEST_PATH)
        safe = [
            case
            for case in manifest["cases"]
            if case["executionClass"] == "safe"
        ]
        extensible = {
            case["id"] for case in safe if case["channelMask"] is not None
        }
        classic = {
            case["id"] for case in safe if case["channelMask"] is None
        }

        self.assertEqual(extensible, EXTENSIBLE_SAFE_IDS)
        self.assertEqual(len(classic), 34)
        self.assertTrue(extensible.isdisjoint(classic))
        self.assertEqual(len(extensible | classic), 39)

    def test_current_direct_pcm_suites_are_exact_and_chunk_independent(self) -> None:
        expected_summary = {
            "status": "match",
            "trackDrBitsMatched": 39,
            "trackDrBitsTotal": 39,
            "channelDrBitsMatched": 62,
            "channelDrBitsTotal": 62,
            "channelRmsBitsMatched": 62,
            "channelRmsBitsTotal": 62,
            "channelPeakBitsMatched": 62,
            "channelPeakBitsTotal": 62,
            "trackDrTokenMatched": 39,
            "trackDrTokenTotal": 39,
            "channelDrTokenMatched": 62,
            "channelDrTokenTotal": 62,
            "overallPeakTokenMatched": 39,
            "overallPeakTokenTotal": 39,
            "overallRmsTokenMatched": 39,
            "overallRmsTokenTotal": 39,
            "channelRmsTokenMatched": 62,
            "channelRmsTokenTotal": 62,
            "durationTokenMatched": 39,
            "durationTokenTotal": 39,
            "differenceCount": 0,
            "fixtureSetExact": True,
            "manifestOrderExact": True,
        }
        artifacts = (
            (
                4096,
                DIRECT_SUITE_4096_PATH,
                DIRECT_SUITE_4096_SHA256,
                DIRECT_COMPARISON_4096_PATH,
            ),
            (
                997,
                DIRECT_SUITE_997_PATH,
                DIRECT_SUITE_997_SHA256,
                DIRECT_COMPARISON_997_PATH,
            ),
        )
        projections = []
        for block_frames, suite_path, suite_sha, comparison_path in artifacts:
            with self.subTest(block_frames=block_frames):
                suite = load_object(suite_path)
                comparison = load_object(comparison_path)

                self.assertEqual(
                    suite["kind"],
                    "macinmeter_candidate_v1_direct_pcm_suite",
                )
                self.assertEqual(
                    suite["summary"],
                    {
                        "failed": 0,
                        "status": "success",
                        "succeeded": 39,
                        "total": 39,
                    },
                )
                self.assertEqual(len(suite["items"]), 39)
                self.assertEqual(
                    suite["corpus"]["manifestSha256"],
                    MANIFEST_SHA256,
                )
                self.assertEqual(
                    suite["implementation"]["sourceCommit"],
                    DIRECT_SOURCE_COMMIT,
                )
                self.assertEqual(
                    suite["implementation"]["workerSha256"],
                    DIRECT_WORKER_SHA256,
                )
                self.assertEqual(
                    suite["implementation"]["compatibility"],
                    "unverified",
                )
                self.assertEqual(
                    suite["execution"]["blockFrames"], block_frames
                )
                self.assertFalse(suite["execution"]["decoderUsed"])
                self.assertEqual(
                    suite["execution"]["processModel"],
                    "one_worker_process_per_input",
                )

                self.assertEqual(
                    comparison["kind"],
                    "macinmeter_candidate_v1_x64_numeric_comparison",
                )
                self.assertEqual(
                    comparison["target"]["sha256"], TARGET_SHA256
                )
                self.assertEqual(
                    comparison["evidence"]["referenceCoreSuiteSha256"],
                    CORE_SUITE_SHA256,
                )
                self.assertEqual(
                    comparison["evidence"]["normalizedReportSha256"],
                    NORMALIZED_REPORT_SHA256,
                )
                self.assertEqual(
                    comparison["implementation"]["candidateSuiteSha256"],
                    suite_sha,
                )
                self.assertEqual(comparison["summary"], expected_summary)
                self.assertEqual(comparison["differences"], [])
                self.assertEqual(
                    comparison["policy"]["numericToleranceDb"], 0.0
                )
                self.assertFalse(
                    comparison["policy"]["intermediateStateCompared"]
                )
                self.assertFalse(comparison["policy"]["decoderUsed"])
                self.assertEqual(
                    comparison["claims"]["compatibility"], "unverified"
                )

                projections.append(
                    [
                        {
                            "inputId": item["result"]["data"]["inputId"],
                            "coreBits": item["result"]["data"]["coreBits"],
                            "analysis": item["result"]["data"]["analysis"],
                        }
                        for item in suite["items"]
                    ]
                )

        self.assertEqual(projections[0], projections[1])


if __name__ == "__main__":
    unittest.main()
