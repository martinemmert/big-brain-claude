<p align="center"><img src="assets/icon-1024.png" width="128" alt="Brain icon"></p>

# Brain

A native macOS and Linux window for everyone running many [Claude Code](https://claude.com/claude-code)
sessions at once. Brain shows every session across all your Claude accounts, what each one
is doing, and above all **which ones are waiting for you**. Answer them right there, or jump
to the session's terminal with one key.

https://github.com/user-attachments/assets/dc7c6023-01a2-48a5-aaae-39ee590d54b0

*A three-minute tour of version 0.1 (German interface), narrated. Brain runs on made-up demo
sessions here.*

![Brain with demo data](docs/screenshot.png)

Built with [GPUI](https://gpui.rs), the GPU-accelerated UI framework from Zed. The interface
follows your system language: English or German.

## What it does

- **Triage inbox.** Sessions that wait for you come first, longest wait on top: red when
  Claude asks a question or needs a permission, amber when a turn finished, quiet blue when the
  turn ended but subagents or background shells still run (from the Stop hook's
  `background_tasks`; Claude's own `brain report --waiting` still marks it red).
- **Answer from Brain.** Allow or deny an open permission prompt (`Y` / `N`), or reply to a
  session that waits for you (`T`). Brain only types when the session's state is unambiguous.
- **All accounts in one place.** `~/.claude`, `~/.claude-second` and any other
  `~/.claude-*` config dir (as used with `CLAUDE_CONFIG_DIR`) are picked up automatically.
- **Latest messages.** Your prompts, Claude's replies rendered as Markdown, tool calls as
  one-line summaries, read live from the session transcript.
- **Session details.** Model, context fill, Claude Code's own cost total and permission mode.
- **Plan usage.** Five-hour and weekly limits per account with their reset times, from the
  status line (Brain wraps your status line command and passes its output through unchanged).
- **Changes.** Branch, ahead/behind and the changed files of the session's working directory.
- **Pull requests.** The PR of each session's branch with its checks (`#12 ✓`, `#12 ✗`, `#12 …`),
  via the GitHub CLI (`gh`, logged in); click it to open the PR.
- **Conflicts.** A warning (and one notification) when two sessions edit the same files, read
  from their transcripts, and a softer hint when they change files in the same checkout.
- **Today.** What every session reported as done today, per project; copy it as Markdown.
- **Timeline.** Every hook event and report of a session.
- **End now, resume later.** `X` twice ends a waiting session (`/exit`); closing the tab works
  too. Ended sessions stay in the list as long as Claude Code keeps their transcript
  (`cleanupPeriodDays`, 30 days by default), with their name, last message and how many days are
  left. `P` saves one under "Saved to resume" at the top; `⏎` resumes it in a new tab.
- **Open the terminal.** iTerm2, Terminal.app and tmux (in any terminal) jump to the exact
  pane; VS Code and Cursor bring the project window forward. On Linux: Konsole, tmux, VS Code
  and Cursor.
- **Search** by name, path, account or last message; **group by project** (git worktrees
  count as their main repository); **pin** and **mute** sessions.
- **Rename** a waiting session; Brain types `/rename <name>` into its terminal.
- **Menu bar count** (macOS) and native **notifications** with *Open* and *Snooze 15 min*, sound only
  when a session calls you; reminders while a session keeps waiting (`remind_after_minutes`
  in `config.json`, 10 by default, 0 turns them off) and snoozing per session.
- **New sessions** in a recent folder and any account (`⌘N`), or from a **template**: a
  Markdown file with folder, account, model and a first prompt with placeholders (below).
- **Move a session to the other account** (`A` twice), e.g. when one hits its five-hour limit:
  Brain copies the transcript, resumes it there with `claude --resume <id> --fork-session` and
  closes the original.
- **Quick replies.** `T`, then `1`–`9` sends a canned reply; set your own with
  `"quick_replies": ["…", "…"]` in `~/.claude-brain/config.json`.
- **English or German**, following your macOS language or, on Linux, your locale (override with `BRAIN_LANG=de|en` or
  `{"language": "de"}` in `~/.claude-brain/config.json`).

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

Requirements: macOS 13+ (Apple Silicon for the release builds), or Linux with glibc 2.35+
(x86_64 or aarch64; see [On Linux](#on-linux)).

### With Homebrew

```sh
brew tap martinemmert/big-brain-claude https://github.com/martinemmert/big-brain-claude
brew install --cask brain
brain install
```

The cask installs `Brain.app` and links the `brain` CLI that ships inside it. The app is
only ad-hoc signed, not notarized, so macOS blocks the first start: System Settings →
Privacy & Security → Open Anyway, or `xattr -dr com.apple.quarantine /Applications/Brain.app`.
`brain install` (once) adds the hooks, see [From source](#from-source).

### From a release

Download `Brain-<version>-macos-arm64.zip` and `brain-<version>-macos-arm64.tar.gz`
(Apple Silicon) from [Releases](https://github.com/martinemmert/big-brain-claude/releases).

1. Unzip and move `Brain.app` to `/Applications`. The app is only ad-hoc signed, not
   notarized, so macOS blocks the first start: right-click → Open (macOS 15: System
   Settings → Privacy & Security → Open Anyway), or run
   `xattr -dr com.apple.quarantine /Applications/Brain.app`.
2. Put `brain` on your `PATH` and run `brain install`. The hooks call `brain` at the path it
   was installed from, so move it first:

   ```sh
   tar -xzf brain-*-macos-arm64.tar.gz
   sudo mv brain /usr/local/bin/   # or any other directory on your PATH
   brain install
   ```

   If macOS refuses to run it, `xattr -d com.apple.quarantine /usr/local/bin/brain`.

### From source

Requires a recent stable Rust toolchain.

```sh
git clone https://github.com/martinemmert/big-brain-claude.git
cd big-brain-claude
./scripts/install-macos.sh
```

This

- builds `dist/Brain.app` and `dist/brain` with `scripts/bundle-macos.sh`,
- installs the `brain` CLI to `~/.cargo/bin`,
- installs `~/Applications/Brain.app` (open it via Spotlight),
- runs `brain install`, which adds Brain's hooks, a `Bash(brain report:*)` permission and a
  short protocol section to the `settings.json` and `CLAUDE.md` of every `~/.claude*`
  account. Existing files are backed up as `*.brain-backup-<timestamp>`; running it again
  is safe.

Running sessions pick up the hooks on their own.

### On Linux

From a release: download `brain-<version>-linux-x86_64.tar.gz` (or `-aarch64`) from
[Releases](https://github.com/martinemmert/big-brain-claude/releases), then

```sh
tar -xzf brain-*-linux-$(uname -m).tar.gz
./brain-*-linux-$(uname -m)/install.sh
```

From source: `./scripts/install-linux.sh` in the repository. It builds with `cargo`, or in
Docker (`compose.yaml`) when Rust is not installed; with Docker, `docker compose run --rm
rust cargo …` runs any other cargo command.

Both install `brain` and `brain-app` to `~/.local/bin`, add Brain to the app launcher
(`~/.local/share/applications/brain.desktop`) and run `brain install` as above.

What differs from macOS:

- Notifications go through the desktop's notification service, with the same actions.
- No menu bar count; the window title shows it.
- Shortcuts use `Ctrl` where macOS uses `⌘`.
- Answering and jumping to a session work in Konsole and tmux; new sessions open in a new
  Konsole tab. For answering in Konsole, turn on *Enable the security sensitive parts of the
  DBus API* in Konsole's settings (General) and restart Konsole windows that were already
  open, they keep the old setting; jumping works without it. Other terminals
  (GNOME Terminal, kitty, …) are recognised but not controlled yet, and `brain show` is not
  supported yet.

### Alfred

Download `Brain.alfredworkflow` from the
[latest release](https://github.com/martinemmert/big-brain-claude/releases/latest) and
double-click it. Type `cc` and part of a session's name, path or latest message:

- `⏎` brings the session's terminal to the front (`brain open <account>:<pid>`),
- `⌥⏎` shows it in Brain (`brain show <account>:<pid>`).

The workflow finds `brain` in `/opt/homebrew/bin`, `/usr/local/bin` or `~/.cargo/bin`.
Sessions are listed in Brain's order: needs you, your turn, working.

## Uninstall

```sh
brain uninstall
```

This removes Brain's hooks, the `Bash(brain report:*)` rule and the protocol section from
the `settings.json` and `CLAUDE.md` of every `~/.claude*` account, with backups as above.
Then delete `Brain.app`, the `brain` binary (`~/.cargo/bin/brain` or wherever you put it)
and `~/.claude-brain`. With Homebrew: `brew uninstall --zap --cask brain` removes all three.
On Linux delete `~/.local/bin/brain`, `~/.local/bin/brain-app`,
`~/.local/share/applications/brain.desktop`,
`~/.local/share/icons/hicolor/{256x256,512x512}/apps/brain.png` and `~/.claude-brain`.

## Keys

| Key | Action |
|---|---|
| `↑` `↓` / `j` `k` | select |
| `⏎`, double-click | open the session's terminal (resume it if it ended) |
| `1`–`9` | open the n-th session |
| `Y` / `N` | allow / deny an open permission prompt |
| `T` | reply to a session that waits for you (then `1`–`9` for a quick reply) |
| `R`, click the name | rename (only while the session waits for you) |
| `/`, `⌘F` | search (`esc` clears) |
| `P` / `M` | pin (an ended session: save it to resume) / mute notifications |
| `S` | snooze: 15 min → 1 h → until tomorrow 9:00 → off |
| `G` / `D` | group by project / today's digest (`⌘C` copies it) |
| `X` `X` | end the session (`/exit`); it stays resumable |
| `A` `A` | move the session to the next account (an ended one resumes there) |
| `⌘N` | start a new session |
| `←` `→` | switch between messages, timeline and changes |
| `⇥` | cycle the account filter |
| `E` | show ended sessions |

On Linux, `Ctrl` takes the place of `⌘`.

`brain status` prints the board in the terminal; `brain sessions --json` lists the open
sessions for scripts (account, pid, name, phase, headline, cwd).

## Templates

Templates live in `~/.claude-brain/templates/*.md` and are edited in your own editor
(`⌘E` in the `⌘N` dialog opens the selected one, `⌘⇧N` creates a new one):

```markdown
---
name: Review a merge request
folder: ~/Work/app
account: main
model: haiku
---
Review the merge request for {branch} and list blocking issues first.
```

Every header line is optional. Brain asks for each `{placeholder}` and, if the template has no
`folder`, for the folder, then starts `claude --model … '<prompt>'` in a new terminal tab.

## Develop

```sh
cargo test -p brain-core              # protocol, state, transcript and Markdown logic
cargo run -p brain-app                # the app against your real sessions
BRAIN_DEMO=1 cargo run -p brain-app   # made-up sessions, e.g. for screenshots
./scripts/make-icns-macos.sh          # re-render the icon
./scripts/bundle-macos.sh             # release build into dist/ (ad-hoc signed Brain.app, brain,
                                      # Brain.alfredworkflow)
```

Pushing a `v*` tag that matches the version in `Cargo.toml` builds a GitHub release with
the zipped app, the `brain` binary, the Alfred workflow and SHA-256 checksums, then points
`Casks/brain.rb` at it (a commit to `main`).

- `crates/brain-core`: event protocol, store, accounts, session discovery, state reducer,
  transcript and Markdown readers
- `crates/brain-terminal`: terminal adapters (iTerm2, Terminal.app, tmux, VS Code, Cursor)
- `crates/brain-cli`: the `brain` binary (`hook`, `report`, `install`, `uninstall`, `status`,
  `sessions`, `open`, `show`)
- `crates/brain-app`: the GPUI app

Design notes: [`docs/superpowers/specs/2026-10-02-claude-brain-design.md`](docs/superpowers/specs/2026-10-02-claude-brain-design.md)

## License

[MIT](LICENSE)
