[English](RELEASE.md) | [中文](RELEASE_CN.md)

# Release artifact staging

MacinMeter 0.3.1 has an explicit artifact contract. Staging builds and verifies
bytes under `target/release-staging`; it never uploads, signs, notarizes, or
creates a GitHub release.

The bounded GitHub Actions workflow runs the same clean staging contract on its
Windows Server 2025 x64 and macOS 26 arm64 jobs after a `main` push. Each job's
CLI archive and GUI installer are verified and then discarded with the runner.
An explicit manual dispatch uses the unsigned-candidate contracts below and
retains both results for 14 days; it still does not create a tag or GitHub
Release.

## 0.3.1 release scope

The 0.3.1 release contains two platform slices:

- Apple Silicon macOS: target `aarch64-apple-darwin`, minimum macOS 11.0, an
  arm64 CLI archive, and an arm64 Tauri DMG;
- Windows x64: target `x86_64-pc-windows-msvc`, an x64 CLI archive, and an NSIS
  installer carrying the x64 Tauri GUI.

Both slices are unsigned. macOS receives no Developer ID signature,
notarization, or stapling; Windows receives no Authenticode signature. Users
may therefore need to explicitly open the macOS app or pass a Windows
SmartScreen unknown-publisher warning. There is no 0.3.1 Intel/universal macOS,
Windows ARM64/32-bit, or Linux GUI artifact.

Unsigned is a standing position, not a pending task. A code-signing certificate
is issued to a named individual, so signing would publish the maintainer's legal
name with every artifact; the obstacle is privacy rather than cost or effort,
and it is not expected to change in the foreseeable future. User-facing material
must say so plainly and must not use wording like "not signed yet", which
invites readers to wait for a signed build that is not coming.

## Requirements

- a clean Git worktree for a release candidate;
- Python 3.11 or newer;
- Rust 1.88 or newer and the locked Cargo graph;
- Node.js for the shared version contract, plus the platform Tauri
  prerequisites when including the GUI;
- 7-Zip on Windows when staging or verifying the NSIS installer.

The script records the exact source commit/state, host target, Rust/Cargo
versions, and both lockfile hashes. It refuses a dirty worktree by default.

## CLI artifact

From the repository root:

```bash
python3 scripts/stage-release.py stage
```

The host CLI is built with:

```bash
cargo build --locked --release -p macinmeter-cli
```

The resulting `macinmeter-cli-<version>-<host>.tar.gz` contains:

- the CLI executable;
- `LICENSE`;
- `README.md`;
- `RELEASE_NOTES.md`;
- `THIRD_PARTY_NOTICES.md`;
- `ARTIFACT_MANIFEST.json`, including each payload file's size and SHA-256.

Verification safely extracts the archive, checks its exact member set and
payload hashes, then runs the extracted executable:

- `mdrmeter --version` must report the workspace version;
- a committed WAV fixture must produce one schema-v4 JSON document;
- the smoke document must use the WAV integer-PCM route and contain the fixed
  algorithm parameters without internal profile or status fields.

## Current-host GUI artifacts

On a supported macOS or Windows host, explicitly include its Tauri installer:

```bash
python3 scripts/stage-release.py stage --include-gui
```

The macOS path supports only `aarch64-apple-darwin`. The verifier:

- validates the DMG checksum structure with `hdiutil`;
- mounts it read-only and requires exactly one top-level `.app`;
- verifies the bundle version and identifier;
- requires an executable with exactly the architecture named by the artifact;
- requires `LSMinimumSystemVersion` to be macOS 11.0;
- records strict `codesign`, Developer ID, and other identified Apple-signature
  observations without treating ad-hoc metadata as developer identity;
- detaches the image without launching the GUI.

The Windows path supports only `x86_64-pc-windows-msvc`. It requires 7-Zip and:

- validates the outer installer's DOS and PE headers and records its COFF
  machine type;
- extracts NSIS into a temporary directory outside the candidate and requires
  exactly one `macinmeter-gui.exe` payload;
- validates that payload's DOS and PE headers and requires its observed COFF
  machine type to be x86_64;
- reads its file-version resource and requires the workspace version;
- queries Authenticode for both the installer and payload and requires each to
  report `NotSigned` with no signer certificate;
- records the extracted payload SHA-256 and cleans up without launching or
  installing it.

Local GUI builds are marked `local_staging_only` plus `local_unnotarized` on
macOS or `local_unsigned` on Windows. A successful structural smoke test is not
a Gatekeeper, Developer ID, notarization, SmartScreen-reputation, or
public-distribution claim.

## Unsigned release candidates

The publication-shaped candidate modes are separate from local staging and
must run on their matching hosts:

```bash
python3 scripts/stage-release.py stage \
  --include-gui \
  --unsigned-macos-arm64-candidate

python scripts/stage-release.py stage \
  --include-gui \
  --unsigned-windows-x64-candidate
```

They write to
`target/release-candidates/0.3.1/aarch64-apple-darwin` and
`target/release-candidates/0.3.1/x86_64-pc-windows-msvc`. Each refuses a dirty
source tree, a mismatched host, toolchains other than Rust 1.88 and Node.js 22,
a missing GUI, `--allow-dirty`, or `--replace`. Their manifests record the full
Rust/Cargo/Node/npm identity and use
`unsigned_macos_arm64_release_candidate` or
`unsigned_windows_x64_release_candidate`; neither claims signing,
notarization, Gatekeeper/SmartScreen readiness, or completed publication.

A manual **Workspace validation** dispatch must target `main`. The Windows and
macOS jobs build their respective candidate and retain one workflow artifact
each for 14 days. The workflow retains read-only repository permissions and
cannot create a tag or Release. Both candidates are eligible for final review
only when the complete workflow succeeds and both manifests name the same
source commit; retention from a failed run is not release evidence. They remain
inputs to human approval, not public assets.

The proposed bilingual GitHub Release body is
[`RELEASE_DRAFT_0.3.1.md`](RELEASE_DRAFT_0.3.1.md).

## Checksums and verification

Each staging directory contains:

- `RELEASE_MANIFEST.json`;
- one or more distribution artifacts;
- `SHA256SUMS`, covering the release manifest and every artifact.

Rerun verification against the final bytes:

```bash
python3 scripts/stage-release.py verify \
  target/release-staging/0.3.1/aarch64-apple-darwin

python scripts/stage-release.py verify \
  target/release-staging/0.3.1/x86_64-pc-windows-msvc
```

The same verifier accepts an unsigned candidate directory after checking its
stricter clean-source, target, artifact-set, and distribution contract.

The verifier requires the checksum file to cover the exact directory contents
and reruns every artifact smoke contract. Checksums establish byte identity;
they are not signatures.

The CLI tar container is deterministic for identical payload bytes and
recorded identity. The process does not claim that Rust binaries, Tauri DMGs,
or NSIS installers are reproducible across toolchains, SDKs, machines, or
signing environments.

## Development-only staging

During script development, a dirty tree may be tested explicitly:

```bash
python3 scripts/stage-release.py stage --allow-dirty
```

Both the directory and artifact names gain `-dirty`, and the manifest records
`source.state = "dirty"`. Such artifacts are never release candidates.
`--replace` only replaces generated staging directories or directories already
carrying the release manifest and checksum markers.
