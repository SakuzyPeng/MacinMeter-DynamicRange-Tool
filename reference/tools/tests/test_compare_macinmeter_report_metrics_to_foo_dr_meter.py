#!/usr/bin/env python3

from __future__ import annotations

import copy
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
REPOSITORY_ROOT = Path(__file__).resolve().parents[3]
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
                "durationToken": "0:03",
                "channelDrDbTokens": ["12.04"],
                "channelRmsDbfsTokens": ["-inf"],
            }
        ],
        "footer": {
            "numberOfTracksToken": "1",
            "officialDrToken": "DR12",
            "sampleRateToken": "8000 Hz",
            "channelsToken": "1",
        },
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
                            "source": {
                                "displayPath": display_path,
                                "sampleRate": 8000,
                                "channels": 1,
                            },
                            "analysis": {
                                "stream": {
                                    "sampleRate": 8000,
                                    "channels": 1,
                                },
                                "algorithm": {
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
                                "aggregates": {
                                    "track": {
                                        "drDb": 12.04,
                                        "roundedDr": 12,
                                    }
                                },
                                "report": {
                                    "primaryPeakDbfs": -0.004,
                                    "overallRmsDbfs": -12.04,
                                    "duration": {
                                        "decodedFrames": 24031,
                                        "sampleRate": 8000,
                                    },
                                },
                            },
                        },
                    },
                }
            ],
        },
    }


def dr0_footer_documents() -> tuple[dict[str, Any], dict[str, Any]]:
    reference = reference_document()
    implementation = implementation_document()

    zero_case = reference["cases"][0]
    zero_case.update(
        {
            "fixtureId": "zero-track",
            "stem": "001_zero_track",
            "trackDr": 0,
            "channelDrDbTokens": ["0.00"],
        }
    )
    ten_case = copy.deepcopy(zero_case)
    ten_case.update(
        {
            "fixtureId": "ten-track",
            "stem": "002_ten_track",
            "channels": 2,
            "trackDr": 10,
            "channelDrDbTokens": ["10.00", "10.00"],
            "channelRmsDbfsTokens": ["-inf", "-inf"],
        }
    )
    reference["cases"].append(ten_case)
    reference["footer"].update(
        {
            "numberOfTracksToken": "2",
            "officialDrToken": "DR5",
            "sampleRateToken": "8000, 48000 Hz",
            "channelsToken": "1, 2",
        }
    )
    reference["validation"].update(
        {
            "observedTrackCount": 2,
            "observedChannelValueCount": 3,
        }
    )

    zero_item = implementation["data"]["items"][0]
    zero_path = "/tmp/001_zero_track.wav"
    zero_item["displayPath"] = zero_path
    zero_report = zero_item["outcome"]["report"]
    zero_report["source"]["displayPath"] = zero_path
    zero_analysis = zero_report["analysis"]
    zero_analysis["channels"][0]["outcome"]["measurement"]["drDb"] = 0.0
    zero_analysis["aggregates"]["track"].update(
        {
            "drDb": 0.0,
            "roundedDr": 0,
        }
    )

    ten_item = copy.deepcopy(zero_item)
    ten_path = "/tmp/002_ten_track.wav"
    ten_item["displayPath"] = ten_path
    ten_report = ten_item["outcome"]["report"]
    ten_report["source"].update(
        {
            "displayPath": ten_path,
            "sampleRate": 48000,
            "channels": 2,
        }
    )
    ten_analysis = ten_report["analysis"]
    ten_analysis["stream"].update(
        {
            "sampleRate": 48000,
            "channels": 2,
        }
    )
    ten_channel = ten_analysis["channels"][0]
    ten_channel["outcome"]["measurement"]["drDb"] = 10.0
    second_channel = copy.deepcopy(ten_channel)
    second_channel["channelIndex"] = 1
    ten_analysis["channels"].append(second_channel)
    ten_analysis["aggregates"]["track"].update(
        {
            "drDb": 10.0,
            "roundedDr": 10,
        }
    )
    ten_analysis["report"]["duration"].update(
        {
            "decodedFrames": 144031,
            "sampleRate": 48000,
        }
    )
    implementation["data"]["items"].append(ten_item)
    implementation["data"]["summary"].update(
        {
            "total": 2,
            "succeeded": 2,
        }
    )
    return reference, implementation


