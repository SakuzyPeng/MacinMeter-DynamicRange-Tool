[English](RELEASE.md) | [中文](RELEASE_CN.md)

# Release artifact staging

MacinMeter 0.2.0 has an explicit artifact contract. Staging builds and verifies
bytes under `target/release-staging`; it never uploads, signs, notarizes, or
creates a GitHub release.

The bounded GitHub Actions workflow runs the same clean staging command on its
macOS 26 arm64 job after a `main` push. The generated CLI archive and DMG are
verified and then discarded with the runner. An explicit manual dispatch uses
the unsigned-candidate contract below and retains its result for 14 days; it
still does not create a tag or GitHub Release.

## 0.2.0 release scope

The 0.2.0 release is limited to Apple Silicon macOS:

- target: `aarch64-apple-darwin`;
- minimum system: macOS 11.0;
- artifacts: an arm64 CLI archive and an arm64 Tauri DMG;
- no Developer ID signature, notarization, or stapling.

There is no 0.2.0 Intel or universal macOS build, and no Windows/Linux GUI
package. “Unsigned” means no Developer ID identity; compiler or linker ad-hoc
metadata is not a developer signature or Gatekeeper claim.

## Requirements

- a clean Git worktree for a release candidate;
- Python 3.11 or newer;
- Rust 1.88 or newer and the locked Cargo graph;
- Node.js for the shared version contract, plus the platform Tauri
  prerequisites when including the GUI.

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

- `macinmeter --version` must report the workspace version;
- a committed WAV fixture must produce one schema-v3 JSON document;
- the smoke document must use the WAV integer-PCM route and contain the fixed
  algorithm parameters without internal profile or status fields.

## Current-host macOS GUI artifact

On macOS, explicitly include the Tauri DMG:

```bash
python3 scripts/stage-release.py stage --include-gui
```

GUI staging supports only the `aarch64-apple-darwin` Rust host. The verifier:

- validates the DMG checksum structure with `hdiutil`;
- mounts it read-only and requires exactly one top-level `.app`;
- verifies the bundle version and identifier;
- requires an executable with exactly the architecture named by the artifact;
- requires `LSMinimumSystemVersion` to be macOS 11.0;
- records strict `codesign`, Developer ID, and other identified Apple-signature
  observations without treating ad-hoc metadata as developer identity;
- detaches the image without launching the GUI.

The current unsigned/unnotarized build, whether produced locally or by the
ephemeral CI gate, is marked
`local_staging_only`/`local_unnotarized`. A successful structural smoke test is
not a Gatekeeper, Developer ID, notarization, or public-distribution claim.
Windows/Linux GUI packages and macOS x86_64/universal packages are outside the
0.2.0 release scope.

## Unsigned Apple Silicon release candidate

The publication-shaped candidate mode is separate from local staging:

```bash
python3 scripts/stage-release.py stage \
  --include-gui \
  --unsigned-macos-arm64-candidate
```

It writes to `target/release-candidates/0.2.0/aarch64-apple-darwin` and refuses
a dirty source tree, a non-arm64 host, toolchains other than Rust 1.88 and
Node.js 22, a missing GUI, `--allow-dirty`, or `--replace`. Its manifest records
the full Rust/Cargo/Node/npm identity and is marked
`unsigned_macos_arm64_release_candidate`; it never claims signing,
notarization, Gatekeeper readiness, or completed publication.

A manual **Workspace validation** dispatch must target `main`. The macOS job
builds this exact candidate and retains one workflow artifact for 14 days. The
workflow retains read-only repository permissions and cannot create a tag or
Release. The candidate is eligible for final review only when the complete
three-platform workflow succeeds; retention from a failed run is not release
evidence. It remains an input to human approval, not a public asset.

The proposed bilingual GitHub Release body is
[`RELEASE_DRAFT_0.2.0.md`](RELEASE_DRAFT_0.2.0.md).

## Checksums and verification

Each staging directory contains:

- `RELEASE_MANIFEST.json`;
- one or more distribution artifacts;
- `SHA256SUMS`, covering the release manifest and every artifact.

Rerun verification against the final bytes:

```bash
python3 scripts/stage-release.py verify \
  target/release-staging/0.2.0/aarch64-apple-darwin
```

The same verifier accepts an unsigned candidate directory after checking its
stricter clean-source, target, artifact-set, and distribution contract.

The verifier requires the checksum file to cover the exact directory contents
and reruns every artifact smoke contract. Checksums establish byte identity;
they are not signatures.

The CLI tar container is deterministic for identical payload bytes and
recorded identity. The process does not claim that Rust binaries or Tauri DMGs
are reproducible across toolchains, SDKs, machines, or signing environments.

## Development-only staging

During script development, a dirty tree may be tested explicitly:

```bash
python3 scripts/stage-release.py stage --allow-dirty
```

Both the directory and artifact names gain `-dirty`, and the manifest records
`source.state = "dirty"`. Such artifacts are never release candidates.
`--replace` only replaces generated staging directories or directories already
carrying the release manifest and checksum markers.
