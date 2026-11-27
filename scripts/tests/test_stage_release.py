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


def version_resource(version: tuple[int, int, int, int]) -> bytes:
    """A minimal VS_VERSIONINFO carrying only the fixed block the reader uses."""
    most = (version[0] << 16) | version[1]
    least = (version[2] << 16) | version[3]
    return (
        b"\x00" * 8
        + stage_release.VS_FIXEDFILEINFO_SIGNATURE.to_bytes(4, "little")
        + (0x00010000).to_bytes(4, "little")
        + most.to_bytes(4, "little")
        + least.to_bytes(4, "little")
        + b"\x00" * 32
    )


def write_pe(
    path: Path,
    machine: int,
    *,
    signed: bool = False,
    version: tuple[int, int, int, int] | None = None,
) -> None:
    """Build a PE with only the structures the release reader parses.

    Synthetic rather than a recorded binary so every branch — architecture, an
    embedded certificate table, and a version resource — can be exercised on any
    host, which is the property that asking a shell did not have.
    """
    pe_offset = 0x80
    directories = 16
    optional_size = 112 + directories * 8
    section_table = pe_offset + 24 + optional_size
    rsrc_rva = 0x1000
    rsrc_raw = section_table + 40

    resource = b""
    if version is not None:
        blob = version_resource(version)
        blob_offset = 0x58

        def directory(entry_name: int, entry_offset: int, is_directory: bool) -> bytes:
            flag = 0x80000000 if is_directory else 0
            return (
                b"\x00" * 12
                + (0).to_bytes(2, "little")
                + (1).to_bytes(2, "little")
                + entry_name.to_bytes(4, "little")
                + (entry_offset | flag).to_bytes(4, "little")
            )

        resource = (
            directory(stage_release.PE_RESOURCE_TYPE_VERSION, 0x18, True)
            + directory(1, 0x30, True)
            + directory(0x409, 0x48, False)
            + (rsrc_rva + blob_offset).to_bytes(4, "little")
            + len(blob).to_bytes(4, "little")
            + b"\x00" * 8
            + blob
        )

    contents = bytearray(rsrc_raw + max(len(resource), 1))
    contents[:2] = stage_release.PE_MAGIC
    contents[
        stage_release.PE_POINTER_OFFSET : stage_release.PE_POINTER_OFFSET + 4
    ] = pe_offset.to_bytes(4, "little")
    contents[pe_offset : pe_offset + 4] = stage_release.PE_SIGNATURE
    contents[pe_offset + 4 : pe_offset + 6] = machine.to_bytes(2, "little")
    contents[pe_offset + 6 : pe_offset + 8] = (1).to_bytes(2, "little")
    contents[pe_offset + 20 : pe_offset + 22] = optional_size.to_bytes(2, "little")
    optional = pe_offset + 24
    contents[optional : optional + 2] = stage_release.PE_OPTIONAL_MAGIC_PE32_PLUS.to_bytes(
        2, "little"
    )
    directory_base = optional + 112
    if version is not None:
        entry = directory_base + stage_release.PE_DIRECTORY_RESOURCE * 8
        contents[entry : entry + 4] = rsrc_rva.to_bytes(4, "little")
        contents[entry + 4 : entry + 8] = len(resource).to_bytes(4, "little")
    if signed:
        entry = directory_base + stage_release.PE_DIRECTORY_SECURITY * 8
        contents[entry : entry + 4] = (0x2000).to_bytes(4, "little")
        contents[entry + 4 : entry + 8] = (0x40).to_bytes(4, "little")
    contents[section_table : section_table + 8] = b".rsrc\x00\x00\x00"
    contents[section_table + 8 : section_table + 12] = len(resource).to_bytes(4, "little")
    contents[section_table + 12 : section_table + 16] = rsrc_rva.to_bytes(4, "little")
    contents[section_table + 16 : section_table + 20] = len(resource).to_bytes(4, "little")
    contents[section_table + 20 : section_table + 24] = rsrc_raw.to_bytes(4, "little")
    if resource:
        contents[rsrc_raw : rsrc_raw + len(resource)] = resource
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(bytes(contents))


