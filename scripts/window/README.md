# Settings window regressions

Run these commands from the repository root in PowerShell. They require the
Windows SDK (`mt.exe`) and WebView2. Probes use an isolated application identifier
and profile under `src-tauri/target`; they do not register real shortcuts, access
clipboard data, load the user's settings, or capture desktop pixels.

```powershell
# Native host/controller visibility, reopen/restore/resize, background-result
# races, explicit input shortcut, and shell restoration/topmost state.
./scripts/window/settings-probe.ps1

# Build the real React UI before testing its document in WebView2.
pnpm build
./scripts/window/settings-probe.ps1 -Frontend
./scripts/window/settings-probe.ps1 -Frontend -CorruptOpacity
./scripts/window/settings-probe.ps1 -Frontend -ThemeError

# Read-only metadata of the real application's hosts and child windows.
./scripts/window/inspect-settings.ps1
```

The frontend probe loads the built assets with the real store plugin, supplying
only isolated test settings and stub service responses. It checks the actual
React DOM and alpha style. `-ThemeError` requires the opaque recovery panel;
the other two frontend cases require usable settings content.

The native race regression opens settings, then delivers late background shell
results from another thread. Before the fix, these results unconditionally hid
settings. The visibility check and shell presentation now execute together on
the UI thread, and background results preserve an open settings window.
Explicit shortcut dispatch can still hide settings and switch to translation.

The document regression covers errors outside the settings route boundary:
`App`'s theme effect could throw after hydrating malformed JSON, leaving the
entire React root empty. Opacity is validated at hydration, and a boundary around
the complete settings root catches parent effects as well as route rendering.

These tests reproduce specific failure mechanisms. A blank taskbar thumbnail
alone does not identify which mechanism occurred; inspect a failing live window
to distinguish hidden hosts, minimized geometry, and rendering failures.
