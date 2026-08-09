#!/usr/bin/env python3
"""Stage and verify MacinMeter release artifacts and candidates.

The default `stage` command builds the host CLI from locked sources, packages
the actual distributed bytes, runs smoke checks after extraction, and writes a
SHA-256 manifest. macOS and Windows GUI staging is explicit through
`--include-gui`. Unsigned release candidates require the matching platform flag
and a clean source tree.

This script never uploads, signs, or creates a GitHub release.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import plistlib
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path, PurePosixPath

try:
    import tomllib
except ImportError:  # pragma: no cover - Python reports the actionable error.
    print("Python 3.11 or newer is required for release staging.", file=sys.stderr)
    raise SystemExit(2)


RELEASE_SCHEMA_VERSION = 1
WIRE_SCHEMA_VERSION = 4
APPLE_SILICON_TARGET = "aarch64-apple-darwin"
WINDOWS_X64_TARGET = "x86_64-pc-windows-msvc"
MACOS_MINIMUM_SYSTEM_VERSION = "11.0"
LOCAL_STAGING_SCOPE = "local_staging_only"
UNSIGNED_MACOS_ARM64_SCOPE = "unsigned_macos_arm64_release_candidate"
UNSIGNED_WINDOWS_X64_SCOPE = "unsigned_windows_x64_release_candidate"
WINDOWS_GUI_EXECUTABLE = "macinmeter-gui.exe"
PE_MAGIC = b"MZ"
PE_SIGNATURE = b"PE\0\0"
PE_POINTER_OFFSET = 0x3C
PE_MACHINE_AMD64 = 0x8664
WINDOWS_INSPECT_PATH_ENV = "MACINMETER_WINDOWS_FILE"
CHECKSUM_FILE = "SHA256SUMS"
RELEASE_MANIFEST = "RELEASE_MANIFEST.json"
ARTIFACT_MANIFEST = "ARTIFACT_MANIFEST.json"
PAYLOAD_DOCUMENTS = (
    "LICENSE",
    "README.md",
    "RELEASE_NOTES.md",
    "THIRD_PARTY_NOTICES.md",
)
DEFAULT_SMOKE_FIXTURE = Path(
    "tests/fixtures/native-pcm-v1/wav-pcm-s16-stereo.wav"
)
MAX_ARCHIVE_MEMBERS = 32
MAX_ARCHIVE_BYTES = 1024 * 1024 * 1024


class ReleaseError(RuntimeError):
    pass


def npm_executable() -> str:
    """Resolve npm to a launchable path.

    On Windows npm ships as `npm.CMD`, which cannot be started by bare name:
    Windows process creation runs executables, not batch files, so the same
    argv that works everywhere else fails there. Resolving through PATH keeps
    one call shape for both hosts.
    """
    resolved = shutil.which("npm")
    if resolved is None:
        raise ReleaseError("npm is required for release staging but was not found")
    return resolved


def run(
    command: list[str],
    *,
    cwd: Path,
    capture: bool = False,
    timeout: float | None = None,
    environment: dict[str, str] | None = None,
) -> subprocess.CompletedProcess:
    try:
        return subprocess.run(
            command,
            cwd=cwd,
            check=True,
            capture_output=capture,
            text=capture,
            timeout=timeout,
            env=environment,
        )
    except subprocess.CalledProcessError as error:
        detail = ""
        if capture:
            detail = f"\nstdout:\n{error.stdout}\nstderr:\n{error.stderr}"
        raise ReleaseError(
            f"command failed ({error.returncode}): {' '.join(command)}{detail}"
        ) from error
    except subprocess.TimeoutExpired as error:
        raise ReleaseError(f"command timed out: {' '.join(command)}") from error


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def json_bytes(document: dict) -> bytes:
    return (json.dumps(document, indent=2, sort_keys=True) + "\n").encode("utf-8")


def write_json(path: Path, document: dict) -> None:
    path.write_bytes(json_bytes(document))


def workspace_identity(root: Path) -> dict:
    with (root / "Cargo.toml").open("rb") as file:
        cargo = tomllib.load(file)
    package = cargo["workspace"]["package"]
    return {
        "version": package["version"],
        "edition": package["edition"],
        "msrv": package["rust-version"],
    }


def toolchain_identity(root: Path) -> dict:
    rustc = run(["rustc", "-vV"], cwd=root, capture=True)
    fields = {}
    for line in rustc.stdout.splitlines():
        if ": " in line:
            key, value = line.split(": ", 1)
            fields[key] = value
    host = fields.get("host")
    release = fields.get("release")
    if not host or not release:
        raise ReleaseError("rustc -vV did not report host and release")
    cargo = run(["cargo", "-V"], cwd=root, capture=True).stdout.strip()
    node = run(["node", "--version"], cwd=root, capture=True).stdout.strip()
    npm = run([npm_executable(), "--version"], cwd=root, capture=True).stdout.strip()
    return {
        "host": host,
        "rustc": release,
        "cargo": cargo,
        "node": node,
        "npm": npm,
    }


def version_tuple(value: str) -> tuple[int, int, int]:
    match = re.match(r"^(\d+)\.(\d+)(?:\.(\d+))?", value)
    if not match:
        raise ReleaseError(f"invalid semantic version: {value}")
    major, minor, patch = match.groups()
    return int(major), int(minor), int(patch or 0)


def git_identity(root: Path, allow_dirty: bool) -> dict:
    commit = run(
        ["git", "rev-parse", "HEAD"], cwd=root, capture=True
    ).stdout.strip()
    status = run(
        ["git", "status", "--porcelain", "--untracked-files=normal"],
        cwd=root,
        capture=True,
    ).stdout
    dirty = bool(status.strip())
    if dirty and not allow_dirty:
        raise ReleaseError(
            "release staging requires a clean worktree; commit the intended "
            "source or pass --allow-dirty for a visibly marked development artifact"
        )
    return {
        "commit": commit,
        "state": "dirty" if dirty else "clean",
    }


def source_identity(root: Path, allow_dirty: bool) -> dict:
    identity = git_identity(root, allow_dirty)
    identity.update(
        {
            "cargoLockSha256": sha256_file(root / "Cargo.lock"),
            "npmLockSha256": sha256_file(root / "tauri-app/package-lock.json"),
        }
    )
    return identity


def ensure_release_inputs(root: Path) -> None:
    run(
        [sys.executable, "scripts/check-repository-contract.py"],
        cwd=root,
    )
    run(
        [npm_executable(), "--prefix", "tauri-app", "run", "check-version"],
        cwd=root,
    )


def normalized_file_record(path: Path, relative: str, role: str) -> dict:
    return {
        "path": relative,
        "role": role,
        "sha256": sha256_file(path),
        "sizeBytes": path.stat().st_size,
    }


def tar_info(name: str, *, mode: int, size: int = 0, directory: bool = False):
    info = tarfile.TarInfo(name)
    info.mode = mode
    info.mtime = 0
    info.uid = 0
    info.gid = 0
    info.uname = ""
    info.gname = ""
    info.size = size
    if directory:
        info.type = tarfile.DIRTYPE
    return info


def write_deterministic_tar_gz(
    payload: Path,
    archive: Path,
    archive_root: str,
) -> None:
    archive.parent.mkdir(parents=True, exist_ok=True)
    with archive.open("wb") as raw:
        with gzip.GzipFile(
            filename="", mode="wb", fileobj=raw, compresslevel=9, mtime=0
        ) as compressed:
            with tarfile.open(
                fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT
            ) as output:
                output.addfile(
                    tar_info(archive_root, mode=0o755, directory=True)
                )
                directories = sorted(
                    path for path in payload.rglob("*") if path.is_dir()
                )
                for directory in directories:
                    relative = directory.relative_to(payload).as_posix()
                    output.addfile(
                        tar_info(
                            f"{archive_root}/{relative}",
                            mode=0o755,
                            directory=True,
                        )
                    )
                files = sorted(path for path in payload.rglob("*") if path.is_file())
                for file in files:
                    relative = file.relative_to(payload).as_posix()
                    mode = 0o755 if os.access(file, os.X_OK) else 0o644
                    with file.open("rb") as contents:
                        output.addfile(
                            tar_info(
                                f"{archive_root}/{relative}",
                                mode=mode,
                                size=file.stat().st_size,
                            ),
                            contents,
                        )


def safe_extract_tar_gz(archive: Path, destination: Path) -> Path:
    destination.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive, mode="r:gz") as source:
        members = source.getmembers()
        if not members or len(members) > MAX_ARCHIVE_MEMBERS:
            raise ReleaseError("CLI archive has an invalid member count")
        total_size = sum(member.size for member in members if member.isfile())
        if total_size > MAX_ARCHIVE_BYTES:
            raise ReleaseError("CLI archive expands beyond the release limit")

        names: set[str] = set()
        roots: set[str] = set()
        for member in members:
            pure = PurePosixPath(member.name)
            if (
                pure.is_absolute()
                or not pure.parts
                or any(part in ("", ".", "..") for part in pure.parts)
            ):
                raise ReleaseError(f"unsafe archive path: {member.name}")
            if member.name in names:
                raise ReleaseError(f"duplicate archive path: {member.name}")
            if not (member.isdir() or member.isfile()):
                raise ReleaseError(f"unsupported archive member: {member.name}")
            names.add(member.name)
            roots.add(pure.parts[0])

        if len(roots) != 1:
            raise ReleaseError("CLI archive must contain exactly one root directory")

        for member in members:
            pure = PurePosixPath(member.name)
            target = destination.joinpath(*pure.parts)
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
                target.chmod(member.mode & 0o777)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            source_file = source.extractfile(member)
            if source_file is None:
                raise ReleaseError(f"archive member has no contents: {member.name}")
            with source_file, target.open("wb") as output:
                shutil.copyfileobj(source_file, output)
            target.chmod(member.mode & 0o777)

    return destination / next(iter(roots))


def validate_analysis_smoke(document: dict, version: str) -> None:
    try:
        algorithm = document["data"]["analysis"]["algorithm"]
        source = document["data"]["source"]
    except (KeyError, TypeError) as error:
        raise ReleaseError("CLI smoke JSON is missing analysis fields") from error
    expected = {
        "schemaVersion": WIRE_SCHEMA_VERSION,
        "toolVersion": version,
        "kind": "analysis",
    }
    for key, value in expected.items():
        if document.get(key) != value:
            raise ReleaseError(
                f"CLI smoke JSON {key} is {document.get(key)!r}, expected {value!r}"
            )
    if "profile" in algorithm or "profileVersion" in algorithm:
        raise ReleaseError("CLI smoke JSON must not expose an algorithm profile")
    if "compatibility" in algorithm:
        raise ReleaseError("CLI smoke JSON must not attach compatibility to a report")
    parameters = algorithm.get("parameters")
    if not isinstance(parameters, dict) or parameters.get("histogramBins") != 10_001:
        raise ReleaseError("CLI smoke JSON is missing the fixed algorithm parameters")
    if (source.get("container"), source.get("codec")) != (
        "wave",
        "pcm_integer",
    ):
        raise ReleaseError("CLI smoke fixture used an unexpected decoder route")


def smoke_cli(binary: Path, version: str, fixture: Path, cwd: Path) -> dict:
    version_result = run(
        [str(binary), "--version"], cwd=cwd, capture=True, timeout=15
    )
    expected_version = f"mdrmeter {version}"
    if version_result.stdout.strip() != expected_version:
        raise ReleaseError(
            f"CLI version smoke returned {version_result.stdout.strip()!r}; "
            f"expected {expected_version!r}"
        )
    analysis = run(
        [
            str(binary),
            "analyze",
            str(fixture),
            "--format",
            "json",
        ],
        cwd=cwd,
        capture=True,
        timeout=30,
    )
    try:
        document = json.loads(analysis.stdout)
    except json.JSONDecodeError as error:
        raise ReleaseError("CLI smoke stdout is not one JSON document") from error
    validate_analysis_smoke(document, version)
    return {
        "version": expected_version,
        "wireSchemaVersion": WIRE_SCHEMA_VERSION,
        "fixtureRoute": "wave/pcm_integer",
    }


def verify_cli_archive(
    archive: Path,
    *,
    version: str,
    target: str,
    source_identity: dict,
    toolchain_identity: dict,
    fixture: Path,
    root: Path,
) -> dict:
    with tempfile.TemporaryDirectory(prefix="macinmeter-cli-smoke-") as temporary:
        payload = safe_extract_tar_gz(archive, Path(temporary))
        manifest_path = payload / ARTIFACT_MANIFEST
        if not manifest_path.is_file():
            raise ReleaseError(f"{archive.name} is missing {ARTIFACT_MANIFEST}")
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        if manifest.get("schemaVersion") != RELEASE_SCHEMA_VERSION:
            raise ReleaseError("CLI artifact manifest schema drifted")
        if manifest.get("kind") != "cli":
            raise ReleaseError("CLI artifact manifest kind drifted")
        if manifest.get("version") != version or manifest.get("target") != target:
            raise ReleaseError("CLI artifact identity does not match the release")
        if manifest.get("source") != source_identity:
            raise ReleaseError("CLI artifact source identity does not match the release")
        if manifest.get("toolchain") != toolchain_identity:
            raise ReleaseError(
                "CLI artifact toolchain identity does not match the release"
            )

        expected_files = {ARTIFACT_MANIFEST}
        executable: Path | None = None
        for record in manifest.get("files", []):
            relative = record.get("path")
            if not isinstance(relative, str) or "/" in relative or "\\" in relative:
                raise ReleaseError("CLI artifact manifest contains an unsafe file path")
            path = payload / relative
            expected_files.add(relative)
            if not path.is_file():
                raise ReleaseError(f"CLI payload is missing {relative}")
            if path.stat().st_size != record.get("sizeBytes"):
                raise ReleaseError(f"CLI payload size drifted for {relative}")
            if sha256_file(path) != record.get("sha256"):
                raise ReleaseError(f"CLI payload hash drifted for {relative}")
            if record.get("role") == "executable":
                if executable is not None:
                    raise ReleaseError("CLI payload has multiple executables")
                executable = path

        actual_files = {
            path.relative_to(payload).as_posix()
            for path in payload.rglob("*")
            if path.is_file()
        }
        if actual_files != expected_files:
            raise ReleaseError("CLI payload contains unrecorded or missing files")
        if executable is None or not os.access(executable, os.X_OK):
            raise ReleaseError("CLI payload executable is missing or not executable")
        return smoke_cli(executable, version, fixture, root)


def build_cli_payload(
    root: Path,
    working: Path,
    *,
    version: str,
    version_label: str,
    target: str,
    source: dict,
    toolchain: dict,
) -> Path:
    run(
        ["cargo", "build", "--locked", "--release", "-p", "macinmeter-cli"],
        cwd=root,
    )
    executable_name = "mdrmeter.exe" if target.endswith("-windows-msvc") else "mdrmeter"
    built_binary = root / "target/release" / executable_name
    if not built_binary.is_file():
        raise ReleaseError(f"release CLI was not produced at {built_binary}")

    payload = working / "cli-payload"
    payload.mkdir()
    staged_binary = payload / executable_name
    shutil.copyfile(built_binary, staged_binary)
    staged_binary.chmod(0o755)
    records = [
        normalized_file_record(staged_binary, executable_name, "executable")
    ]
    for document_name in PAYLOAD_DOCUMENTS:
        source_document = root / document_name
        if not source_document.is_file():
            raise ReleaseError(f"release document is missing: {document_name}")
        destination = payload / document_name
        shutil.copyfile(source_document, destination)
        destination.chmod(0o644)
        records.append(
            normalized_file_record(destination, document_name, "documentation")
        )

    artifact_manifest = {
        "schemaVersion": RELEASE_SCHEMA_VERSION,
        "kind": "cli",
        "product": "macinmeter",
        "version": version,
        "target": target,
        "analysis": {
            "wireSchemaVersion": WIRE_SCHEMA_VERSION,
        },
        "source": source,
        "toolchain": toolchain,
        "files": records,
    }
    write_json(payload / ARTIFACT_MANIFEST, artifact_manifest)

    archive_root = f"macinmeter-cli-{version_label}-{target}"
    archive = working / f"{archive_root}.tar.gz"
    write_deterministic_tar_gz(payload, archive, archive_root)
    return archive


def macos_bundle_arch(target: str) -> str:
    if target == APPLE_SILICON_TARGET:
        return "aarch64"
    raise ReleaseError(
        f"GUI release staging supports Apple Silicon only; found {target}"
    )


def macos_binary_arch(target: str) -> str:
    if target == APPLE_SILICON_TARGET:
        return "arm64"
    raise ReleaseError(
        f"macOS binary inspection supports Apple Silicon only; found {target}"
    )


def hdiutil(command: list[str], root: Path, *, capture: bool = False):
    return run(["hdiutil", *command], cwd=root, capture=capture, timeout=120)


def smoke_macos_dmg(
    dmg: Path,
    *,
    version: str,
    identifier: str,
    target: str,
    root: Path,
) -> dict:
    if sys.platform != "darwin":
        raise ReleaseError("macOS DMG verification requires macOS")
    hdiutil(["verify", str(dmg)], root)
    attached = hdiutil(
        [
            "attach",
            "-readonly",
            "-nobrowse",
            "-noautoopen",
            "-plist",
            str(dmg),
        ],
        root,
        capture=True,
    )
    try:
        attachment = plistlib.loads(attached.stdout.encode("utf-8"))
    except plistlib.InvalidFileException as error:
        raise ReleaseError("hdiutil attach did not return a valid plist") from error
    entities = attachment.get("system-entities", [])
    mounted = [
        entity
        for entity in entities
        if entity.get("mount-point")
    ]
    detach_target = next(
        (
            entity["dev-entry"]
            for entity in entities
            if entity.get("dev-entry")
        ),
        None,
    )

    try:
        if len(mounted) != 1:
            raise ReleaseError("DMG must expose exactly one mounted volume")
        mount_point = Path(mounted[0]["mount-point"])
        detach_target = mounted[0].get("dev-entry", detach_target)
        applications = list(mount_point.glob("*.app"))
        if len(applications) != 1:
            raise ReleaseError("DMG must contain exactly one top-level app bundle")
        app = applications[0]
        info_path = app / "Contents/Info.plist"
        with info_path.open("rb") as file:
            info = plistlib.load(file)
        if info.get("CFBundleShortVersionString") != version:
            raise ReleaseError("GUI bundle version does not match the release")
        if info.get("CFBundleIdentifier") != identifier:
            raise ReleaseError("GUI bundle identifier does not match tauri.conf.json")
        if info.get("LSMinimumSystemVersion") != MACOS_MINIMUM_SYSTEM_VERSION:
            raise ReleaseError(
                "GUI bundle minimum system version does not match the "
                "Apple Silicon release contract"
            )
        executable_name = info.get("CFBundleExecutable")
        executable = app / "Contents/MacOS" / str(executable_name)
        if not executable.is_file() or not os.access(executable, os.X_OK):
            raise ReleaseError("GUI bundle executable is missing or not executable")
        architectures = run(
            ["lipo", "-archs", str(executable)],
            cwd=root,
            capture=True,
        ).stdout.split()
        expected_architecture = macos_binary_arch(target)
        if architectures != [expected_architecture]:
            raise ReleaseError(
                f"GUI bundle architectures are {architectures}, expected "
                f"[{expected_architecture!r}]"
            )
        signature = subprocess.run(
            ["codesign", "--verify", "--deep", "--strict", str(app)],
            cwd=root,
            capture_output=True,
            text=True,
        )
        signature_details = subprocess.run(
            ["codesign", "--display", "--verbose=4", str(app)],
            cwd=root,
            capture_output=True,
            text=True,
        )
        signature_description = (
            signature_details.stdout + "\n" + signature_details.stderr
        )
        developer_id_signed = "Authority=Developer ID Application:" in (
            signature_description
        )
        team_identifier = re.search(
            r"^TeamIdentifier=(.+)$", signature_description, re.MULTILINE
        )
        identified_apple_signature = (
            "Authority=" in signature_description
            or (
                team_identifier is not None
                and team_identifier.group(1).strip() not in ("", "not set")
            )
        )
        smoke = {
            "container": "dmg",
            "bundle": app.name,
            "bundleIdentifier": identifier,
            "bundleVersion": version,
            "architecture": expected_architecture,
            "minimumSystemVersion": MACOS_MINIMUM_SYSTEM_VERSION,
            "strictCodeSignatureValid": signature.returncode == 0,
            "developerIdSigned": developer_id_signed,
            "identifiedAppleSignature": identified_apple_signature,
            "launch": "not_performed",
        }
    finally:
        if detach_target is None:
            raise ReleaseError("could not identify the attached DMG device")
        detached = subprocess.run(
            ["hdiutil", "detach", str(detach_target)],
            cwd=root,
            capture_output=True,
            text=True,
        )
        if detached.returncode != 0:
            forced = subprocess.run(
                ["hdiutil", "detach", "-force", str(detach_target)],
                cwd=root,
                capture_output=True,
                text=True,
            )
            if forced.returncode != 0:
                raise ReleaseError(
                    f"failed to detach DMG volume {detach_target}: {forced.stderr}"
                )
    return smoke


def seven_zip(command: list[str], root: Path):
    """Run 7-Zip, which ADR-0015 makes an explicit Windows staging dependency.

    A missing 7-Zip fails the stage rather than silently degrading to a shallow
    check, because the point of the extraction is that "verified" means the same
    thing on both platforms.
    """
    try:
        return run(["7z", *command], cwd=root, capture=True, timeout=180)
    except FileNotFoundError as error:
        raise ReleaseError(
            "7z is required to verify the Windows installer; install 7-Zip"
        ) from error


def windows_pe_machine(path: Path) -> int:
    """Read and validate the COFF Machine field from a PE executable."""
    with path.open("rb") as file:
        dos_header = file.read(PE_POINTER_OFFSET + 4)
        if len(dos_header) < PE_POINTER_OFFSET + 4 or dos_header[:2] != PE_MAGIC:
            raise ReleaseError(f"{path.name} does not have a valid DOS header")
        pe_offset = int.from_bytes(
            dos_header[PE_POINTER_OFFSET : PE_POINTER_OFFSET + 4], "little"
        )
        file.seek(pe_offset)
        pe_header = file.read(6)
    if len(pe_header) != 6 or pe_header[:4] != PE_SIGNATURE:
        raise ReleaseError(f"{path.name} does not have a valid PE header")
    return int.from_bytes(pe_header[4:6], "little")


def windows_machine_name(machine: int) -> str:
    return {
        0x014C: "x86",
        PE_MACHINE_AMD64: "x86_64",
        0xAA64: "arm64",
    }.get(machine, f"unknown_0x{machine:04x}")


def windows_executable_info(path: Path, root: Path) -> dict:
    """Read version and Authenticode status without interpolating the path."""
    resolved = path.resolve()
    # PowerShell reports an unreadable file as a non-terminating error and still
    # exits zero, so an inspection failure would otherwise arrive as an empty
    # status and be reported as a signing problem. Establish the precondition
    # here, where the message can name the file.
    if not resolved.is_file():
        raise ReleaseError(f"cannot inspect a missing Windows executable: {resolved}")
    environment = os.environ.copy()
    environment[WINDOWS_INSPECT_PATH_ENV] = str(resolved)
    command = (
        "$ErrorActionPreference = 'Stop'; "
        f"$target = $env:{WINDOWS_INSPECT_PATH_ENV}; "
        "if ([string]::IsNullOrEmpty($target)) { "
        "throw 'the inspection path did not reach PowerShell' }; "
        "$item = Get-Item -LiteralPath $target; "
        "$signature = Get-AuthenticodeSignature -LiteralPath $target; "
        "if ($null -eq $signature) { "
        "throw \"Get-AuthenticodeSignature returned nothing for $target\" }; "
        "$signerSubject = $null; "
        "if ($null -ne $signature.SignerCertificate) { "
        "$signerSubject = $signature.SignerCertificate.Subject }; "
        "[ordered]@{ "
        "fileVersion = $item.VersionInfo.FileVersion; "
        "authenticodeStatus = [string]$signature.Status; "
        "signerSubject = $signerSubject "
        "} | ConvertTo-Json -Compress"
    )
    try:
        result = run(
            [
                "powershell.exe",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                command,
            ],
            cwd=root,
            capture=True,
            timeout=60,
            environment=environment,
        )
    except ReleaseError as error:
        raise ReleaseError(
            f"failed to inspect Windows executable {path}: {error}"
        ) from error
    try:
        document = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise ReleaseError(
            f"Windows executable inspection returned invalid JSON for {path}"
        ) from error
    if not isinstance(document, dict):
        raise ReleaseError(
            f"Windows executable inspection returned no object for {path}"
        )
    file_version = document.get("fileVersion")
    status = document.get("authenticodeStatus")
    signer = document.get("signerSubject")
    if file_version is not None and not isinstance(file_version, str):
        raise ReleaseError(f"Windows executable version is invalid for {path}")
    # An empty status is an inspection that did not happen, not a file that is
    # unsigned. Refusing it here keeps a failed check from being read as a
    # verified absence of a signature.
    if not isinstance(status, str) or not status:
        raise ReleaseError(f"Windows Authenticode status is missing for {path}")
    if signer is not None and not isinstance(signer, str):
        raise ReleaseError(f"Windows Authenticode signer is invalid for {path}")
    return document


def require_unsigned_authenticode(info: dict, label: str) -> None:
    status = info["authenticodeStatus"]
    signer = info["signerSubject"]
    if status != "NotSigned" or signer is not None:
        raise ReleaseError(
            f"{label} must be unsigned; Authenticode status is {status!r}, "
            f"signer is {signer!r}"
        )


def smoke_windows_installer(
    installer: Path,
    *,
    version: str,
    target: str,
    root: Path,
) -> dict:
    """Open the installer and inspect what it actually carries.

    This is the Windows counterpart to mounting the DMG: confirming that the
    file is a well-formed PE would not establish that the payload inside it is
    the GUI that was built, which is exactly what the macOS side does establish.
    """
    if target != WINDOWS_X64_TARGET:
        raise ReleaseError(
            f"Windows GUI staging supports {WINDOWS_X64_TARGET} only; found {target}"
        )
    installer_machine = windows_pe_machine(installer)
    installer_info = windows_executable_info(installer, root)
    require_unsigned_authenticode(installer_info, "Windows installer")

    # Extraction is deliberately outside the candidate directory. Even if the
    # host prevents cleanup, unmanifested payload bytes can never be moved into
    # or uploaded with a successfully staged release.
    with tempfile.TemporaryDirectory(prefix="mdrmeter-nsis-") as temporary:
        extracted = Path(temporary)
        seven_zip(["x", str(installer), f"-o{extracted}", "-y"], root)
        payloads = sorted(
            path
            for path in extracted.rglob(WINDOWS_GUI_EXECUTABLE)
            if path.is_file() and not path.is_symlink()
        )
        if len(payloads) != 1:
            raise ReleaseError(
                f"the installer must carry exactly one {WINDOWS_GUI_EXECUTABLE}; found "
                + (
                    ", ".join(
                        str(path.relative_to(extracted)) for path in payloads
                    )
                    or "none"
                )
            )
        payload = payloads[0]
        payload_machine = windows_pe_machine(payload)
        if payload_machine != PE_MACHINE_AMD64:
            raise ReleaseError(
                f"installer payload architecture is "
                f"{windows_machine_name(payload_machine)!r}; expected 'x86_64'"
            )
        payload_info = windows_executable_info(payload, root)
        require_unsigned_authenticode(payload_info, "Windows installer payload")
        payload_version = payload_info["fileVersion"]
        if not payload_version:
            raise ReleaseError("installer payload has no file version resource")
        if version_tuple(payload_version) != version_tuple(version):
            raise ReleaseError(
                f"installer payload reports version {payload_version!r}; "
                f"expected {version}"
            )
        smoke = {
            "container": "nsis",
            "payload": WINDOWS_GUI_EXECUTABLE,
            "payloadVersion": payload_version,
            "payloadSha256": sha256_file(payload),
            "installerMachine": windows_machine_name(installer_machine),
            "payloadMachine": windows_machine_name(payload_machine),
            "architecture": windows_machine_name(payload_machine),
            "installerAuthenticodeStatus": installer_info["authenticodeStatus"],
            "payloadAuthenticodeStatus": payload_info["authenticodeStatus"],
            "launch": "not_performed",
        }
    return smoke


def build_windows_installer(
    root: Path,
    working: Path,
    *,
    version: str,
    version_label: str,
    target: str,
) -> tuple[Path, str]:
    config = json.loads(
        (root / "tauri-app/src-tauri/tauri.conf.json").read_text(encoding="utf-8")
    )
    identifier = config["identifier"]
    run(
        [
            npm_executable(),
            "--prefix",
            "tauri-app",
            "run",
            "tauri",
            "--",
            "build",
            "--bundles",
            "nsis",
        ],
        cwd=root,
    )
    candidates = sorted(
        path
        for path in (root / "target/release/bundle/nsis").glob("*-setup.exe")
        if version in path.name
    )
    if len(candidates) != 1:
        raise ReleaseError(
            "Tauri must produce exactly one NSIS installer; found "
            + (", ".join(path.name for path in candidates) or "none")
        )
    destination = working / f"macinmeter-gui-{version_label}-{target}-setup.exe"
    shutil.copyfile(candidates[0], destination)
    return destination, identifier


def build_gui_artifact(
    root: Path,
    working: Path,
    *,
    version: str,
    version_label: str,
    target: str,
) -> tuple[Path, str]:
    # The GUI is bundled by the host for the host: neither platform can produce
    # the other's installer, so the split is on where this runs rather than on a
    # requested target.
    if sys.platform == "win32":
        return build_windows_installer(
            root,
            working,
            version=version,
            version_label=version_label,
            target=target,
        )
    if sys.platform != "darwin":
        raise ReleaseError("--include-gui supports macOS and Windows hosts only")
    arch = macos_bundle_arch(target)
    config = json.loads(
        (root / "tauri-app/src-tauri/tauri.conf.json").read_text(encoding="utf-8")
    )
    identifier = config["identifier"]
    run(
        [npm_executable(), "--prefix", "tauri-app", "run", "tauri", "--", "build"],
        cwd=root,
    )
    candidates = sorted(
        path
        for path in (root / "target/release/bundle/dmg").glob("*.dmg")
        if version in path.name and path.stem.endswith(f"_{arch}")
    )
    if len(candidates) != 1:
        raise ReleaseError(
            "Tauri must produce exactly one current host DMG; found "
            + ", ".join(path.name for path in candidates)
        )
    destination = (
        working / f"macinmeter-gui-{version_label}-{target}.dmg"
    )
    shutil.copyfile(candidates[0], destination)
    return destination, identifier


def write_checksums(release_dir: Path, names: set[str]) -> None:
    lines = [f"{sha256_file(release_dir / name)}  {name}" for name in sorted(names)]
    (release_dir / CHECKSUM_FILE).write_text("\n".join(lines) + "\n", encoding="utf-8")


def parse_checksums(path: Path) -> dict[str, str]:
    checksums = {}
    pattern = re.compile(r"^([0-9a-f]{64})  ([A-Za-z0-9][A-Za-z0-9._-]*)$")
    for line in path.read_text(encoding="utf-8").splitlines():
        match = pattern.fullmatch(line)
        if not match:
            raise ReleaseError(f"invalid checksum line: {line!r}")
        digest, name = match.groups()
        if name in checksums:
            raise ReleaseError(f"duplicate checksum entry: {name}")
        checksums[name] = digest
    return checksums


def distribution_contract(
    unsigned_macos_arm64_candidate: bool,
    unsigned_windows_x64_candidate: bool = False,
) -> dict:
    if unsigned_windows_x64_candidate:
        return {
            "scope": UNSIGNED_WINDOWS_X64_SCOPE,
            "platform": "windows",
            "architecture": "x86_64",
            "signing": "authenticode_not_performed",
            "notarization": "not_applicable",
            "smartScreen": "not_claimed",
            "upload": "permitted_after_verification",
            "publication": "requires_explicit_confirmation",
        }
    if unsigned_macos_arm64_candidate:
        return {
            "scope": UNSIGNED_MACOS_ARM64_SCOPE,
            "platform": "macos",
            "architecture": "arm64",
            "minimumSystemVersion": MACOS_MINIMUM_SYSTEM_VERSION,
            "signing": "developer_id_not_performed",
            "notarization": "not_performed",
            "gatekeeper": "not_claimed",
            "upload": "permitted_after_verification",
            "publication": "requires_explicit_confirmation",
        }
    return {
        "scope": LOCAL_STAGING_SCOPE,
        "signing": "not_performed",
        "notarization": "not_performed",
        "upload": "not_performed",
    }


def validate_stage_scope(
    *,
    unsigned_macos_arm64_candidate: bool,
    unsigned_windows_x64_candidate: bool,
    include_gui: bool,
    allow_dirty: bool,
    replace: bool,
    target: str,
) -> None:
    # One stage produces one platform's candidate. ADR-0015 merges the two into
    # a single release by hand rather than letting either host claim to have
    # produced the other's GUI.
    if unsigned_macos_arm64_candidate and unsigned_windows_x64_candidate:
        raise ReleaseError("a stage produces one platform's candidate, not both")
    if unsigned_macos_arm64_candidate:
        expected_target = APPLE_SILICON_TARGET
        platform_label = "Apple Silicon"
    elif unsigned_windows_x64_candidate:
        expected_target = WINDOWS_X64_TARGET
        platform_label = "Windows x64"
    else:
        return
    if target != expected_target:
        raise ReleaseError(
            f"unsigned {platform_label} release candidates support "
            f"{expected_target} only; found {target}"
        )
    if not include_gui:
        raise ReleaseError(
            f"unsigned {platform_label} release candidates must include the GUI"
        )
    if allow_dirty:
        raise ReleaseError("unsigned release candidates require a clean source tree")
    if replace:
        raise ReleaseError("unsigned release candidates cannot replace prior bytes")


def validate_candidate_toolchain(
    *,
    unsigned_macos_arm64_candidate: bool,
    unsigned_windows_x64_candidate: bool,
    package: dict,
    toolchain: dict,
) -> None:
    if not (unsigned_macos_arm64_candidate or unsigned_windows_x64_candidate):
        return
    if version_tuple(toolchain["rustc"]) != version_tuple(package["msrv"]):
        raise ReleaseError(
            "unsigned release candidates require the exact Rust 1.88 toolchain"
        )
    if version_tuple(toolchain["node"].removeprefix("v"))[0] != 22:
        raise ReleaseError(
            "unsigned release candidates require the pinned Node.js 22 toolchain"
        )


def validate_distribution_manifest(manifest: dict, artifacts: list[dict]) -> str:
    distribution = manifest.get("distribution")
    if not isinstance(distribution, dict):
        raise ReleaseError("release manifest distribution contract is missing")
    scope = distribution.get("scope")
    if scope == LOCAL_STAGING_SCOPE:
        if distribution != distribution_contract(False):
            raise ReleaseError("local staging distribution contract drifted")
        return scope
    if scope == UNSIGNED_MACOS_ARM64_SCOPE:
        expected_contract = distribution_contract(True)
        expected_target = APPLE_SILICON_TARGET
        gui_kind = "gui_macos_dmg"
        platform_label = "Apple Silicon"
    elif scope == UNSIGNED_WINDOWS_X64_SCOPE:
        expected_contract = distribution_contract(False, True)
        expected_target = WINDOWS_X64_TARGET
        gui_kind = "gui_windows_nsis"
        platform_label = "Windows x64"
    else:
        raise ReleaseError("release manifest has an unknown distribution scope")
    if distribution != expected_contract:
        raise ReleaseError(f"unsigned {platform_label} distribution contract drifted")
    if manifest.get("target") != expected_target:
        raise ReleaseError(
            f"unsigned release candidate target must be {expected_target}"
        )
    if manifest.get("source", {}).get("state") != "clean":
        raise ReleaseError("unsigned release candidate source must be clean")
    if len(artifacts) != 2 or {
        artifact.get("kind") for artifact in artifacts
    } != {"cli", gui_kind}:
        raise ReleaseError("unsigned release candidate must contain CLI and GUI")
    gui = next(artifact for artifact in artifacts if artifact.get("kind") == gui_kind)
    if gui.get("publicationStatus") != "unsigned_release_candidate":
        raise ReleaseError("unsigned GUI publication status drifted")
    return scope


def verify_release_dir(
    release_dir: Path,
    *,
    root: Path,
    fixture: Path,
) -> dict:
    if not release_dir.is_dir() or release_dir.is_symlink():
        raise ReleaseError("release path must be a real directory")
    manifest_path = release_dir / RELEASE_MANIFEST
    checksum_path = release_dir / CHECKSUM_FILE
    if (
        not manifest_path.is_file()
        or manifest_path.is_symlink()
        or not checksum_path.is_file()
        or checksum_path.is_symlink()
    ):
        raise ReleaseError("release directory is missing its manifest or checksums")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("schemaVersion") != RELEASE_SCHEMA_VERSION:
        raise ReleaseError("release manifest schema drifted")
    version = manifest.get("version")
    target = manifest.get("target")
    if not isinstance(version, str) or not isinstance(target, str):
        raise ReleaseError("release manifest identity is incomplete")
    source = manifest.get("source")
    toolchain = manifest.get("toolchain")
    if not isinstance(source, dict) or not isinstance(toolchain, dict):
        raise ReleaseError("release manifest source or toolchain identity is incomplete")
    artifacts = manifest.get("artifacts")
    if not isinstance(artifacts, list) or not artifacts:
        raise ReleaseError("release manifest contains no artifacts")
    distribution_scope = validate_distribution_manifest(manifest, artifacts)
    expected_names = {RELEASE_MANIFEST}
    for artifact in artifacts:
        name = artifact.get("file")
        if (
            not isinstance(name, str)
            or "/" in name
            or "\\" in name
            or name in expected_names
        ):
            raise ReleaseError("release manifest contains an unsafe artifact name")
        expected_names.add(name)

    checksums = parse_checksums(checksum_path)
    if set(checksums) != expected_names:
        raise ReleaseError("SHA256SUMS does not cover the exact release file set")
    actual_names = {
        path.name for path in release_dir.iterdir() if path.name != CHECKSUM_FILE
    }
    if actual_names != expected_names:
        raise ReleaseError("release directory contains unrecorded or missing files")
    for name, digest in checksums.items():
        path = release_dir / name
        if not path.is_file() or path.is_symlink():
            raise ReleaseError(f"release entry is not a regular file: {name}")
        if sha256_file(path) != digest:
            raise ReleaseError(f"checksum mismatch: {name}")

    smoke_results = {}
    for artifact in artifacts:
        path = release_dir / artifact["file"]
        if path.stat().st_size != artifact.get("sizeBytes"):
            raise ReleaseError(f"release artifact size drifted: {path.name}")
        if sha256_file(path) != artifact.get("sha256"):
            raise ReleaseError(f"release artifact hash drifted: {path.name}")
        kind = artifact.get("kind")
        if kind == "cli":
            smoke_results[path.name] = verify_cli_archive(
                path,
                version=version,
                target=target,
                source_identity=source,
                toolchain_identity=toolchain,
                fixture=fixture,
                root=root,
            )
        elif kind == "gui_windows_nsis":
            smoke_results[path.name] = smoke_windows_installer(
                path,
                version=version,
                target=target,
                root=root,
            )
        elif kind == "gui_macos_dmg":
            gui_smoke = smoke_macos_dmg(
                path,
                version=version,
                identifier=artifact["bundleIdentifier"],
                target=target,
                root=root,
            )
            if (
                distribution_scope == UNSIGNED_MACOS_ARM64_SCOPE
                and gui_smoke["identifiedAppleSignature"]
            ):
                raise ReleaseError(
                    "unsigned release candidate unexpectedly has an Apple identity signature"
                )
            smoke_results[path.name] = gui_smoke
        else:
            raise ReleaseError(f"unknown release artifact kind: {kind}")
    return smoke_results


def stage_release(arguments: argparse.Namespace, root: Path) -> Path:
    ensure_release_inputs(root)
    package = workspace_identity(root)
    toolchain = toolchain_identity(root)
    if version_tuple(toolchain["rustc"]) < version_tuple(package["msrv"]):
        raise ReleaseError(
            f"rustc {toolchain['rustc']} is older than MSRV {package['msrv']}"
        )
    source = source_identity(root, arguments.allow_dirty)
    version = package["version"]
    version_label = (
        f"{version}-dirty" if source["state"] == "dirty" else version
    )
    target = toolchain["host"]
    macos_candidate = arguments.unsigned_macos_arm64_candidate
    windows_candidate = arguments.unsigned_windows_x64_candidate
    # Either flag makes this an immutable candidate rather than local staging;
    # which one decides the platform contract below.
    unsigned_candidate = macos_candidate or windows_candidate
    validate_stage_scope(
        unsigned_macos_arm64_candidate=macos_candidate,
        unsigned_windows_x64_candidate=windows_candidate,
        include_gui=arguments.include_gui,
        allow_dirty=arguments.allow_dirty,
        replace=arguments.replace,
        target=target,
    )
    validate_candidate_toolchain(
        unsigned_macos_arm64_candidate=macos_candidate,
        unsigned_windows_x64_candidate=windows_candidate,
        package=package,
        toolchain=toolchain,
    )
    default_output_root = root / (
        "target/release-candidates"
        if unsigned_candidate
        else "target/release-staging"
    )
    destination = (
        arguments.output_dir.resolve()
        if arguments.output_dir
        else default_output_root / version_label / target
    )
    if destination.exists() and not arguments.replace:
        if unsigned_candidate:
            raise ReleaseError(
                f"unsigned candidate directory already exists and is immutable: "
                f"{destination}"
            )
        raise ReleaseError(
            f"release directory already exists: {destination}; pass --replace"
        )
    default_staging_root = default_output_root.resolve()
    if destination.exists() and arguments.replace:
        try:
            relative_to_staging = destination.relative_to(default_staging_root)
        except ValueError:
            relative_to_staging = None
        generated_default = (
            relative_to_staging is not None
            and len(relative_to_staging.parts) >= 2
        )
        marked_release = (
            (destination / RELEASE_MANIFEST).is_file()
            and not (destination / RELEASE_MANIFEST).is_symlink()
            and (destination / CHECKSUM_FILE).is_file()
            and not (destination / CHECKSUM_FILE).is_symlink()
        )
        if destination.is_symlink() or not (generated_default or marked_release):
            raise ReleaseError(
                "--replace only removes a generated staging directory or a "
                "directory containing the release manifest and checksums"
            )
    destination.parent.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(
        prefix=".macinmeter-release-", dir=destination.parent
    ) as temporary:
        temporary_path = Path(temporary)
        release_dir = temporary_path / "release"
        release_dir.mkdir()
        cli_archive = build_cli_payload(
            root,
            temporary_path,
            version=version,
            version_label=version_label,
            target=target,
            source=source,
            toolchain=toolchain,
        )
        staged_cli = release_dir / cli_archive.name
        shutil.move(cli_archive, staged_cli)
        artifacts = [
            {
                "kind": "cli",
                "file": staged_cli.name,
                "sha256": sha256_file(staged_cli),
                "sizeBytes": staged_cli.stat().st_size,
                "smokeContract": [
                    "archive_manifest",
                    "payload_hashes",
                    "extracted_version",
                    "fixture_json",
                ],
            }
        ]

        if arguments.include_gui:
            gui, identifier = build_gui_artifact(
                root,
                temporary_path,
                version=version,
                version_label=version_label,
                target=target,
            )
            staged_gui = release_dir / gui.name
            shutil.move(gui, staged_gui)
            windows_gui = sys.platform == "win32"
            artifacts.append(
                {
                    "kind": "gui_windows_nsis" if windows_gui else "gui_macos_dmg",
                    "file": staged_gui.name,
                    "sha256": sha256_file(staged_gui),
                    "sizeBytes": staged_gui.stat().st_size,
                    "bundleIdentifier": identifier,
                    "publicationStatus": (
                        "unsigned_release_candidate"
                        if unsigned_candidate
                        else (
                            "local_unsigned" if windows_gui else "local_unnotarized"
                        )
                    ),
                    "smokeContract": [
                        "installer_pe_identity",
                        "installer_authenticode_absence",
                        "extracted_payload_identity",
                        "payload_x86_64_pe_identity",
                        "payload_version_resource",
                        "payload_authenticode_absence",
                        "extracted_payload_sha256",
                    ]
                    if windows_gui
                    else [
                        "dmg_integrity",
                        "mounted_bundle_identity",
                        "bundle_executable",
                        "binary_architecture",
                        "code_signature_observation",
                    ],
                }
            )

        release_manifest = {
            "schemaVersion": RELEASE_SCHEMA_VERSION,
            "product": "macinmeter",
            "version": version,
            "versionLabel": version_label,
            "target": target,
            "source": source,
            "toolchain": toolchain,
            "workspace": {
                "edition": package["edition"],
                "msrv": package["msrv"],
            },
            "analysis": {
                "wireSchemaVersion": WIRE_SCHEMA_VERSION,
            },
            "distribution": distribution_contract(
                macos_candidate, windows_candidate
            ),
            "artifacts": artifacts,
        }
        write_json(release_dir / RELEASE_MANIFEST, release_manifest)
        write_checksums(
            release_dir,
            {artifact["file"] for artifact in artifacts} | {RELEASE_MANIFEST},
        )
        fixture = (root / arguments.fixture).resolve()
        if not fixture.is_file():
            raise ReleaseError(f"release smoke fixture is missing: {fixture}")
        smoke = verify_release_dir(release_dir, root=root, fixture=fixture)
        print(json.dumps({"smoke": smoke}, indent=2, sort_keys=True))

        if destination.exists():
            shutil.rmtree(destination)
        os.replace(release_dir, destination)

    label = "unsigned release candidate" if unsigned_candidate else "release"
    print(f"{label} staged at {destination}")
    return destination


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)

    stage = subcommands.add_parser("stage", help="build, stage, and verify artifacts")
    stage.add_argument(
        "--include-gui",
        action="store_true",
        help=(
            "also build and verify the current-host Tauri bundle: a macOS DMG "
            "or a Windows NSIS installer"
        ),
    )
    stage.add_argument(
        "--unsigned-macos-arm64-candidate",
        action="store_true",
        help=(
            "stage a clean, unsigned Apple Silicon CLI and GUI release candidate"
        ),
    )
    stage.add_argument(
        "--unsigned-windows-x64-candidate",
        action="store_true",
        help="stage a clean, unsigned Windows x64 CLI and GUI release candidate",
    )
    stage.add_argument(
        "--allow-dirty",
        action="store_true",
        help="allow a visibly marked dirty development artifact",
    )
    stage.add_argument(
        "--replace",
        action="store_true",
        help="replace an existing generated release directory",
    )
    stage.add_argument(
        "--output-dir",
        type=Path,
        help=(
            "explicit output directory (defaults under target/release-staging "
            "or target/release-candidates)"
        ),
    )
    stage.add_argument(
        "--fixture",
        type=Path,
        default=DEFAULT_SMOKE_FIXTURE,
        help="repository-relative WAV fixture used by the CLI smoke test",
    )

    verify = subcommands.add_parser(
        "verify", help="verify checksums and rerun artifact smoke tests"
    )
    verify.add_argument("release_dir", type=Path)
    verify.add_argument(
        "--fixture",
        type=Path,
        default=DEFAULT_SMOKE_FIXTURE,
        help="repository-relative WAV fixture used by the CLI smoke test",
    )
    return parser


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    arguments = build_parser().parse_args()
    try:
        if arguments.command == "stage":
            stage_release(arguments, root)
        else:
            smoke = verify_release_dir(
                arguments.release_dir.resolve(),
                root=root,
                fixture=(root / arguments.fixture).resolve(),
            )
            print(json.dumps({"smoke": smoke}, indent=2, sort_keys=True))
    except (OSError, ReleaseError, json.JSONDecodeError, KeyError) as error:
        print(f"release staging failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
