from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def load_script(name: str, filename: str):
    specification = importlib.util.spec_from_file_location(
        name, ROOT / "scripts" / filename
    )
    assert specification is not None and specification.loader is not None
    module = importlib.util.module_from_spec(specification)
    sys.modules[name] = module
    specification.loader.exec_module(module)
    return module


profile = load_script("run_performance_profile", "run-performance-profile.py")


class PerformanceProfileTests(unittest.TestCase):
    def test_time_profile_parser_resolves_refs_and_filters_scope(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "crates/macinmeter-analysis/src/session.rs"
            xml = root / "time-profile.xml"
            xml.write_text(
                f"""<?xml version="1.0"?>
<trace-query-result>
  <node>
    <row>
      <weight id="1">1000000</weight>
      <tagged-backtrace id="10">
        <frame id="11" name="macinmeter_analysis::session::AnalyzerSession::validate_numeric_safety::h0123456789abcdef">
          <binary id="20" name="m6_baseline_worker"/>
          <source line="130"><path id="30">{source}</path></source>
        </frame>
        <frame id="12" name="m6_baseline_worker::timed_analysis_workload::hfedcba9876543210">
          <binary ref="20"/>
          <source line="118"><path ref="30"/></source>
        </frame>
      </tagged-backtrace>
    </row>
    <row>
      <weight ref="1"/>
      <tagged-backtrace id="13">
        <frame ref="11"/>
        <frame ref="12"/>
      </tagged-backtrace>
    </row>
    <row>
      <weight ref="1"/>
      <tagged-backtrace id="14">
        <frame id="15" name="dyld4::start">
          <binary id="21" name="dyld"/>
        </frame>
      </tagged-backtrace>
    </row>
  </node>
</trace-query-result>
""",
                encoding="utf-8",
            )

            parsed = profile.TimeProfileParser(
                root, "m6_baseline_worker::timed_analysis_workload"
            ).parse(xml)

            self.assertEqual(parsed["rows"], 3)
            self.assertEqual(parsed["rowsWithStack"], 3)
            self.assertEqual(parsed["scopedSamples"], 2)
            self.assertEqual(parsed["scopedWeightNs"], 2_000_000)
            self.assertEqual(
                parsed["projectLeafFunctions"][0]["function"],
                "macinmeter_analysis::session::AnalyzerSession::validate_numeric_safety",
            )
            self.assertEqual(
                parsed["projectLeafFunctions"][0]["source"],
                "$REPO/crates/macinmeter-analysis/src/session.rs",
            )
            self.assertEqual(
                parsed["foldedStacks"][0]["stack"],
                [
                    "m6_baseline_worker::timed_analysis_workload",
                    "macinmeter_analysis::session::AnalyzerSession::validate_numeric_safety",
                ],
            )
            self.assertEqual(
                parsed["leafCategories"][0]["category"], "macinmeter_analysis"
            )
            self.assertEqual(
                parsed["leafCategories"][0]["percentOfScopedWeight"], 100.0
            )

    def test_parser_rejects_unresolved_reference(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            xml = Path(temporary) / "bad.xml"
            xml.write_text(
                """<trace-query-result><node><row><weight>1</weight>
<tagged-backtrace ref="missing"/></row></node></trace-query-result>""",
                encoding="utf-8",
            )
            with self.assertRaises(profile.ProfileError):
                profile.TimeProfileParser(
                    Path(temporary), "m6_baseline_worker::timed_analysis_workload"
                ).parse(xml)

    def test_trace_toc_binds_profiler_configuration(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            toc = Path(temporary) / "toc.xml"
            toc.write_text(
                """<trace-toc><run number="1"><info><target>
<process name="worker" pid="42" arguments="analysis" return-exit-status="0"
termination-reason="exit(0)"/></target><summary>
<start-date>2026-07-20T00:00:00Z</start-date>
<end-date>2026-07-20T00:00:05Z</end-date>
<duration>5.0</duration><end-reason>Target app exited</end-reason>
<instruments-version>16.0</instruments-version>
<template-name>Time Profiler</template-name>
<recording-mode>Deferred</recording-mode>
</summary></info><data>
<table schema="time-profile" high-frequency-sampling="0"
context-switch-sampling="0" record-waiting-threads="0"
needs-kernel-callstack="0"/>
<table schema="time-sample" sample-rate-micro-seconds="1000"/>
</data></run></trace-toc>""",
                encoding="utf-8",
            )
            parsed = profile.parse_trace_toc(toc)
            self.assertEqual(parsed["target"]["returnExitStatus"], 0)
            self.assertEqual(parsed["recording"]["template"], "Time Profiler")
            self.assertEqual(parsed["recording"]["sampleIntervalNs"], 1_000_000)
            case = profile.ProfileCase(
                "analysis/test",
                "analysis",
                "test",
                ("analysis",),
                "anchor",
            )
            profile.validate_trace_configuration(case, parsed)
            parsed["recording"]["sampleIntervalNs"] = 2_000_000
            with self.assertRaises(profile.ProfileError):
                profile.validate_trace_configuration(case, parsed)

    def test_profile_coverage_is_bound_to_worker_elapsed_time(self) -> None:
        case = profile.ProfileCase(
            "analysis/test",
            "analysis",
            "test",
            ("analysis", "2", "48000", "1", "1"),
            "anchor",
        )
        parsed = {"scopedSamples": 1_000, "scopedWeightNs": 1_000_000_000}
        output = {"workerElapsedNs": 1_010_000_000}
        profile.validate_profile_coverage(case, parsed, output)
        self.assertAlmostEqual(
            parsed["scopedWeightToWorkerElapsedRatio"], 1 / 1.01
        )

        with self.assertRaises(profile.ProfileError):
            profile.validate_profile_coverage(
                case,
                {"scopedSamples": 999, "scopedWeightNs": 1_000_000_000},
                output,
            )
        with self.assertRaises(profile.ProfileError):
            profile.validate_profile_coverage(
                case,
                {"scopedSamples": 1_000, "scopedWeightNs": 500_000_000},
                output,
            )

    def test_merge_profiles_retains_folded_sample_weights(self) -> None:
        def capture(weight: int) -> dict[str, object]:
            entry = {
                "function": "hot",
                "binary": "worker",
                "source": "$REPO/source.rs",
                "line": 1,
                "samples": 1,
                "weightNs": weight,
                "percentOfScopedWeight": 100.0,
            }
            return {
                "workerOutput": {"workerElapsedNs": weight},
                "profile": {
                    "rows": 1,
                    "rowsWithStack": 1,
                    "allWeightNs": weight,
                    "scopedSamples": 1,
                    "scopedWeightNs": weight,
                    "scopedWeightToWorkerElapsedRatio": 1.0,
                    "leafFunctions": [entry],
                    "projectLeafFunctions": [entry],
                    "inclusiveFunctions": [entry],
                    "leafCategories": [
                        {
                            "category": "other",
                            "samples": 1,
                            "weightNs": weight,
                            "percentOfScopedWeight": 100.0,
                        }
                    ],
                    "projectSourceLines": [
                        {
                            "source": "$REPO/source.rs",
                            "line": 1,
                            "function": "hot",
                            "samples": 1,
                            "weightNs": weight,
                            "percentOfScopedWeight": 100.0,
                        }
                    ],
                    "foldedStacks": [
                        {
                            "stack": ["anchor", "hot"],
                            "samples": 1,
                            "weightNs": weight,
                            "percentOfScopedWeight": 100.0,
                        }
                    ],
                },
            }

        merged = profile.merge_profiles([capture(1_000_000), capture(2_000_000)])
        self.assertEqual(merged["captures"], 2)
        self.assertEqual(merged["scopedWeightNs"], 3_000_000)
        self.assertEqual(merged["leafFunctions"][0]["samples"], 2)
        self.assertEqual(merged["leafFunctions"][0]["weightNs"], 3_000_000)
        self.assertEqual(merged["foldedStacks"][0]["stack"], ["anchor", "hot"])

    def test_tree_hash_binds_relative_paths_and_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "a").mkdir()
            (root / "a/data").write_bytes(b"one")
            first = profile.sha256_tree(root)
            (root / "a/data").write_bytes(b"two")
            second = profile.sha256_tree(root)
            self.assertNotEqual(first[0], second[0])
            self.assertEqual(first[1:], second[1:])

    def test_flac_bundle_symbols_have_a_specific_leaf_category(self) -> None:
        frame = profile.Frame(
            "symphonia_bundle_flac::decoder::lpc_predict",
            "m6_baseline_worker",
            None,
            None,
        )
        self.assertEqual(profile.category_for_frame(frame), "symphonia_flac")


if __name__ == "__main__":
    unittest.main()
