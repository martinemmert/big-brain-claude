# Round eight — Brain on Iced

Date: 2026-10-09. Replaces GPUI 0.2.2 (no longer published by Zed, hand-rolled text input without
dead keys, IME, undo or selection) with Iced 0.14.

- **Same crate, same bundle.** `brain-app` keeps its name, Info.plist, URL scheme and release.
  The model, prefs, config, notifications, menu bar item, trash, login `PATH` and demo data are
  unchanged; new are `app.rs` (state, keys, actions, background work), `ui.rs` (drawing),
  `style.rs` (palette and small widgets), `markdown.rs` (messages, timeline), `format.rs` (text).
- **Keys:** one `event::listen_with` subscription sees every key press and whether a text field
  took it. Shortcuts act only on keys no field took; Esc always leaves the current mode (fields
  take Esc to unfocus themselves). Text fields are Iced's `text_input` (search, reply, rename,
  new session).
- **Scrolling the selection into view:** list entries have fixed heights (`Item::height`), so the
  selected entry's offset is known and `scroll_to` keeps it visible.
- **Title bar:** an empty unified `NSToolbar` gives the window the standard 52 pt title bar with
  centred traffic lights; Brain draws its title row underneath (`window::drag` on press).
- **`brain://` links:** an `NSAppleEventManager` handler for `kAEGetURL` (Iced has no
  `open_urls` hook). The app menu is named "Brain" via `NSProcessInfo.setProcessName`.
- **Fonts:** San Francisco (`.SF NS`) and Menlo; the variable system font's bold cut doesn't
  resolve, so titles use semibold.
- **Demo mode** neither loads nor saves the user's `state.json`.

Verified: demo walkthrough (arrow keys, search, reply field with ⌥-dead-key umlauts and paste,
new-session dialog, clean-up, Today, Projects); the installed bundle with real sessions:
`brain show` link selects and scrolls to the session, menu bar item shows the count,
notifications are delivered from `local.claude-brain`. Not ported: the pulsing halo around the
"calls you" dot.

## 0.8.1 — the conversation in the terminal's font

The message list is drawn like Claude Code: `> prompt` on a faint band, `●` before replies (white)
and tool calls (green, `Tool(argument)`), Markdown in one size with bold headings. The font is
iTerm2's default profile font (`NSUserDefaults` suite `com.googlecode.iterm2`, PostScript name →
family via `NSFont`), else `chat_font`/`chat_font_size` from config.json win, else Menlo 13.
`⏺` is replaced by `●`: terminal fonts lack it and Iced falls back to the colour emoji font.
Tool name and argument are separate texts, because Iced shapes a whole word in its first
letter's font ("Bash(pnpm" would come out bold).