def write_licenses(directory: Path, font_build: str) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    for name in stage_release.gui_license_names(font_build):
        (directory / name).write_text("licence\n", encoding="utf-8")


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

    def test_windows_pe_image_reads_machine_signature_and_version(self) -> None:
        # Read from the file rather than asked of a shell: two CI failures came
        # from PowerShell module resolution differing between hosts, while these
        # three facts are in the bytes and answer the same on every machine.
        unsigned = self.root / "unsigned.exe"
        write_pe(unsigned, stage_release.PE_MACHINE_AMD64, version=(0, 3, 0, 0))
        image = stage_release.windows_pe_image(unsigned)
        self.assertEqual(image.machine, stage_release.PE_MACHINE_AMD64)
        self.assertFalse(image.has_embedded_signature())
        self.assertEqual(image.file_version(), "0.3.0.0")

        signed = self.root / "signed.exe"
        write_pe(signed, stage_release.PE_MACHINE_AMD64, signed=True)
        self.assertTrue(stage_release.windows_pe_image(signed).has_embedded_signature())
        with self.assertRaisesRegex(stage_release.ReleaseError, "must be unsigned"):
            stage_release.require_unsigned_pe(
                stage_release.windows_pe_image(signed), "Windows installer"
            )

        with self.assertRaisesRegex(
            stage_release.ReleaseError, "missing Windows executable"
        ):
            stage_release.windows_pe_image(self.root / "absent.exe")

        truncated = self.root / "truncated.exe"
        truncated.write_bytes(b"MZ" + b"\x00" * 8)
        with self.assertRaisesRegex(stage_release.ReleaseError, "valid DOS header"):
            stage_release.windows_pe_image(truncated)

        without_version = self.root / "no-version.exe"
        write_pe(without_version, stage_release.PE_MACHINE_AMD64)
        with self.assertRaisesRegex(stage_release.ReleaseError, "no version resource"):
            stage_release.windows_pe_image(without_version).file_version()

    def test_windows_installer_smoke_observes_payload_and_unsigned_state(self) -> None:
        installer = self.root / "candidate" / "MacinMeter-setup.exe"
        write_pe(installer, 0x014C)
        extraction_paths: list[Path] = []

        def extract(command: list[str], _root: Path) -> SimpleNamespace:
            extraction = Path(
                next(part[2:] for part in command if part.startswith("-o"))
            )
            extraction_paths.append(extraction)
            # Never under the staged release: a failed cleanup there would leave
            # unmanifested payload bytes inside the tree that gets checksummed.
            self.assertFalse(extraction.is_relative_to(installer.parent))
            write_pe(
                extraction / "nested" / stage_release.WINDOWS_GUI_EXECUTABLE,
                stage_release.PE_MACHINE_AMD64,
                version=(0, 3, 0, 0),
            )
            write_licenses(extraction / "nested" / "licenses", "system-fonts")
            return SimpleNamespace(stdout="")

        with mock.patch.object(stage_release, "seven_zip", side_effect=extract):
            smoke = stage_release.smoke_windows_installer(
                installer,
                version="0.3.0",
                target=stage_release.WINDOWS_X64_TARGET,
                font_build="system-fonts",
                root=self.root,
            )

        # An NSIS stub is a 32-bit executable carrying a 64-bit payload, so the
        # two are reported apart rather than collapsed into one claim.
        self.assertEqual(smoke["installerMachine"], "x86")
        self.assertEqual(smoke["payloadMachine"], "x86_64")
        self.assertEqual(smoke["architecture"], "x86_64")
        self.assertFalse(smoke["installerEmbeddedSignature"])
        self.assertFalse(smoke["payloadEmbeddedSignature"])
        self.assertEqual(smoke["payloadVersion"], "0.3.0.0")
        self.assertEqual(smoke["fontBuild"], "system-fonts")
        self.assertEqual(smoke["licenses"], ["LICENSE", "THIRD_PARTY_NOTICES.md"])
        self.assertEqual(len(extraction_paths), 1)
        self.assertFalse(extraction_paths[0].exists())

    def test_windows_installer_smoke_rejects_the_states_it_exists_to_catch(self) -> None:
        installer = self.root / "candidate" / "MacinMeter-setup.exe"

        def payload_writer(
            machine: int,
            version: tuple[int, int, int, int] | None,
            licenses: str | None = "system-fonts",
        ):
            def extract(command: list[str], _root: Path) -> SimpleNamespace:
                extraction = Path(
                    next(part[2:] for part in command if part.startswith("-o"))
                )
                write_pe(
                    extraction / stage_release.WINDOWS_GUI_EXECUTABLE,
                    machine,
                    version=version,
                )
                if licenses is not None:
                    write_licenses(extraction / "licenses", licenses)
                return SimpleNamespace(stdout="")

            return extract

        write_pe(installer, 0x014C)
        amd64 = stage_release.PE_MACHINE_AMD64
        for writer, font_build, message in (
            (payload_writer(0xAA64, (0, 3, 0, 0)), "system-fonts", "expected 'x86_64'"),
            (payload_writer(amd64, None), "system-fonts", "no version resource"),
            (payload_writer(amd64, (0, 2, 0, 0)), "system-fonts", "expected 0.3.0"),
            # A payload without the embedded fonts is not the bundled build,
            # whatever the artifact record says.
            (
                payload_writer(amd64, (0, 3, 0, 0), "bundled-fonts"),
                "bundled-fonts",
                "is the system-fonts build",
            ),
            (payload_writer(amd64, (0, 3, 0, 0), None), "system-fonts", "licences are"),
            (
                payload_writer(amd64, (0, 3, 0, 0), "bundled-fonts"),
                "system-fonts",
                "licences are",
            ),
        ):
            with mock.patch.object(stage_release, "seven_zip", side_effect=writer):
                with self.assertRaisesRegex(stage_release.ReleaseError, message):
                    stage_release.smoke_windows_installer(
                        installer,
                        version="0.3.0",
                        target=stage_release.WINDOWS_X64_TARGET,
                        font_build=font_build,
                        root=self.root,
                    )

        # A signed installer is refused before extraction: an unsigned release
        # that quietly became signed is the state this check exists for.
        write_pe(installer, 0x014C, signed=True)
        with self.assertRaisesRegex(stage_release.ReleaseError, "must be unsigned"):
            stage_release.smoke_windows_installer(
                installer,
                version="0.3.0",
                target=stage_release.WINDOWS_X64_TARGET,
                font_build="system-fonts",
                root=self.root,
            )

    def test_font_build_is_read_from_the_executable_bytes(self) -> None:
        executable = self.root / "gui"
        executable.write_bytes(b"code" + b"".join(stage_release.EMBEDDED_FONT_MARKERS))
        self.assertEqual(stage_release.font_build_of(executable), "bundled-fonts")
        executable.write_bytes(b"code without fonts")
        self.assertEqual(stage_release.font_build_of(executable), "system-fonts")
        executable.write_bytes(b"code" + stage_release.EMBEDDED_FONT_MARKERS[0])
        with self.assertRaisesRegex(stage_release.ReleaseError, "only part"):
            stage_release.font_build_of(executable)
        with self.assertRaisesRegex(stage_release.ReleaseError, "unknown GUI font build"):
            stage_release.gui_license_names("fonts")
        # The OFL texts travel with the build that embeds the fonts, and only it.
        self.assertEqual(
            stage_release.gui_license_names("bundled-fonts"),
            {
                "LICENSE",
                "THIRD_PARTY_NOTICES.md",
                "OFL-NotoSansCJK.txt",
                "OFL-SourceSerif4.txt",
            },
        )

    def test_gui_names_match_the_installer_build_script_and_tauri_install(self) -> None:
        # An install replaces a 0.3.x Tauri install only while all four places
        # agree on the names it was installed under.
        repository = SCRIPT.parents[1]
        installer = (repository / stage_release.WINDOWS_INSTALLER_SCRIPT).read_text(
            encoding="utf-8"
        )
        build_script = (repository / stage_release.GUI_PACKAGE_DIR / "build.rs").read_text(
            encoding="utf-8"
        )
        tauri = json.loads(
            (repository / "tauri-app/src-tauri/tauri.conf.json").read_text(
                encoding="utf-8"
            )
        )
        name = stage_release.GUI_PRODUCT_NAME
        self.assertEqual(tauri["productName"], name)
        self.assertEqual(tauri["identifier"], stage_release.GUI_BUNDLE_IDENTIFIER)
        self.assertIn(f'!define PRODUCTNAME "{name}"', installer)
        self.assertIn(
            f'!define MAINBINARYNAME "{stage_release.WINDOWS_GUI_EXECUTABLE}"', installer
        )
        manufacturer = stage_release.GUI_BUNDLE_IDENTIFIER.split(".")[1]
        self.assertIn(f'!define MANUFACTURER "{manufacturer}"', installer)
        self.assertIn(f'const PRODUCT_NAME: &str = "{name}";', build_script)
        self.assertIn(
            f'const INSTALLED_EXECUTABLE: &str = "{stage_release.WINDOWS_GUI_EXECUTABLE}";',
            build_script,
        )
        self.assertEqual(
            stage_release.WINDOWS_GUI_EXECUTABLE,
            stage_release.MACOS_GUI_EXECUTABLE + ".exe",
        )
        info = stage_release.macos_info_plist("0.3.1", "Copyright")
        self.assertEqual(info["CFBundleIdentifier"], stage_release.GUI_BUNDLE_IDENTIFIER)
        self.assertEqual(info["CFBundleExecutable"], stage_release.MACOS_GUI_EXECUTABLE)
        self.assertEqual(info["LSMinimumSystemVersion"], "11.0")

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
            *(
                {
                    "kind": "gui_macos_dmg",
                    "fontBuild": font_build,
                    "publicationStatus": "unsigned_release_candidate",
                }
                for font_build in ("bundled-fonts", "system-fonts")
            ),
        ]
        self.assertEqual(
            stage_release.validate_distribution_manifest(
                candidate, candidate_artifacts
            ),
            "unsigned_macos_arm64_release_candidate",
        )

        # A candidate ships both font builds or neither.
        for artifacts in (
            candidate_artifacts[:2],
            [candidate_artifacts[0], candidate_artifacts[1], candidate_artifacts[1]],
        ):
            with self.assertRaisesRegex(stage_release.ReleaseError, "both font builds"):
                stage_release.validate_distribution_manifest(candidate, artifacts)

        candidate["source"]["state"] = "dirty"
        with self.assertRaisesRegex(stage_release.ReleaseError, "source must be clean"):
            stage_release.validate_distribution_manifest(
                candidate, candidate_artifacts
            )

    def test_unsigned_candidate_requires_the_pinned_rust_toolchain(self) -> None:
        package = {"msrv": "1.98"}
        toolchain = {"rustc": "1.98.0"}
        # Both platforms are pinned to the same toolchain, so a candidate from
        # either host is built with the versions the evidence names.
        for macos, windows in ((True, False), (False, True)):
            stage_release.validate_candidate_toolchain(
                unsigned_macos_arm64_candidate=macos,
                unsigned_windows_x64_candidate=windows,
                package=package,
                toolchain=toolchain,
            )

            for key, value, message in (("rustc", "1.99.0", "exact Rust 1.98"),):
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
            toolchain={"rustc": "1.99.0"},
        )

    def test_windows_distribution_manifest_requires_its_own_contract(self) -> None:
        candidate = {
            "target": "x86_64-pc-windows-msvc",
            "source": {"state": "clean"},
            "distribution": stage_release.distribution_contract(False, True),
        }
        artifacts = [
            {"kind": "cli"},
            *(
                {
                    "kind": "gui_windows_nsis",
                    "fontBuild": font_build,
                    "publicationStatus": "unsigned_release_candidate",
                }
                for font_build in ("bundled-fonts", "system-fonts")
            ),
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
                    artifacts[0],
                    *({**gui, "kind": "gui_macos_dmg"} for gui in artifacts[1:]),
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
