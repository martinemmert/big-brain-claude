# Fork: Linux-Unterstützung – was bisher gebaut wurde

Stand: 8 Commits auf `main` (`ca27c3d` … `cc77185`) über Upstream-Basis `824ff2c`
(Brain 0.6.x, UI noch auf **gpui**). Upstream hat danach die UI auf **Iced** umgestellt
(0.8.0+); die Spalte „Merge“ sagt, was davon unabhängig ist.

| Bereich | Merge |
|---|---|
| Plattform-Kern (`brain-core`, `brain-terminal`, `brain-cli`) | ✅ UI-unabhängig |
| Installer, Packaging, CI | ✅ UI-unabhängig (CI-Deps für Iced anpassen) |
| Linux-Module der App (Notify, Clipboard, Menubar, i18n) | ⚠️ Logik übernehmbar, gpui-Anteile auf Iced umstellen |
| gpui-UI-Features (Auswahl, Scrolling, Codeblöcke, Fensterrahmen) | ❌ müssen auf Iced neu gebaut werden |

## 1. Plattform-Kern

**`brain-core/src/process.rs`**
- `process_started_ms` für Linux: Startzeit aus `/proc/<pid>/stat` + `btime` aus `/proc/stat`
  (Prozessnamen mit Leerzeichen/Klammern berücksichtigt, mit Test).
- `tty_of`: „kein Terminal“ ist `?` (Linux) bzw. `??` (macOS).

**`brain-core/src/markdown.rs`**
- Codefences mit ``` und ~~~, beliebiger Länge ≥ 3; verschachtelte Fences
  (z. B. `sql` in `markdown`) bleiben im äußeren Block; Einrückung der öffnenden Fence
  wird entfernt (2 Tests).

**`brain-terminal`**
- Dateien nach Plattform benannt: `iterm_macos.rs`, `terminal_app_macos.rs`,
  `applescript_macos.rs` (aus `script.rs` herausgelöst), `konsole_{linux,macos}.rs`,
  `launch_linux.rs`; Auswahl per `#[cfg_attr(..., path = ...)]`.
- **Konsole über D-Bus** (`zbus`, `konsole_linux.rs`): Tab über die Shell-PID finden
  (`org.kde.konsole-<pid>`, `/Sessions/<id>`), Tab fokussieren (Fenster über KWin anheben,
  da Wayland das Apps verbietet), Text tippen, Tasten senden. Ist in Konsole
  „security sensitive parts of the DBus API“ aus, wird das erkannt
  (`Host::Konsole { typing: false }`) und erklärt.
- Neu: `Limit` / `limit(host)` – warum Brain in ein Terminal nicht tippen kann
  (KonsoleLocked, TerminalApp, Editor, Unknown), damit die UI sagen kann, was zu ändern ist.
- `open_new` auf Linux: Konsole → Ptyxis → GNOME Terminal → kgx → `x-terminal-emulator`.
- `tree.rs`: Linux-`ps`-Ausgabe (`tmux: server`, `?`-TTY, Abbruch bei User-`systemd`),
  Erkennung von `konsole`, `code`, `cursor`; `child_toward()` für die Shell unter Konsole
  (mit Linux-Testfall).
- VS Code / Cursor fokussieren unter Linux über die CLI (`code <cwd>`, `cursor <cwd>`).

**`brain-cli`**: `brain show` nutzt `xdg-open` statt `open`.

## 2. Installer, Packaging, CI

- `scripts/install-linux.sh`: installiert `brain` + `brain-app` nach `~/.local/bin`
  (atomar per Copy+Rename), Desktop-Eintrag + Icons (hicolor 256/512), führt
  `brain install` aus; baut im Repo selbst (cargo, sonst Docker).
- `scripts/package-linux.sh`: Release-Tarball `brain-<ver>-linux-<arch>.tar.gz`.
- Icons: `assets/icon.png` → `icon-1024.png`, dazu `icon-256.png`, `icon-512.png`.
- macOS-Skripte umbenannt: `bundle-macos.sh`, `install-macos.sh`, `alfred-macos.sh`,
  `make-icns-macos.sh`, `make-icon-macos.swift`.
- CI/Release: Linux-Jobs (x86_64 + aarch64, ubuntu-22.04 / glibc 2.35), Linux-Tarballs
  im Release, Installationsanleitung in den Release-Notes. *Die apt-Pakete sind für gpui
  gewählt – für Iced prüfen.*
- README: Linux-Abschnitt.

## 3. Linux-Module der App (`brain-app`)

