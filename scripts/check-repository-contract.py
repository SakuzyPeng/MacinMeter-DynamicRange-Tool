#!/usr/bin/env python3
"""Validate the repository identity and direct-dependency contract.

This check is intentionally read-only and uses only the Python standard
library. It does not resolve or download dependencies.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

try:
    import tomllib
except ImportError:  # pragma: no cover - Python reports the actionable error.
    print("Python 3.11 or newer is required for repository contract checks.")
    raise SystemExit(2)


INHERITED_PACKAGE_FIELDS = (
    "version",
    "edition",
    "rust-version",
    "authors",
    "license",
    "repository",
)
DEPENDENCY_SECTIONS = ("dependencies", "dev-dependencies", "build-dependencies")


def load_toml(path: Path) -> dict:
    with path.open("rb") as file:
        return tomllib.load(file)


def dependency_tables(manifest: dict):
    for section in DEPENDENCY_SECTIONS:
        yield section, manifest.get(section, {})
    for target_name, target in manifest.get("target", {}).items():
        for section in DEPENDENCY_SECTIONS:
            yield f"target.{target_name}.{section}", target.get(section, {})


def tracked_files(root: Path) -> list[str]:
    completed = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z"],
        check=True,
        capture_output=True,
    )
    return [
        entry.decode("utf-8")
        for entry in completed.stdout.split(b"\0")
        if entry
    ]


def workflow_events(path: Path) -> list[str] | None:
    lines = path.read_text(encoding="utf-8").splitlines()
    try:
        start = lines.index("on:") + 1
    except ValueError:
        return None

    body: list[str] = []
    for line in lines[start:]:
        if line and not line[0].isspace():
            break
        body.append(line)
    return [
        match.group(1)
        for line in body
        if (match := re.match(r"^  ([A-Za-z0-9_-]+):", line))
    ]


def workflow_event_body(path: Path, event: str) -> list[str] | None:
    lines = path.read_text(encoding="utf-8").splitlines()
    marker = f"  {event}:"
    try:
        start = lines.index(marker) + 1
    except ValueError:
        return None

    body: list[str] = []
    for line in lines[start:]:
        if re.match(r"^  [A-Za-z0-9_-]+:", line):
            break
        if line and not line.startswith("    "):
            break
        if line.strip():
            body.append(line)
    return body


def validate(root: Path) -> list[str]:
    root = root.resolve()
    errors: list[str] = []

    def require(condition: bool, message: str) -> None:
        if not condition:
            errors.append(message)

    root_manifest_path = root / "Cargo.toml"
    root_manifest = load_toml(root_manifest_path)
    require(
        "package" not in root_manifest,
        "Cargo.toml must remain a virtual workspace without [package]",
    )

    workspace = root_manifest.get("workspace", {})
    workspace_package = workspace.get("package", {})
    workspace_dependencies = workspace.get("dependencies", {})
    version = workspace_package.get("version")
    require(isinstance(version, str), "workspace.package.version must be a string")
    require(
        workspace_package.get("rust-version") == "1.88",
        "workspace.package.rust-version must remain the verified MSRV 1.88",
    )
    require(
        workspace_package.get("edition") == "2024",
        "workspace.package.edition must remain 2024",
    )

    member_manifests = {
        (root / member / "Cargo.toml").resolve()
        for member in workspace.get("members", [])
    }
    require(bool(member_manifests), "workspace.members must not be empty")
    used_workspace_dependencies: set[str] = set()

    for manifest_path in sorted(member_manifests):
        relative_manifest = manifest_path.relative_to(root)
        require(
            manifest_path.is_file(),
            f"{relative_manifest}: workspace member manifest is missing",
        )
        if not manifest_path.is_file():
            continue
        manifest = load_toml(manifest_path)
        package = manifest.get("package", {})
        package_name = package.get("name", str(relative_manifest))

        for field in INHERITED_PACKAGE_FIELDS:
            require(
                package.get(field) == {"workspace": True},
                f"{relative_manifest}: package.{field} must inherit from workspace",
            )

        for section, dependencies in dependency_tables(manifest):
            for dependency_name, specification in dependencies.items():
                location = f"{relative_manifest}: {section}.{dependency_name}"
                if isinstance(specification, dict) and "path" in specification:
                    dependency_manifest = (
                        manifest_path.parent
                        / specification["path"]
                        / "Cargo.toml"
                    ).resolve()
                    require(
                        dependency_manifest in member_manifests,
                        f"{location} must point to another workspace member",
                    )
                    continue

                require(
                    isinstance(specification, dict)
                    and specification.get("workspace") is True,
                    f"{location} must use `{dependency_name}.workspace = true`",
                )
                require(
                    dependency_name in workspace_dependencies,
                    f"{location} is missing from [workspace.dependencies]",
                )
                used_workspace_dependencies.add(dependency_name)

        require(
            isinstance(package_name, str) and package_name,
            f"{relative_manifest}: package.name must be non-empty",
        )

    unused_dependencies = sorted(
        set(workspace_dependencies) - used_workspace_dependencies
    )
    require(
        not unused_dependencies,
        "[workspace.dependencies] contains unused entries: "
        + ", ".join(unused_dependencies),
    )

    gui_package_path = root / "tauri-app/package.json"
    gui_package = json.loads(gui_package_path.read_text(encoding="utf-8"))
    gui_scripts = gui_package.get("scripts", {})
    require(
        gui_scripts.get("check-version")
        == "node scripts/sync-version.cjs --check",
        "tauri-app/package.json: check-version must remain read-only",
    )
    require(
        gui_scripts.get("sync-version")
        == "node scripts/sync-version.cjs --write",
        "tauri-app/package.json: sync-version must be the explicit writer",
    )
    for script_name in ("build", "tauri"):
        require(
            gui_scripts.get(script_name, "").startswith("npm run check-version && "),
            f"tauri-app/package.json: {script_name} must check version without syncing",
        )

    if isinstance(version, str):
        json_versions = (
            (gui_package_path, ("version",)),
            (root / "tauri-app/package-lock.json", ("version",)),
            (
                root / "tauri-app/package-lock.json",
                ("packages", "", "version"),
            ),
            (root / "tauri-app/src-tauri/tauri.conf.json", ("version",)),
        )
        for path, keys in json_versions:
            document = json.loads(path.read_text(encoding="utf-8"))
            value = document
            for key in keys:
                value = value.get(key) if isinstance(value, dict) else None
            label = f"{path.relative_to(root)}:{'.'.join(keys)}"
            require(value == version, f"{label} must equal workspace version {version}")

    tracked = tracked_files(root)
    cargo_locks = sorted(path for path in tracked if Path(path).name == "Cargo.lock")
    package_locks = sorted(
        path for path in tracked if Path(path).name == "package-lock.json"
    )
    require(
        cargo_locks == ["Cargo.lock"],
        "tracked Cargo lockfiles must be exactly: Cargo.lock",
    )
    require(
        package_locks == ["tauri-app/package-lock.json"],
        "tracked npm lockfiles must be exactly: tauri-app/package-lock.json",
    )

    workflow_paths = sorted(
        [
            *root.glob(".github/workflows/*.yml"),
            *root.glob(".github/workflows/*.yaml"),
        ]
    )
    require(
        len(workflow_paths) == 1,
        "exactly one GitHub Actions validation workflow is required",
    )
    for path in workflow_paths:
        require(
            path == root / ".github/workflows/workspace-validation.yml",
            "the sole workflow must be .github/workflows/workspace-validation.yml",
        )
        events = workflow_events(path)
        require(
            events is not None
            and sorted(events) == ["pull_request", "push", "workflow_dispatch"],
            f"{path.relative_to(root)} must use exactly pull_request, push, and "
            f"workflow_dispatch triggers; found {events}",
        )
        require(
            workflow_event_body(path, "pull_request") == [],
            f"{path.relative_to(root)} pull_request must not use path or branch filters",
        )
        require(
            workflow_event_body(path, "push")
            == ["    branches:", "      - main"],
            f"{path.relative_to(root)} push must target main only",
        )
        require(
            workflow_event_body(path, "workflow_dispatch") == [],
            f"{path.relative_to(root)} workflow_dispatch must remain input-free",
        )

        workflow_text = path.read_text(encoding="utf-8")
        require(
            "group: workspace-validation-${{ github.event_name }}-"
            "${{ github.event.pull_request.number || github.ref }}"
            in workflow_text,
            f"{path.relative_to(root)} must group superseded runs by trigger and ref",
        )
        require(
            "cancel-in-progress: true" in workflow_text,
            f"{path.relative_to(root)} must cancel superseded runs",
        )
        require(
            "permissions:\n  contents: read" in workflow_text,
            f"{path.relative_to(root)} must retain read-only repository permissions",
        )
        forbidden_ci_tasks = {
            "verify-malformed-corpus.py": "the hostile corpus verifier",
            "run-performance-baseline.py": "performance baselines",
            "run-performance-profile.py": "performance profiling",
            "stage-release.py": "release staging",
        }
        for command, label in forbidden_ci_tasks.items():
            require(
                command not in workflow_text,
                f"{path.relative_to(root)} must not run {label}",
            )

    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parent.parent,
        help="repository root (defaults to the parent of scripts/)",
    )
    arguments = parser.parse_args()
    root = arguments.root.resolve()

    try:
        errors = validate(root)
    except (
        KeyError,
        OSError,
        TypeError,
        ValueError,
        json.JSONDecodeError,
        subprocess.CalledProcessError,
    ) as error:
        print(f"repository contract check could not run: {error}", file=sys.stderr)
        return 2

    if errors:
        print("repository contract violations:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1

    print("repository contract is consistent")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
