#!/usr/bin/env bash
# Usage: bash smoke.sh /path/to/transportal-gestures.so
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
plugin=$(realpath "${1:?pass the built native KWin plugin}")
tmp=$(mktemp -d "${TMPDIR:-/tmp}/transportal-kwin-smoke.XXXXXX")
trap 'rm -rf -- "$tmp"' EXIT
mkdir -p "$tmp/plugins/kwin/plugins" "$tmp/runtime" "$tmp/config/TransPortal" "$tmp/data"
chmod 700 "$tmp/runtime"
cp "$plugin" "$tmp/plugins/kwin/plugins/transportal-gestures.so"
# Allow an extracted distro SDK to supply its own header/library flags.
flags=${QT_SMOKE_FLAGS:-$(pkg-config --cflags --libs Qt6Widgets Qt6DBus)}
c++ -std=c++20 -fPIC -Wall -Wextra -Werror "$here/smoke.cpp" -o "$tmp/smoke" $flags
cat > "$tmp/config/kwinrc" <<'EOF'
[Plugins]
transportal-gesturesEnabled=true
transportal-overlayEnabled=true
EOF
{
    printf '[General]\ncount=1\nrules=TransPortalOverlay\n\n'
    cat "$here/../transportal-overlay.kwinrule"
} > "$tmp/config/kwinrulesrc"
mkdir -p "$tmp/data/kwin/scripts"
cp -R "$here/../transportal-overlay" "$tmp/data/kwin/scripts/transportal-overlay"
cat >> "$tmp/data/kwin/scripts/transportal-overlay/contents/code/main.js" <<'EOF'
// Test-only assertion; user scripts never log cursor coordinates.
workspace.windowAdded.connect(function (window) {
    if (String(window.captionNormal) !== "TransPortal Overlay [1]") return;
    var point = workspace.cursorPos;
    if (window.frameGeometry.x === point.x + 12 && window.frameGeometry.y === point.y + 12)
        print("Real KWin script: overlay positioned relative to the cursor");
});
EOF
printf '[General]\nApplicationExecutable=%s\nEnableInPlaceReplacement=true\n' "${RUST_GESTURE_PROBE:-${RUST_REPLACEMENT_PROBE:-$tmp/smoke}}" > "$tmp/config/TransPortal/KWinIntegration.conf"
env -u DISPLAY -u WAYLAND_DISPLAY -u DBUS_SESSION_BUS_ADDRESS -u QT_PLUGIN_PATH \
  XDG_RUNTIME_DIR="$tmp/runtime" XDG_CONFIG_HOME="$tmp/config" XDG_DATA_HOME="$tmp/data" \
  XDG_SESSION_TYPE=wayland XDG_CURRENT_DESKTOP=KDE LIBGL_ALWAYS_SOFTWARE=1 \
  QT_QPA_PLATFORM=wayland QT_WAYLAND_TEXT_INPUT_PROTOCOL="${QT_WAYLAND_TEXT_INPUT_PROTOCOL:-zwp_text_input_v3}" \
  QT_PLUGIN_PATH="$tmp/plugins" QT_LINUX_ACCESSIBILITY_ALWAYS_ON=1 QT_LOGGING_RULES='kwin_scripting.debug=true' \
  timeout 15 dbus-run-session -- kwin_wayland --virtual --no-lockscreen --no-kactivities \
    --exit-with-session "$tmp/smoke" > "$tmp/log" 2>&1 || { cat "$tmp/log"; exit 1; }
if [[ -n ${RUST_GESTURE_PROBE:-} ]]; then
    grep -F 'Real KWin Rust integration' "$tmp/log" && grep -F 'KWin gesture negatives:' "$tmp/log" \
      && grep -F 'did not edit text or clipboard' "$tmp/log" || { cat "$tmp/log"; exit 1; }
else
    grep -F 'selected range replaced' "$tmp/log" || { cat "$tmp/log"; exit 1; }
fi
grep -F 'without taking keyboard focus' "$tmp/log" \
  && grep -F 'overlay positioned relative' "$tmp/log" || { cat "$tmp/log"; exit 1; }
if [[ -n ${RUST_SELECTION_PROBE:-} ]]; then grep -F 'Real AT-SPI/Qt:' "$tmp/log"; fi
if [[ -n ${RUST_REPLACEMENT_PROBE:-} ]]; then grep -F 'Real Rust/KWin:' "$tmp/log"; fi
