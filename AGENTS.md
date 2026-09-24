# AGENTS.md

Guidance for coding agents working in this repository.

## Commands

Frontend (repo root):

- Install deps: `pnpm install`
- Typecheck: `pnpm exec tsc --noEmit`
- Unit tests (all): `pnpm test`
- Single test file: `pnpm exec vitest run <path/to/test.ts>`
- Build frontend: `pnpm build`
- Dev (Vite + Tauri): `pnpm tauri dev`

Backend (`src-tauri/`):

- Fast check: `cargo check`
- Format: `cargo fmt` (verify with `cargo fmt --check`)
- Lint: `cargo clippy --all-targets`
- Tests (all): `cargo test`
- Focused test: `cargo test <module>::tests` (e.g. `cargo test native::windows::tests`, `cargo test shortcut::tests`, `cargo test dict::tests`)

## Development & Quality Workflow (效率与质量分层平衡)

为了兼顾**秒级响应的极客开发效率**与**工业级代码质量**，严禁在每个微小改动时都全量跑耗时流程，实行双层分级验证流：

### Level 1: 实时高频迭代流 (0.5s ~ 2s 极速反馈)
- **单模块极速单测**：验证逻辑时务必按模块过滤运行（例如 `cargo test native::windows::tests` 仅需 **0.7 秒**，前端单测 `pnpm exec vitest run src/hooks/useWindowAnimation.test.ts` 仅需 **0.2 秒**），避免无谓链接 ONNX / 静态大模型。
- **增量语法与类型检查**：直接执行 `cargo check`（实测 0.7s）与 `pnpm exec tsc --noEmit`（实测 1.5s）。
- **日常热重载调试**：使用 `pnpm tauri dev` 即可，利用成熟的增量编译与 Vite 热重载缓存。

### Level 2: 阶段性落地与 Commit 前质量闭环 (3s ~ 8s 质量铁律)
在完成一个功能点或准备本地 `git commit` 前，必须按顺序执行以下质量闭环：
1. **代码风格统一**：在 `src-tauri/` 运行 `cargo fmt` 自动对齐代码格式（以 `cargo fmt --check` 零输出为准）。
2. **静态扫描与代码异味清零**：在 `src-tauri/` 运行 `cargo clippy --all-targets`，坚持 **0 errors, 0 warnings** 零容忍标准（重点治理 `collapsible_if`、未注解的 `transmute`、`type_complexity` 等隐患）。
3. **双端全量测试**：执行 `cargo test` 业务核心单测 + `pnpm test`（确保所有测试文件 100% 绿色通过）。
4. **前端打包与分包审查**：运行 `pnpm build`，确保 Rolldown / Vite 构建无 500kB 超标警告，主浮层维持轻量轻灵（~130 kB）。

### 核心能效与安全红线
- **严禁无故修改 `Cargo.toml`**：只要不修改 `Cargo.toml`，增量检查与编译仅需 **~1 秒**。改动 `Cargo.toml` 或清理 target 会触发全量依赖元数据重新扫描（耗时 ~3 分钟）。
- **禁止 Agent 自动执行 Git Push**：可以且鼓励在本地进行规范的原子化 `git commit`。推送（Git Push）由开发者人工掌控，严禁 Agent 自动执行 `git push`。

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
- Do not run `git push`.
