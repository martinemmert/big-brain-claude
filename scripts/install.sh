#!/usr/bin/env bash
# Builds and installs Brain:
#   - `brain` CLI into ~/.cargo/bin
#   - Brain.app into ~/Applications
#   - hooks + protocol section into every ~/.claude* account (with backups)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

# Sign with the first Apple Development identity in the keychain (or $BRAIN_SIGN_IDENTITY), so
# macOS keeps Brain's permissions across updates. Without one, the build stays ad-hoc signed.
if [ -z "${BRAIN_SIGN_IDENTITY:-}" ]; then
  BRAIN_SIGN_IDENTITY="$(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(Apple Development: [^"]*\)".*/\1/p' | head -n 1)"
fi
export BRAIN_SIGN_IDENTITY
./scripts/bundle.sh

echo "→ brain CLI"
mkdir -p "$HOME/.cargo/bin"
# Copy, then rename: overwriting a signed binary in place can get it killed
# by macOS on its next start, and hooks may call it at any moment.
cp dist/brain "$HOME/.cargo/bin/.brain.new"
mv -f "$HOME/.cargo/bin/.brain.new" "$HOME/.cargo/bin/brain"

echo "→ Brain.app"
app="$HOME/Applications/Brain.app"
mkdir -p "$HOME/Applications"
rm -rf "$app"
ditto dist/Brain.app "$app"

# Make Finder and the Dock pick up a changed icon.
touch "$app"

echo "→ Hooks & Protokoll"
"$HOME/.cargo/bin/brain" install
