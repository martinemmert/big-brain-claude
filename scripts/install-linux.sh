#!/usr/bin/env bash
# Installs Brain on Linux:
#   - `brain` and `brain-app` into ~/.local/bin
#   - a desktop entry and the icon, so Brain shows up in the app launcher
#   - hooks + protocol section into every ~/.claude* account (with backups)
# From an unpacked release it installs the binaries next to it. In the repository it
# builds them first, with the host's cargo if there is one, else in Docker (compose.yaml).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"

if [[ -x "$here/brain" && -x "$here/brain-app" ]]; then
  binaries="$here"
  icon="$here/brain.png"
else
  root="$(cd "$here/.." && pwd)"
  cd "$root"
  echo "→ release build"
  if command -v cargo >/dev/null; then
    cargo build --quiet --release -p brain-app -p brain-cli
  else
    env UID="$(id -u)" GID="$(id -g)" docker compose run --rm rust cargo build --quiet --release -p brain-app -p brain-cli
  fi
  binaries="$root/target/release"
  icon="$root/assets/icon.png"
fi

bin="$HOME/.local/bin"
data="${XDG_DATA_HOME:-$HOME/.local/share}"

echo "→ brain, brain-app"
mkdir -p "$bin"
# Copy, then rename: hooks may call `brain` at any moment.
for name in brain brain-app; do
  cp "$binaries/$name" "$bin/.$name.new"
  mv -f "$bin/.$name.new" "$bin/$name"
done

echo "→ desktop entry"
mkdir -p "$data/icons/hicolor/1024x1024/apps" "$data/applications"
cp "$icon" "$data/icons/hicolor/1024x1024/apps/brain.png"
cat > "$data/applications/brain.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Brain
Comment=All your Claude Code sessions, and which ones wait for you
Exec=$bin/brain-app
Icon=brain
Categories=Development;
StartupWMClass=brain-app
EOF
update-desktop-database "$data/applications" 2>/dev/null || true

echo "→ Hooks & Protokoll"
"$bin/brain" install

case ":$PATH:" in
  *":$bin:"*) ;;
  *) echo "Hinweis: $bin ist nicht im PATH." ;;
esac
