<p align="center"><img src="assets/icon.png" width="128" alt="Brain icon"></p>

# Brain

A native macOS window for everyone running many [Claude Code](https://claude.com/claude-code)
sessions at once. Brain shows every session across all your Claude accounts, what each one
is doing, and above all **which ones are waiting for you**. One key jumps to the session's
iTerm2 tab.

![Brain with demo data](docs/screenshot.png)

Built with [GPUI](https://gpui.rs), the GPU-accelerated UI framework from Zed. The interface
is in German.

## What it does

- **Triage inbox.** Sessions that wait for you come first, longest wait on top: red when
  Claude asks a question or needs a permission, amber when a turn finished.
- **All accounts in one place.** `~/.claude`, `~/.claude-second` and any other
  `~/.claude-*` config dir (as used with `CLAUDE_CONFIG_DIR`) are picked up automatically.
- **Latest messages.** Your prompts, Claude's replies rendered as Markdown, tool calls as
  one-line summaries, read live from the session transcript.
- **Timeline.** Every hook event and report of a session.
- **Jump to iTerm2.** Selects the right window, tab and split pane.
- **Search** by name, path, account or last message.
- **Rename** a waiting session; Brain types `/rename <name>` into its terminal.
- **Notifications** when a session starts waiting for you.

## How sessions report

Every session appends JSON lines to `~/.claude-brain/events/YYYY-MM-DD.jsonl`, from two
sources:

- **Hooks** (automatic): `SessionStart`, `UserPromptSubmit`, `Notification`, `Stop` and
  `SessionEnd` call `brain hook`.
- **Reports** (written by Claude, instructed via `CLAUDE.md`):
  `brain report --doing|--waiting|--done "<one line>"`.

Without hooks Brain still shows a coarse state from Claude Code's own
`<config>/sessions/<pid>.json` files.

## Install

Requirements: macOS 13+, iTerm2, a recent stable Rust toolchain.

```sh
git clone https://github.com/martinemmert/big-brain-claude.git
cd big-brain-claude
./scripts/install.sh
```

This

- installs the `brain` CLI to `~/.cargo/bin`,
- builds `~/Applications/Brain.app` (open it via Spotlight),
- adds Brain's hooks, a `Bash(brain report:*)` permission and a short protocol section
  to the `settings.json` and `CLAUDE.md` of every `~/.claude*` account. Existing files are
  backed up as `*.brain-backup-<timestamp>`; running it again is safe.

Running sessions pick up the hooks on their own.

### Uninstall

Remove `~/Applications/Brain.app`, `~/.cargo/bin/brain` and `~/.claude-brain`, then delete
the `brain hook` entries and the `Bash(brain report:*)` rule from each `settings.json` and the
block between `<!-- brain:start -->` and `<!-- brain:end -->` from each `CLAUDE.md` (or
restore the backups).

## Keys

| Key | Action |
|---|---|
| `↑` `↓` / `j` `k` | select |
| `⏎`, double-click | open the session in iTerm2 |
| `1`–`9` | open the n-th session |
| `/`, `⌘F` | search (`esc` clears) |
| `R`, click the name | rename (only while the session waits for you) |
| `←` `→` | switch between messages and timeline |
| `⇥` | cycle the account filter |
| `E` | show ended sessions |

`brain status` prints the board in the terminal.

## Develop

```sh
cargo test -p brain-core              # protocol, state, transcript and Markdown logic
cargo run -p brain-app                # the app against your real sessions
BRAIN_DEMO=1 cargo run -p brain-app   # made-up sessions, e.g. for screenshots
./scripts/make-icns.sh                # re-render the icon
```

- `crates/brain-core`: event protocol, store, accounts, session discovery, state reducer,
  transcript and Markdown readers
- `crates/brain-cli`: the `brain` binary (`hook`, `report`, `install`, `status`)
- `crates/brain-app`: the GPUI app

Design notes: [`docs/superpowers/specs/2026-10-02-claude-brain-design.md`](docs/superpowers/specs/2026-10-02-claude-brain-design.md)

## License

[MIT](LICENSE)
