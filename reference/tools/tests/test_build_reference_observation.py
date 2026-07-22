#!/usr/bin/env python3

from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path
from typing import Any


TOOL_PATH = (
    Path(__file__).resolve().parents[1] / "build_reference_observation.py"
)
SPEC = importlib.util.spec_from_file_location("reference_observation_harness", TOOL_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot import {TOOL_PATH}")
HARNESS = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = HARNESS
SPEC.loader.exec_module(HARNESS)


def sha256(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def json_bytes(value: Any) -> bytes:
    return (
        json.dumps(
            value,
            ensure_ascii=False,
            sort_keys=True,
            indent=2,
            allow_nan=False,
        )
        + "\n"
    ).encode()


def report_bytes(stems: list[str] | None = None) -> bytes:
    ordered_stems = stems or ["001_alpha", "002_beta"]
    rows = {
        "001_alpha": (
            "DR12 0.00 dBFS -12.04 dBFS 0:03 "
            "?-001_alpha 12.04 dB -12.04 dBFS"
        ),
        "002_beta": (
            "DR9 -0.07 dBFS -9.03 dBFS 0:06 "
            "?-002_beta 8.96 dB -9.03 dBFS"
        ),
    }
    lines = [
        "foobar2000 v2.25.10 / DR Meter v1.0.8",
        "log date: 2026-07-18 17:54:55",
        "",
        *(rows[stem] for stem in ordered_stems),
        "",
        "Number of tracks: 2",
        "Official DR value: DR11",
        "Samplerate: 8000 Hz",
        "Channels: 1",
        "Bits per sample: 32",
        "Bitrate: 256 kbps",
        "Codec: PCM (floating-point)",
        "",
    ]
    return "\r\n".join(lines).encode("ascii")


class SyntheticInputs:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.corpus = root / "corpus"
        self.corpus.mkdir()
        fixture_values = {
            "01-core/001_alpha.wav": b"synthetic-wave-alpha",
            "01-core/002_beta.wav": b"synthetic-wave-beta",
        }
        cases = []
        for order, (relative, value) in enumerate(fixture_values.items(), 1):
            path = self.corpus / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(value)
            cases.append(
                {
                    "id": "alpha" if order == 1 else "beta",
                    "order": order,
                    "path": relative,
                    "channels": 1,
                    "byteLength": len(value),
                    "fileSha256": sha256(value),
                }
            )
        self.manifest_value = {
            "schemaVersion": 2,
            "corpusId": "synthetic-complete-v2",
            "generator": {
                "name": "synthetic_generator.py",
                "sourceSha256": "a" * 64,
            },
            "budgets": {"expectedSafeMasterEntries": 2},
            "isolatedFixtureIds": [],
            "playlists": {"00-safe-master": ["alpha", "beta"]},
            "cases": cases,
        }
        self.manifest = (
            root
            / "reference/fixtures/synthetic-complete-v2.manifest.json"
        )
        self.manifest.parent.mkdir(parents=True)
        generated_manifest = self.corpus / "manifest.json"
        manifest_raw = json_bytes(self.manifest_value)
        self.manifest.write_bytes(manifest_raw)
        generated_manifest.write_bytes(manifest_raw)
        playlist = self.corpus / "playlists/00-safe-master.m3u8"
        playlist.parent.mkdir()
        playlist_raw = (
            "#EXTM3U\n"
            "../01-core/001_alpha.wav\n"
            "../01-core/002_beta.wav\n"
        ).encode("utf-8")
        playlist.write_bytes(playlist_raw)
        checksum_lines = [f"{sha256(manifest_raw)}  manifest.json"]
        checksum_lines.extend(
            f"{case['fileSha256']}  {case['path']}" for case in cases
        )
        checksum_lines.append(
            f"{sha256(playlist_raw)}  playlists/00-safe-master.m3u8"
        )
        self.files_sha = self.corpus / "FILES.sha256"
        files_sha_raw = ("\n".join(checksum_lines) + "\n").encode("ascii")
        self.files_sha.write_bytes(files_sha_raw)
        self.report = root / "source-report.txt"
        raw_report = report_bytes()
        self.report.write_bytes(raw_report)
        self.capture_value = {
            "schemaVersion": 1,
            "kind": "foo_dr_meter_observation_capture",
            "observationId": "OBS-synthetic-x64-safe-master-run2-20260718",
            "status": "safe_master_repeat",
            "target": {
                "id": "TARGET-synthetic-x64",
                "architecture": "x86_64",
                "expectedHeader": {
                    "foobar2000Version": "2.25.10",
                    "drMeterVersion": "1.0.8",
                },
                "identities": [
                    {
                        "role": "foo_dr_meter",
                        "version": "1.0.8",
                        "architecture": "x86_64",
                        "sha256": "1" * 64,
                        "byteLength": 101,
                        "binding": "fixed_target_record",
                    },
                    {
                        "role": "foobar2000",
                        "version": "2.25.10",
                        "architecture": "x86_64",
                        "sha256": "2" * 64,
                        "byteLength": 102,
                        "binding": "fixed_target_record",
                    },
                    {
                        "role": "foo_input_std",
                        "version": "fixed-install",
                        "architecture": "x86_64",
                        "sha256": "3" * 64,
                        "byteLength": 103,
                        "binding": "fixed_target_record",
                    },
                ],
            },
            "experimentId": "EXP-synthetic-complete-v2",
            "corpus": {
                "id": "synthetic-complete-v2",
                "repositoryManifestPath": (
                    "reference/fixtures/synthetic-complete-v2.manifest.json"
                ),
                "manifestSha256": sha256(manifest_raw),
                "filesSha256FileSha256": sha256(files_sha_raw),
                "playlist": "00-safe-master",
            },
            "run": {
                "repeat": 2,
                "repeatOfObservationId": "OBS-synthetic-x64-safe-master-run1",
                "procedure": (
                    "foobar2000 GUI Measure Dynamic Range followed by Save Log"
                ),
                "timezone": {
                    "name": "China Standard Time",
                    "utcOffset": "+08:00",
                },
                "settings": {
                    "automaticallySaveTags": {
                        "value": False,
                        "source": "operator_attested",
                    },
                    "stereoPerChannelStats": {
                        "value": True,
                        "source": "operator_attested_and_report_corroborated",
                    },
                    "albumLengthWeighting": {
                        "value": False,
                        "source": "operator_attested",
                    },
                    "multichannelLoudnessWeighting": {
                        "value": False,
                        "source": "operator_attested",
                    },
                },
                "operatorNotes": "none",
                "repeatConsistency": "not_assessed_by_this_import",
            },
            "rawReport": {
                "group": "safe-master",
                "outputName": "x64-run2-safe-master.txt",
                "sha256": sha256(raw_report),
                "byteLength": len(raw_report),
            },
            "limitations": [
                "Only exported final report fields are dynamically observable."
            ],
            "claims": {
                "scope": "safe-master exported report text for this fixed target",
                "appliesToVersion": (
                    "foo_dr_meter 1.0.8 x64 under foobar2000 2.25.10 only"
                ),
            },
        }
        self.metadata = root / "capture-input.json"
        self.metadata.write_bytes(json_bytes(self.capture_value))


class ReferenceObservationHarnessTests(unittest.TestCase):
    def test_build_is_deterministic_and_verify_reconstructs_package(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            inputs = SyntheticInputs(Path(directory))
            first = inputs.root / "package-one"
            second = inputs.root / "package-two"
            first_summary = HARNESS.build_package(
                inputs.metadata,
                inputs.manifest,
                inputs.corpus,
                inputs.report,
                first,
                repository_root=inputs.root,
            )
            second_summary = HARNESS.build_package(
                inputs.metadata,
                inputs.manifest,
                inputs.corpus,
                inputs.report,
                second,
                repository_root=inputs.root,
            )
            self.assertEqual(first_summary, second_summary)
            first_files = {
                path.relative_to(first).as_posix(): path.read_bytes()
                for path in first.rglob("*")
                if path.is_file()
            }
            second_files = {
                path.relative_to(second).as_posix(): path.read_bytes()
                for path in second.rglob("*")
                if path.is_file()
            }
            self.assertEqual(first_files, second_files)
            self.assertEqual(
                set(first_files),
                {
                    "capture.json",
                    "observation.json",
                    "normalized/x64-run2-safe-master.json",
                    "raw/x64-run2-safe-master.txt",
                },
            )
            verify_summary = HARNESS.verify_package(
                first,
                inputs.manifest,
                inputs.corpus,
                repository_root=inputs.root,
            )
            self.assertEqual(first_summary, verify_summary)
            for value in first_files.values():
                rendered = value.decode("ascii", errors="ignore")
                self.assertNotIn(directory, rendered)
                self.assertNotIn("/Users/", rendered)

    def test_fixture_hash_mismatch_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            inputs = SyntheticInputs(Path(directory))
            (inputs.corpus / "01-core/001_alpha.wav").write_bytes(b"tampered")
            with self.assertRaisesRegex(HARNESS.HarnessError, "fixture SHA-256"):
                HARNESS.build_package(
                    inputs.metadata,
                    inputs.manifest,
                    inputs.corpus,
                    inputs.report,
                    inputs.root / "package",
                    repository_root=inputs.root,
                )

    def test_raw_report_hash_mismatch_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            inputs = SyntheticInputs(Path(directory))
            inputs.report.write_bytes(inputs.report.read_bytes() + b"x")
            with self.assertRaisesRegex(HARNESS.HarnessError, "raw report SHA-256"):
                HARNESS.build_package(
                    inputs.metadata,
                    inputs.manifest,
                    inputs.corpus,
                    inputs.report,
                    inputs.root / "package",
                    repository_root=inputs.root,
                )

    def test_manifest_order_mismatch_in_report_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            inputs = SyntheticInputs(Path(directory))
            swapped = report_bytes(["002_beta", "001_alpha"])
            inputs.report.write_bytes(swapped)
            capture = copy.deepcopy(inputs.capture_value)
            capture["rawReport"]["sha256"] = sha256(swapped)
            capture["rawReport"]["byteLength"] = len(swapped)
            inputs.metadata.write_bytes(json_bytes(capture))
            with self.assertRaisesRegex(
                HARNESS.HarnessError, "expected '001_alpha'"
            ):
                HARNESS.build_package(
                    inputs.metadata,
                    inputs.manifest,
                    inputs.corpus,
                    inputs.report,
                    inputs.root / "package",
                    repository_root=inputs.root,
                )

    def test_report_stem_requires_an_exact_token_boundary(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            inputs = SyntheticInputs(Path(directory))
            changed = inputs.report.read_bytes().replace(
                b"?-001_alpha ",
                b"?-001_alphaX ",
            )
            inputs.report.write_bytes(changed)
            capture = copy.deepcopy(inputs.capture_value)
            capture["rawReport"]["sha256"] = sha256(changed)
            capture["rawReport"]["byteLength"] = len(changed)
            inputs.metadata.write_bytes(json_bytes(capture))
            with self.assertRaisesRegex(
                HARNESS.HarnessError, "expected '001_alpha'"
            ):
                HARNESS.build_package(
                    inputs.metadata,
                    inputs.manifest,
                    inputs.corpus,
                    inputs.report,
                    inputs.root / "package",
                    repository_root=inputs.root,
                )

    def test_report_header_must_match_fixed_target(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            inputs = SyntheticInputs(Path(directory))
            capture = copy.deepcopy(inputs.capture_value)
            capture["target"]["expectedHeader"]["foobar2000Version"] = "2.0"
            capture["target"]["identities"][1]["version"] = "2.0"
            inputs.metadata.write_bytes(json_bytes(capture))
            with self.assertRaisesRegex(
                HARNESS.HarnessError, "foobar2000 version differs"
            ):
                HARNESS.build_package(
                    inputs.metadata,
                    inputs.manifest,
                    inputs.corpus,
                    inputs.report,
                    inputs.root / "package",
                    repository_root=inputs.root,
                )

    def test_normalization_must_describe_the_validated_byte_snapshots(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            inputs = SyntheticInputs(Path(directory))
            original_normalize = HARNESS.NORMALIZER.normalize

            def swap_report_before_normalizing(
                report_path: Path, manifest_path: Path, playlist: str
            ) -> dict[str, Any]:
                changed = report_path.read_bytes().replace(
                    b"2026-07-18 17:54:55",
                    b"2026-07-18 17:54:56",
                )
                report_path.write_bytes(changed)
                return original_normalize(report_path, manifest_path, playlist)

            with mock.patch.object(
                HARNESS.NORMALIZER,
                "normalize",
                side_effect=swap_report_before_normalizing,
            ):
                with self.assertRaisesRegex(
                    HARNESS.HarnessError,
                    "normalized report source rawReportSha256 differs",
                ):
                    HARNESS.build_package(
                        inputs.metadata,
                        inputs.manifest,
                        inputs.corpus,
                        inputs.report,
                        inputs.root / "package",
                        repository_root=inputs.root,
                    )

    def test_manifest_input_must_be_the_declared_repository_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            inputs = SyntheticInputs(Path(directory))
            alternate = inputs.root / "alternate" / inputs.manifest.name
            alternate.parent.mkdir()
            alternate.write_bytes(inputs.manifest.read_bytes())
            with self.assertRaisesRegex(
                HARNESS.HarnessError, "not the declared canonical repository manifest"
            ):
                HARNESS.build_package(
                    inputs.metadata,
                    alternate,
                    inputs.corpus,
                    inputs.report,
                    inputs.root / "package",
                    repository_root=inputs.root,
                )

    def test_private_path_in_capture_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            inputs = SyntheticInputs(Path(directory))
            capture = copy.deepcopy(inputs.capture_value)
            capture["run"]["operatorNotes"] = (
                "report copied from C:\\Users\\operator\\Desktop"
            )
            inputs.metadata.write_bytes(json_bytes(capture))
            with self.assertRaisesRegex(
                HARNESS.HarnessError, "absolute/private path"
            ):
                HARNESS.build_package(
                    inputs.metadata,
                    inputs.manifest,
                    inputs.corpus,
                    inputs.report,
                    inputs.root / "package",
                    repository_root=inputs.root,
                )

    def test_private_path_variants_and_nonportable_output_names_are_rejected(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            inputs = SyntheticInputs(Path(directory))
            for private_path in (
                "/etc",
                "~/report.txt",
                "file:///tmp/report.txt",
                "\\\\server\\share\\report.txt",
            ):
                with self.subTest(private_path=private_path):
                    capture = copy.deepcopy(inputs.capture_value)
                    capture["run"]["operatorNotes"] = private_path
                    inputs.metadata.write_bytes(json_bytes(capture))
                    with self.assertRaisesRegex(
                        HARNESS.HarnessError, "absolute/private path"
                    ):
                        HARNESS.build_package(
                            inputs.metadata,
                            inputs.manifest,
                            inputs.corpus,
                            inputs.report,
                            inputs.root / f"path-package-{len(private_path)}",
                            repository_root=inputs.root,
                        )

            for output_name in ("CON.txt", "report."):
                with self.subTest(output_name=output_name):
                    capture = copy.deepcopy(inputs.capture_value)
                    capture["rawReport"]["outputName"] = output_name
                    inputs.metadata.write_bytes(json_bytes(capture))
                    with self.assertRaisesRegex(
                        HARNESS.HarnessError, "portable file basename"
                    ):
                        HARNESS.build_package(
                            inputs.metadata,
                            inputs.manifest,
                            inputs.corpus,
                            inputs.report,
                            inputs.root / f"name-package-{len(output_name)}",
                            repository_root=inputs.root,
                        )

    def test_import_rejects_undeclared_claims_and_repeat_consistency(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            inputs = SyntheticInputs(Path(directory))
            capture = copy.deepcopy(inputs.capture_value)
            capture["claims"]["status"] = "verified"
            inputs.metadata.write_bytes(json_bytes(capture))
            with self.assertRaisesRegex(
                HARNESS.HarnessError, "capture.claims"
            ):
                HARNESS.build_package(
                    inputs.metadata,
                    inputs.manifest,
                    inputs.corpus,
                    inputs.report,
                    inputs.root / "extra-claim-package",
                    repository_root=inputs.root,
                )

            capture["claims"].pop("status")
            capture["run"]["repeatConsistency"] = "identical"
            inputs.metadata.write_bytes(json_bytes(capture))
            with self.assertRaisesRegex(
                HARNESS.HarnessError,
                "repeatConsistency must remain 'not_assessed_by_this_import'",
            ):
                HARNESS.build_package(
                    inputs.metadata,
                    inputs.manifest,
                    inputs.corpus,
                    inputs.report,
                    inputs.root / "repeat-package",
                    repository_root=inputs.root,
                )

    def test_verify_rejects_tampered_or_extra_package_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            inputs = SyntheticInputs(Path(directory))
            package = inputs.root / "package"
            HARNESS.build_package(
                inputs.metadata,
                inputs.manifest,
                inputs.corpus,
                inputs.report,
                package,
                repository_root=inputs.root,
            )
            observation_path = package / "observation.json"
            original = observation_path.read_bytes()
            observation_path.write_bytes(original.replace(b'"offline": true', b'"offline": false'))
            with self.assertRaisesRegex(
                HARNESS.HarnessError, "differs from reconstruction"
            ):
                HARNESS.verify_package(
                    package,
                    inputs.manifest,
                    inputs.corpus,
                    repository_root=inputs.root,
                )
            observation_path.write_bytes(original)
            (package / "unexpected.txt").write_text("unexpected", encoding="ascii")
            with self.assertRaisesRegex(HARNESS.HarnessError, "package path set"):
                HARNESS.verify_package(
                    package,
                    inputs.manifest,
                    inputs.corpus,
                    repository_root=inputs.root,
                )


if __name__ == "__main__":
    unittest.main()
