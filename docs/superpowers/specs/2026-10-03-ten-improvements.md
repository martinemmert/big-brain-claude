# Ten improvements — plan

Date: 2026-10-03. Decisions by the owner: answer permissions and questions from Brain; unsigned
(ad-hoc) release builds for now; UI language follows the system (German if the first preferred
language is German, otherwise English), overridable in `~/.claude-brain/config.json`
(`{"language": "de" | "en"}`) or `BRAIN_LANG`; terminals: iTerm2, Terminal.app, tmux fully,
VS Code/Cursor focus the project window only.

| # | Improvement | Where |
|---|---|---|
| 1 | English UI, German optional | `brain-app/src/i18n.rs`; every UI string goes through it; demo data in English |
| 2 | Terminal adapters | `brain-app/src/terminal/` (see interface below) |
| 3 | Answer from Brain | permission card: Allow (Return) / Deny (Esc); reply field for sessions that accept input |
| 4 | State without the undocumented session file | transcript tail as a third signal: interrupted turns, rejected tool uses |
| 5 | Menu bar item with the waiting count | `brain-app/src/menubar.rs`, objc2-app-kit `NSStatusItem` |
| 6 | Native notifications | `brain-app/src/notify.rs`, objc2-user-notifications, inside the .app bundle only (osascript fallback); actions Open / Snooze 15 min; sound only for "calls you"; mute per session |
| 7 | Group by project, pin | project = git top level of the cwd (worktrees grouped under their main repo); pins and mutes in `~/.claude-brain/state.json` |
| 8 | History beyond today | load the last 7 days of events; ended sessions searchable; Resume opens `claude --resume <id>` in a new terminal tab |
| 9 | Context per session | model, permission mode and context tokens from the transcript (`message.usage`, `permission-mode` entries); no prices (none we could verify) |
| 10 | Release builds | GitHub Actions: CI (tests + build) and a tag-triggered release with `Brain.app` (ad-hoc signed, zipped) and the `brain` binary; `brain uninstall` |

## Terminal interface (`brain-app/src/terminal/mod.rs`)

```rust
pub enum Host { ITerm, TerminalApp, Tmux { target: String, client_tty: Option<String> }, VsCode, Cursor, Unknown(String) }
pub struct Capabilities { pub focus: bool, pub type_text: bool, pub keys: bool }
pub enum Key { Return, Escape }
pub enum Outcome { Done, NoTerminal, Unsupported(String), Failed(String) }

pub fn host_of(pid: u32) -> Option<Host>;               // walks the process tree
pub fn capabilities(host: &Host) -> Capabilities;
pub fn focus(pid: u32, cwd: Option<&str>) -> Outcome;  // bring the session's pane to the front
pub fn type_text(pid: u32, text: &str) -> Outcome;     // text + Return, without focusing
pub fn send_key(pid: u32, key: Key) -> Outcome;        // without focusing
pub fn open_new(cwd: &str, config_dir: Option<&str>, command: &str) -> Outcome; // new tab in the preferred terminal
```

The pure parts (process-tree classification, tmux output parsing, script building) are unit-tested.

## Outcome (2026-10-03)

All ten shipped in 0.2.0. Verified live: Allow (Return) and Deny (Esc) on real permission
prompts, replying with `T`, terminal adapters for iTerm2, Terminal.app and tmux (VS Code/Cursor
only by fixture), the transcript ending a stale permission state after a denial. Findings on
the way: the Stop hook payload, `system/turn_duration` and `cost-state` transcript entries,
`shell` status; iTerm2 split panes in background tabs need tab → session → window selection.
