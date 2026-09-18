# AGENTS.md

Guidance for coding agents working in this repository.

## Commands

Frontend (repo root):

- Install deps: `pnpm install`
- Typecheck: `pnpm exec tsc --noEmit`
- Unit tests: `pnpm test`
- Build frontend: `pnpm build`
- Dev (Vite + Tauri): `pnpm tauri dev`

Backend (`src-tauri/`):

- Check: `cargo check`
- Tests: `cargo test`
- Lint: `cargo clippy`

Always run `cargo check` + `cargo test` and `pnpm exec tsc --noEmit` (or `pnpm build`) after changes.

## Conventions

- Rust edition 2024. Keep `#[cfg(target_os = "...")]` platform code isolated behind traits (see `src/native/`).
- Provider engines are registered in `src-tauri/src/provider/mod.rs::registry`; the command layer must not match on provider names.
- Dictionary sources are pluggable via `dict::DictSource` and orchestrated in `src-tauri/src/dictionary.rs` (`LookupService`); the frontend edits enable/order through the `dict_sources` / `set_dict_source_enabled` / `move_dict_source` / `set_dict_source_order` commands.
- All network I/O lives in Rust (Youdao/Google dictionary, providers, Anki). The webview CSP is locked down; do not add `fetch()` to remote hosts in the frontend.
- Frontend windows are addressed by hash route: `#/shell` (overlay) and `#/settings` (settings window). `SettingsView` is the persistence writer (`useBootstrap(true)`); `ShellView` is a reader (`useBootstrap(false)`).

## Data

- Local dictionary is ECCEDICT CSV imported into `redb`. The upstream `ecdict.mini.csv` is a ~53-word sample and must not be used as a real dictionary.
- `scripts/eccedict/build_enriched_csv.py` (run via `uv run`) merges the ECCEDICT fork's DuckDB `phonetics.ddb` / `tag.ddb` into an importable CSV.

## Do not

- Do not commit secrets or API keys.
- Do not disable tests or weaken the CSP to work around a build error.
