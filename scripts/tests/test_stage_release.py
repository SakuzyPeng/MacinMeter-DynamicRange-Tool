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

    def test_analysis_smoke_pins_wire_profile_and_route(self) -> None:
        document = {
            "schemaVersion": 3,
            "toolVersion": "0.2.0",
            "kind": "analysis",
            "data": {
                "source": {"container": "wave", "codec": "pcm_integer"},
                "analysis": {
                    "algorithm": {
                        "profile": "foo_dr_meter_1_0_8_candidate_v1",
                        "compatibility": "unverified",
                    }
                },
            },
        }
        stage_release.validate_analysis_smoke(document, "0.2.0")

        changed = json.loads(json.dumps(document))
        changed["data"]["analysis"]["algorithm"]["compatibility"] = "verified"
        with self.assertRaisesRegex(stage_release.ReleaseError, "status drifted"):
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
        self.assertEqual(
            stage_release.macos_bundle_arch("x86_64-apple-darwin"),
            "x64",
        )
        self.assertEqual(
            stage_release.macos_binary_arch("x86_64-apple-darwin"),
            "x86_64",
        )
        with self.assertRaises(stage_release.ReleaseError):
            stage_release.macos_binary_arch("universal-apple-darwin")


if __name__ == "__main__":
    unittest.main()
