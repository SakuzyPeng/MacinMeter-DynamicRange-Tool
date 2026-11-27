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
  arm64 CLI archive, and two arm64 GUI DMGs;
- Windows x64: target `x86_64-pc-windows-msvc`, an x64 CLI archive, and two
  NSIS installers carrying the x64 GUI.

The GUI is the egui desktop GUI (`apps/macinmeter-desktop`, see
[`desktop_egui.md`](desktop_egui.md)), shipped in both of its font builds:
`bundled-fonts` embeds Noto Sans CJK SC and Source Serif 4 Semibold, so its
window and exported images look the same on every host, and `system-fonts`
embeds neither and draws with the host's fonts, at roughly half the size. The
two are one application under the 0.3.x Tauri GUI's names: installing either
replaces a Tauri install, or the other build, in place. The Tauri GUI's
sources remain in the repository but are no longer packaged.

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
- Rust 1.98 or newer and the locked Cargo graph;
- network access or `MACINMETER_UI_FONT_PATH` for the bundled-fonts GUI build's
  checksum-pinned Noto Sans CJK SC (see [`desktop_egui.md`](desktop_egui.md));
- NSIS 3 (`makensis`) and 7-Zip on Windows when staging the GUI, and 7-Zip when
  verifying the installers;
- Xcode's command-line tools (`codesign`, `hdiutil`, `lipo`, `otool`) on macOS.

Node.js is no longer a staging requirement: no packaged artifact contains the
Tauri frontend. The script records the exact source commit/state, host target,
Rust/Cargo versions, and the Cargo lockfile hash. It refuses a dirty worktree
by default.

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

On a supported macOS or Windows host, explicitly include its GUI installers:

```bash
python3 scripts/stage-release.py stage --include-gui
```

Staging builds `macinmeter-desktop` twice in release, once with its default
`bundled-fonts` feature and once with `--no-default-features`, and packages each
build. Every GUI artifact names its `fontBuild`, and the verifier reads the
build from the executable's own bytes: the bundled build contains the two
embedded fonts' PostScript names, the system build neither. It also requires
the licence texts that build carries: `LICENSE` and `THIRD_PARTY_NOTICES.md`
for both, plus the two SIL OFL texts for the bundled build.

The macOS path supports only `aarch64-apple-darwin`. It assembles
`MacinMeter DR GUI.app` (identifier `com.macinmeter.dr.gui`, executable
`macinmeter-gui`, licences under `Contents/Resources/licenses`), seals it with
an ad hoc signature that names no identity, and writes an HFS+ DMG beside an
Applications link: `macinmeter-gui-<version>-aarch64-apple-darwin-<fontBuild>.dmg`.
The verifier:

- validates the DMG checksum structure with `hdiutil`;
- mounts it read-only and requires exactly one top-level `.app`, with the
  expected name;
- verifies the bundle version and identifier and the executable's name;
- requires an executable with exactly the architecture named by the artifact;
- requires `LSMinimumSystemVersion` and the executable's own `LC_BUILD_VERSION`
  deployment target to be macOS 11.0;
- requires the executable to be the artifact's font build and the bundle to
  carry exactly that build's licences;
- records strict `codesign`, Developer ID, and other identified Apple-signature
  observations without treating ad-hoc metadata as developer identity;
- detaches the image without launching the GUI.

The Windows path supports only `x86_64-pc-windows-msvc`. Its executable embeds
the icon, a version resource with a `FontBuild` value, and an application
manifest (`asInvoker`, Common Controls 6). `makensis` compiles
`apps/macinmeter-desktop/packaging/windows/installer.nsi` with warnings as
errors into `macinmeter-gui-<version>-x86_64-pc-windows-msvc-<fontBuild>-setup.exe`.
The installer installs for the current user into the Tauri GUI's directory,
under its uninstall key, Start menu entry and `macinmeter-gui.exe` name. The
verifier requires 7-Zip and:

- validates the outer installer's DOS and PE headers and records its COFF
  machine type;
- extracts NSIS into a temporary directory outside the candidate and requires
  exactly one `macinmeter-gui.exe` payload;
- validates that payload's DOS and PE headers and requires its observed COFF
  machine type to be x86_64;
- reads its file-version resource and requires the workspace version;
- requires the payload to be the artifact's font build and the installed
  `licenses` directory to hold exactly that build's licences;
- requires neither the installer nor the payload to carry an embedded
  Authenticode certificate table;
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
source tree, a mismatched host, a toolchain other than Rust 1.98, a missing
GUI in either font build, `--allow-dirty`, or `--replace`. Their manifests
record the full Rust/Cargo identity and use
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
recorded identity. The process does not claim that Rust binaries, DMGs,
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
