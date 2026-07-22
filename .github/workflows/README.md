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

The Linux release CLI build runs only for an explicit manual dispatch. The
Windows job builds and smoke-tests the release CLI after a `main` push or
manual dispatch, but does not retain or upload that binary. On the same
main/manual boundary, the macOS job runs the existing clean release staging
contract for both the final CLI archive and current-host arm64 Tauri DMG. It
checks the extracted CLI, mounts the DMG read-only, validates bundle identity
and architecture, and verifies the SHA-256 manifest. Those runner-local bytes
are intentionally discarded rather than uploaded.

The hostile malformed-media verifier, performance corpus, profiler, artifact
upload, signing, notarization, release publication, advisory-network access,
and broad platform matrices remain outside this workflow. In particular, the
hostile corpus is never decoded in the ordinary Cargo test process. A
successful macOS staging run establishes an unsigned arm64 DMG structural
smoke; it does not establish Gatekeeper or public-distribution readiness.

## Manual use

Open the repository's **Actions** page, choose **Workspace validation**, and
select **Run workflow**. The manual run executes all three platform gates,
including macOS CLI/GUI staging; it still does not retain or publish artifacts.
