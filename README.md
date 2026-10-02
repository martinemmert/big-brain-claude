# Brain

One native window (GPUI) for every Claude Code session across all accounts
(`~/.claude` → **main**, `~/.claude-second` → **second**, any `~/.claude-*`):
who is working, who finished, who waits for you — and one keystroke to jump to
the session's iTerm2 tab.

## Install

```sh
./scripts/install.sh
```

- `brain` CLI → `~/.cargo/bin/brain`
- `Brain.app` → `~/Applications/Brain.app` (open via Spotlight)
- Hooks + protocol section into each account's `settings.json` / `CLAUDE.md`
  (backups as `*.brain-backup-<timestamp>`; safe to re-run)

## Protocol

Every session writes one JSON line per event to
`~/.claude-brain/events/YYYY-MM-DD.jsonl`:

- **Hooks** (automatic): `SessionStart`, `UserPromptSubmit`, `Notification`,
  `Stop`, `SessionEnd` → `brain hook`
- **Reports** (Claude, per `CLAUDE.md`):
  `brain report --doing|--waiting|--done "<one line>"`

Without hooks the app still shows a coarse state from Claude Code's own
`<config>/sessions/<pid>.json` files.

The **Nachrichten** tab shows the last 40 messages of the selected session
(your prompts, Claude's replies in full, tool calls as one-liners), read live
from `<config>/projects/*/<session-id>.jsonl`.

`brain status` prints the board in the terminal.

## Keys

`↑↓`/`jk` select · `⏎` or double-click jump to iTerm · `1–9` jump directly ·
`←→` switch Nachrichten/Verlauf · `⇥` cycle account filter · `E` show ended sessions ·
`⌘Q` quit

## Develop

```sh
cargo test -p brain-core
cargo run -p brain-app
```

Design: `docs/superpowers/specs/2026-10-02-claude-brain-design.md`
