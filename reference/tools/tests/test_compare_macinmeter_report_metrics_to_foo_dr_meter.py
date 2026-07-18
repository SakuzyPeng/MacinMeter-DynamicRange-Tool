#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Any


TOOL_PATH = (
    Path(__file__).resolve().parents[1]
    / "compare_macinmeter_report_metrics_to_foo_dr_meter.py"
)
SPEC = importlib.util.spec_from_file_location("report_metrics_comparator", TOOL_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot import {TOOL_PATH}")
COMPARATOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = COMPARATOR
SPEC.loader.exec_module(COMPARATOR)


def reference_document() -> dict[str, Any]:
    return {
        "schemaVersion": 1,
        "kind": "foo_dr_meter_report_normalization",
        "source": {
            "rawReportSha256": "1" * 64,
            "manifestSha256": "2" * 64,
            "corpusId": "synthetic",
            "playlist": "safe",
        },
        "header": {
            "foobar2000Version": "test",
            "drMeterVersion": "1.0.8",
        },
        "cases": [
            {
                "fixtureId": "case-one",
                "stem": "001_case_one",
                "channels": 1,
                "trackDr": 12,
                "peakDbfsToken": "0.00",
                "rmsDbfsToken": "-12.04",
                "channelDrDbTokens": ["12.04"],
                "channelRmsDbfsTokens": ["-inf"],
            }
        ],
        "validation": {
            "observedTrackCount": 1,
            "observedChannelValueCount": 1,
            "manifestStemsExactlyOnce": True,
            "manifestOrderExact": True,
        },
    }


def implementation_document(schema_version: int = 3) -> dict[str, Any]:
    display_path = "/tmp/001_case_one.wav"
    return {
        "schemaVersion": schema_version,
        "toolVersion": "0.2.0",
        "kind": "batch",
        "data": {
            "status": "succeeded",
            "summary": {"total": 1, "succeeded": 1, "failed": 0},
            "items": [
                {
                    "displayPath": display_path,
                    "outcome": {
                        "status": "success",
                        "report": {
                            "source": {"displayPath": display_path},
                            "analysis": {
                                "algorithm": {
                                    "profile": "foo_dr_meter_1_0_8_candidate_v1",
                                    "compatibility": "unverified",
                                    "parameters": {"silentChannelDrDb": 0.0},
                                },
                                "channels": [
                                    {
                                        "channelIndex": 0,
                                        "report": {"overallRmsDbfs": None},
                                        "outcome": {
                                            "status": "measured",
                                            "measurement": {"drDb": 12.04},
                                        },
                                    }
                                ],
                                "aggregates": {"track": {"roundedDr": 12}},
                                "report": {
                                    "primaryPeakDbfs": -0.004,
                                    "overallRmsDbfs": -12.04,
                                },
                            },
                        },
                    },
                }
            ],
        },
    }


class ReportTokenTests(unittest.TestCase):
    def test_null_and_near_zero_values_follow_reference_rules(self) -> None:
        self.assertEqual(COMPARATOR.report_db_token(None, "value"), "-inf")
        self.assertEqual(COMPARATOR.report_db_token(-0.004, "value"), "0.00")
        self.assertEqual(COMPARATOR.report_db_token(0.004, "value"), "0.00")
        self.assertEqual(COMPARATOR.report_db_token(-0.005, "value"), "-0.01")
        self.assertEqual(COMPARATOR.report_db_token(0.005, "value"), "0.01")


class ComparatorContractTests(unittest.TestCase):
    def compare_documents(
        self, reference: dict[str, Any], implementation: dict[str, Any]
    ) -> dict[str, Any]:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            reference_path = root / "reference.json"
            implementation_path = root / "implementation.json"
            binary_path = root / "macinmeter"
            reference_path.write_text(
                json.dumps(reference), encoding="utf-8", newline="\n"
            )
            implementation_path.write_text(
                json.dumps(implementation), encoding="utf-8", newline="\n"
            )
            binary_path.write_bytes(b"synthetic implementation binary")
            return COMPARATOR.compare(
                reference_path, implementation_path, binary_path
            )

    def test_schema_v3_happy_path_compares_all_five_metric_classes(self) -> None:
        result = self.compare_documents(
            reference_document(), implementation_document()
        )

        self.assertEqual(
            result["summary"],
            {
                "status": "match",
                "trackDrMatched": 1,
                "trackDrTotal": 1,
                "channelDrMatched": 1,
                "channelDrTotal": 1,
                "overallPeakMatched": 1,
                "overallPeakTotal": 1,
                "overallRmsMatched": 1,
                "overallRmsTotal": 1,
                "channelRmsMatched": 1,
                "channelRmsTotal": 1,
                "differenceCount": 0,
                "fixtureSetExact": True,
                "implementationOrderMatchesReference": True,
            },
        )
        self.assertEqual(result["differences"], [])
        self.assertEqual(result["implementation"]["wireSchemaVersion"], 3)

    def test_schema_v2_is_rejected_explicitly(self) -> None:
        with self.assertRaisesRegex(
            COMPARATOR.ComparisonError,
            r"schemaVersion must be 3; got 2",
        ):
            self.compare_documents(
                reference_document(), implementation_document(schema_version=2)
            )

    def test_nonfinite_report_metric_is_rejected(self) -> None:
        implementation = implementation_document()
        implementation["data"]["items"][0]["outcome"]["report"]["analysis"]["report"][
            "overallRmsDbfs"
        ] = float("nan")

        with self.assertRaisesRegex(COMPARATOR.ComparisonError, "must be finite"):
            self.compare_documents(reference_document(), implementation)


if __name__ == "__main__":
    unittest.main()