- Modulwahl per Plattform: `notify_{linux,macos}.rs`, `menubar_{linux,macos}.rs`,
  `clipboard_{linux,macos}.rs`; macOS-Abhängigkeiten (`objc2…`) nur für macOS,
  `notify-rust` für Linux.
- **Benachrichtigungen** (`notify_linux.rs`): D-Bus via `notify-rust` mit Aktionen
  Öffnen / Schlummern, Antwort pro Benachrichtigung in eigenem Thread.
- **Clipboard** (`clipboard_linux.rs`): unter Wayland kopiert ein Klick über System-Tools
  (`wl-copy` → Klipper → `xclip` → `xsel`), da gpui dort einen frischen Tastendruck braucht.
- **Menubar** (`menubar_linux.rs`): Stub, Zähler steht im Titel.
- **i18n**: Systemsprache aus `LC_ALL` / `LC_MESSAGES` / `LANG`.
- **Tastenkürzel**: `primary()` = ⌘ auf macOS, Strg sonst; Labels „Strg+“/„Shift+“;
  Beenden mit Strg+Q.
- `app_id = "brain"`, passend zu `brain.desktop` (Taskleisten-Icon).
- UI-Hinweise, was im Terminal umzustellen ist, wenn Brain nicht tippen kann
  (nutzt `brain_terminal::limit`), plattformabhängig formuliert.
- Fehler bei Terminal-Aktionen bleiben unten rechts stehen, bis sie weggeklickt werden
  oder ablaufen.

## 4. gpui-UI-Features (nur gpui, für Iced neu zu bauen)

- **Kopierbare Codeblöcke**: Kopfzeile mit Sprache + Kopieren-Button und „Kopiert“-Rückmeldung;
  die Kopfzeile ist *sticky* (eigenes `StickyHeader`-Element, da gpui kein `position: sticky`
  hat).
- **Scrolling**: einheitliche Scroll-Bereiche für alle Tabs mit eigener Scrollbar
  (Thumb ziehbar, Drag läuft auch außerhalb der Leiste weiter), Button „ans Ende springen“,
  Timeline folgt neuen Events, solange nicht hochgescrollt.
- **Textauswahl** (`selection.rs`): Maus-Auswahl über mehrere Texte eines Scroll-Bereichs,
  Doppelklick wählt Wörter (Ziehen erweitert wortweise), Kopieren.
- **Eigener Fensterrahmen unter GNOME** (`window_frame_linux.rs`): Resize-Kanten und
  -Ecken, Fenster-Buttons, Fenster per Titelleiste ziehen (erst ab Bewegung, damit Klicks
  auf Titelleisten-Controls funktionieren).
- Dateien öffnen mit der Standard-App des Systems (`xdg-open`).

## 5. Stand nach dem Merge mit Upstream 0.10.1 (Iced)

- Abschnitt 4 ist mit dem Merge entfallen (gpui-Code gelöscht); die Features stehen weiter in
  den Fork-Commits und sind auf Iced neu zu bauen. Kopieren mit der Maus und Textauswahl bringt
  Iced zum Teil selbst mit.
- Neue Upstream-Module mit macOS-Code bekommen Linux-Varianten (Auswahl per `cfg_attr` in
  `main.rs`):
  - `clipboard_linux.rs`: Screenshot aus der Zwischenablage über `wl-paste` / `xclip`.
  - `chrome_linux.rs`: Fensterrahmen vom Desktop, Brains Titelzeile 40 pt.
  - `hotkey_linux.rs`: Stub, kein globaler Hotkey (Wayland).
  - `iterm_linux.rs`: Stub, keine iTerm2-Schrift/-Farben.
  - `trash_linux.rs`: `gio trash`, sonst `kioclient move … trash:/`.
- In bestehenden Dateien per `#[cfg]`: `brain://`-URL-Handler (Linux: nur Kommandozeile),
  `NSApplication` (Aktivieren, Ausblenden, Fokus-Abfrage), `open` → `xdg-open`, YAMV nur auf
  macOS, Screenshot-Ordner über `xdg-user-dir PICTURES`, Schriften: Systemschrift und
  `Font::MONOSPACE` statt SF / Menlo, kein Platz für die Ampel-Buttons.
- `notify_linux.rs`: `Response::Reply` ergänzt (noch ungenutzt; Antworten aus der
  Benachrichtigung gibt es bisher nur auf macOS).
- Companion-Abhängigkeiten (hyper, tokio, rcgen …) sind plattformunabhängig.

Offen: Iced-Features aus Abschnitt 4, globaler Hotkey (z. B. über das GlobalShortcuts-Portal),
Antworten aus Benachrichtigungen, `brain://`-Links an eine laufende Instanz.
