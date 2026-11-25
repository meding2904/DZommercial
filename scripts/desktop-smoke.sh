#!/usr/bin/env bash
# Headless smoke test for the Tauri desktop host.
#
# Starts a private Weston headless compositor, opens the real azahar-admin
# window for five seconds, and fails if the process exits early, panics, or
# logs a Tauri/WebKit runtime error. This exercises window creation, the
# mounted renderer and the initialization bridge without a physical display.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOCKET="wayland-azahar-smoke-$$"
PROFILE="$(mktemp -d)"
LOG="$(mktemp)"
WESTON_LOG="$(mktemp)"
REPORT="$(mktemp)"
WESTON_PID=""

cleanup() {
  pkill -x azahar-admin 2>/dev/null || true
  if [[ -n "$WESTON_PID" ]]; then
    kill "$WESTON_PID" 2>/dev/null || true
    wait "$WESTON_PID" 2>/dev/null || true
  fi
  rm -rf "$PROFILE"
  rm -f "$REPORT"
}
trap cleanup EXIT

pkill -x azahar-admin 2>/dev/null || true
weston --backend=headless-backend.so --width=1380 --height=900 \
  --socket="$SOCKET" --idle-time=0 >"$WESTON_LOG" 2>&1 &
WESTON_PID=$!

for _ in $(seq 1 50); do
  [[ -S "${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/$SOCKET" ]] && break
  sleep 0.1
done

cargo build -p azahar-desktop --manifest-path "$ROOT/Cargo.toml" >/dev/null

WAYLAND_DISPLAY="$SOCKET" GDK_BACKEND=wayland AZAHAR_PROFILE_DIR="$PROFILE" \
  AZAHAR_SMOKE_REPORT="$REPORT" AZAHAR_SMOKE_CLOSE=1 \
  timeout 15s "$ROOT/target/debug/azahar-admin" >"$LOG" 2>&1 &
APP_PID=$!

sleep 1
if ! kill -0 "$APP_PID" 2>/dev/null; then
  echo "desktop smoke failed: azahar-admin exited before the window was up" >&2
  tail -30 "$LOG" >&2
  exit 1
fi
if ! pgrep -x azahar-admin >/dev/null; then
  echo "desktop smoke failed: no azahar-admin process" >&2
  tail -30 "$LOG" >&2
  exit 1
fi

set +e
wait "$APP_PID"
STATUS=$?
set -e
if [[ "$STATUS" -ne 0 ]]; then
  echo "desktop smoke failed: the smoke close hook exited with $STATUS" >&2
  tail -30 "$LOG" >&2
  exit 1
fi
if [[ ! -s "$REPORT" ]]; then
  echo "desktop smoke failed: no runtime geometry report" >&2
  tail -30 "$LOG" >&2
  exit 1
fi
if ! jq -e '.window.width > 0 and .window.height > 0 and .monitor.width > 0 and .monitor.height > 0' "$REPORT" >/dev/null; then
  echo "desktop smoke failed: invalid window/monitor geometry: $(cat "$REPORT")" >&2
  exit 1
fi
if ! find "$PROFILE" -name 'settings.json' -exec grep -q '"width"' {} +; then
  echo "desktop smoke failed: the close hook did not persist window bounds" >&2
  exit 1
fi
if grep -Eq "panicked at|thread 'main' panicked|ERROR .*tauri|Failed to create window" "$LOG"; then
  echo "desktop smoke failed: runtime error in the log" >&2
  tail -30 "$LOG" >&2
  exit 1
fi

echo "desktop smoke passed: window $(jq -r '.window.width' "$REPORT")x$(jq -r '.window.height' "$REPORT"), monitor $(jq -r '.monitor.width' "$REPORT")x$(jq -r '.monitor.height' "$REPORT"), bounds persisted"
