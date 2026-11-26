# Linux native adaptation

## Scope and invariants

Implement native Linux services behind `platform/linux`, keeping Windows APIs,
the `shell/settings/ocr-region` declarations, IPC names, lookup/learning data,
and reader/writer settings ownership unchanged. Do not change SelectionContent
or collect surrounding text. No clipboard or screenshot collection while idle
except the existing explicitly enabled clipboard monitor.

The GSD skill path mentioned by AGENTS.md is not present in this checkout; this
document records the plan and verification gates instead.

## Execution plan

1. Detect Wayland and unsupported/headless sessions at runtime. Replace fixed capability
   flags for Linux with truthful session-dependent flags.
2. Wayland: use GlobalShortcuts portal rather than X11 global hotkeys.
3. Wayland: use the Screenshot portal on explicit request, never use XWayland
   root screenshots or synthesize input to native Wayland windows. Keep
   continuous capture and privileged input unsupported until dedicated adapters.
4. Replace placeholder Linux memory statistics with `/proc` process-tree data.
5. Verify pure logic + private D-Bus portal mocks; run architecture, rustfmt, clippy,
   full Rust/frontend tests and frontend build. Record desktop limitations.

## Non-goals for this increment

PipeWire continuous capture, libei input injection, AT-SPI range/bounds and
layer-shell placement require separate compositor/permission testing. X11 is
explicitly out of scope per user direction. Do not grant unimplemented capabilities.

## Verification record

The X11 work initially started in this uncommitted increment was removed after
the user explicitly narrowed the scope to Wayland-only. Native implementations
now cover portal requests, global shortcut sessions, GTK clipboard and primary
selection, compositor-controlled windows, and /proc accounting. See
[Linux Wayland](LINUX_WAYLAND.md) for supported scope and desktop validation.

Private D-Bus mocks exercise immediate-response races, cancel/deny, session
closure and actual shortcut descriptions. A headless CI pass is not proof of
GNOME/KDE interoperability, mixed-DPI placement or permission UX.

The frontend regression suite exposed an existing version test that wrote
0.1.0 into the real repository. It now runs against a disposable four-file
fixture and verifies the actual project stays unchanged; project version remains
0.1.3. The single-instance plugin must remain first on Windows: the optional
native global-shortcut plugin is appended after it, never before it.
