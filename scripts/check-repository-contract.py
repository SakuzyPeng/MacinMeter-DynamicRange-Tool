#!/usr/bin/env python3
"""Validate the repository identity and direct-dependency contract.

This check is intentionally read-only and uses only the Python standard
library. It does not resolve or download dependencies.
"""

from __future__ import annotations

import argparse
import hashlib
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
# Cargo dereferences these tracked aliases into the published archive. Keeping
# the media corpus at one repository path avoids two copies drifting apart.
PACKAGE_FIXTURE_MEMBERS = {"macinmeter-codecs", "macinmeter"}


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


def tracked_symlink_targets(root: Path) -> dict[str, str]:
    completed = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-s", "-z"],
        check=True,
        capture_output=True,
    )
    targets: dict[str, str] = {}
    for entry in completed.stdout.split(b"\0"):
        if not entry:
            continue
        metadata, relative = entry.split(b"\t", 1)
        mode, object_id, _stage = metadata.split()
        if mode != b"120000":
            continue
        blob = subprocess.run(
            ["git", "-C", str(root), "cat-file", "blob", object_id.decode("ascii")],
            check=True,
            capture_output=True,
        )
        targets[relative.decode("utf-8")] = blob.stdout.decode("utf-8")
    return targets


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


def workflow_job_body(path: Path, job: str) -> list[str] | None:
    lines = path.read_text(encoding="utf-8").splitlines()
    marker = f"  {job}:"
    try:
        start = lines.index(marker) + 1
    except ValueError:
        return None

    body: list[str] = []
    for line in lines[start:]:
        if re.match(r"^  [A-Za-z0-9_-]+:", line):
            break
        body.append(line)
    return body


