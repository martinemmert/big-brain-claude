#!/usr/bin/env bash
# Captures the app shots for the explainer: Brain in demo mode (made-up sessions), driven by
# keyboard through System Events, recorded as window-only screenshots (nothing else on screen).
# Needs Accessibility and Screen Recording permission for the terminal.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/video/build/shots"
mkdir -p "$out"
cd "$root"
cargo build --quiet -p brain-app
pkill -f "target/debug/brain-app" 2>/dev/null || true
BRAIN_DEMO=1 ./target/debug/brain-app >/dev/null 2>&1 &
sleep 4

osascript -e 'tell application "System Events" to tell (first process whose name is "brain-app")
  set frontmost to true
  set position of front window to {80, 60}
  set size of front window to {1440, 860}
end tell'
sleep 1

window_id=$(swift - <<'SWIFT'
import CoreGraphics
let list = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as! [[String: Any]]
for w in list where (w[kCGWindowOwnerName as String] as? String) == "brain-app" {
  let b = w[kCGWindowBounds as String] as? [String: Any] ?? [:]
  if (b["Width"] as? Double) == 1440 { print(w[kCGWindowNumber as String]!); break }
}
SWIFT
)

key() {
  osascript -e 'tell application "System Events" to tell (first process whose name is "brain-app") to set frontmost to true' \
            -e 'delay 0.25' -e "tell application \"System Events\" to $1"
  sleep 0.9
}
shot() { screencapture -x -o -l "$window_id" "$out/$1.png"; echo "  $1"; }

key 'key code 53';  shot overview                       # esc: plain start
key 'key code 124'; shot timeline                       # →: timeline tab
key 'key code 123'                                      # ←: back to messages
key 'key code 48'; key 'key code 48'; shot account-second   # ⇥ ⇥: only "second"
key 'key code 48'                                       # ⇥: all accounts again
key 'keystroke "/"'; key 'keystroke "billing"'; shot search
key 'key code 53'
for _ in 1 2 3 4 5 6; do key 'key code 126'; done       # ↑ to the top session
key 'keystroke "r"'; key 'keystroke " rate limits"'; shot rename
key 'key code 53'
pkill -f "target/debug/brain-app" || true
