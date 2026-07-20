# Repository tooling

Repository checks are split by risk and cost. None of these scripts triggers
GitHub Actions.

## Fast repository contract

Python 3.11 or newer is required for repository tooling. The read-only check:

```bash
python3 scripts/check-repository-contract.py
```

enforces the virtual-workspace identity, inherited package metadata, centralized
direct dependencies, the single Cargo/npm lockfiles, GUI version mirrors, and
manual-only workflow triggers. It does not resolve or download dependencies.

## Native PCM product fixtures

The committed `tests/fixtures/native-pcm-v1` codec matrix is generated and
verified with:

```bash
python3 scripts/generate-native-pcm-v1.py
python3 scripts/generate-native-pcm-v1.py --check
```

WAV/AIFF generation uses the Python standard library. Exact FLAC regeneration
is intentionally pinned to reference `flac 1.5.0`; normal tests use committed
bytes and do not require libFLAC, FFmpeg, network access, or personal audio.
Corpus geometry, hashes, PCM oracles, and provenance are recorded in its
[`manifest.json`](../tests/fixtures/native-pcm-v1/manifest.json).

## Hostile malformed-media corpus

The files under `tests/fixtures/malformed-media-v1` include forged
multi-gigabyte length declarations. Standard workspace tests only verify their
committed byte identity; they never decode those hostile cases in the Cargo
test process.

Regeneration and byte auditing are safe, read-only with respect to decoders:

```bash
python3 scripts/generate-malformed-media-v1.py
python3 scripts/generate-malformed-media-v1.py --check
```

Behavioral verification is a separate, opt-in isolation task:

```bash
cargo build --locked -p macinmeter-cli
python3 scripts/verify-malformed-corpus.py
```

Each case gets its own CLI subprocess and timeout. The default invocation
requires Linux `RLIMIT_AS`; it refuses to decode the corpus where that memory
limit cannot be enforced. `--allow-timeout-only` is an explicit risk
acknowledgement, not a normal gate. See
[`tests/fixtures/malformed-media-v1/README.md`](../tests/fixtures/malformed-media-v1/README.md)
for the corpus contract and the non-default `malformed-dev` fuzz seam.

## Pre-commit

The local hook remains fast, deterministic, and offline:

```bash
python3 scripts/check-repository-contract.py
cargo fmt --all -- --check
cargo check --locked --workspace
```

It does not run Clippy, full tests, the hostile corpus verifier, Docker, or
`cargo audit`, and it never refreshes a network advisory database.

Install it from the repository root:

```bash
chmod +x scripts/install-pre-commit.sh
./scripts/install-pre-commit.sh
```

The installer backs up an existing `.git/hooks/pre-commit`, copies
[`scripts/pre-commit`](pre-commit), and prints the installed checks. Direct use
is also supported:

```bash
scripts/pre-commit
```

For a local standard validation, run:

```bash
python3 scripts/check-repository-contract.py
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets
cargo build --locked --release -p macinmeter-cli
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
python3 -m unittest discover -s reference/tools/tests -p 'test_*.py'
(cd tauri-app && npm run build)
```

The matching GitHub Actions workflow remains `workflow_dispatch` only.

## M6 performance baseline

The performance corpus is deterministic, contains no personal audio, and stays
under ignored `target/`:

```bash
python3 scripts/generate-performance-corpus.py
python3 scripts/generate-performance-corpus.py --check
```

The default generator requires the reference `flac` command to create the FLAC
route, and records its exact version. From a clean worktree, run the release
scalar baseline with:

```bash
python3 scripts/run-performance-baseline.py
```

This is an explicit local task, not a pre-commit, workspace-test, or CI gate.
It records all raw samples and rejects result/PCM/work-unit drift before
summarizing. Dirty runs require `--allow-dirty` and are development evidence
only. Use `--list-cases` and `--case ID` for scoped harness checks. Future A/B
comparisons pass every prebuilt worker through repeated
`--variant NAME=EXECUTABLE` arguments plus matching
`--variant-source NAME=COMMIT` identities in the same interleaved run.

See [`docs/BENCHMARKS.md`](../docs/BENCHMARKS.md) and
[`ADR-0007`](../docs/adr/0007-m6-reproducible-performance-baseline.md).

## Local release staging

From a clean worktree, build and verify the current-host CLI artifact:

```bash
python3 scripts/stage-release.py stage
```

On macOS, explicitly include the current-host Tauri DMG:

```bash
python3 scripts/stage-release.py stage --include-gui
```

Both commands create `RELEASE_MANIFEST.json` and `SHA256SUMS`, then verify the
final files. CLI verification extracts and runs the distributed binary. GUI
verification checks and mounts the DMG, validates its bundle identity and
architecture, records strict code-signature status, and does not launch it.
No staging command uploads, signs, notarizes, or creates a GitHub release.

See [`docs/RELEASE.md`](../docs/RELEASE.md) for the exact artifact and dirty-tree
contracts.

## Failure handling

- Repository contract failure: repair the reported source-of-truth drift; use
  `npm run sync-version` in `tauri-app/` only when intentionally changing the
  workspace version.
- Formatting failure: run `cargo fmt --all`, inspect the diff, and retry.
- Compile failure: run `cargo check --locked --workspace` for full diagnostics.
- Emergency bypass: `git commit --no-verify`. A bypass is not validation.

To uninstall the copied hook:

```bash
rm .git/hooks/pre-commit
```

If the installer created a backup, select the desired
`.git/hooks/pre-commit.backup.*` file manually.
