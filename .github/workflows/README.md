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
- The workflow uses one pinned Ubuntu 24.04 job with read-only repository
  permissions and a 45-minute timeout. Rust is pinned to the verified MSRV,
  1.88, and Node.js is pinned to 22.

## Standard automatic gate

Pull requests and `main` pushes validate the repository identity, formatting,
strict workspace Clippy, standard workspace tests, repository/reference-tool
unit tests, and the TypeScript/Vite frontend. Cargo always uses the root
lockfile with `--locked`; npm uses `tauri-app/package-lock.json` through
`npm ci`.

The additional release-mode CLI build runs only for an explicit manual
dispatch. Local release staging remains the authoritative artifact boundary.

The hostile malformed-media verifier, performance corpus, profiler, release
staging, artifact upload, signing, notarization, advisory-network access, and
platform matrices remain outside this workflow. In particular, the hostile
corpus is never decoded in the ordinary Cargo test process.

## Manual use

Open the repository's **Actions** page, choose **Workspace validation**, and
select **Run workflow**. The manual run executes the same standard gate and the
additional release CLI build; it still does not publish anything.
