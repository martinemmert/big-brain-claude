# Brain's new line: the terminal, a project tree, and nothing else in the way

Date: 2026-11-11. Mockups: `2026-11-11-brain-new-line-mockups.html` (next to this file; direction
**C "Baum"** is the chosen one, with the calm of **A "Werkbank"**). Published copy:
https://claude.ai/artifact/WVD1XjJ7kmHddTEE2dbyDv

## Why

Brain grew from many single wishes and lost its line. It started (10-02) as a monitor that watched
sessions in other terminals; since round nine (10-09) sessions run as `claude --bg` in Brain's own
terminal, but the terminal was only added as one more tab, and the monitor-era UI stayed on top of
it: a status box ("Fertig, du bist dran" / "Noch keine Meldung von dieser Session." with a red Reply
button), a reply mode, an "Open ⏎" button, tabs for Messages, Timeline, Changes, Files, Questions,
Team and Konsil, a footer with "T reply / ⏎ open". Martin uses none of that.

**How Martin actually works (his words, condensed):** he uses the **sidebar** and the **terminal**,
nothing else. He goes into a project, opens a session, carries the conversation on in the terminal.
He avoids iTerm wherever he can. He rarely reads code, but would like to see **which files a session
is changing right now**, with a diff when he wants one (as PhpStorm shows it). He has never used
the Messages, Timeline, Files or Konsil tabs. An own chat UI instead of the terminal is not wanted:
Claude Code's TUI is good, and rebuilding it is pointless.

## Goals

1. **The terminal is the main thing** and gets all the room the window has: no status box, no tab
   row, no footer above or below it, beyond a slim header line.
2. **A project tree on the left** (direction C): project → its sessions → for the session being
   looked at, its changed files. **Projects never disappear** from it because they are old or
   quiet; only Martin hides one.
3. **"Needs you" shows in exactly one place**, on the session in the tree (a red ring and one status
   line), and goes away once he has looked at it.
4. **Every command is reachable without the palette**: through the macOS menu bar (today it shows
   only "Brain" with nothing in it) and, for a session, its context menu. ⌘K stays as the shortcut.
5. **Notes for later** (`brain later`, `/later`) show inside their project in the tree, where he
   looks anyway. Today they live in `~/.claude-brain/later.jsonl` and only show behind the `L` key;
   Martin did not know where they went.
6. **Usage per account** (5-hour and weekly limit, forecast "voll ~Do 14:00", which account has
   room) stays visible, calmly: it is a reason to use Brain over `claude agents`.
7. **The changes of a session** are near, not a place to visit: files under the session in the
   tree, a click shows the diff as a floating preview over the terminal (like Quick Look).
8. **Mac basics:** zoom (⌘+ / ⌘− / ⌘0), copy only after selecting and pressing ⌘C.
9. **It feels fast.** The lag Martin feels is fixed (see *Performance*), independent of the layout.

## Non-goals

- An own chat or conversation view. The terminal is the conversation.
- Agent teams. Removed completely (see *Removed*); Martin will set teams up differently later.
- Reworking the phone companion. It keeps working (see *Constraints*), but this design is the Mac.
- A switch away from Iced. The research (below) shows the lag comes from how Brain uses Iced.
- New states such as "ready for review" or diff comments sent to the session (ideas from other
  tools; not wanted now).

## Anti-goals (it fails even if it works, when …)

- … the sidebar reorders itself under the mouse (sessions jumping between "needs you / working /
  done" groups). Order is by project and stays put. The by-state order lives only in the status item
  and behind ⇧⌘U.
- … a project or session vanishes from the tree on its own.
- … a single-letter key does something when Martin types into the terminal.
- … the terminal loses height to a panel that is open by default.
- … "needs you" is shown twice (on the session and again in a box elsewhere in the window).

## The window (direction C with the calm of A)

See the mockup, tab C. From left to right: tree, terminal. At the bottom a status line.

### The tree (left)

- **Project rows** as fixed headings, in a stable order (the order today: the project with the most
  urgent session first is *not* kept – that reorders; see *Left to the builder* for the order).
  Each shows the account chip (main / second …) and folds open/closed. A project without a live
  session shows "ruht" and stays. "+ Neues Projekt …" at the end.
- **Session rows** under their project: state dot, the session's name (Claude Code's title), the
  time since its state changed. A **status line** under the name **only when the session needs
  Martin** (red: a question or a permission, e.g. "Freigabe: Bash(npm run e2e)") **or finished and
  is unseen** (amber: the last sentence of its reply). All other sessions are one line; the detail
  shows on hover. A session that needs him carries a **red ring**.
