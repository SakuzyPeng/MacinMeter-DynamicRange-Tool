from __future__ import annotations

import importlib.util
import io
import json
import os
import tarfile
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / "stage-release.py"
SPEC = importlib.util.spec_from_file_location("stage_release", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
stage_release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(stage_release)


def write_pe(path: Path, machine: int) -> None:
    contents = bytearray(0x86)
    contents[:2] = stage_release.PE_MAGIC
    contents[stage_release.PE_POINTER_OFFSET : stage_release.PE_POINTER_OFFSET + 4] = (
        0x80
    ).to_bytes(4, "little")
    contents[0x80:0x84] = stage_release.PE_SIGNATURE
    contents[0x84:0x86] = machine.to_bytes(2, "little")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(contents)


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
            "schemaVersion": 4,
            "toolVersion": "0.3.0",
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
        stage_release.validate_analysis_smoke(document, "0.3.0")

        changed = json.loads(json.dumps(document))
        changed["data"]["analysis"]["algorithm"]["compatibility"] = "legacy_status"
        with self.assertRaisesRegex(stage_release.ReleaseError, "must not attach"):
            stage_release.validate_analysis_smoke(changed, "0.3.0")

        changed = json.loads(json.dumps(document))
        changed["data"]["analysis"]["algorithm"]["profile"] = "internal_name"
        with self.assertRaisesRegex(stage_release.ReleaseError, "must not expose"):
            stage_release.validate_analysis_smoke(changed, "0.3.0")

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

    def test_windows_pe_machine_validates_the_headers(self) -> None:
        executable = self.root / "gui.exe"
        write_pe(executable, stage_release.PE_MACHINE_AMD64)
        self.assertEqual(
            stage_release.windows_pe_machine(executable),
            stage_release.PE_MACHINE_AMD64,
        )
        self.assertEqual(
            stage_release.windows_machine_name(stage_release.PE_MACHINE_AMD64),
            "x86_64",
        )

        executable.write_bytes(b"not a PE")
        with self.assertRaisesRegex(stage_release.ReleaseError, "DOS header"):
            stage_release.windows_pe_machine(executable)

        write_pe(executable, stage_release.PE_MACHINE_AMD64)
        contents = bytearray(executable.read_bytes())
        contents[0x80:0x84] = b"nope"
        executable.write_bytes(contents)
        with self.assertRaisesRegex(stage_release.ReleaseError, "PE header"):
            stage_release.windows_pe_machine(executable)

    def test_windows_executable_info_passes_path_through_environment(self) -> None:
        # A real file, because the inspection now refuses a missing one: an
        # unreadable path used to arrive as an empty Authenticode status and be
        # reported as a signing problem instead of a failed inspection.
        executable = self.root / "O'Brien.exe"
        executable.write_bytes(b"MZ")
        response = {
            "fileVersion": "0.3.0.0",
            "authenticodeStatus": "NotSigned",
            "signerSubject": None,
        }
        with mock.patch.object(
            stage_release,
            "run",
            return_value=SimpleNamespace(stdout=json.dumps(response)),
        ) as mocked_run:
            self.assertEqual(
                stage_release.windows_executable_info(executable, self.root),
                response,
            )

        command = mocked_run.call_args.args[0]
        self.assertNotIn(str(executable), " ".join(command))
        self.assertIn(stage_release.WINDOWS_INSPECT_PATH_ENV, command[-1])
        self.assertEqual(
            mocked_run.call_args.kwargs["environment"][
                stage_release.WINDOWS_INSPECT_PATH_ENV
            ],
            str(executable.resolve()),
        )

    def test_windows_executable_info_refuses_a_missing_file(self) -> None:
        with self.assertRaisesRegex(
            stage_release.ReleaseError, "missing Windows executable"
        ):
            stage_release.windows_executable_info(self.root / "absent.exe", self.root)

    def test_windows_executable_info_refuses_an_empty_authenticode_status(self) -> None:
        executable = self.root / "empty-status.exe"
        executable.write_bytes(b"MZ")
        response = {
            "fileVersion": "0.3.0.0",
            "authenticodeStatus": "",
            "signerSubject": None,
        }
        with mock.patch.object(
            stage_release,
            "run",
            return_value=SimpleNamespace(stdout=json.dumps(response)),
        ):
            with self.assertRaisesRegex(
                stage_release.ReleaseError, "Authenticode status is missing"
            ):
                stage_release.windows_executable_info(executable, self.root)

    def test_windows_installer_smoke_observes_payload_and_unsigned_state(self) -> None:
        installer = self.root / "candidate" / "MacinMeter-setup.exe"
        write_pe(installer, 0x014C)
        extraction_paths: list[Path] = []

        def extract(command: list[str], _root: Path) -> SimpleNamespace:
            extraction = Path(
                next(part[2:] for part in command if part.startswith("-o"))
            )
            extraction_paths.append(extraction)
            self.assertFalse(extraction.is_relative_to(installer.parent))
            write_pe(
                extraction / "nested" / stage_release.WINDOWS_GUI_EXECUTABLE,
                stage_release.PE_MACHINE_AMD64,
            )
            return SimpleNamespace(stdout="")

        def inspect(path: Path, _root: Path) -> dict:
            return {
                "fileVersion": "0.3.0.0" if path != installer else None,
                "authenticodeStatus": "NotSigned",
                "signerSubject": None,
            }

        with (
            mock.patch.object(stage_release, "seven_zip", side_effect=extract),
            mock.patch.object(
                stage_release, "windows_executable_info", side_effect=inspect
            ),
        ):
            smoke = stage_release.smoke_windows_installer(
                installer,
                version="0.3.0",
                target=stage_release.WINDOWS_X64_TARGET,
                root=self.root,
            )

        self.assertEqual(smoke["installerMachine"], "x86")
        self.assertEqual(smoke["payloadMachine"], "x86_64")
        self.assertEqual(smoke["architecture"], "x86_64")
        self.assertEqual(smoke["installerAuthenticodeStatus"], "NotSigned")
        self.assertEqual(smoke["payloadAuthenticodeStatus"], "NotSigned")
        self.assertEqual(len(extraction_paths), 1)
        self.assertFalse(extraction_paths[0].exists())

    def test_windows_installer_smoke_rejects_non_x64_or_signed_bytes(self) -> None:
        installer = self.root / "candidate" / "MacinMeter-setup.exe"
        write_pe(installer, 0x014C)
        unsigned = {
            "fileVersion": None,
            "authenticodeStatus": "NotSigned",
            "signerSubject": None,
        }

        with mock.patch.object(
            stage_release, "windows_executable_info", return_value=unsigned
        ):
            with mock.patch.object(stage_release, "seven_zip") as mocked_seven_zip:
                mocked_seven_zip.side_effect = lambda command, _root: write_pe(
                    Path(next(part[2:] for part in command if part.startswith("-o")))
                    / stage_release.WINDOWS_GUI_EXECUTABLE,
                    0xAA64,
                )
                with self.assertRaisesRegex(
                    stage_release.ReleaseError, "expected 'x86_64'"
                ):
                    stage_release.smoke_windows_installer(
                        installer,
                        version="0.3.0",
                        target=stage_release.WINDOWS_X64_TARGET,
                        root=self.root,
                    )

        signed = {
            "fileVersion": None,
            "authenticodeStatus": "Valid",
            "signerSubject": "CN=Unexpected Signer",
        }
        with mock.patch.object(
            stage_release, "windows_executable_info", return_value=signed
        ):
            with self.assertRaisesRegex(stage_release.ReleaseError, "must be unsigned"):
                stage_release.smoke_windows_installer(
                    installer,
                    version="0.3.0",
                    target=stage_release.WINDOWS_X64_TARGET,
                    root=self.root,
                )

        def extract_x64(command: list[str], _root: Path) -> None:
            write_pe(
                Path(next(part[2:] for part in command if part.startswith("-o")))
                / stage_release.WINDOWS_GUI_EXECUTABLE,
                stage_release.PE_MACHINE_AMD64,
            )

        def signed_payload(path: Path, _root: Path) -> dict:
            return signed if path != installer else unsigned

        with (
            mock.patch.object(stage_release, "seven_zip", side_effect=extract_x64),
            mock.patch.object(
                stage_release,
                "windows_executable_info",
                side_effect=signed_payload,
            ),
        ):
            with self.assertRaisesRegex(
                stage_release.ReleaseError, "payload must be unsigned"
            ):
                stage_release.smoke_windows_installer(
                    installer,
                    version="0.3.0",
                    target=stage_release.WINDOWS_X64_TARGET,
                    root=self.root,
                )

    def test_unsigned_candidate_scope_requires_clean_immutable_arm64_gui(self) -> None:
        valid = {
            "unsigned_macos_arm64_candidate": True,
            "unsigned_windows_x64_candidate": False,
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

    def test_unsigned_windows_candidate_scope_mirrors_the_macos_conditions(self) -> None:
        valid = {
            "unsigned_macos_arm64_candidate": False,
            "unsigned_windows_x64_candidate": True,
            "include_gui": True,
            "allow_dirty": False,
            "replace": False,
            "target": "x86_64-pc-windows-msvc",
        }
        stage_release.validate_stage_scope(**valid)

        invalid = (
            ("include_gui", False, "must include the GUI"),
            ("allow_dirty", True, "clean source tree"),
            ("replace", True, "cannot replace"),
            ("target", "aarch64-apple-darwin", "x86_64-pc-windows-msvc only"),
        )
        for field, value, message in invalid:
            changed = dict(valid)
            changed[field] = value
            with self.assertRaisesRegex(stage_release.ReleaseError, message):
                stage_release.validate_stage_scope(**changed)

        # One stage yields one platform. Accepting both flags would let a single
        # host claim to have produced a GUI it cannot build.
        both = dict(valid)
        both["unsigned_macos_arm64_candidate"] = True
        with self.assertRaisesRegex(
            stage_release.ReleaseError, "one platform's candidate"
        ):
            stage_release.validate_stage_scope(**both)

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
        # Both platforms are pinned to the same toolchain, so a candidate from
        # either host is built with the versions the evidence names.
        for macos, windows in ((True, False), (False, True)):
            stage_release.validate_candidate_toolchain(
                unsigned_macos_arm64_candidate=macos,
                unsigned_windows_x64_candidate=windows,
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
                        unsigned_macos_arm64_candidate=macos,
                        unsigned_windows_x64_candidate=windows,
                        package=package,
                        toolchain=changed,
                    )

        stage_release.validate_candidate_toolchain(
            unsigned_macos_arm64_candidate=False,
            unsigned_windows_x64_candidate=False,
            package=package,
            toolchain={"rustc": "1.99.0", "node": "v24.0.0"},
        )

    def test_windows_distribution_manifest_requires_its_own_contract(self) -> None:
        candidate = {
            "target": "x86_64-pc-windows-msvc",
            "source": {"state": "clean"},
            "distribution": stage_release.distribution_contract(False, True),
        }
        artifacts = [
            {"kind": "cli"},
            {
                "kind": "gui_windows_nsis",
                "publicationStatus": "unsigned_release_candidate",
            },
        ]
        self.assertEqual(
            stage_release.validate_distribution_manifest(candidate, artifacts),
            "unsigned_windows_x64_release_candidate",
        )

        # A Windows manifest carrying the macOS GUI kind must not pass: the two
        # scopes verify different things, so the artifact has to match the scope.
        crossed = dict(candidate)
        with self.assertRaisesRegex(
            stage_release.ReleaseError, "must contain CLI and GUI"
        ):
            stage_release.validate_distribution_manifest(
                crossed,
                [
                    {"kind": "cli"},
                    {
                        "kind": "gui_macos_dmg",
                        "publicationStatus": "unsigned_release_candidate",
                    },
                ],
            )

        mismatched_target = dict(candidate)
        mismatched_target["target"] = "aarch64-apple-darwin"
        with self.assertRaisesRegex(
            stage_release.ReleaseError, "must be x86_64-pc-windows-msvc"
        ):
            stage_release.validate_distribution_manifest(mismatched_target, artifacts)


if __name__ == "__main__":
    unittest.main()
