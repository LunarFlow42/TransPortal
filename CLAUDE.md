# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

@AGENTS.md

The imported `AGENTS.md` is the source of truth for commands, conventions, and prohibitions. This file adds only what it doesn't cover.

## Extra commands

- Single frontend test file: `pnpm exec vitest run src/hooks/useSmartDismiss.test.ts`
- Release integrity tooling tests: `pnpm test:integrity` (Node `--test` over `scripts/integrity/*.node.mjs`)
- ONNX OCR assets: `pnpm ocr:prepare` downloads, verifies, and stages the model and runtime into `src-tauri/resources/ocr`. Debug builds read that directory directly. Release builds read the bundled resource dir.
- Release build: `pnpm build:release` (NSIS + `SHA256SUMS` + `integrity.json`)
- CI (`.github/workflows/ci.yml`, windows-latest) runs `pnpm build`, `pnpm test`, `pnpm test:integrity`, `cargo test`, then `scripts/ocr/smoke-onnx.ps1`.
- `.cargo/config.toml` (and `src-tauri/.cargo/config.toml`) sets `rustc-wrapper` to a local sccache at `D:/RUST/...` and the linker to `rust-lld.exe`. If sccache isn't at that path, set `CARGO_BUILD_RUSTC_WRAPPER=""` as CI does.

## Architecture

Tauri v2 app: Rust backend in `src-tauri/`, React 19 + Zustand frontend in `src/`. It is Windows-first. Most native features (UI Automation selection, desktop capture, Windows OCR, the region picker) have only non-Windows fallbacks that report the feature is unavailable.

### Backend wiring (`src-tauri/src/lib.rs`)

- `run()` first runs `integrity::startup_report()`. A release EXE whose SHA-256 is wrong exits before Tauri starts.
- `setup` registers managed state: `OcrService`, `RegionService`, `MonitorService`, `ShortcutBindings`, `SelectionService`, and `LookupService` (a redb file at `app_data_dir/transportal.redb`). It also sets up the system tray, global shortcuts, and the clipboard monitor.
- Every `#[tauri::command]` lives in `commands.rs` and must be listed in `generate_handler!` in `lib.rs`.
- Windows:
  - `shell` (overlay, declared in `tauri.conf.json`).
  - `settings` (created on demand by `open_or_focus_settings`; closing only hides it).
  - The OCR region picker, routed at `#/ocr-region`.

### Pluggable subsystems (trait + registry)

- **Translation**: the `provider::Translator` trait, with engines registered in `provider::registry()`. Current engines: google, openai (OpenAI-compatible SSE streaming), tencent, youdao. `provider/lang.rs` handles language codes.
- **Dictionary**: the `dict::DictSource` trait. `dictionary::LookupService` orders sources, enables them, caches results in an LRU, and reports status. `dict/mod.rs` holds `DictStore` (redb storage plus ECCEDICT CSV import).
- **OCR**:
  - The `ocr::OcrEngine` trait with `ocr/registry.rs`. Engines are Windows `Media.Ocr` (in `native/ocr/`) and ONNX PP-OCRv4 (`ocr/onnx.rs`, using `ort` with `load-dynamic`, so the runtime DLL is loaded at runtime from staged assets managed by `ocr/assets.rs`).
  - `ocr/workflow.rs` drives capture → recognize → optional translate sessions.
  - `ocr/picture.rs` keeps the image plus text boxes for in-place translation.
  - `ocr/region.rs` implements the frozen-desktop region picker.
  - `ocr/monitor.rs` implements auto region translation (polling a fixed screen region for changes).
- **Native**: `native/mod.rs` exposes traits (for example `SelectionReader`) and selects `windows.rs` or `fallback.rs` via `#[cfg]`. Subfolders cover `selection` (UIA + clipboard fallback, in-place replace), `desktop` (DXGI capture), `ocr`, `integrity`, and `memory`.
- **Shortcuts**: `shortcut.rs` handles global hotkey registration, conflict reporting, window positioning near the caret or selection, and the clipboard monitor. `shortcut_sequence.rs` parses multi-key sequences such as `Ctrl+C+C`.

### Rust → frontend events

The backend pushes state with `emit` / `emit_to("shell", ...)` on `transportal://*` channels. Examples:
- `transportal://selection`, `transportal://ocr`, `transportal://auto-ocr`
- `transportal://shortcut-status`, `transportal://shell-shown`, `transportal://notice`
- `transportal://clipboard-text`, `transportal://focus-input`, `transportal://ocr-assets-changed`

Frontend hooks in `src/hooks/` (`useOcr`, `useAutoOcr`, `useSmartDismiss`, `useShortcutStatus`) and `ShellView` subscribe with `listen`. When you add an event, keep the channel name identical on both sides.

### Frontend state and settings

- A single Zustand store lives in `src/store/useAppStore.ts`, and shared TS types in `src/types/index.ts`. Those types mirror Rust serde structs, so keep them in sync.
- Settings are stored via `tauri-plugin-store` (`settings.json`, `src/store/persistence.ts`).
- In `useBootstrap(true)` (SettingsView), store changes are diffed into a patch. The patch is saved and then broadcast as a settings-changed event. `useBootstrap(false)` (ShellView) only hydrates from that event.
- When you add a persisted setting, update `PersistedSettings`, `loadSettings`/`saveSettings`, and the diff in `useBootstrap.ts`.
- Styling uses Tailwind v4 via `@tailwindcss/vite`. Framer Motion `layoutId` drives the word-card ↔ sentence-bubble morph.

## Docs

User-facing docs are in Chinese: `docs/USER_GUIDE.md`, `docs/OCR.md`, and `docs/STARTUP_CHECKS.md`. UI strings are mostly Chinese too. Asset versions, hashes, and licenses for ONNX are in `src-tauri/resources/ocr/manifest.json`.
