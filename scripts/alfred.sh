#!/usr/bin/env bash
# Builds dist/Brain.alfredworkflow: keyword `cc` lists the Claude sessions
# (`brain sessions --alfred`); ⏎ opens the session's terminal (`brain open`),
# ⌥⏎ shows it in Brain (`brain show`).
#
# The info.plist follows the structure Alfred 5 exports (objects with uid,
# connections keyed by source uid, uidata positions). Usage: alfred.sh <version>
set -euo pipefail

version="${1:?usage: alfred.sh <version>}"
root="$(cd "$(dirname "$0")/.." && pwd)"
dist="$root/dist"
out="$dist/Brain.alfredworkflow"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Fixed uids, so every build produces the same workflow.
filter_uid="6E1E7A50-6C1B-4C44-9C1F-2B6A3E6F0C01"
open_uid="6E1E7A50-6C1B-4C44-9C1F-2B6A3E6F0C02"
show_uid="6E1E7A50-6C1B-4C44-9C1F-2B6A3E6F0C03"
# NSEventModifierFlagOption (1 << 19): the connection taken on ⌥⏎.
option_modifier=524288

# Alfred runs scripts without the login PATH; find `brain` from Homebrew
# (Apple Silicon, Intel) or cargo install. The scripts below are XML text:
# `&` and `<` must be written as &amp; and &lt;.
path_prefix='PATH="/opt/homebrew/bin:/usr/local/bin:$HOME/.cargo/bin:$PATH"'

cp "$root/assets/icon-1024.png" "$work/icon.png"
cat > "$work/info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>bundleid</key>
	<string>local.claude-brain.alfred</string>
	<key>category</key>
	<string>Productivity</string>
	<key>connections</key>
	<dict>
		<key>$filter_uid</key>
		<array>
			<dict>
				<key>destinationuid</key>
				<string>$open_uid</string>
				<key>modifiers</key>
				<integer>0</integer>
				<key>modifiersubtext</key>
				<string></string>
				<key>vitoclose</key>
				<false/>
			</dict>
			<dict>
				<key>destinationuid</key>
				<string>$show_uid</string>
				<key>modifiers</key>
				<integer>$option_modifier</integer>
				<key>modifiersubtext</key>
				<string>Show in Brain</string>
				<key>vitoclose</key>
				<false/>
			</dict>
		</array>
	</dict>
	<key>createdby</key>
	<string>Martin Emmert</string>
	<key>description</key>
	<string>Find a Claude Code session and jump to its terminal or show it in Brain</string>
	<key>disabled</key>
	<false/>
	<key>name</key>
	<string>Brain</string>
	<key>objects</key>
	<array>
		<dict>
			<key>config</key>
			<dict>
				<key>alfredfiltersresults</key>
				<true/>
				<key>alfredfiltersresultsmatchmode</key>
				<integer>0</integer>
				<key>argumenttreatemptyqueryasnil</key>
				<false/>
				<key>argumenttrimmode</key>
				<integer>0</integer>
				<key>argumenttype</key>
				<integer>1</integer>
				<key>escaping</key>
				<integer>102</integer>
				<key>keyword</key>
				<string>cc</string>
				<key>queuedelaycustom</key>
				<integer>3</integer>
				<key>queuedelayimmediatelyinitially</key>
				<true/>
				<key>queuedelaymode</key>
				<integer>0</integer>
				<key>queuemode</key>
				<integer>1</integer>
				<key>runningsubtext</key>
				<string>Loading sessions…</string>
				<key>script</key>
				<string>$path_prefix
if ! command -v brain &gt;/dev/null; then
  echo '{"items":[{"title":"brain not found","subtitle":"Install Brain: brew install --cask brain","valid":false}]}'
  exit 0
fi
brain sessions --alfred</string>
				<key>scriptargtype</key>
				<integer>1</integer>
				<key>scriptfile</key>
				<string></string>
				<key>subtext</key>
				<string>Claude Code sessions: ⏎ terminal, ⌥⏎ Brain</string>
				<key>title</key>
				<string>Claude sessions</string>
				<key>type</key>
				<integer>0</integer>
				<key>withspace</key>
				<true/>
			</dict>
			<key>type</key>
			<string>alfred.workflow.input.scriptfilter</string>
			<key>uid</key>
			<string>$filter_uid</string>
			<key>version</key>
			<integer>3</integer>
		</dict>
		<dict>
			<key>config</key>
			<dict>
				<key>concurrently</key>
				<false/>
				<key>escaping</key>
				<integer>102</integer>
				<key>script</key>
				<string>$path_prefix
brain open "\$1"</string>
				<key>scriptargtype</key>
				<integer>1</integer>
				<key>scriptfile</key>
				<string></string>
				<key>type</key>
				<integer>0</integer>
			</dict>
			<key>type</key>
			<string>alfred.workflow.action.script</string>
			<key>uid</key>
			<string>$open_uid</string>
			<key>version</key>
			<integer>2</integer>
		</dict>
		<dict>
			<key>config</key>
			<dict>
				<key>concurrently</key>
				<false/>
				<key>escaping</key>
				<integer>102</integer>
				<key>script</key>
				<string>$path_prefix
brain show "\$1"</string>
				<key>scriptargtype</key>
				<integer>1</integer>
				<key>scriptfile</key>
				<string></string>
				<key>type</key>
				<integer>0</integer>
			</dict>
			<key>type</key>
			<string>alfred.workflow.action.script</string>
			<key>uid</key>
			<string>$show_uid</string>
			<key>version</key>
			<integer>2</integer>
		</dict>
	</array>
	<key>readme</key>
	<string>Type cc and part of a session's name, path or latest message.

⏎ brings the session's terminal to the front (brain open), ⌥⏎ shows it in Brain (brain show).

Needs the brain CLI in /opt/homebrew/bin, /usr/local/bin or ~/.cargo/bin: https://github.com/martinemmert/big-brain-claude</string>
	<key>uidata</key>
	<dict>
		<key>$filter_uid</key>
		<dict>
			<key>xpos</key>
			<real>50</real>
			<key>ypos</key>
			<real>50</real>
		</dict>
		<key>$open_uid</key>
		<dict>
			<key>xpos</key>
			<real>270</real>
			<key>ypos</key>
			<real>50</real>
		</dict>
		<key>$show_uid</key>
		<dict>
			<key>xpos</key>
			<real>270</real>
			<key>ypos</key>
			<real>180</real>
		</dict>
	</dict>
	<key>userconfigurationconfig</key>
	<array/>
	<key>variablesdontexport</key>
	<array/>
	<key>version</key>
	<string>$version</string>
	<key>webaddress</key>
	<string>https://github.com/martinemmert/big-brain-claude</string>
</dict>
</plist>
PLIST
plutil -lint -s "$work/info.plist"

mkdir -p "$dist"
rm -f "$out"
# info.plist at the root of the zip, like the workflows Alfred exports.
(cd "$work" && zip -q -X "$out" info.plist icon.png)
echo "→ $out"
