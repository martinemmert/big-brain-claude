# Claude Brain — Design

Date: 2026-10-02

## Goal

One native macOS window that shows every running Claude Code session across
both accounts (`~/.claude` = **main**, `~/.claude-second` = **second**), what
each one is doing, and — above all — which ones are waiting for the user.
One keystroke jumps to the iTerm2 window of a session.

Success: the user stops cycling through 20 terminal windows; a glance at Brain
tells them where they are needed.

## Decisions

| Topic | Decision |
|---|---|
| Interaction | Show + jump to terminal (no answering from the app in v1) |
| Terminal | iTerm2 tabs/windows, no tmux |
| UI framework | GPUI (`gpui` 0.2.2 from crates.io), pure Rust |
| Protocol | Claude Code hooks (automatic state) + `brain report` (explicit content) |
| Layout | Triage inbox (left) + session timeline (right), account filter |

## Components (Rust workspace)

- `crates/brain-core` — event model, JSONL store, account detection, process-tree
  lookup, session-file reader, state reducer. All logic lives here and is tested.
- `crates/brain-cli` — binary `brain`:
  - `brain hook` — called by Claude Code hooks, reads hook JSON from stdin,
    appends an event. Always exits 0 and never blocks.
  - `brain report --doing|--waiting|--done "<text>"` — explicit status by Claude.
  - `brain install` — adds hooks to both `settings.json` and the protocol
    section to both `CLAUDE.md`, with backups; idempotent.
- `crates/brain-app` — GPUI app reading the store and both `sessions/` dirs live.

## Data sources

1. `<config>/sessions/<pid>.json`, written by Claude Code itself: `pid`,
   `sessionId`, `cwd`, `name`, `status` (`busy|idle|waiting|shell`), `updatedAt`.
   Gives a coarse state even for sessions without hooks.
2. `~/.claude-brain/events/YYYY-MM-DD.jsonl` — append-only, shared by both
   accounts:

```json
{"v":1,"ts":"2026-10-02T14:15:00Z","account":"second","pid":40461,
 "session_id":"…","cwd":"…","source":"report","kind":"waiting","text":"…"}
```

`source` is `hook` or `report`. `kind` is one of `prompt`, `permission`,
`stop`, `session_start`, `session_end`, `doing`, `waiting`, `done`.

**Account** comes from `CLAUDE_CONFIG_DIR` (unset → main; its basename
`.claude-second` → second; any other → the basename without the dot).
**Pid** is found by walking the parent-process chain until a pid has a
`<config>/sessions/<pid>.json` file.

## State

| Trigger | State |
|---|---|
| hook `UserPromptSubmit`, `report --doing`, file `busy` | Working |
| hook `Notification` | NeedsYou (permission/input) |
| `report --waiting` | NeedsYou (question text) |
| hook `Stop`, `report --done`, file `idle`/`waiting` | YourTurn (finished) |
| hook `SessionEnd`, pid not alive | Ended |

The newest event wins. Report text is shown as the session's headline until a
newer prompt arrives. NeedsYou and YourTurn are both listed under "Braucht dich",
sorted by how long they have been waiting.

## UI

Left: "Braucht dich" cards (name, account badge, waiting time, question),
"Arbeitet" compact rows, "Beendet" collapsed. Right: selected session header,
path/pid, highlighted open question, timeline of events tagged hook/report.
Top bar: account filter Alle/main/second. Keys: ↑↓ select, ⏎ jump, 1–9 pick,
Tab cycles the account filter. macOS notification when a session enters NeedsYou.

Jump: `ps -o tty= -p <pid>` → AppleScript asks iTerm2 for the session with that
tty and selects its window, tab and session.

## Messages tab (added 2026-10-02)

The detail pane has two tabs: **Nachrichten** (default) and **Verlauf**.
Nachrichten reads the last 40 messages of the selected session from its
transcript (`<config>/projects/*/<session-id>.jsonl`, tail only, reloaded when
the file grows): user prompts, Claude's full replies, tool calls as one-line
summaries, and harness input (subagent hand-backs, task notifications) as faint
system notes. Thinking blocks, tool results and sidechain lines are skipped. New
messages scroll into view unless the user scrolled up.

## Search and rename (added 2026-10-02)

- `/` or `⌘F` focuses a search field above the list; every whitespace-separated
  term must appear in name, path, account or headline (case-insensitive).
- `R` (or clicking the name) edits the session name. Enter types
  `/rename <name>` plus Return into the session's iTerm2 pane without focusing
  it. Only offered while the turn has ended (`Session::accepts_input`), so the
  text never lands in a permission dialog or a running turn.
- Jumping addresses iTerm2 windows by `id` and selects session → tab → window;
  index-based references broke with several windows because selecting a
  window reorders them.

## Visual design (added 2026-10-02)

Ink-blue surfaces (`#0F1322` → `#1D2338`) and four signal colours that each mean one thing:
coral `#FF5C6C` calls you, amber `#F2B84B` your turn, blue `#5AA9FF` working, green
`#45D19A` done. The memorable element is the waiting card: a colour rail, the waiting time
as the strongest number, and a soft glow on the selected one. Sentence-case section titles
with counts, metadata as chips, monospace only for real code (paths, commands, code
blocks). Claude's replies are rendered from Markdown (`brain_core::markdown`). The icon is
a 3 × 3 grid of sessions with one calling (`scripts/make-icon.swift`). `BRAIN_DEMO=1`
loads made-up sessions for screenshots.

## Error handling

- Malformed JSONL lines are skipped.
- App not running → nothing is lost; today's log is replayed on start.
- `brain report` outside a Claude session → error message, non-zero exit.
- `brain hook` swallows all errors (exit 0) so it never disturbs Claude.
- Jump failure → status message in the app.

## Testing

Unit/integration tests in `brain-core`: reducer (event sequences → state),
account detection, session lookup against fixture directories, hook payload
parsing, installer idempotency on copied config files. GPUI rendering is not
unit-tested; the app is verified by running it.

## Out of scope (v1)

Menu bar icon, answering from the app, multi-day history and search,
claude.ai cloud sessions.
