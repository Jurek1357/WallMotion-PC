#!/usr/bin/env bash
set -euo pipefail

export TMPDIR="${TMPDIR:-/tmp}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-$(mktemp -d)}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
export XDG_SESSION_TYPE=x11
unset WAYLAND_DISPLAY
export LIBGL_ALWAYS_SOFTWARE=1

rm -f "$TMPDIR/wallmotion-settings-debug.log"
rm -f "$TMPDIR/wallmotion-settings.lock"

status=0
xvfb-run -a -s "-screen 0 1280x800x24" bash -c '
  wallmotion-settings &
  app_pid=$!
  found=0
  for _ in $(seq 1 40); do
    if ! kill -0 "$app_pid" 2>/dev/null; then
      echo "wallmotion-settings exited before showing a window" >&2
      wait "$app_pid" || true
      exit 1
    fi
    if xdotool search --onlyvisible --name "WallMotion" >/dev/null 2>&1; then
      found=1
      break
    fi
    sleep 0.25
  done
  if [ "$found" -ne 1 ]; then
    echo "no WallMotion window after 10 seconds" >&2
    kill "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
    exit 1
  fi
  sleep 2
  if ! kill -0 "$app_pid" 2>/dev/null; then
    echo "wallmotion-settings died after the window appeared" >&2
    wait "$app_pid" || true
    exit 1
  fi
  echo "GUI window is up and the process is alive"
  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
' || status=$?

if [ "$status" -ne 0 ]; then
  echo "--- $TMPDIR/wallmotion-settings-debug.log ---" >&2
  cat "$TMPDIR/wallmotion-settings-debug.log" >&2 2>/dev/null || true
fi
exit "$status"
