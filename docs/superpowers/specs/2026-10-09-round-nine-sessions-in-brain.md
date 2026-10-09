# Round nine — sessions run in Brain (stage 1 of 5)

Date: 2026-10-09.

**Direction.** Brain becomes where sessions run: Claude Code's own background sessions
(`claude --bg`), shown in an embedded terminal (`claude attach`). Anthropic's 2026 limits on
third-party harnesses don't apply: Brain runs the official `claude` client, like any terminal.

- **Terminal:** `crates/iced-term`, a fork of iced_term 0.8.0 (alacritty_terminal). Changes: ⌘-click
  links are returned to the app (`Action::OpenLink`) instead of opened, and also match file paths;
  a click updates the pointer's grid cell before link lookup; Ctrl+letter sends control bytes;
  ⇧⏎/⌥⏎ send ESC CR, ⇧⇥ back-tab; ⌘ combinations never type text; `⏺ ⏵ ⏸` are drawn as
  `● ▸ ‖` (terminal fonts lack them; the fallback is a colour emoji).
- **Look:** iTerm2's default profile font (via `NSFont` family) and ANSI colours on Brain's ink;
  line height 1.4; padding; at most ~120 columns, centred; a focus ring.
- **Starting:** `⌘N` runs `claude --bg [--model] [-n] [prompt]` in the folder; the printed short
  id is selected and attached once `claude agents` lists it. Untrusted folders: Claude Code's
  message is shown. `⌘I` in the dialog starts in iTerm as before.
- **Taking over:** `I I` on an iTerm session sends `/exit`, waits for the process to end, then
  `claude --bg --resume <id> -n <name>`; verified: the conversation (a code word) survived.
- **Attach:** selecting a running background session switches to the Terminal tab and attaches
  (also when the selected session just moved into Brain). Terminals keep running per session.
- **Focus:** clicks ask the terminal whether it has the keyboard (ring); `⌘J` into it, `⌘[` out;
  Esc goes to Claude while the terminal has the keyboard. On the Terminal tab, typing after a
  click elsewhere goes to the terminal, never to shortcuts.
- **Files in:** a dropped file's path is inserted shell-escaped (like iTerm); `⌘V` with a
  picture on the clipboard saves it to `~/.claude-brain/attachments/` and inserts the path.
  Verified: Claude read a pasted screenshot and described it. Drag & drop is not verified live.
- **Reader:** `⌘`-click a path (relative ones from the session's folder, `:line` ignored):
  Markdown rendered in the chat font, code highlighted (Base16 Ocean), images shown, else
  "open in its app"; `⌘⏎` opens the editor, Esc or ✕ closes. Verified with README.md and a PNG.
- **State:** `claude agents` reports "working" late; an idle process status wins. Background
  sessions show unless they had no activity for a day (then only with `B`). Agents are polled
  every 5 s.

**Safety (from the commit review):** links from the output open only with `http`, `https` or
`mailto` (`open` would hand `file://`, `ssh:` or app schemes to any app); the reader's "in
editor" never uses plain `open` on a path from the output (a `.command`/`.app` would run): text
goes to `open -t`, pictures and PDFs to Preview, anything else nowhere.
