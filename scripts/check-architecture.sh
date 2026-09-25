#!/usr/bin/env bash
# Enforce the core/ + platform/ dependency rules (see AGENTS.md).
# Run from the repository root: bash scripts/check-architecture.sh
set -uo pipefail
cd "$(dirname "$0")/../src-tauri/src"

status=0
fail() {
  echo "::error::$1"
  echo "$2"
  status=1
}

backends='^platform/(windows|linux|unsupported)/'

# 1. OS conditions live only in the backend switch and inside backends.
hits=$(grep -rnE 'target_os|cfg\((not\()?windows|cfg!\(windows' --include='*.rs' . \
  | sed 's#^\./##' | grep -vE "^platform/mod\.rs:|$backends")
[ -z "$hits" ] || fail "target_os outside platform/mod.rs and the backends" "$hits"

# 2. Shared code never touches the windows crate.
hits=$(grep -rnE '\bwindows::' --include='*.rs' core commands.rs lib.rs)
[ -z "$hits" ] || fail "windows crate used outside platform/windows" "$hits"

# 3. Contracts and backends only reach into core for the OCR engine interface,
#    and never use app state, window labels or event names.
hits=$(grep -rnE 'crate::core::' --include='*.rs' platform | grep -v 'crate::core::ocr::')
[ -z "$hits" ] || fail "platform code depends on core services" "$hits"
hits=$(grep -rnE '\.state::<|get_webview_window|transportal://|"(shell|settings)"' --include='*.rs' platform)
[ -z "$hits" ] || fail "platform code uses app state, window labels or event names" "$hits"

[ $status -eq 0 ] && echo "architecture rules hold"
exit $status
