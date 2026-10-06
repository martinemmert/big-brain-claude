#!/usr/bin/env bash
# Builds the release binaries and packs dist/brain-<version>-linux-<arch>.tar.gz:
# brain, brain-app, the icon and install.sh (scripts/install-linux.sh) in one folder.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

# The version from [workspace.package] in Cargo.toml.
version="$(awk '
  /^\[/ { section = $0 }
  section == "[workspace.package]" && $1 == "version" { gsub(/"/, "", $3); print $3; exit }
' Cargo.toml)"
if [[ -z "$version" ]]; then
  echo "package-linux.sh: no version in [workspace.package] of Cargo.toml" >&2
  exit 1
fi

echo "→ release build $version"
cargo build --quiet --release -p brain-app -p brain-cli

name="brain-$version-linux-$(uname -m)"
stage="$root/dist/$name"
rm -rf "$stage" "$stage.tar.gz"
mkdir -p "$stage"
cp target/release/brain target/release/brain-app "$stage/"
cp assets/icon.png "$stage/brain.png"
cp scripts/install-linux.sh "$stage/install.sh"
tar -czf "$stage.tar.gz" -C "$root/dist" "$name"
rm -rf "$stage"

echo "→ $stage.tar.gz"
