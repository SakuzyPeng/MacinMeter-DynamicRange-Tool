from __future__ import annotations

import importlib.util
import hashlib
import json
import subprocess
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "check-repository-contract.py"
SPEC = importlib.util.spec_from_file_location("repository_contract", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
repository_contract = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(repository_contract)


class RepositoryContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        (self.root / "tauri-app/src-tauri").mkdir(parents=True)
        (self.root / ".github/workflows").mkdir(parents=True)

        self.write(
            "Cargo.toml",
            """
[workspace]
members = ["tauri-app/src-tauri"]
resolver = "2"

[workspace.package]
version = "0.2.0"
edition = "2024"
rust-version = "1.88"
authors = ["Test"]
license = "MIT"
repository = "https://example.invalid/repository"

[workspace.dependencies]
serde = "1"
""".lstrip(),
        )
        self.write(
            "tauri-app/src-tauri/Cargo.toml",
            """
[package]
name = "test-gui"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
description = "Test GUI"
publish = false

[dependencies]
serde.workspace = true
""".lstrip(),
        )
        package_json = {
            "name": "test-gui",
            "version": "0.2.0",
            "scripts": {
                "check-version": "node scripts/sync-version.cjs --check",
                "sync-version": "node scripts/sync-version.cjs --write",
                "build": "npm run check-version && tsc",
                "tauri": "npm run check-version && tauri",
            },
        }
        self.write("tauri-app/package.json", json.dumps(package_json))
        self.write(
            "tauri-app/package-lock.json",
            json.dumps(
                {
                    "name": "test-gui",
                    "version": "0.2.0",
                    "packages": {"": {"version": "0.2.0"}},
                }
            ),
        )
        self.write(
            "tauri-app/src-tauri/tauri.conf.json",
            json.dumps(
                {
                    "version": "0.2.0",
                    "app": {"windows": [{"dragDropEnabled": True}]},
                    "bundle": {
                        "targets": ["app", "dmg"],
                        "macOS": {"minimumSystemVersion": "11.0"},
                    },
                }
            ),
        )
        self.write(
            "tauri-app/src/main.ts",
            "getCurrentWebview().onDragDropEvent(({ payload }) => "
            "selectInputs(payload.paths));\n",
        )
        # Any bytes that are not the scaffold's own icons satisfy the guard,
        # which is the point: it rejects one known artwork rather than trying to
        # describe a correct one.
        for icon in ("32x32.png", "128x128.png", "icon.ico", "icon.icns", "icon.png"):
            self.write(f"tauri-app/src-tauri/icons/{icon}", f"not the scaffold {icon}")
        self.write("tauri-app/icons-src/macinmeter-icon.svg", "<svg/>\n")
        self.write("tauri-app/icons-src/OFL-SourceSerif4.txt", "SIL Open Font License 1.1\n")
        self.write(
            ".github/workflows/workspace-validation.yml",
            """name: Validation

on:
  pull_request:
  push:
    branches:
      - main
  workflow_dispatch:

concurrency:
  group: workspace-validation-${{ github.event_name }}-${{ github.event.pull_request.number || github.ref }}
  cancel-in-progress: true

permissions:
  contents: read

jobs:
  workspace:
    runs-on: ubuntu-24.04
  windows:
    runs-on: windows-2025
    steps:
      - run: cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
      - run: cargo test --locked --workspace --all-targets
      - if: github.event_name != 'pull_request'
        run: cargo build --locked --release -p macinmeter-cli
      - if: github.event_name != 'pull_request'
        run: .\\target\\release\\macinmeter.exe analyze fixture.wav
      - if: github.event_name == 'workflow_dispatch'
        uses: actions/setup-node@node-sha
      - if: github.event_name == 'workflow_dispatch'
        run: npm ci
      - if: github.event_name == 'workflow_dispatch'
        run: npm run tauri -- build --bundles nsis
      - if: github.event_name == 'workflow_dispatch'
        run: dir .\\target\\release\\macinmeter-gui.exe .\\target\\release\\bundle\\nsis
      - if: github.event_name == 'workflow_dispatch'
        uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a
        with:
          name: macinmeter-windows-test-build-${{ github.sha }}
          path: target/release/bundle/nsis
  macos:
    runs-on: macos-26
    steps:
      - run: cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
      - run: cargo test --locked --workspace --all-targets
      - if: github.event_name != 'pull_request'
        uses: actions/setup-node@node-sha
      - if: github.event_name != 'pull_request'
        run: npm ci
      - if: github.event_name == 'push'
        run: python3 scripts/stage-release.py stage --include-gui
      - if: github.event_name == 'workflow_dispatch'
        run: test "$GITHUB_REF" = "refs/heads/main"
      - if: github.event_name == 'workflow_dispatch'
        run: >-
          python3 scripts/stage-release.py stage --include-gui
          --unsigned-macos-arm64-candidate
      - if: github.event_name == 'workflow_dispatch'
        uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a
        with:
          name: macinmeter-unsigned-macos-arm64-${{ github.sha }}
          path: target/release-candidates/
          if-no-files-found: error
          retention-days: 14
          compression-level: 0
""",
        )
        self.write("Cargo.lock", "")
        self.write("LICENSE", "MIT License\n")

        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        subprocess.run(["git", "-C", str(self.root), "add", "."], check=True)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def write(self, relative: str, contents: str) -> None:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents, encoding="utf-8")

    def errors(self) -> list[str]:
        return repository_contract.validate(self.root)

    def test_accepts_one_consistent_source_of_truth(self) -> None:
        self.assertEqual(self.errors(), [])

    def test_rejects_a_member_owned_third_party_version(self) -> None:
        manifest = self.root / "tauri-app/src-tauri/Cargo.toml"
        manifest.write_text(
            manifest.read_text(encoding="utf-8").replace(
                "serde.workspace = true", 'serde = "1"'
            ),
            encoding="utf-8",
        )

        self.assertTrue(
            any("dependencies.serde must use" in error for error in self.errors())
        )

    def add_library_member(self, version: str | None) -> None:
        """Give the fixture a second member that path-depends on the first."""
        root_manifest = self.root / "Cargo.toml"
        root_manifest.write_text(
            root_manifest.read_text(encoding="utf-8").replace(
                'members = ["tauri-app/src-tauri"]',
                'members = ["tauri-app/src-tauri", "crates/test-lib"]',
            ),
            encoding="utf-8",
        )
        dependency = '{ path = "../../tauri-app/src-tauri"'
        dependency += "" if version is None else f', version = "{version}"'
        self.write(
            "crates/test-lib/Cargo.toml",
            f"""
[package]
name = "test-lib"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
description = "Test library"

[dependencies]
test-gui = {dependency} }}
""".lstrip(),
        )
        self.write("crates/test-lib/LICENSE", "MIT License\n")
        subprocess.run(
            [
                "git",
                "-C",
                str(self.root),
                "add",
                "crates/test-lib/Cargo.toml",
                "crates/test-lib/LICENSE",
            ],
            check=True,
        )

    def make_package_fixture_member(self) -> None:
        self.add_library_member("0.2.0")
        manifest = self.root / "crates/test-lib/Cargo.toml"
        manifest.write_text(
            manifest.read_text(encoding="utf-8").replace(
                'name = "test-lib"', 'name = "macinmeter-codecs"'
            ),
            encoding="utf-8",
        )
        (self.root / "tests/fixtures").mkdir(parents=True)

    def stage_symlink(self, relative: str, target: str) -> None:
        object_id = subprocess.run(
            ["git", "-C", str(self.root), "hash-object", "-w", "--stdin"],
            input=target.encode("utf-8"),
            check=True,
            capture_output=True,
        ).stdout.decode("ascii").strip()
        subprocess.run(
            [
                "git",
                "-C",
                str(self.root),
                "update-index",
                "--add",
                "--cacheinfo",
                "120000",
                object_id,
                relative,
            ],
            check=True,
        )

    def test_accepts_an_internal_dependency_pinned_to_the_workspace_version(self) -> None:
        self.add_library_member("0.2.0")

        self.assertEqual(self.errors(), [])

    def test_rejects_a_stale_internal_dependency_version(self) -> None:
        # A registry refuses a path dependency with no version, so publishing
        # forces the workspace version to be repeated here. That duplicate is
        # only safe while it cannot drift: a stale one would make a published
        # crate resolve an older sibling than it was built and tested against.
        self.add_library_member("0.1.0")

        self.assertTrue(
            any(
                "dependencies.test-gui must carry" in error
                for error in self.errors()
            ),
            self.errors(),
        )

    def test_rejects_an_internal_dependency_without_a_version(self) -> None:
        self.add_library_member(None)

        self.assertTrue(
            any(
                "dependencies.test-gui must carry" in error
                for error in self.errors()
            ),
            self.errors(),
        )

    def test_rejects_a_member_without_a_description(self) -> None:
        manifest = self.root / "tauri-app/src-tauri/Cargo.toml"
        manifest.write_text(
            manifest.read_text(encoding="utf-8").replace(
                'description = "Test GUI"\n', ""
            ),
            encoding="utf-8",
        )

        self.assertTrue(
            any(
                "package.description must be a non-empty string" in error
                for error in self.errors()
            ),
            self.errors(),
        )

    def test_rejects_a_publishable_member_without_a_packaged_license(self) -> None:
        self.add_library_member("0.2.0")
        (self.root / "crates/test-lib/LICENSE").unlink()

        self.assertTrue(
            any(
                "publishable packages must include LICENSE" in error
                for error in self.errors()
            ),
            self.errors(),
        )

    def test_rejects_a_packaged_license_that_drifted_from_the_workspace(self) -> None:
        self.add_library_member("0.2.0")
        self.write("crates/test-lib/LICENSE", "a different license\n")

        self.assertTrue(
            any(
                "packaged LICENSE must match" in error for error in self.errors()
            ),
            self.errors(),
        )

    def test_rejects_a_missing_workspace_license_file(self) -> None:
        (self.root / "LICENSE").unlink()

        self.assertTrue(
            any(
                "workspace license file must exist" in error
                for error in self.errors()
            ),
            self.errors(),
        )

    def test_accepts_the_package_fixture_alias(self) -> None:
        self.make_package_fixture_member()
        self.stage_symlink(
            "crates/test-lib/package-fixtures", "../../tests/fixtures"
        )

        self.assertEqual(self.errors(), [])

    def test_rejects_a_missing_package_fixture_alias(self) -> None:
        self.make_package_fixture_member()

        self.assertTrue(
            any(
                "package-fixtures must be a tracked symlink" in error
                for error in self.errors()
            ),
            self.errors(),
        )

    def test_rejects_a_package_fixture_alias_with_the_wrong_target(self) -> None:
        self.make_package_fixture_member()
        self.stage_symlink("crates/test-lib/package-fixtures", "../wrong")

        self.assertTrue(
            any(
                "package-fixtures must be a tracked symlink" in error
                for error in self.errors()
            ),
            self.errors(),
        )

    def test_rejects_mutating_builds_and_unsupported_workflow_triggers(self) -> None:
        package_path = self.root / "tauri-app/package.json"
        package = json.loads(package_path.read_text(encoding="utf-8"))
        package["scripts"]["build"] = "npm run sync-version && tsc"
        package_path.write_text(json.dumps(package), encoding="utf-8")
        self.write(
            ".github/workflows/workspace-validation.yml",
            "name: Validation\n\non:\n  schedule:\n\njobs: {}\n",
        )

        errors = self.errors()
        self.assertTrue(any("build must check version" in error for error in errors))
        self.assertTrue(any("must use exactly" in error for error in errors))

    def test_rejects_pushes_outside_main_and_missing_cancellation(self) -> None:
        self.write(
            ".github/workflows/workspace-validation.yml",
            """name: Validation

on:
  pull_request:
  push:
    branches:
      - main
      - develop
  workflow_dispatch:

permissions:
  contents: read

jobs: {}
""",
        )

        errors = self.errors()
        self.assertTrue(any("push must target main only" in error for error in errors))
        self.assertTrue(any("group superseded runs" in error for error in errors))
        self.assertTrue(any("cancel superseded runs" in error for error in errors))

    def test_rejects_ci_permission_and_release_scope_expansion(self) -> None:
        workflow = self.root / ".github/workflows/workspace-validation.yml"
        contents = workflow.read_text(encoding="utf-8")
        contents = contents.replace("contents: read", "contents: write")
        contents = contents.replace(
            "cargo test --locked --workspace --all-targets",
            "python3 scripts/stage-release.py stage --include-gui",
            1,
        )
        workflow.write_text(contents, encoding="utf-8")

        errors = self.errors()
        self.assertTrue(any("read-only repository permissions" in error for error in errors))
        self.assertTrue(
            any("must appear only inside" in error for error in errors)
        )

    def test_rejects_removing_the_fixed_windows_gate(self) -> None:
        workflow = self.root / ".github/workflows/workspace-validation.yml"
        contents = workflow.read_text(encoding="utf-8").replace(
            "runs-on: windows-2025", "runs-on: windows-latest"
        )
        workflow.write_text(contents, encoding="utf-8")

        self.assertTrue(
            any("Windows Server 2025 gate" in error for error in self.errors())
        )

    def test_rejects_removing_the_fixed_macos_gate(self) -> None:
        workflow = self.root / ".github/workflows/workspace-validation.yml"
        contents = workflow.read_text(encoding="utf-8").replace(
            "runs-on: macos-26", "runs-on: macos-latest"
        )
        workflow.write_text(contents, encoding="utf-8")

        self.assertTrue(
            any("macOS 26 arm64 gate" in error for error in self.errors())
        )

    def test_rejects_dirty_or_pull_request_gui_staging(self) -> None:
        workflow = self.root / ".github/workflows/workspace-validation.yml"
        contents = workflow.read_text(encoding="utf-8")
        contents = contents.replace(
            "python3 scripts/stage-release.py stage --include-gui",
            "python3 scripts/stage-release.py stage --include-gui --allow-dirty",
            1,
        )
        contents = contents.replace(
            "if: github.event_name == 'workflow_dispatch'\n        run: test",
            "run: test",
        )
        workflow.write_text(contents, encoding="utf-8")

        errors = self.errors()
        self.assertTrue(any("must remain manual-only" in error for error in errors))
        self.assertTrue(any("must require a clean" in error for error in errors))

    def test_rejects_artifact_upload_from_validation_workflow(self) -> None:
        workflow = self.root / ".github/workflows/workspace-validation.yml"
        contents = workflow.read_text(encoding="utf-8").replace(
            "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
            "actions/upload-artifact@artifact-sha",
        )
        workflow.write_text(contents, encoding="utf-8")

        self.assertTrue(
            any("must be pinned and stay" in error for error in self.errors())
        )

    def test_rejects_a_windows_build_that_claims_a_release_candidate(self) -> None:
        # The Windows GUI is built to be exercised, not distributed: ADR-0011
        # keeps the release scope on unsigned Apple Silicon macOS. The guard is
        # on the claim, so that widening the scope has to be a decision rather
        # than a workflow edit.
        workflow = self.root / ".github/workflows/workspace-validation.yml"
        contents = workflow.read_text(encoding="utf-8").replace(
            "name: macinmeter-windows-test-build-${{ github.sha }}",
            "name: macinmeter-windows-release-candidate-${{ github.sha }}",
        )
        workflow.write_text(contents, encoding="utf-8")

        self.assertTrue(
            any("must be named a test build" in error for error in self.errors())
        )

    def test_rejects_the_tauri_scaffold_icon(self) -> None:
        # Checked against the real digest map by substituting the fixture's own
        # bytes for one entry, so the mechanism is exercised without committing
        # a copy of Tauri's artwork to test against.
        icon = self.root / "tauri-app/src-tauri/icons/icon.ico"
        digest = hashlib.sha256(icon.read_bytes()).hexdigest()
        original = dict(repository_contract.TAURI_SCAFFOLD_ICON_SHA256)
        repository_contract.TAURI_SCAFFOLD_ICON_SHA256["icon.ico"] = digest
        try:
            self.assertTrue(
                any("still the Tauri scaffold icon" in error for error in self.errors())
            )
        finally:
            repository_contract.TAURI_SCAFFOLD_ICON_SHA256.clear()
            repository_contract.TAURI_SCAFFOLD_ICON_SHA256.update(original)
        self.assertEqual(self.errors(), [])

    def test_rejects_macos_release_target_drift(self) -> None:
        config_path = self.root / "tauri-app/src-tauri/tauri.conf.json"
        config = json.loads(config_path.read_text(encoding="utf-8"))
        config["bundle"]["macOS"]["minimumSystemVersion"] = "10.13"
        config_path.write_text(json.dumps(config), encoding="utf-8")

        self.assertTrue(
            any("must require macOS 11.0" in error for error in self.errors())
        )

    def test_rejects_removing_native_drop_handling(self) -> None:
        self.write("tauri-app/src/main.ts", "void getCurrentWebview();\n")

        self.assertTrue(
            any("handle native dropped paths" in error for error in self.errors())
        )

    def test_rejects_disabled_native_file_drag_and_drop(self) -> None:
        config_path = self.root / "tauri-app/src-tauri/tauri.conf.json"
        config = json.loads(config_path.read_text(encoding="utf-8"))
        config["app"]["windows"][0]["dragDropEnabled"] = False
        config_path.write_text(json.dumps(config), encoding="utf-8")

        self.assertTrue(
            any("drag-and-drop enabled" in error for error in self.errors())
        )

    def test_rejects_nested_lockfiles(self) -> None:
        self.write("tauri-app/src-tauri/Cargo.lock", "")
        subprocess.run(
            [
                "git",
                "-C",
                str(self.root),
                "add",
                "tauri-app/src-tauri/Cargo.lock",
            ],
            check=True,
        )

        self.assertTrue(
            any("lockfiles must be exactly" in error for error in self.errors())
        )


if __name__ == "__main__":
    unittest.main()
