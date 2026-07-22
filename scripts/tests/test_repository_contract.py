from __future__ import annotations

import importlib.util
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
            json.dumps({"version": "0.2.0"}),
        )
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

jobs: {}
""",
        )
        self.write("Cargo.lock", "")

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
            "jobs: {}", "jobs:\n  release:\n    run: python3 scripts/stage-release.py stage"
        )
        workflow.write_text(contents, encoding="utf-8")

        errors = self.errors()
        self.assertTrue(any("read-only repository permissions" in error for error in errors))
        self.assertTrue(any("must not run release staging" in error for error in errors))

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
