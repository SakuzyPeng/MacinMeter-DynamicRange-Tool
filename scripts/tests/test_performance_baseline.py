from __future__ import annotations

import importlib.util
import json
import math
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


baseline = load_script("run_performance_baseline", "run-performance-baseline.py")
corpus = load_script("generate_performance_corpus", "generate-performance-corpus.py")


class PerformanceBaselineTests(unittest.TestCase):
    def test_suite_pairs_long_flac_decode_and_application_cases(self) -> None:
        cases = baseline.suite_cases(Path("/corpus"))
        case_ids = {case.case_id for case in cases}
        for track in (
            "flac-s24-240s",
            "flac-s24-tonal-240s",
            "flac-s16-240s",
        ):
            self.assertIn(f"application/{track}", case_ids)
            for workers in baseline.PACKET_WORKER_COUNTS:
                self.assertIn(f"decode/{track}-w{workers}", case_ids)

    def test_seeded_schedule_is_deterministic_and_balanced(self) -> None:
        cases = (
            baseline.BenchmarkCase("a", "analysis", "a", ("analysis",)),
            baseline.BenchmarkCase("b", "decode", "b", ("decode",)),
        )
        first = baseline.randomized_schedule(cases, ("scalar", "candidate"), 3, 42)
        second = baseline.randomized_schedule(cases, ("scalar", "candidate"), 3, 42)
        self.assertEqual(first, second)
        self.assertNotEqual(
            first,
            baseline.randomized_schedule(cases, ("scalar", "candidate"), 3, 43),
        )
        counts: dict[tuple[str, str], int] = {}
        for case, variant, _ in first:
            key = (case.case_id, variant)
            counts[key] = counts.get(key, 0) + 1
        self.assertEqual(set(counts.values()), {3})

    def test_darwin_time_parser_preserves_native_counters(self) -> None:
        parsed = baseline.parse_darwin_time(
            """        0.14 real         0.13 user         0.01 sys
             2342912  maximum resident set size
                   2  page faults
          3591349272  instructions retired
           558769659  cycles elapsed
             1491376  peak memory footprint
"""
        )
        self.assertEqual(parsed["realSeconds"], 0.14)
        self.assertEqual(parsed["maxResidentSetBytes"], 2_342_912)
        self.assertEqual(parsed["instructionsRetired"], 3_591_349_272)
        self.assertEqual(parsed["cyclesElapsed"], 558_769_659)

    def test_process_tree_rss_excludes_measurement_wrapper(self) -> None:
        rows = {
            10: (1, 1_000),
            11: (10, 2_000),
            12: (11, 3_000),
            13: (99, 9_000),
        }
        rss, processes = baseline.descendant_rss(rows, 10)
        self.assertEqual(rss, 5_000)
        self.assertEqual(processes, 2)

    def test_worker_output_rejects_nonfinite_and_protocol_drift(self) -> None:
        valid = {
            "schemaVersion": 1,
            "mode": "analysis",
            "workerElapsedNs": 10,
            "work": {
                "iterations": 1,
                "audioFrames": 1,
                "interleavedSamples": 2,
                "audioSeconds": 1.0,
                "logicalItems": 1,
            },
            "resultFingerprintSha256": "a" * 64,
            "resultBytes": 1,
            "details": {},
        }
        parsed = baseline.parse_worker_output(
            json.dumps(valid).encode("utf-8"), "analysis"
        )
        self.assertEqual(parsed["workerElapsedNs"], 10)

        invalid = dict(valid)
        invalid["details"] = {"bad": math.nan}
        with self.assertRaises(baseline.BaselineError):
            baseline.parse_worker_output(
                json.dumps(invalid).encode("utf-8"), "analysis"
            )
        invalid = dict(valid)
        invalid["schemaVersion"] = 2
        with self.assertRaises(baseline.BaselineError):
            baseline.parse_worker_output(
                json.dumps(invalid).encode("utf-8"), "analysis"
            )

    def test_distribution_retains_exact_sample_extent(self) -> None:
        result = baseline.distribution([1.0, 2.0, 100.0])
        self.assertEqual(result["min"], 1.0)
        self.assertEqual(result["median"], 2.0)
        self.assertEqual(result["max"], 100.0)
        self.assertEqual(result["medianAbsoluteDeviation"], 1.0)

    def test_summary_rejects_result_drift(self) -> None:
        def sample(fingerprint: str) -> dict[str, object]:
            return {
                "caseId": "analysis/a",
                "variant": "scalar",
                "workerElapsedNs": 100,
                "processTree": {"peakRssBytes": 200},
                "nativeMetrics": {
                    "maxResidentSetBytes": 210,
                    "userSeconds": 0.1,
                    "systemSeconds": 0.0,
                },
                "work": {
                    "audioSeconds": 1.0,
                    "interleavedSamples": 2,
                    "logicalItems": 1,
                },
                "details": {},
                "resultFingerprintSha256": fingerprint,
            }

        with self.assertRaises(baseline.BaselineError):
            baseline.summarize_samples([sample("a" * 64), sample("b" * 64)])

    def test_cross_variant_gate_requires_identical_fingerprint(self) -> None:
        samples = [
            {
                "caseId": "analysis/a",
                "variant": "scalar",
                "resultFingerprintSha256": "a" * 64,
            },
            {
                "caseId": "analysis/a",
                "variant": "candidate",
                "resultFingerprintSha256": "b" * 64,
            },
        ]
        with self.assertRaises(baseline.BaselineError):
            baseline.validate_cross_variant_fingerprints(samples)

    def test_corpus_gate_binds_decode_work_and_pcm_oracle(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            case = baseline.BenchmarkCase(
                "decode/wave",
                "decode",
                "decode",
                ("decode", str(root / "input.wav"), "2"),
            )
            manifest = {
                "media": [
                    {
                        "path": "input.wav",
                        "frames": 48_000,
                        "channels": 2,
                        "sampleRate": 48_000,
                        "normalizedInterleavedF64LeSha256": "a" * 64,
                    }
                ]
            }
            sample = {
                "caseId": case.case_id,
                "work": {
                    "audioFrames": 96_000,
                    "interleavedSamples": 192_000,
                    "audioSeconds": 2.0,
                    "logicalItems": 2,
                },
                "details": {
                    "pcmF64LeSha256": "a" * 64,
                    "decodeWorkers": 1,
                    "decodeQueueCapacity": 1,
                    "decodeMaxInFlightPcmBytes": 0,
                },
            }
            baseline.validate_corpus_work([sample], [case], root, manifest)
            sample["details"] = {
                "pcmF64LeSha256": "b" * 64,
                "decodeWorkers": 1,
                "decodeQueueCapacity": 1,
                "decodeMaxInFlightPcmBytes": 0,
            }
            with self.assertRaises(baseline.BaselineError):
                baseline.validate_corpus_work([sample], [case], root, manifest)

    def test_decode_allocation_gate_matches_the_application_plan(self) -> None:
        def case(workers: int | None) -> baseline.BenchmarkCase:
            arguments = ["decode", "/corpus/input.m4a", "1"]
            if workers is not None:
                arguments.append(str(workers))
            return baseline.BenchmarkCase(
                "decode/alac", "decode", "decode", tuple(arguments)
            )

        # The plan grants a serial reservation at one worker, and four queued
        # packets plus 4 MiB of in-flight PCM per worker above that.
        baseline.assert_decode_allocation(
            case(None),
            {
                "decodeWorkers": 1,
                "decodeQueueCapacity": 1,
                "decodeMaxInFlightPcmBytes": 0,
            },
        )
        baseline.assert_decode_allocation(
            case(4),
            {
                "decodeWorkers": 4,
                "decodeQueueCapacity": 16,
                "decodeMaxInFlightPcmBytes": 16 * 1024 * 1024,
            },
        )

        # An explicit queue capacity moves only the queue bound; the in-flight
        # PCM permit must stay on the plan's derivation.
        baseline.assert_decode_allocation(
            baseline.BenchmarkCase(
                "decode/alac", "decode", "decode",
                ("decode", "/corpus/input.m4a", "1", "8", "8"),
            ),
            {
                "decodeWorkers": 8,
                "decodeQueueCapacity": 8,
                "decodeMaxInFlightPcmBytes": 32 * 1024 * 1024,
            },
        )
        with self.assertRaises(baseline.BaselineError):
            baseline.assert_decode_allocation(
                baseline.BenchmarkCase(
                    "decode/alac", "decode", "decode",
                    ("decode", "/corpus/input.m4a", "1", "8", "8"),
                ),
                {
                    "decodeWorkers": 8,
                    "decodeQueueCapacity": 32,
                    "decodeMaxInFlightPcmBytes": 32 * 1024 * 1024,
                },
            )

        # A worker that silently fell back to the serial route, or one whose
        # derivation drifted from the plan, must fail the run.
        for details in (
            {
                "decodeWorkers": 1,
                "decodeQueueCapacity": 1,
                "decodeMaxInFlightPcmBytes": 0,
            },
            {
                "decodeWorkers": 4,
                "decodeQueueCapacity": 8,
                "decodeMaxInFlightPcmBytes": 16 * 1024 * 1024,
            },
            {
                "decodeWorkers": 4,
                "decodeQueueCapacity": 16,
                "decodeMaxInFlightPcmBytes": 4 * 1024 * 1024,
            },
        ):
            with self.assertRaises(baseline.BaselineError):
                baseline.assert_decode_allocation(case(4), details)

    def test_variant_parser_requires_safe_name_and_executable(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            executable = Path(temporary) / "worker"
            executable.write_text("#!/bin/sh\n", encoding="utf-8")
            executable.chmod(0o755)
            name, path = baseline.parse_variant(f"candidate={executable}")
            self.assertEqual(name, "candidate")
            self.assertEqual(path, executable.resolve())
            with self.assertRaises(baseline.BaselineError):
                baseline.parse_variant(f"Bad Name={executable}")
            self.assertEqual(
                baseline.parse_variant_source(f"candidate={'a' * 40}"),
                ("candidate", "a" * 40),
            )
            with self.assertRaises(baseline.BaselineError):
                baseline.parse_variant_source("candidate=not-a-commit")

    def test_generated_pcm_routes_share_normalized_f64_bytes(self) -> None:
        values, normalized = corpus.deterministic_integer_block(
            channels=2, bits=16, seed=1
        )
        self.assertEqual(len(values), corpus.BLOCK_FRAMES * 2)
        self.assertEqual(len(normalized), corpus.BLOCK_FRAMES * 2 * 8)
        digest = corpus.normalized_pcm_sha256(
            normalized, frames=corpus.BLOCK_FRAMES + 1, channels=2
        )
        expected = corpus.hashlib.sha256(
            normalized + normalized[: 2 * 8]
        ).hexdigest()
        self.assertEqual(digest, expected)

    def test_generated_container_headers_pin_geometry(self) -> None:
        wave = corpus.wave_header(
            frames=48_000,
            channels=2,
            sample_rate=48_000,
            bits=16,
            format_tag=1,
        )
        self.assertEqual(wave[:4], b"RIFF")
        self.assertEqual(wave[8:12], b"WAVE")
        self.assertEqual(len(wave), 44)
        self.assertEqual(int.from_bytes(wave[40:44], "little"), 192_000)

        aiff = corpus.aiff_header(
            frames=48_000,
            channels=2,
            sample_rate=48_000,
            bits=16,
        )
        self.assertEqual(aiff[:4], b"FORM")
        self.assertEqual(aiff[8:12], b"AIFF")
        self.assertEqual(len(aiff), 54)
        self.assertEqual(aiff[28:38], bytes.fromhex("400ebb80000000000000"))

    def test_discovery_fixture_paths_are_unique_and_relative(self) -> None:
        supported, ignored = corpus.discovery_relative_paths()
        combined = [*supported, *ignored]
        self.assertEqual(len(supported), corpus.DISCOVERY_SUPPORTED_FILES)
        self.assertEqual(len(ignored), corpus.DISCOVERY_IGNORED_FILES)
        self.assertEqual(len(set(combined)), len(combined))
        self.assertTrue(all(not path.is_absolute() for path in combined))
        self.assertTrue(all(".." not in path.parts for path in combined))


if __name__ == "__main__":
    unittest.main()
