# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

@AGENTS.md

The imported `AGENTS.md` is the source of truth for commands, conventions, and prohibitions. This file adds only what it doesn't cover.

## Extra commands

- Single frontend test file: `pnpm exec vitest run src/hooks/useHideOnBlur.test.ts`
- Release integrity tooling tests: `pnpm test:integrity` (Node `--test` over `scripts/integrity/*.node.mjs`)
- ONNX OCR assets: `pnpm ocr:prepare` downloads, verifies, and stages the model and runtime into `src-tauri/resources/ocr`. Debug builds read that directory directly. Release builds read the bundled resource dir.
- Release build: `pnpm build:release` (NSIS + `SHA256SUMS` + `integrity.json`)
- Linux packages: `APPIMAGE_EXTRACT_AND_RUN=1 pnpm tauri build` on Linux (`.deb`, `.rpm`, `.AppImage`; the Linux ONNX runtime is staged by `ocr:prepare`).
- CI (`.github/workflows/ci.yml`) runs a `windows-latest` + `ubuntu-24.04` matrix: clippy `-D warnings` and `cargo test` on both; Linux adds `cargo fmt --check` and `scripts/check-architecture.sh`; Windows adds `pnpm build`, `pnpm test`, `pnpm test:integrity` and `scripts/ocr/smoke-onnx.ps1`. A `bundle-linux` job on `feat/linux-native` uploads the Linux packages.
- `.cargo/config.toml` (and `src-tauri/.cargo/config.toml`) only sets the Windows linker to `rust-lld.exe`. Use sccache via the `RUSTC_WRAPPER` environment variable (or your user-level `~/.cargo/config.toml`), not the repo config.

## Architecture

Tauri v2 app: Rust backend in `src-tauri/`, React 19 + Zustand frontend in `src/`. The backend is split into `core/` (cross-platform logic) and `platform/` (OS contracts + one backend per target); the layout and dependency rules are in AGENTS.md. Windows implements every native feature. Linux currently delegates to the `unsupported` backend: ONNX OCR on image files, translation, dictionaries, tray, regular global shortcuts and a polling clipboard monitor work; UI Automation selection, desktop capture, keyboard gestures, in-place replace and Windows OCR are reported unavailable through `platform::CAPABILITIES`. `platform/linux/mod.rs` lists the planned Linux implementations.

### Backend wiring (`src-tauri/src/lib.rs`)

- `run()` first runs `core::integrity::startup_report()`. A release EXE whose SHA-256 is wrong exits before Tauri starts (enforced only where `SystemOps::enforce_release_hash()` is true, i.e. Windows).
- `setup` registers managed state: `OcrService` (given `platform::native_ocr()`), `RegionService`, `MonitorService`, `ShortcutBindings`, `SelectionService`, and `LookupService` (a redb file at `app_data_dir/transportal.redb`). It also sets up the system tray, global shortcuts, and the clipboard monitor.
- Every `#[tauri::command]` lives in `commands.rs` and must be listed in `generate_handler!` in `lib.rs`.
- Windows:
  - `shell` (overlay, declared in `tauri.conf.json`).
  - `settings` (created on demand by `open_or_focus_settings`; closing only hides it).
  - The OCR region picker, routed at `#/ocr-region`.

### Pluggable subsystems (trait + registry)

All paths below are under `src-tauri/src/`.

- **Translation**: the `core::translation::Translator` trait, with engines registered in `core::translation::registry()`. Current engines: google, openai (OpenAI-compatible SSE streaming), tencent, youdao. `core/translation/lang.rs` handles language codes. `core/intent.rs` classifies text as word or sentence.
- **Dictionary**: the `core::dictionary::DictSource` trait. `core/dictionary/lookup.rs` (`LookupService`) orders sources, enables them, caches results in an LRU, and reports status. `core/dictionary/mod.rs` holds `DictStore` (redb storage plus ECCEDICT CSV import); `eccedict.rs` downloads/imports the full CSV and `examples.rs` generates LLM example sentences.
- **OCR**:
  - The `core::ocr::OcrEngine` trait with `core/ocr/registry.rs`. Engines are Windows `Media.Ocr` (`platform/windows/ocr.rs`, injected via `Backend::native_ocr()`) and ONNX PP-OCRv5 (`core/ocr/onnx.rs`, using `ort` with `load-dynamic`, so the runtime library is loaded at runtime from staged assets managed by `core/ocr/assets.rs`).
  - `core/ocr/workflow.rs` drives capture → recognize → optional translate sessions.
  - `core/ocr/picture.rs` keeps the image plus text boxes for in-place translation.
  - `core/ocr/region.rs` implements the frozen-desktop region picker.
  - `core/ocr/monitor.rs` implements auto region translation (polling a fixed screen region for changes).
- **Platform**: `platform/mod.rs` defines `Backend` and `Capabilities` and picks `ActiveBackend` with the only `#[cfg]` switch. Contracts: `WindowOps` (DWM, placement, native hide, outside-click probe, move/resize), `SelectionOps` (UIA read, synthesized copy, foreground target, in-place replace), `ClipboardOps` (sequence number, origin, change listener), `GestureSource` (low-level keyboard hook: Ctrl+C+C, ink key, Win+Shift+S, Esc), `DesktopCapture` (GDI/DXGI, capture exclusion), `SystemOps` (memory, integrity policy, fatal error dialog). Call them through the `Platform*` aliases (`PlatformWindow::place_window_smartly(...)`) with the contract trait in scope.
- **Shortcuts**: `core/shortcut.rs` handles global hotkey registration, conflict reporting, capability-based availability and window positioning near the caret or selection. `platform/key_sequence.rs` detects double Ctrl+C from raw key events. `core/clipboard_monitor.rs` forwards copied text when clipboard translation is on.

### Rust → frontend events

The backend pushes state with `emit` / `emit_to("shell", ...)` on `transportal://*` channels. Examples:
- `transportal://selection`, `transportal://ocr`, `transportal://auto-ocr`
- `transportal://shortcut-status`, `transportal://shell-shown`, `transportal://notice`
- `transportal://clipboard-text`, `transportal://focus-input`, `transportal://ocr-assets-changed`

Frontend hooks in `src/hooks/` (`useOcr`, `useAutoOcr`, `useHideOnBlur`, `useShortcutStatus`) and `ShellView` subscribe with `listen`. When you add an event, keep the channel name identical on both sides.

### Frontend state and settings

- A single Zustand store lives in `src/store/useAppStore.ts`, and shared TS types in `src/types/index.ts`. Those types mirror Rust serde structs, so keep them in sync.
- Settings are stored via `tauri-plugin-store` (`settings.json`, `src/store/persistence.ts`).
- In `useBootstrap(true)` (SettingsView), store changes are diffed into a patch. The patch is saved and then broadcast as a settings-changed event. `useBootstrap(false)` (ShellView) only hydrates from that event.
- When you add a persisted setting, update `PersistedSettings`, `loadSettings`/`saveSettings`, and the diff in `useBootstrap.ts`.
- Styling uses Tailwind v4 via `@tailwindcss/vite`. Framer Motion `layoutId` drives the word-card ↔ sentence-bubble morph.

## Docs

User-facing docs are in Chinese: `docs/USER_GUIDE.md`, `docs/OCR.md`, and `docs/STARTUP_CHECKS.md`. UI strings are mostly Chinese too. Asset versions, hashes, and licenses for ONNX are in `src-tauri/resources/ocr/manifest.json`.
