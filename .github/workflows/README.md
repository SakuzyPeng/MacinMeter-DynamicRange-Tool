# Workspace validation

The repository has one bounded validation workflow:
[`workspace-validation.yml`](workspace-validation.yml).

## Trigger and resource contract

- Pull requests and pushes to `main` run the standard validation gate.
- `workflow_dispatch` remains available for an explicit full validation.
- Feature-branch pushes without a pull request do not run remotely, avoiding a
  duplicate push and pull-request run for each update.
- One concurrency group is retained per trigger and pull request or ref. A
  newer update cancels an obsolete in-progress run for that group without a
  manual dispatch cancelling an automatic `main` validation.
- The workflow uses explicit Ubuntu 24.04, Windows Server 2025 x64, and macOS
  26 arm64 jobs with read-only repository permissions. Linux and Windows have
  45-minute timeouts; macOS has 60 minutes for the release-profile GUI build.
  Rust is pinned to the verified MSRV, 1.88, and Node.js is pinned to 22.

## Standard automatic gate

The Linux job validates repository identity, formatting, strict workspace
Clippy, standard workspace tests, repository/reference-tool unit tests, and
the TypeScript/Vite frontend. The parallel Windows and macOS jobs validate
repository identity, strict Clippy, and all workspace targets on their actual
x64 and arm64 toolchains, including the CLI black-box and Tauri Rust tests.
Cargo always uses the root lockfile with `--locked`; npm uses
`tauri-app/package-lock.json` through `npm ci`.

The Linux release CLI build runs only for an explicit manual dispatch. After a
`main` push, the Windows and macOS jobs each run clean local-only staging for
their final CLI archive and current-host Tauri installer (NSIS or DMG), then
discard those runner-local bytes. Windows staging requires 7-Zip, extracts the
NSIS installer outside the candidate directory, validates the payload's x86_64
PE identity and version, and requires both installer and payload to be
Authenticode `NotSigned` without a signer certificate.

An explicit manual dispatch must target `main`. It instead creates two stricter
unsigned candidates: Windows x64 CLI plus NSIS and Apple Silicon CLI plus DMG,
both from the dispatched commit, with clean source, exact host architecture,
and SHA-256 coverage. macOS retains its 11.0 minimum with no Developer ID or
notarization claim; Windows retains no Authenticode or SmartScreen-reputation
claim. One pinned `actions/upload-artifact` step in each platform job retains
its candidate for 14 days. The workflow retains read-only repository
permissions, so neither upload is a tag or GitHub Release.

The hostile malformed-media verifier, performance corpus, profiler, signing,
notarization, release publication, advisory-network access, and broad platform
matrices remain outside this workflow. Candidate retention is the sole artifact
upload and occurs only on manual dispatch. A successful staging run does not
establish Gatekeeper, SmartScreen reputation, or public-distribution readiness.

## Manual use

Open the repository's **Actions** page, choose **Workspace validation**, and
select **Run workflow**. The manual run executes all three platform gates,
then retains the unsigned Windows x64 and Apple Silicon CLI/GUI candidates for
14 days. It does not create a tag, publish a Release, or make either candidate
publicly downloadable. Final release review must confirm that both manifests
name the same source commit.