class ReportTokenTests(unittest.TestCase):
    def test_null_and_near_zero_values_follow_reference_rules(self) -> None:
        self.assertEqual(COMPARATOR.report_db_token(None, "value"), "-inf")
        self.assertEqual(COMPARATOR.report_db_token(-0.004, "value"), "0.00")
        self.assertEqual(COMPARATOR.report_db_token(0.004, "value"), "0.00")
        self.assertEqual(COMPARATOR.report_db_token(-0.005, "value"), "-0.01")
        self.assertEqual(COMPARATOR.report_db_token(0.005, "value"), "0.01")


class DurationTokenTests(unittest.TestCase):
    def assert_duration(
        self, decoded_frames: int, sample_rate: int, expected: str
    ) -> None:
        self.assertEqual(
            COMPARATOR.require_reference_duration_token(expected, "reference"),
            expected,
        )
        self.assertEqual(
            COMPARATOR.duration_token(
                {
                    "decodedFrames": decoded_frames,
                    "sampleRate": sample_rate,
                },
                "duration",
            ),
            expected,
        )

    def test_halfway_seconds_round_away_from_zero(self) -> None:
        self.assert_duration(499, 1000, "0:00")
        self.assert_duration(1, 2, "0:01")
        self.assert_duration(1499, 1000, "0:01")
        self.assert_duration(3, 2, "0:02")

    def test_hour_day_and_week_rendering_matches_fixed_pfc_rules(self) -> None:
        self.assert_duration(59, 1, "0:59")
        self.assert_duration(60, 1, "1:00")
        self.assert_duration(3 * 3600 + 4 * 60 + 5, 1, "3:04:05")
        self.assert_duration(2 * 86400 + 3 * 3600 + 4 * 60 + 5, 1, "2d 3:04:05")
        self.assert_duration(
            604800 + 2 * 86400 + 3 * 3600 + 4 * 60 + 5,
            1,
            "1wk 2d 3:04:05",
        )
        self.assert_duration(604800, 1, "1wk 0d 0:00:00")


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

    def test_schema_v3_happy_path_compares_all_six_metric_classes(self) -> None:
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
                "durationMatched": 1,
                "durationTotal": 1,
                "footerConsistencyMatched": 4,
                "footerConsistencyTotal": 4,
                "differenceCount": 0,
                "fixtureSetExact": True,
                "implementationOrderMatchesReference": True,
            },
        )
        self.assertEqual(result["differences"], [])
        self.assertEqual(result["implementation"]["wireSchemaVersion"], 3)
        self.assertEqual(
            result["footerConsistency"],
            {
                "scope": (
                    "normalized reference footer versus successful schema-v3 "
                    "track reports; unweighted reconstruction only"
                ),
                "reference": {
                    "trackCount": 1,
                    "sampleRates": [8000],
                    "channelCounts": [1],
                    "unweightedDrToken": "DR12",
                },
                "implementation": {
                    "trackCount": 1,
                    "sampleRates": [8000],
                    "channelCounts": [1],
                    "unweightedDrToken": "DR12",
                },
                "matches": {
                    "trackCount": True,
                    "sampleRates": True,
                    "channelCounts": True,
                    "unweightedDrToken": True,
                },
                "numericDr0ExclusionCounterfactual": {
                    "scope": (
                        "same implementation track set, excluding only tracks "
                        "whose public f32 track drDb is numeric zero"
                    ),
                    "excludedNumericDr0TrackCount": 0,
                    "remainingTrackCount": 1,
                    "unweightedDrToken": "DR12",
                    "referenceMatchesAllTracksToken": True,
                    "referenceMatchesCounterfactualToken": True,
                    "distinguishesNumericDr0Inclusion": False,
                },
            },
        )
        self.assertNotIn(
            "durationToken",
            {
                entry.get("referenceField")
                for entry in result["notCompared"]
            },
        )

    def test_complete_v2_matches_all_39_tracks_and_footer_consistency(self) -> None:
        reference_path = (
            REPOSITORY_ROOT
            / "reference/observations/"
            "obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/"
            "normalized/safe-master.json"
        )
        implementation_path = (
            REPOSITORY_ROOT
            / "reference/conformance/"
            "conf-foo-dr-meter-108-x64-complete-v2-safe-master-"
            "macinmeter-020-report-v3-20260718/implementation/schema3-wire.json"
        )
        with tempfile.TemporaryDirectory() as directory:
            binary_path = Path(directory) / "macinmeter"
            binary_path.write_bytes(b"synthetic implementation binary")
            result = COMPARATOR.compare(
                reference_path, implementation_path, binary_path
            )

        self.assertEqual(result["summary"]["durationMatched"], 39)
        self.assertEqual(result["summary"]["durationTotal"], 39)
        self.assertEqual(result["summary"]["footerConsistencyMatched"], 4)
        self.assertEqual(result["summary"]["footerConsistencyTotal"], 4)
        self.assertEqual(result["summary"]["differenceCount"], 0)
        self.assertEqual(
            result["footerConsistency"]["implementation"],
            {
                "trackCount": 39,
                "sampleRates": [8000, 44100, 48000],
                "channelCounts": [1, 2, 3, 6, 8],
                "unweightedDrToken": "DR12",
            },
        )
        self.assertEqual(
            result["footerConsistency"]["numericDr0ExclusionCounterfactual"],
            {
                "scope": (
                    "same implementation track set, excluding only tracks whose "
                    "public f32 track drDb is numeric zero"
                ),
                "excludedNumericDr0TrackCount": 3,
                "remainingTrackCount": 36,
                "unweightedDrToken": "DR13",
                "referenceMatchesAllTracksToken": True,
                "referenceMatchesCounterfactualToken": False,
                "distinguishesNumericDr0Inclusion": True,
            },
        )

    def test_synthetic_footer_distinguishes_numeric_dr0_inclusion(self) -> None:
        reference, implementation = dr0_footer_documents()

        result = self.compare_documents(reference, implementation)

        self.assertEqual(result["summary"]["status"], "match")
        self.assertEqual(result["summary"]["footerConsistencyMatched"], 4)
        self.assertEqual(result["summary"]["footerConsistencyTotal"], 4)
        self.assertEqual(result["differences"], [])
        self.assertEqual(
            result["footerConsistency"]["implementation"],
            {
                "trackCount": 2,
                "sampleRates": [8000, 48000],
                "channelCounts": [1, 2],
                "unweightedDrToken": "DR5",
            },
        )
        counterfactual = result["footerConsistency"][
            "numericDr0ExclusionCounterfactual"
        ]
        self.assertEqual(counterfactual["excludedNumericDr0TrackCount"], 1)
        self.assertEqual(counterfactual["remainingTrackCount"], 1)
        self.assertEqual(counterfactual["unweightedDrToken"], "DR10")
        self.assertTrue(counterfactual["referenceMatchesAllTracksToken"])
        self.assertFalse(counterfactual["referenceMatchesCounterfactualToken"])
        self.assertTrue(counterfactual["distinguishesNumericDr0Inclusion"])

    def test_footer_difference_contributes_to_comparison_status(self) -> None:
        reference, implementation = dr0_footer_documents()
        reference["footer"]["officialDrToken"] = "DR6"

        result = self.compare_documents(reference, implementation)

        self.assertEqual(result["summary"]["status"], "systematic_difference")
        self.assertEqual(result["summary"]["footerConsistencyMatched"], 3)
        self.assertEqual(result["summary"]["footerConsistencyTotal"], 4)
        self.assertEqual(
            result["differences"],
            [
                {
                    "scope": "footerConsistency",
                    "field": "unweightedDrToken",
                    "reference": "DR6",
                    "implementation": "DR5",
                }
            ],
        )

    def test_duration_difference_is_reported_as_a_compared_field(self) -> None:
        implementation = implementation_document()
        implementation["data"]["items"][0]["outcome"]["report"]["analysis"]["report"][
            "duration"
        ]["decodedFrames"] = 0

        result = self.compare_documents(reference_document(), implementation)

        self.assertEqual(result["summary"]["durationMatched"], 0)
        self.assertEqual(result["summary"]["durationTotal"], 1)
        self.assertEqual(result["summary"]["status"], "systematic_difference")
        self.assertEqual(
            result["differences"],
            [
                {
                    "fixtureId": "case-one",
                    "stem": "001_case_one",
                    "field": "durationToken",
                    "reference": "0:03",
                    "implementation": "0:00",
                }
            ],
        )

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
