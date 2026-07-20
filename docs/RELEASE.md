[English](RELEASE.md) | [中文](RELEASE_CN.md)

# Local release staging

MacinMeter 0.2.0 has a local, explicit artifact contract. Staging builds and
verifies bytes under `target/release-staging`; it never uploads, signs,
notarizes, or creates a GitHub release.

## Requirements

- a clean Git worktree for a release candidate;
- Python 3.11 or newer;
- Rust 1.88 or newer and the locked Cargo graph;
- Node.js plus the platform Tauri prerequisites when including the GUI.

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
- the smoke document must use the WAV integer-PCM route and remain
  `foo_dr_meter_1_0_8_candidate_v1 / unverified`.

## Current-host macOS GUI artifact

On macOS, explicitly include the Tauri DMG:

```bash
python3 scripts/stage-release.py stage --include-gui
```

This builds only the current Rust host architecture. An
`aarch64-apple-darwin` result is not a universal or x86_64 artifact. The
verifier:

- validates the DMG checksum structure with `hdiutil`;
- mounts it read-only and requires exactly one top-level `.app`;
- verifies the bundle version and identifier;
- requires an executable with exactly the architecture named by the artifact;
- records whether strict `codesign` verification succeeds;
- detaches the image without launching the GUI.

The current unsigned/unnotarized build is marked
`local_staging_only`/`local_unnotarized`. A successful structural smoke test is
not a Gatekeeper, Developer ID, notarization, or public-distribution claim.
Windows/Linux GUI packages and macOS x86_64/universal packages remain
unverified until built and checked on their actual target.

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