# The Tauri scaffold's own icons, by digest. Its logo is Tauri's trademark
# rather than a neutral placeholder, and it shipped in every build until
# 2026-08-09. Pinning the exact bytes is narrower and more honest than trying to
# describe what a correct icon looks like, and it is the mistake that happened.
TAURI_SCAFFOLD_ICON_SHA256 = {
    "32x32.png": "1c6782dc65c8111c12cbc1882a0fea5e71ab8e51b18da2ce9580f5c88860ed02",
    "128x128.png": "19b4fec485db7df51a691fcce72a3dd6f983e754fc4262da7154e4a4c688f69e",
    "icon.ico": "392206b573a809997f3ff16fe68f456a52e931c372107eade9572b329bbe3321",
    "icon.icns": "3dc10493b7de48a61de58f768f8a5708d3a44a068c148cedf0502b9b9b71ba5d",
    "icon.png": "273cd669e07c455ad1c7c095890a37984652157cee73128a867300067dfb80e7",
}


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
    workspace_license_path = root / "LICENSE"
    require(
        workspace_license_path.is_file(),
        "the workspace license file must exist at LICENSE",
    )
    workspace_license = (
        workspace_license_path.read_bytes() if workspace_license_path.is_file() else None
    )
    tracked = tracked_files(root)
    tracked_set = set(tracked)
    tracked_symlinks = tracked_symlink_targets(root)

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
                    # A registry refuses a path dependency without a version, so
                    # publishing requires the version to be repeated here. That
                    # duplicate is only safe while it cannot drift: a stale one
                    # would resolve a released crate against the wrong sibling.
                    require(
                        specification.get("version") == version,
                        f"{location} must carry `version = \"{version}\"` "
                        "so a published crate resolves its workspace sibling",
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
        # A registry rejects an upload without one, and the field is the only
        # description a consumer sees before reading any code. Requiring it of
        # every member keeps a new crate from becoming the one that blocks a
        # release at upload time.
        description = package.get("description")
        require(
            isinstance(description, str) and description.strip(),
            f"{relative_manifest}: package.description must be a non-empty string",
        )
        if package.get("publish") is not False:
            # Cargo does not include a workspace-root license automatically.
            # Retain SPDX metadata and ship the exact notice as a package file.
            package_license_path = manifest_path.parent / "LICENSE"
            package_license_relative = package_license_path.relative_to(root).as_posix()
            require(
                package_license_path.is_file(),
                f"{relative_manifest}: publishable packages must include LICENSE",
            )
            require(
                package_license_relative in tracked_set,
                f"{relative_manifest}: packaged LICENSE must be tracked by git",
            )
            if package_license_path.is_file() and workspace_license is not None:
                require(
                    package_license_path.read_bytes() == workspace_license,
                    f"{relative_manifest}: packaged LICENSE must match the workspace LICENSE",
                )
        if package_name in PACKAGE_FIXTURE_MEMBERS:
            fixture_alias = (manifest_path.parent / "package-fixtures").relative_to(root)
            fixture_alias_text = fixture_alias.as_posix()
            require(
                tracked_symlinks.get(fixture_alias_text) == "../../tests/fixtures",
                f"{relative_manifest}: package-fixtures must be a tracked symlink "
                "to ../../tests/fixtures",
            )
            require(
                (root / "tests/fixtures").is_dir(),
                "the package fixture source must exist at tests/fixtures",
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

    tauri_config_path = root / "tauri-app/src-tauri/tauri.conf.json"
    tauri_config = json.loads(tauri_config_path.read_text(encoding="utf-8"))
    bundle = tauri_config.get("bundle", {})
    require(
        bundle.get("targets") == ["app", "dmg"],
        "tauri.conf.json must retain the macOS app and DMG bundle targets",
    )
    for name, digest in TAURI_SCAFFOLD_ICON_SHA256.items():
        icon_path = root / "tauri-app/src-tauri/icons" / name
        require(
            icon_path.is_file(),
            f"tauri-app/src-tauri/icons/{name} must exist",
        )
        if icon_path.is_file():
            actual = hashlib.sha256(icon_path.read_bytes()).hexdigest()
            require(
                actual != digest,
                f"tauri-app/src-tauri/icons/{name} is still the Tauri scaffold icon; "
                "regenerate from tauri-app/icons-src/macinmeter-icon.svg",
            )
    icon_source = root / "tauri-app/icons-src/macinmeter-icon.svg"
    require(
        icon_source.is_file(),
        "tauri-app/icons-src/macinmeter-icon.svg must remain the icon source of truth",
    )
    require(
        (root / "tauri-app/icons-src/OFL-SourceSerif4.txt").is_file(),
        "the icon's outlined letterforms require their SIL Open Font License text",
    )
    require(
        bundle.get("macOS", {}).get("minimumSystemVersion") == "11.0",
        "tauri.conf.json must require macOS 11.0 for Apple Silicon releases",
    )
    windows = tauri_config.get("app", {}).get("windows", [])
    require(
        isinstance(windows, list)
        and len(windows) == 1
        and windows[0].get("dragDropEnabled") is True,
        "tauri.conf.json must keep native file drag-and-drop enabled",
    )
    frontend_source_path = root / "tauri-app/src/main.ts"
    frontend_source = frontend_source_path.read_text(encoding="utf-8")
    require(
        "getCurrentWebview().onDragDropEvent" in re.sub(r"\s+", "", frontend_source)
        and "payload.paths" in frontend_source,
        "tauri-app/src/main.ts must handle native dropped paths",
    )

    if isinstance(version, str):
        json_versions = (
            (gui_package_path, ("version",)),
            (root / "tauri-app/package-lock.json", ("version",)),
            (
                root / "tauri-app/package-lock.json",
                ("packages", "", "version"),
            ),
            (tauri_config_path, ("version",)),
        )
        for path, keys in json_versions:
            document = json.loads(path.read_text(encoding="utf-8"))
            value = document
            for key in keys:
                value = value.get(key) if isinstance(value, dict) else None
            label = f"{path.relative_to(root)}:{'.'.join(keys)}"
            require(value == version, f"{label} must equal workspace version {version}")

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

        linux_job = workflow_job_body(path, "workspace")
        windows_job = workflow_job_body(path, "windows")
        macos_job = workflow_job_body(path, "macos")
        require(
            linux_job is not None and "    runs-on: ubuntu-24.04" in linux_job,
            f"{path.relative_to(root)} must retain the Ubuntu 24.04 full gate",
        )
        require(
            windows_job is not None and "    runs-on: windows-2025" in windows_job,
            f"{path.relative_to(root)} must retain the Windows Server 2025 gate",
        )
        require(
            macos_job is not None and "    runs-on: macos-26" in macos_job,
            f"{path.relative_to(root)} must retain the macOS 26 arm64 gate",
        )
        macos_stage_command = "python3 scripts/stage-release.py stage --include-gui"
        unsigned_candidate_flag = "--unsigned-macos-arm64-candidate"
        upload_action = (
            "actions/upload-artifact@"
            "043fb46d1a93c77aae656e7c1c64a875d1fc6a0a"
        )
        if windows_job is not None:
            windows_text = "\n".join(windows_job)
            required_windows_commands = (
                "cargo clippy --locked --workspace --all-targets --all-features -- -D warnings",
                "cargo test --locked --workspace --all-targets",
                "cargo build --locked --release -p macinmeter-cli",
                ".\\target\\release\\mdrmeter.exe analyze",
            )
            for command in required_windows_commands:
                require(
                    command in windows_text,
                    f"{path.relative_to(root)} Windows gate is missing: {command}",
                )
            require(
                windows_text.count("if: github.event_name != 'pull_request'") == 2,
                f"{path.relative_to(root)} Windows release build and smoke must remain "
                "main/manual-only",
            )
            required_windows_gui_commands = (
                "npm ci",
                "npm run tauri -- build --bundles nsis",
                "macinmeter-gui.exe",
                "bundle\\nsis",
            )
            for command in required_windows_gui_commands:
                require(
                    command in windows_text,
                    f"{path.relative_to(root)} Windows GUI test build is missing: {command}",
                )
            require(
                windows_text.count("if: github.event_name == 'workflow_dispatch'") == 5,
                f"{path.relative_to(root)} the Windows GUI test build, its verification "
                "and its retention must remain manual-only",
            )
            require(
                "release-candidate" not in windows_text
                and unsigned_candidate_flag not in windows_text,
                f"{path.relative_to(root)} the Windows GUI build is a test build and may "
                "not claim or produce a release candidate",
            )
            require(
                "name: macinmeter-windows-test-build-${{ github.sha }}" in windows_text,
                f"{path.relative_to(root)} the Windows artifact must be named a test build",
            )

        if macos_job is not None:
            macos_text = "\n".join(macos_job)
            required_macos_commands = (
                "cargo clippy --locked --workspace --all-targets --all-features -- -D warnings",
                "cargo test --locked --workspace --all-targets",
                "npm ci",
                macos_stage_command,
                unsigned_candidate_flag,
                'test "$GITHUB_REF" = "refs/heads/main"',
                upload_action,
                "name: macinmeter-unsigned-macos-arm64-${{ github.sha }}",
                "path: target/release-candidates/",
                "if-no-files-found: error",
                "retention-days: 14",
                "compression-level: 0",
            )
            for command in required_macos_commands:
                require(
                    command in macos_text,
                    f"{path.relative_to(root)} macOS gate is missing: {command}",
                )
            require(
                macos_text.count("if: github.event_name != 'pull_request'") == 2,
                f"{path.relative_to(root)} macOS Node and npm installs must remain "
                "main/manual-only",
            )
            require(
                macos_text.count("if: github.event_name == 'push'") == 1,
                f"{path.relative_to(root)} local-only macOS staging must remain "
                "main-push-only",
            )
            require(
                macos_text.count("if: github.event_name == 'workflow_dispatch'") == 3,
                f"{path.relative_to(root)} unsigned candidate guard, staging, and "
                "retention must remain manual-only",
            )
            require(
                "--allow-dirty" not in macos_text and "--replace" not in macos_text,
                f"{path.relative_to(root)} macOS CI staging must require a clean, "
                "new release directory",
            )
        require(
            workflow_text.count(macos_stage_command) == 2
            and macos_job is not None
            and "\n".join(macos_job).count(macos_stage_command) == 2,
            f"{path.relative_to(root)} local staging and unsigned candidate staging "
            "must appear only inside the macOS arm64 job",
        )
        # Two uploads, and each job may hold only its own. The macOS one is the
        # release candidate ADR-0011 defines; the Windows one is a test build
        # outside that scope. Counting them separately keeps a future edit from
        # turning the Windows path into a second candidate by accident.
        require(
            workflow_text.count("actions/upload-artifact@") == 2,
            f"{path.relative_to(root)} must retain exactly the macOS candidate upload "
            "and the Windows test-build upload",
        )
        require(
            macos_job is not None
            and "\n".join(macos_job).count(upload_action) == 1
            and windows_job is not None
            and "\n".join(windows_job).count(upload_action) == 1,
            f"{path.relative_to(root)} each retained upload must be pinned and stay "
            "inside its own platform job",
        )

        forbidden_ci_tasks = {
            "verify-malformed-corpus.py": "the hostile corpus verifier",
            "run-performance-baseline.py": "performance baselines",
            "run-performance-profile.py": "performance profiling",
            "gh release": "GitHub Release publication",
            "notarytool": "notarization",
            "codesign --sign": "release signing",
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
