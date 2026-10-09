#!/usr/bin/env bash
# Builds the release binaries and packages them into dist/:
#   - dist/Brain.app (signed with $BRAIN_SIGN_IDENTITY when set, else ad-hoc; the CLI
#     ships inside at Contents/MacOS/brain for the Homebrew cask)
#   - dist/brain (the CLI)
#   - dist/Brain.alfredworkflow (scripts/alfred.sh)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

# The version from [workspace.package] in Cargo.toml.
version="$(awk '
  /^\[/ { section = $0 }
  section == "[workspace.package]" && $1 == "version" { gsub(/"/, "", $3); print $3; exit }
' Cargo.toml)"
if [[ -z "$version" ]]; then
  echo "bundle.sh: no version in [workspace.package] of Cargo.toml" >&2
  exit 1
fi

echo "→ release build $version"
cargo build --quiet --release -p brain-app -p brain-cli

dist="$root/dist"
app="$dist/Brain.app"
rm -rf "$app" "$dist/brain"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/brain "$dist/brain"
cp target/release/brain-app "$app/Contents/MacOS/brain-app"
cp target/release/brain "$app/Contents/MacOS/brain"
cp assets/Brain.icns "$app/Contents/Resources/Brain.icns"
cat > "$app/Contents/Info.plist" <<PLIST
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
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSAppleEventsUsageDescription</key><string>Brain brings the terminal of a Claude Code session to the front and answers it when you ask it to.</string>
  <key>CFBundleURLTypes</key>
  <array>
    <dict>
      <key>CFBundleURLName</key><string>local.claude-brain</string>
      <key>CFBundleURLSchemes</key><array><string>brain</string></array>
    </dict>
  </array>
</dict>
</plist>
PLIST

# A fixed identity keeps macOS permissions (accessibility, data of other apps, …) across
# updates; an ad-hoc signature is a new app to macOS with every build.
if [ -n "${BRAIN_SIGN_IDENTITY:-}" ]; then
  echo "→ signing as $BRAIN_SIGN_IDENTITY"
  codesign --force --deep --sign "$BRAIN_SIGN_IDENTITY" "$app"
else
  echo "→ ad-hoc signing"
  codesign --force --deep --sign - "$app"
fi
codesign --verify --deep --strict "$app"

echo "→ Alfred workflow"
"$root/scripts/alfred.sh" "$version"

echo "→ $app"
echo "→ $dist/brain"
