#!/usr/bin/env python3

from __future__ import annotations

import copy
import importlib.util
import json
import math
import struct
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Any


TOOL_PATH = (
    Path(__file__).resolve().parents[1]
    / "compare_foo_dr_meter_core_suite_to_report.py"
)
REPOSITORY_ROOT = Path(__file__).resolve().parents[3]
SUITE_PATH = (
    REPOSITORY_ROOT
    / "reference/observations/"
    "obs-foo-dr-meter-108-x64-isolated-core-safe-master-run1-20260719/"
    "suite.json"
)
REPORT_PATH = (
    REPOSITORY_ROOT
    / "reference/observations/"
    "obs-foo-dr-meter-108-x64-complete-v2-safe-master-run1-20260718/"
    "normalized/safe-master.json"
)
SPEC = importlib.util.spec_from_file_location("core_report_comparator", TOOL_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot import {TOOL_PATH}")
COMPARATOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = COMPARATOR
SPEC.loader.exec_module(COMPARATOR)


def load_document(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise AssertionError("test input must be an object")
    return value


def f32_bits(value: float) -> str:
    return f"{struct.unpack('<I', struct.pack('<f', value))[0]:08x}"


class RendererDataFlowTests(unittest.TestCase):
    def test_track_dr_uses_nonnegative_binary32_plus_half_then_truncation(self) -> None:
        self.assertEqual(math.trunc(COMPARATOR.f32_from_bits(f32_bits(12.49), "dr") + 0.5), 12)
        self.assertEqual(math.trunc(COMPARATOR.f32_from_bits(f32_bits(12.50), "dr") + 0.5), 13)

    def test_channel_dr_formats_the_decoded_binary32_value(self) -> None:
        self.assertEqual(
            COMPARATOR.two_decimal_token(
                COMPARATOR.f32_from_bits(f32_bits(8.0051), "dr"), "dr"
            ),
            "8.01",
        )

    def test_linear_rms_uses_binary64_log_then_binary32_narrowing(self) -> None:
        linear = COMPARATOR.f32_from_bits("3e800000", "rms")
        expected_db = COMPARATOR.narrow_f32(20.0 * math.log10(linear), "rms")
        self.assertEqual(COMPARATOR.linear_f32_db_token(linear, "rms"), "-12.04")
        self.assertEqual(
            COMPARATOR.linear_f32_db_token(linear, "rms"),
            COMPARATOR.two_decimal_token(expected_db, "rms"),
        )

    def test_zero_linear_metric_renders_negative_infinity(self) -> None:
        self.assertEqual(COMPARATOR.linear_f32_db_token(0.0, "rms"), "-inf")

    def test_near_zero_db_uses_fixed_renderer_centi_db_correction(self) -> None:
        self.assertEqual(COMPARATOR.two_decimal_token(-0.004, "db"), "0.00")
        self.assertEqual(COMPARATOR.two_decimal_token(-0.005, "db"), "-0.01")

    def test_nonfinite_raw_bits_are_rejected(self) -> None:
        for bits in ("7f800000", "ff800000", "7fc00000"):
            with self.subTest(bits=bits):
                with self.assertRaises(COMPARATOR.ComparisonError):
                    COMPARATOR.f32_from_bits(bits, "metric")


class FixedObservationContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.suite = load_document(SUITE_PATH)
        cls.report = load_document(REPORT_PATH)

    def compare_documents(
        self, suite: dict[str, Any], report: dict[str, Any]
    ) -> dict[str, Any]:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            suite_path = root / "suite.json"
            report_path = root / "report.json"
            suite_path.write_text(
                json.dumps(suite, ensure_ascii=False), encoding="utf-8", newline="\n"
            )
            report_path.write_text(
                json.dumps(report, ensure_ascii=False), encoding="utf-8", newline="\n"
            )
            return COMPARATOR.compare(suite_path, report_path)

    def test_fixed_observations_match_exact_expected_counts(self) -> None:
        result = COMPARATOR.compare(SUITE_PATH, REPORT_PATH)
        self.assertEqual(
            result["summary"],
            {
                "status": "match",
                "trackDrMatched": 39,
                "trackDrTotal": 39,
                "channelDrMatched": 62,
                "channelDrTotal": 62,
                "channelRmsMatched": 62,
                "channelRmsTotal": 62,
                "overallPeakMatched": 39,
                "overallPeakTotal": 39,
                "differenceCount": 0,
                "fixtureSetExact": True,
                "manifestOrderExact": True,
                "successfulCoreItems": 39,
            },
        )
        self.assertEqual(result["differences"], [])
        self.assertEqual(result["claims"]["foobarParity"], "not_assessed")
        self.assertEqual(result["claims"]["compatibility"], "none")

    def test_output_is_canonical_and_path_free(self) -> None:
        result = COMPARATOR.compare(SUITE_PATH, REPORT_PATH)
        raw = COMPARATOR.canonical_json_bytes(result)
        self.assertTrue(raw.endswith(b"\n"))
        self.assertEqual(raw, COMPARATOR.canonical_json_bytes(json.loads(raw)))
        COMPARATOR.assert_path_free(result, "comparison")
        self.assertNotIn(str(REPOSITORY_ROOT).encode(), raw)

    def test_suite_schema_kind_and_fixed_order_are_strict(self) -> None:
        for mutate in (
            lambda value: value.__setitem__("schemaVersion", 2),
            lambda value: value.__setitem__("kind", "other"),
            lambda value: value["items"].__setitem__(
                slice(0, 2), list(reversed(value["items"][0:2]))
            ),
            lambda value: value["corpus"]["safeCaseIds"].__setitem__(0, "other"),
        ):
            with self.subTest(mutate=mutate):
                suite = copy.deepcopy(self.suite)
                mutate(suite)
                with self.assertRaises(COMPARATOR.ComparisonError):
                    self.compare_documents(suite, self.report)

    def test_report_schema_kind_and_fixed_order_are_strict(self) -> None:
        for mutate in (
            lambda value: value.__setitem__("schemaVersion", 2),
            lambda value: value.__setitem__("kind", "other"),
            lambda value: value["cases"][0].__setitem__("manifestOrder", 2),
            lambda value: value["cases"][0].__setitem__("fixtureId", "other"),
        ):
            with self.subTest(mutate=mutate):
                report = copy.deepcopy(self.report)
                mutate(report)
                with self.assertRaises(COMPARATOR.ComparisonError):
                    self.compare_documents(self.suite, report)

    def test_absolute_or_private_paths_are_rejected(self) -> None:
        suite = copy.deepcopy(self.suite)
        suite["limitations"].append("/Users/private/input.wav")
        with self.assertRaises(COMPARATOR.ComparisonError):
            self.compare_documents(suite, self.report)

        report = copy.deepcopy(self.report)
        report["cases"][0]["path"] = "C:\\private\\input.wav"
        with self.assertRaises(COMPARATOR.ComparisonError):
            self.compare_documents(self.suite, report)

    def test_every_core_item_must_be_successful(self) -> None:
        suite = copy.deepcopy(self.suite)
        suite["items"][0]["result"] = {
            "kind": "error",
            "stage": "worker",
            "code": "synthetic",
            "workerCode": None,
        }
        suite["summary"] = {
            "status": "partial",
            "total": 39,
            "succeeded": 38,
            "failed": 1,
        }
        with self.assertRaises(COMPARATOR.ComparisonError):
            self.compare_documents(suite, self.report)

    def test_input_result_and_channel_geometry_are_cross_checked(self) -> None:
        mutations = [
            lambda value: value["items"][0]["input"].__setitem__(
                "sourceSha256", "0" * 64
            ),
            lambda value: value["items"][0]["input"].__setitem__(
                "sampleRateHz", 8001
            ),
            lambda value: value["items"][0]["input"].__setitem__(
                "pcmByteLength", 8
            ),
            lambda value: value["items"][0]["result"]["data"].__setitem__(
                "frames", 1
            ),
            lambda value: value["items"][0]["result"]["data"][
                "channelResults"
            ][0].__setitem__("index", 1),
            lambda value: value["items"][0]["result"]["data"][
                "channelResults"
            ].clear(),
        ]
        for mutate in mutations:
            with self.subTest(mutate=mutate):
                suite = copy.deepcopy(self.suite)
                mutate(suite)
                with self.assertRaises(COMPARATOR.ComparisonError):
                    self.compare_documents(suite, self.report)

    def test_all_compared_raw_bits_must_be_finite_nonnegative_binary32(self) -> None:
        mutations = [
            ("trackDrBits", "7f800000"),
            ("trackDrBits", "bf800000"),
        ]
        for key, bits in mutations:
            with self.subTest(key=key, bits=bits):
                suite = copy.deepcopy(self.suite)
                suite["items"][0]["result"]["data"][key] = bits
                with self.assertRaises(COMPARATOR.ComparisonError):
                    self.compare_documents(suite, self.report)

        for key, bits in (
            ("drBits", "7fc00000"),
            ("drBits", "bf800000"),
            ("peakBits", "ff800000"),
            ("peakBits", "bf800000"),
            ("rmsBits", "7f800000"),
            ("rmsBits", "bf800000"),
        ):
            with self.subTest(key=key, bits=bits):
                suite = copy.deepcopy(self.suite)
                suite["items"][0]["result"]["data"]["channelResults"][0][key] = bits
                with self.assertRaises(COMPARATOR.ComparisonError):
                    self.compare_documents(suite, self.report)

    def test_metric_difference_is_reported_without_claiming_parity(self) -> None:
        report = copy.deepcopy(self.report)
        report["cases"][0]["trackDr"] += 1
        result = self.compare_documents(self.suite, report)
        self.assertEqual(result["summary"]["status"], "different")
        self.assertEqual(result["summary"]["trackDrMatched"], 38)
        self.assertEqual(result["summary"]["differenceCount"], 1)
        self.assertEqual(result["differences"][0]["field"], "trackDr")
        self.assertEqual(result["claims"]["foobarParity"], "not_assessed")

    def test_cli_writes_canonical_output_and_returns_zero(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "comparison.json"
            completed = subprocess.run(
                [
                    sys.executable,
                    str(TOOL_PATH),
                    "--core-suite",
                    str(SUITE_PATH),
                    "--normalized-report",
                    str(REPORT_PATH),
                    "--output",
                    str(output),
                ],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertEqual(completed.returncode, 0)
            self.assertEqual(completed.stdout, b"")
            self.assertEqual(completed.stderr, b"")
            self.assertEqual(
                output.read_bytes(),
                COMPARATOR.canonical_json_bytes(json.loads(output.read_bytes())),
            )


class StrictJsonTests(unittest.TestCase):
    def test_duplicate_keys_and_nonfinite_constants_are_rejected(self) -> None:
        for raw in (
            b'{"schemaVersion":1,"schemaVersion":1}',
            b'{"value":NaN}',
            b'{"value":Infinity}',
        ):
            with self.subTest(raw=raw):
                with tempfile.TemporaryDirectory() as directory:
                    path = Path(directory) / "input.json"
                    path.write_bytes(raw)
                    with self.assertRaises(COMPARATOR.ComparisonError):
                        COMPARATOR.load_json_bytes(path)


if __name__ == "__main__":
    unittest.main()
