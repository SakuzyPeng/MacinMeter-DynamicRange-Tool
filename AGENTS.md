# Repository Guidelines

## Project Structure & Module Organization
- `src/` — Rust sources
  - `audio/` decoders and format handling (Symphonia/Opus)
  - `processing/` DR window logic, SIMD paths, statistics
  - `tools/` CLI args, file scanning, output helpers
  - `core/` shared utilities; `error.rs` unified `AudioError`
  - `main.rs` binary entry; `lib.rs` library exports
- `tests/` — integration tests (`*_tests.rs`), fixtures under `tests/fixtures/`
- `scripts/` — pre‑commit and quality checks; `docs/` documentation; `target/` build outputs; sample audio in `audio/`.

## Build, Test, and Development Commands
- Build release: `cargo build --release`
- Run (example): `cargo run --release -- ./audio/example.flac`
- Binary path: `./target/release/MacinMeter-DynamicRange-Tool-foo_dr`
- All tests: `cargo test`
- Specific file: `cargo test --test boundary_tests`
- Install pre‑commit hook: `chmod +x scripts/install-pre-commit.sh && ./scripts/install-pre-commit.sh`

## Coding Style & Naming Conventions
- Rust 2024 edition. Format with `cargo fmt` and lint with `cargo clippy -- -D warnings`.
- Indentation: 4 spaces; line width: rustfmt defaults.
- Naming: snake_case (modules/functions), CamelCase (types/traits), SCREAMING_SNAKE_CASE (consts).
- Errors: prefer `Result<T, AudioError>`; avoid panics in non‑test code. Use `eprintln!` for errors, `println!` for user output.

## Testing Guidelines
- Framework: Rust `#[test]` integration tests in `tests/`. Name files `*_tests.rs` (e.g., `audio_format_tests.rs`).
- Run ignored/long tests: `cargo test -- --ignored` or `cargo test --test boundary_tests --ignored`.
- Use fixtures from `tests/audio_test_fixtures.rs`; generated files live in `tests/fixtures/`.
- Aim to maintain or increase coverage; keep tests deterministic and file‑system local.

## Commit & Pull Request Guidelines
- Conventional Commits: `feat:`, `fix:`, `perf:`, `refactor:`, `docs:`, `test:` …
  - Example: `feat: add SIMD i32→f32 sample conversion`
- Before pushing: `cargo fmt && cargo clippy -- -D warnings && cargo test`.
- PRs include: clear description, rationale, linked issues, test coverage/outputs (e.g., sample CLI run), and performance notes if relevant.

## Security & Configuration Tips
- No network I/O paths; keep processing local. New codecs/features should be gated and tested with `--release`.
- Prefer safe Rust; justify any `unsafe` with comments and tests.
- For realistic performance numbers, build with `--release` (LTO enabled by default in `Cargo.toml`).

