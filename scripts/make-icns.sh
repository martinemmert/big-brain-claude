#!/usr/bin/env bash
# Renders assets/Brain.icns (and assets/icon.png) from scripts/make-icon.swift.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
set="$work/Brain.iconset"
mkdir -p "$set"
for size in 16 32 128 256 512; do
  swift "$root/scripts/make-icon.swift" "$set/icon_${size}x${size}.png" "$size"
  swift "$root/scripts/make-icon.swift" "$set/icon_${size}x${size}@2x.png" "$((size * 2))"
done
iconutil -c icns "$set" -o "$root/assets/Brain.icns"
swift "$root/scripts/make-icon.swift" "$root/assets/icon.png" 1024
rm -rf "$work"
echo "assets/Brain.icns"
