#!/usr/bin/env bash
# Builds and installs Brain:
#   - `brain` CLI into ~/.cargo/bin
#   - Brain.app into ~/Applications
#   - hooks + protocol section into every ~/.claude* account (with backups)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

echo "→ brain CLI"
cargo install --quiet --path crates/brain-cli --force

echo "→ Brain.app"
cargo build --quiet --release -p brain-app
app="$HOME/Applications/Brain.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/brain-app "$app/Contents/MacOS/brain-app"
cp assets/Brain.icns "$app/Contents/Resources/Brain.icns"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Brain</string>
  <key>CFBundleDisplayName</key><string>Brain</string>
  <key>CFBundleIdentifier</key><string>local.claude-brain</string>
  <key>CFBundleExecutable</key><string>brain-app</string>
  <key>CFBundleIconFile</key><string>Brain</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSAppleEventsUsageDescription</key><string>Brain fokussiert das iTerm2-Fenster einer Claude-Session.</string>
</dict>
</plist>
PLIST

# Make Finder and the Dock pick up a changed icon.
touch "$app"

echo "→ Hooks & Protokoll"
"$HOME/.cargo/bin/brain" install
