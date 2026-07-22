from __future__ import annotations

import importlib.util
import io
import json
import os
import tarfile
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "stage-release.py"
SPEC = importlib.util.spec_from_file_location("stage_release", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
stage_release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(stage_release)


class StageReleaseTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_archive_container_is_deterministic_and_preserves_executable(self) -> None:
        payload = self.root / "payload"
        payload.mkdir()
        executable = payload / "macinmeter"
        executable.write_bytes(b"fixed executable bytes\n")
        executable.chmod(0o755)
        (payload / "LICENSE").write_text("license\n", encoding="utf-8")

        first = self.root / "first.tar.gz"
        second = self.root / "second.tar.gz"
        stage_release.write_deterministic_tar_gz(payload, first, "release-root")
        stage_release.write_deterministic_tar_gz(payload, second, "release-root")

        self.assertEqual(
            stage_release.sha256_file(first),
            stage_release.sha256_file(second),
        )
        extracted = stage_release.safe_extract_tar_gz(
            first, self.root / "extracted"
        )
        self.assertEqual((extracted / "LICENSE").read_text(), "license\n")
        self.assertTrue(os.access(extracted / "macinmeter", os.X_OK))

    def test_safe_extraction_rejects_parent_traversal(self) -> None:
        archive = self.root / "unsafe.tar.gz"
        with tarfile.open(archive, "w:gz") as output:
            contents = b"escape"
            member = tarfile.TarInfo("release-root/../../escape")
            member.size = len(contents)
            output.addfile(member, io.BytesIO(contents))

        with self.assertRaisesRegex(stage_release.ReleaseError, "unsafe archive"):
            stage_release.safe_extract_tar_gz(archive, self.root / "destination")
        self.assertFalse((self.root / "escape").exists())

    def test_checksum_parser_rejects_duplicates_and_unsafe_names(self) -> None:
        checksum = self.root / "SHA256SUMS"
        digest = "a" * 64
        checksum.write_text(
            f"{digest}  artifact.tar.gz\n{digest}  artifact.tar.gz\n",
            encoding="utf-8",
        )
        with self.assertRaisesRegex(stage_release.ReleaseError, "duplicate"):
            stage_release.parse_checksums(checksum)

        checksum.write_text(
            f"{digest}  ../artifact.tar.gz\n",
            encoding="utf-8",
        )
        with self.assertRaisesRegex(stage_release.ReleaseError, "invalid"):
            stage_release.parse_checksums(checksum)

    def test_analysis_smoke_pins_parameters_and_route_without_status_fields(self) -> None:
        document = {
            "schemaVersion": 3,
            "toolVersion": "0.2.0",
            "kind": "analysis",
            "data": {
                "source": {"container": "wave", "codec": "pcm_integer"},
                "analysis": {
                    "algorithm": {
                        "parameters": {"histogramBins": 10_001},
                    }
                },
            },
        }
        stage_release.validate_analysis_smoke(document, "0.2.0")

        changed = json.loads(json.dumps(document))
        changed["data"]["analysis"]["algorithm"]["compatibility"] = "legacy_status"
        with self.assertRaisesRegex(stage_release.ReleaseError, "must not attach"):
            stage_release.validate_analysis_smoke(changed, "0.2.0")

        changed = json.loads(json.dumps(document))
        changed["data"]["analysis"]["algorithm"]["profile"] = "internal_name"
        with self.assertRaisesRegex(stage_release.ReleaseError, "must not expose"):
            stage_release.validate_analysis_smoke(changed, "0.2.0")

    def test_version_tuple_accepts_toolchain_suffixes(self) -> None:
        self.assertEqual(stage_release.version_tuple("1.88"), (1, 88, 0))
        self.assertEqual(stage_release.version_tuple("1.88.0"), (1, 88, 0))
        self.assertEqual(stage_release.version_tuple("1.96.0-nightly"), (1, 96, 0))
        with self.assertRaises(stage_release.ReleaseError):
            stage_release.version_tuple("stable")

    def test_macos_target_names_do_not_imply_a_universal_binary(self) -> None:
        self.assertEqual(
            stage_release.macos_bundle_arch("aarch64-apple-darwin"),
            "aarch64",
        )
        self.assertEqual(
            stage_release.macos_binary_arch("aarch64-apple-darwin"),
            "arm64",
        )
        for target in ("x86_64-apple-darwin", "universal-apple-darwin"):
            with self.assertRaisesRegex(
                stage_release.ReleaseError, "Apple Silicon only"
            ):
                stage_release.macos_bundle_arch(target)
            with self.assertRaisesRegex(
                stage_release.ReleaseError, "Apple Silicon only"
            ):
                stage_release.macos_binary_arch(target)

    def test_unsigned_candidate_scope_requires_clean_immutable_arm64_gui(self) -> None:
        valid = {
            "unsigned_macos_arm64_candidate": True,
            "include_gui": True,
            "allow_dirty": False,
            "replace": False,
            "target": "aarch64-apple-darwin",
        }
        stage_release.validate_stage_scope(**valid)

        invalid = (
            ("include_gui", False, "must include the GUI"),
            ("allow_dirty", True, "clean source tree"),
            ("replace", True, "cannot replace"),
            ("target", "x86_64-apple-darwin", "aarch64-apple-darwin only"),
        )
        for field, value, message in invalid:
            changed = dict(valid)
            changed[field] = value
            with self.assertRaisesRegex(stage_release.ReleaseError, message):
                stage_release.validate_stage_scope(**changed)

    def test_distribution_manifest_distinguishes_local_and_unsigned_candidate(self) -> None:
        local = {
            "target": "aarch64-apple-darwin",
            "source": {"state": "dirty"},
            "distribution": stage_release.distribution_contract(False),
        }
        local_artifacts = [{"kind": "cli"}]
        self.assertEqual(
            stage_release.validate_distribution_manifest(local, local_artifacts),
            "local_staging_only",
        )

        candidate = {
            "target": "aarch64-apple-darwin",
            "source": {"state": "clean"},
            "distribution": stage_release.distribution_contract(True),
        }
        candidate_artifacts = [
            {"kind": "cli"},
            {
                "kind": "gui_macos_dmg",
                "publicationStatus": "unsigned_release_candidate",
            },
        ]
        self.assertEqual(
            stage_release.validate_distribution_manifest(
                candidate, candidate_artifacts
            ),
            "unsigned_macos_arm64_release_candidate",
        )

        candidate["source"]["state"] = "dirty"
        with self.assertRaisesRegex(stage_release.ReleaseError, "source must be clean"):
            stage_release.validate_distribution_manifest(
                candidate, candidate_artifacts
            )

    def test_unsigned_candidate_requires_pinned_rust_and_node_toolchains(self) -> None:
        package = {"msrv": "1.88"}
        toolchain = {"rustc": "1.88.0", "node": "v22.18.0"}
        stage_release.validate_candidate_toolchain(
            unsigned_macos_arm64_candidate=True,
            package=package,
            toolchain=toolchain,
        )

        for key, value, message in (
            ("rustc", "1.89.0", "exact Rust 1.88"),
            ("node", "v24.0.0", "Node.js 22"),
        ):
            changed = dict(toolchain)
            changed[key] = value
            with self.assertRaisesRegex(stage_release.ReleaseError, message):
                stage_release.validate_candidate_toolchain(
                    unsigned_macos_arm64_candidate=True,
                    package=package,
                    toolchain=changed,
                )


if __name__ == "__main__":
    unittest.main()
