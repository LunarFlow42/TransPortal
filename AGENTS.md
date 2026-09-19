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
- Tests: `cargo test` (or focused tests e.g. `cargo test commands::tests`)
- Lint: `cargo clippy`

### Development & Testing Tips

- **日常开发调试**：继续使用 `pnpm tauri dev` 即可，其增量编译与热重载缓存非常成熟。
- **快速语法与类型检查**：直接在 `src-tauri/` 执行 `cargo check` 即可。**只要不修改 `Cargo.toml`，在已编译缓存下增量检查只需 ~5 秒**（实测 5.4s），不要有耗时心理负担。只有改动了 `Cargo.toml` 或清理了 target 时才会全量重新扫描依赖元数据（耗时 ~3 分钟）。
- **单测优先跑特定业务模块**：验证逻辑时务必指定模块过滤运行（例如 `cargo test commands::tests` 仅需 **0.8 秒**，`cargo test dict::tests` 仅需 1 秒），避免每次全量链接所有静态大模型与 ONNX 依赖。
- **阶段性/提交前最终校验**：在功能完整落地或提交前，再统一执行全量 `cargo check` + `cargo test` 以及 `pnpm exec tsc --noEmit`（或 `pnpm build`）。

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
