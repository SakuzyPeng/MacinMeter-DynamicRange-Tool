# Manual workspace validation

The repository has one opt-in workflow:
[`workspace-validation.yml`](workspace-validation.yml).

## Contract

- `workflow_dispatch` is the only trigger. Pushes, pull requests, and tags do
  not consume Actions resources.
- Rust is pinned to the verified MSRV, 1.88. Node.js is pinned to 22 for the
  frontend build.
- The job validates the repository identity, formatting, strict workspace
  Clippy, standard workspace tests, repository/reference-tool unit tests, the
  release CLI, and the TypeScript/Vite frontend.
- Cargo always uses the root lockfile with `--locked`; npm uses
  `tauri-app/package-lock.json` through `npm ci`.
- The hostile malformed-media corpus is not decoded in-process by standard
  tests. Its opt-in subprocess verifier requires an enforceable memory limit by
  default and is deliberately outside this workflow.

The workflow does not publish artifacts, create a release, run network
advisory databases, or build a platform matrix. M5 release packaging and smoke
checks remain local and explicit until their artifact contract is accepted.

## Manual use

Open the repository's **Actions** page, choose **Manual workspace validation**,
and select **Run workflow**. Ordinary development does not require or wait for
this workflow.