- **Changed files** under the session that is being looked at (only that one, so the tree stays
  calm): "Änderungen · 4 · +259 −3", then one row per file, newest first, with +/−. A click opens the
  floating diff preview. The data exists today (`brain_core` files of a session from its transcript,
  and the Git changes view).
- **"Für später · n"** as a foldable row inside its project; unfolded it lists the notes. ⏎ starts
  one as a new session there; checking off as today.
- Ended sessions: the last day stays under its project (dimmed); older ones through search, as today.
- States and colours stay as today (`style.rs`): red `CALLS` asks / needs a permission, amber `TURN`
  finished and unseen, blue `WORKING`, violet-blue `BACKGROUND` turn ended but subagents/shells
  run, grey `ENDED`.

### The terminal (centre)

- Full height. Above it only a **slim header line**: project › session name, branch, account chip,
  context fill. The context gauge's amber "offer /compact" from 80 % stays in that line.
- No tabs. ⌘D (split: a second terminal beside) and ⇧⌘A (all terminals as live previews) stay.
- ⌘T (login shell in the session's folder below its terminal) stays.
- An ended session's terminal area keeps today's "before resuming" summary.

### The diff preview

- Floating over the terminal, like Quick Look: file path, +/−, "vor 1 min · <session>", the diff,
  "In PhpStorm öffnen" (the editor Brain opens today), "Schließen" (esc). It does not take layout
  room from the terminal.

### The status line (bottom, one line, ~26 px)

- Left: "1 braucht dich ⇧⌘U", "1 fertig", "3 arbeiten".
- Right: usage per account ("main 5 h 46 % Woche 21 % voll ~Do 14:00", "second … hat Platz"), and
  the zoom level when not 100 %.
- It replaces today's footer with key hints and the usage block in the sidebar.

### The status item (menu bar, right)

- Brain's mark with a badge = number of sessions that need Martin.
- Opened: the sessions that need him (with Erlauben / Ablehnen for a permission), the finished
  unseen ones, usage per account, "Neue Session …". This is where the by-state order lives.

### The menu bar

Exactly as in the mockup (section "Die Menüleiste"), built from today's `palette_entries`
(`app.rs`) and the session context menu (`menu_lines`, `ui.rs`). Shortcuts are on ⌘ or ⌥⌘ so they
never collide with typing into the terminal; items that don't apply are greyed out.

- **Brain:** Über Brain, Einstellungen, Konten (set up a new account, align accounts), iPhone
  koppeln (Brain Link), Ausblenden, Beenden.
- **Ablage:** Neue Session … ⌘N, Neue Session in „<project>“ ⌥⌘N, Neues Projekt … ⇧⌘N, Session
  fortsetzen … ⌘O, Aus iTerm nach Brain holen … ⌥⌘O, Für später notieren … ⌘L, Später-Notizen des
  Projekts ⇧⌘L, Aufräumen …, Projekt ausblenden, Fenster schließen ⌘W.
- **Bearbeiten:** Widerrufen, Wiederholen, Ausschneiden, Kopieren ⌘C, Einsetzen ⌘V, Bildschirmfoto
  einsetzen ⌃⌘V, Alles auswählen, Im Terminal suchen … ⌘F, Sessions suchen … ⌥⌘F.
- **Session:** Erlauben ⌘Y, Ablehnen ⇧⌘Y, Nächste, die dich braucht ⇧⌘U, Nächste / Vorherige
  Session ⌃⇥ / ⌃⇧⇥, Umbenennen … ⌘R (any time, not only while it waits), Anheften, Stummschalten,
  Pausieren ▸, In anderes Konto ▸, Forken in ▸, In anderes Projekt ▸, Neu starten mit …, In iTerm
  öffnen, Composer … ⌘E, Session beenden ⌥⌘X, Hart beenden, In den Papierkorb ⌘⌫.
- **Darstellung:** Befehlspalette … ⌘K, Seitenleiste ⌃⌘S, Änderungen unter der Session zeigen ⌥⌘0,
  Terminal teilen ⌘D, Alle Terminals im Überblick ⇧⌘A, Beendete / ruhige Hintergrund- / automatisch
  gestartete Sessions zeigen, Größer ⌘+, Kleiner ⌘−, Originalgröße ⌘0, Vollbild ⌃⌘F.
- **Fenster:** Im Dock ablegen, Zoomen, the sessions that need Martin as ⌘1…⌘9, Alle nach vorne.
- **Hilfe:** Alle Tasten … ⌘?, Log öffnen, Gärtner: kaputte Sessions finden …, Was ist neu.

## Mac basics

- **Zoom:** ⌘+ / ⌘− / ⌘0 scale the whole window (UI and terminal font together); the level is
  remembered (prefs).
- **Copy:** text is copied only on ⌘C after selecting. Finding: the "copy on select" Martin sees
  is Claude Code copying its own selection through OSC 52; iced-term turns that into
  `Event::ClipboardStore` (`iced-term/src/backend.rs`), and Brain writes it to the clipboard in
  `Message::TerminalCopy` (`app.rs`). Brain's own ⌘C (`BindingAction::Copy`, `iced-term/src/view.rs`)
  copies Brain's selection. The menu shows "Kopieren beim Markieren (aus)" as a toggle, off by
  default.

## Removed

Removed from the UI and, where nothing else needs it, from the code:

- The status box `callout` (`ui.rs`), the reply mode (`Mode::Reply`, quick replies 1–9 and the
  `/` completion in it), the "Open ⏎" button, the monitor footer with key hints, the empty state's
  "start one in the terminal" text (now: ⌘N).
- The detail tab row and the tabs **Messages, Timeline, Changes, Files, Questions, Team, Konsil**.
  What survives of them: changed files (under the session in the tree), ⌘F find in the terminal,
  "Open the conversation as Markdown" (⌘K / menu) and ⇧⌘C copy last answer if cheap to keep.
- **Agent teams** entirely: the Team tab, team start in tmux from New session (⌘T in that dialog),
  `team_panes` polling, the tmux attach path, `brain_core::teams` where unused afterwards.
- **Konsil** entirely: the tab, `konsil.rs` in brain-app and brain-core.
- The screenshot offer above the footer (paste with ⌘V / drag stays).
- Single-letter keys on the list (see *Answers*, 4).

## Performance (independent of the layout, can go first)

Measured and read in the code on 2026-11-11 (Iced 0.14, wgpu). The lag comes from Brain's use of
Iced, not from Iced. In order of effect, each shippable on its own:

1. **No full `sync()` after every message** (`app.rs`, end of `update`): terminal output of any
   session, mouse moves and scroll events end without it. Exceptions that still sync: terminal
   shutdown, title, bell. Keep "seen" marking correct (`prefs.see` runs inside `sync`).
2. **No `ps` on selection change** (`sync_host` → `terminal::host_of`, 90–140 ms measured): skip it
   for sessions with a Brain terminal; off-thread for the rest; cache per pid.
3. **Terminal snapshot without the whole scrollback** (`iced-term/src/backend.rs` `internal_sync`
   clones `grid()` incl. 10 000 lines under the lock on every event): copy what is displayed;
   `sync_font`/resize only on real change; at most one redraw per frame. Selection and copy across
   the screen edge must keep working.
4. **Group once per update**: `groups()` runs about six times per update over all sessions;
   `Session::matches` builds a lowercase search text even when the search is empty.
5. **Tick work off the UI thread**: `model.refresh()` parses the last 512 KiB of every grown
   transcript into JSON every 800 ms; tail from a remembered offset instead. `git`/`prefs.save`
   off-thread.
6. Afterwards, measure again; the overview (⇧⌘A) without a canvas cache and per-cell `fill_text`
   are the next step if still needed.

The removals (Messages tab re-parsing Markdown every view, Team polling) take further work off the
hot path for free.

## Constraints

- Rust + Iced as today; AppKit for the menu bar / status item / notifications as today
  (`menubar.rs`). No new dependency without asking Martin first; no workaround dependencies.
- The phone companion apps (separate repos: `brain-companion-ios`, `brain-companion-native`) read
  Brain Link (`companion.rs`, `app/link.rs`). Removing teams and Konsil must not break them.
- `brain konsil …` is called by the konsil skill (another repository). It must not fail a session's
  Bash call after the removal (see *Answers*, 8).
- The interface follows the macOS language (German / English) as today; every new string in both.
- iTerm sessions are not a focus (Martin avoids iTerm). They still show in the tree and keep "Aus
  iTerm nach Brain holen" and "In iTerm öffnen"; they get no special UI.

## Research behind this (2026-11-11, short)

- Similar tools: Claude Code Agent View (`claude agents`: groups Needs input / Working / Completed,
  peek with number keys), cmux (native, libghostty; a ring on the pane that needs you, ⇧⌘U to the
  latest unread), Warp (a notification counts as read once the tab is opened), Superset, Conductor,
  Codex app (Electron, many lag complaints), Happy, Agent Deck. The good ones show "needs you" once
  and clear it on viewing. Nobody else has multi-account usage forecasts, permissions answered in
  any terminal through a mod, and a real TUI side by side with project overview.
- Directions considered: A "Werkbank" (calm sidebar, changes as a drawer: too close to the unused
  Files tab), B "Lage" (sidebar grouped by state like Agent View: reorders under the mouse, loses the
  per-project way of working), C "Baum" (chosen).

## Decided by Martin

- Direction C with the calm of A, as recommended.
- Terminal stays the conversation; no own chat UI.
- Teams and Konsil go. Messages, Timeline, Files tabs go (unused).
- Projects never disappear from the sidebar.
- All commands reachable without the palette; use the macOS menu bar.
- Zoom; copy only on ⌘C after selecting.
- Notes for later findable in their project.

## Calls I made (Martin can overrule)

- Screenshot offer removed (raised in the playback, not objected to).
- Composer (⌘E) and Quick Terminal stay, reachable from the menu.
- Questions and Changes tabs go with the others (Martin uses only the terminal).
- Usage moves from the sidebar into the bottom status line.
- "Kopieren beim Markieren" stays as a menu toggle, off by default.
- Rename any time (the "only while waiting" guard was for typing into foreign terminals).

## Left to the builder

- The project order in the tree, as long as it is stable (e.g. alphabetical, or by Martin's own
  order with drag; not by urgency).
- How the tree remembers folded projects; hover detail design; exact sizes and spacing within the
  mockup's look.
- How the floating preview is built in Iced (overlay/stack), and how the diff is computed (session
  files vs. Git) as long as it shows the session's changes.
- The order in which removals, performance steps and the new layout land, as long as each step
  ships working.
- Whether `brain_core` code only the removed features used is deleted or left.

## Answers after the builder check

Decided by Martin (2026-11-11):

1. **Which projects are in the tree: only by hand.** A project is in the tree when Martin adds it
   ("+ Neues Projekt …": folder picker) or starts a session in it from Brain (⌘N / ⌥⌘N). It stays
   until he hides it. A new persisted project list (prefs) holds them; worktrees count as their main
   checkout (as `project_of` does today); "In anderes Projekt" moves a session into a listed project
   (or adds the named one). *My call:* sessions living in a folder that is not in the list (started
   in iTerm, by `claude --bg` from a shell, programs) show in a section **"Weitere"** at the end of
   the tree, each with "Als Projekt behalten"; they are not lost.
2. **Red goes away only when answered.** A permission, an options question or `brain report
   --waiting` keeps the red ring, status line, badge and ⇧⌘U/⌘1…⌘9 target until answered. Amber
   (finished, unseen) goes away on viewing; the yellow "Question?" guess too, as today.
3. **Copy: select, then ⌘C.** Brain holds the text Claude Code copies via OSC 52 right after a mouse
   selection in the terminal (Brain already tracks `last_terminal_release`) and writes it to the
   clipboard only on ⌘C. An OSC 52 copy without a preceding mouse release (e.g. `/copy`) goes to the
   clipboard at once, as today. Brain's own selection (outside mouse mode) copies on ⌘C as today.
   No "copy on select" toggle in the menu.

My calls (Martin can overrule):

4. **Focus:** a click on a session hands the keyboard to its terminal. Single-letter keys on the
   list go away; their actions live in the menu bar with ⌘ / ⌥⌘ shortcuts and in the context menu.
   ↑/↓ and ⏎ in the tree work while the tree has focus (after ⌃⌘S-focus or a click on a project row
   or a later-note).
5. **Menu items without a feature today:** "Einstellungen … ⌘," opens `~/.claude-brain/config.json`
   in the editor; "Neues Projekt … ⇧⌘N" opens a folder picker and adds the project; "Session
   fortsetzen … ⌘O" focuses the session search with ended sessions shown; "Über Brain" is the
   standard macOS About panel; "Was ist neu" is dropped.
6. **Shortcut clashes:** keys the terminal uses win while it has focus. "In den Papierkorb" moves
   to ⌥⌘⌫ (⌘⌫ deletes the line in the terminal). ⌘H stays macOS's Hide, like in every Mac terminal.
7. **Status item:** a standard NSMenu (sessions that need Martin with Erlauben / Ablehnen as
   submenu items, finished unseen ones, usage as text lines, "Neue Session …"). The mockup's custom
   panel is not built now.
8. **`brain konsil …`** keeps reporting start/end as doing/done and exits 0; `stand` becomes a
   no-op. Only the tab and its parsing go.
9. **Phone:** remove "team" from `FEATURES` in `companion.rs` and leave the team fields empty; the
   native companion only shows team UI when the feature is advertised (checked by the builder).
