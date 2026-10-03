# Round three — plan

Date: 2026-10-04. Owner's decisions in brackets.

| # | What | Where |
|---|---|---|
| 1 | Alfred workflow instead of an own hotkey [search → ⏎ terminal, ⌥⏎ show in Brain] | `brain sessions --alfred` (Script Filter JSON), `brain open <account>:<pid>`, `brain show <account>:<pid>` (opens `brain://session/<account>/<pid>`); `dist/Brain.alfredworkflow` built by `scripts/bundle.sh` |
| 2 | Start a new session from Brain | dialog: project (recent folders) + account (+ optional first prompt) → `brain_terminal::open_new` |
| 3 | Remind again + snooze | re-notify while a session keeps waiting (default every 10 min, `remind_after_minutes` in config.json); snooze per session (`S`: 15 min → 1 h → until tomorrow 9:00) persisted in state.json; snoozed sessions are listed apart and never notify |
| 4 | Changes per session | third detail tab: branch, ahead/behind, `git status --porcelain` + `git diff --numstat HEAD` per file |
| 5 | Background work [explicit `--waiting` = needs you; else running `background_tasks` = working in background; else your turn] | Stop hook `background_tasks` (documented in hooks.md, type shell/subagent/…) stored on the stop event; new phase `Background`; protocol text in CLAUDE.md updated |
| 6 | Today overview | third layout next to Status/Projects: done reports and turn ends per project; Copy as Markdown |
| 7 | Usage [wrap the statusline] | `brain statusline`: stores the statusline JSON (`rate_limits`, `context_window`, `cost`) per session in `~/.claude-brain/status/`, then runs the user's previous statusline command with the same stdin and passes its output through; install/uninstall set and restore it; app shows plan limits per account and today's cost |
| 9 | Homebrew + update notice [cask in this repo] | `Casks/brain.rb` (app + `brain` binary shipped inside the bundle), release workflow updates version and sha256; app checks the latest GitHub release once a day |

URL scheme `brain://session/<account>/<pid>`: `CFBundleURLTypes` in the bundle's Info.plist; the app selects that session and comes forward.
