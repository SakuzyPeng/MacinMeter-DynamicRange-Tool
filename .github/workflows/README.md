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
- The workflow uses explicit Ubuntu 24.04 and Windows Server 2025 x64 jobs with
  read-only repository permissions and 45-minute timeouts. Rust is pinned to
  the verified MSRV, 1.88, and the Linux frontend build uses Node.js 22.

## Standard automatic gate

The Linux job validates repository identity, formatting, strict workspace
Clippy, standard workspace tests, repository/reference-tool unit tests, and
the TypeScript/Vite frontend. The parallel Windows job validates repository
identity, strict Clippy, and all workspace targets on the actual Windows x64
toolchain, including the CLI black-box and Tauri Rust tests. Cargo always uses
the root lockfile with `--locked`; npm uses `tauri-app/package-lock.json`
through `npm ci`.

The Linux release CLI build runs only for an explicit manual dispatch. The
Windows job builds and smoke-tests the release CLI after a `main` push or
manual dispatch, but does not retain or upload that binary. Local release
staging remains the authoritative artifact boundary.

The hostile malformed-media verifier, performance corpus, profiler, release
staging, artifact upload, signing, notarization, advisory-network access, and
macOS runners and broad platform matrices remain outside this workflow. In
particular, the hostile corpus is never decoded in the ordinary Cargo test
process, and a successful Windows compile/smoke does not claim a verified GUI
installer.

## Manual use

Open the repository's **Actions** page, choose **Workspace validation**, and
select **Run workflow**. The manual run executes both platform gates and their
release CLI builds; it still does not publish anything.
