#!/usr/bin/env python3

from __future__ import annotations

import argparse
import importlib.util
import sys
import unittest
from array import array
from pathlib import Path
from unittest import mock


TOOL_PATH = (
    Path(__file__).resolve().parents[1]
    / "run_foo_dr_meter_108_numeric_boundaries.py"
)
SPEC = importlib.util.spec_from_file_location(
    "foo_dr_meter_numeric_boundaries", TOOL_PATH
)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot import {TOOL_PATH}")
BOUNDARIES = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = BOUNDARIES
SPEC.loader.exec_module(BOUNDARIES)


class BoundaryMatrixTests(unittest.TestCase):
    def test_duration_matrix_covers_half_seconds_and_unit_carries(self) -> None:
        cases = BOUNDARIES._duration_cases()
        self.assertEqual(len(cases), 24)
        for case in cases:
            with self.subTest(case=case.case_id):
                self.assertEqual(
                    BOUNDARIES._expected_duration_text(
                        case.decoded_frames, case.sample_rate_hz
                    ),
                    case.expected_text,
                )
        by_id = {case.case_id: case.expected_text for case in cases}
        self.assertEqual(by_id["duration-44100-half"], "0:01")
        self.assertEqual(by_id["duration-48000-half"], "0:01")
        self.assertEqual(by_id["duration-minute-half"], "1:00")
        self.assertEqual(by_id["duration-hour-half"], "1:00:00")
        self.assertEqual(by_id["duration-day-half"], "1d 0:00:00")
        self.assertEqual(by_id["duration-week-half"], "1wk 0d 0:00:00")

    def test_weighting_matrix_discriminates_formula_and_channel_gate(self) -> None:
        scenarios = list(BOUNDARIES._weighting_scenarios())
        self.assertEqual(len(scenarios), 4)
        expected = {
            "weighting-balanced-3ch": ("41a00000", "415a524a"),
            "weighting-overall-rms-source-3ch": ("41a00000", "413714ce"),
            "weighting-gate-2ch": ("41a00000", "41a00000"),
            "weighting-partial-silence-3ch": ("41555555", "413d1746"),
        }
        for scenario in scenarios:
            with self.subTest(scenario=scenario.scenario_id):
                self.assertTrue(
                    all(
                        len(channel)
                        == BOUNDARIES._window_frames(
                            BOUNDARIES.WEIGHTING_SAMPLE_RATE
                        )
                        * BOUNDARIES.WEIGHTING_WINDOW_COUNT
                        for channel in scenario.channels
                    )
                )
                self.assertEqual(
                    (
                        BOUNDARIES._expected_weighted_track_bits(
                            scenario, False
                        ),
                        BOUNDARIES._expected_weighted_track_bits(
                            scenario, True
                        ),
                    ),
                    expected[scenario.scenario_id],
                )

    def test_histogram_matrix_has_exact_inside_and_beyond_endpoint_cases(self) -> None:
        cases = BOUNDARIES._histogram_cases()
        self.assertEqual(len(cases), 6)
        counts = {
            case.case_id: (
                case.expected_minus_100_db_count,
                case.expected_zero_db_count,
            )
            for case in cases
        }
        self.assertEqual(counts["histogram-lower-beyond"], (1, 0))
        self.assertEqual(counts["histogram-lower-exact"], (1, 0))
        self.assertEqual(counts["histogram-lower-inside"], (0, 0))
        self.assertEqual(counts["histogram-upper-inside"], (0, 0))
        self.assertEqual(counts["histogram-upper-exact"], (0, 1))
        self.assertEqual(counts["histogram-upper-beyond"], (0, 1))

    def test_suite_records_all_three_families_and_pair_invariant(self) -> None:
        duration = BOUNDARIES.DurationCase(
            "duration-test", 1, 2, "0:01", "test"
        )
        weighting = BOUNDARIES.WeightingScenario(
            "weighting-test",
            (
                array("d", [0.25, -0.25]),
                array("d", [0.5, -0.5]),
                array("d", [0.75, -0.75]),
            ),
            (10.0, 20.0, 30.0),
            (0.3, 0.1, 0.03),
            "test",
        )
        histogram = BOUNDARIES.HistogramCase(
            "histogram-test", -101.0, 1.0e-6, 1.0e-4, 1, 0, "test"
        )
        expected_off = BOUNDARIES._expected_weighted_track_bits(
            weighting, False
        )
        expected_on = BOUNDARIES._expected_weighted_track_bits(
            weighting, True
        )

        def fake_duration_worker(**_: object) -> dict[str, object]:
            return {"result": {"text": "0:01"}}

        def fake_core_worker(
            prepared: object,
            *,
            multichannel_loudness_weighting: bool,
            **_: object,
        ) -> dict[str, object]:
            input_id = getattr(prepared, "input_id")
            common_state = {
                "channelResults": (
                    [
                        {
                            "index": index,
                            "drBits": item["drBits"],
                            "rmsBits": item["rmsBits"],
                        }
                        for index, item in enumerate(
                            BOUNDARIES._expected_channel_bits(weighting)
                        )
                    ]
                    if input_id == "weighting-test"
                    else [{"index": 0, "drBits": "00000000", "rmsBits": "00000000"}]
                ),
                "sessionBeforeFinish": {"windowCount": 1},
                "sessionAfterFinish": {"windowCount": 1},
                "channelStateAfterFinish": [{"index": 0}],
                "histogramAfterFinish": {
                    "channels": [
                        {
                            "index": 0,
                            "totalCount": 1,
                            "nonzeroBinCount": 1,
                            "minus100DbCount": 1
                            if input_id == "histogram-test"
                            else 0,
                            "zeroDbCount": 0,
                        }
                    ]
                },
            }
            if input_id == "weighting-test":
                track = expected_on if multichannel_loudness_weighting else expected_off
            else:
                track = "00000000"
            return {
                "input": {
                    "pcmSha256": prepared.pcm_identity.sha256,
                    "pcmByteLength": prepared.pcm_identity.byte_length,
                    "sampleRateHz": prepared.sample_rate,
                    "channels": prepared.channels,
                    "frames": prepared.frames,
                },
                "result": {"trackDrBits": track, **common_state},
            }

        args = argparse.Namespace(
            worker=Path("worker.exe"),
            worker_sha256="a" * 64,
            target_dll=Path("foo_dr_meter.dll"),
            shared_dll=Path("shared.dll"),
            shared_sha256="b" * 64,
            msvcp140_dll=Path("msvcp140.dll"),
            msvcp140_sha256="c" * 64,
            vcruntime140_dll=Path("vcruntime140.dll"),
            vcruntime140_sha256="d" * 64,
            vcruntime140_1_dll=Path("vcruntime140_1.dll"),
            vcruntime140_1_sha256="e" * 64,
            timeout_seconds=1.0,
            block_frames=2,
            output=None,
        )
        with (
            mock.patch.object(
                BOUNDARIES, "_duration_cases", return_value=(duration,)
            ),
            mock.patch.object(
                BOUNDARIES,
                "_weighting_scenarios",
                side_effect=lambda: iter((weighting,)),
            ),
            mock.patch.object(
                BOUNDARIES, "_histogram_cases", return_value=(histogram,)
            ),
            mock.patch.object(
                BOUNDARIES.PARENT,
                "run_duration_worker",
                autospec=True,
                side_effect=fake_duration_worker,
            ) as duration_run,
            mock.patch.object(
                BOUNDARIES.PARENT,
                "run_core_worker",
                autospec=True,
                side_effect=fake_core_worker,
            ),
        ):
            record = BOUNDARIES.run_suite(args)

        duration_kwargs = duration_run.call_args.kwargs
        self.assertNotIn("input_id", duration_kwargs)
        self.assertEqual(
            set(duration_kwargs),
            {
                "decoded_frames",
                "sample_rate_hz",
                "fractional_digits",
                "worker_path",
                "worker_sha256",
                "target_path",
                "runtime_artifact_sources",
                "runtime_profile",
                "timeout_seconds",
            },
        )
        self.assertTrue(record["summary"]["allMatched"])
        self.assertEqual(record["summary"]["durationTotal"], 1)
        self.assertEqual(record["summary"]["weightingTrackBitsTotal"], 2)
        self.assertEqual(
            record["summary"]["weightingChannelPreconditionsMatched"], 2
        )
        self.assertEqual(record["summary"]["weightingPairInvariantsTotal"], 1)
        self.assertEqual(record["summary"]["histogramTotal"], 1)
        self.assertFalse(record["execution"]["foobarStarted"])


if __name__ == "__main__":
    unittest.main()
