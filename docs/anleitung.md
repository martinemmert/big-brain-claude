# Brain – kleine Anleitung

Brain behält alle deine Claude-Code-Sessions im Blick, über alle Konten (`~/.claude`,
`~/.claude-second`, …). Es zeigt, wer auf dich wartet, lässt Sessions in seinem eigenen
Terminal laufen und hilft, die Liste kurz zu halten.

## Einrichten

- **Installieren:** `./scripts/install.sh` baut Brain, legt `Brain.app` nach `~/Applications`, den
  `brain`-Befehl nach `~/.cargo/bin` und richtet jedes Konto ein. Danach gilt alles für jede neue
  Claude-Session.
- **Wie Sessions melden:** über den **Brain-Mod** (läuft in Claude Code selbst). Eigene Hooks
  und einen Wrapper um die Statuszeile trägt Brain nicht mehr ein; `brain install` baut die einer
  älteren Version wieder aus. Laufende Sessions laden den Mod mit `/reload-plugins`;
  `/plugin` zeigt „1 mod active · brain“.
- **Berechtigungen:** Brain ist mit deiner Apple-Development-Identität signiert. macOS fragt jede
  Berechtigung einmal und merkt sich die Antwort auch über Updates.

## Die Liste

Links stehen die Sessions nach **Projekt** gruppiert, unter „Projekte“ (die Zahl daneben, rot: wie
viele auf eine Entscheidung warten). Oben steht das Projekt mit dem dringendsten Anliegen: erst eine
Entscheidung, dann ein Bericht zum Lesen, dann der Rest. In jedem Projekt kommen die Sessions in
dieser Reihenfolge, jede mit ihrer Karte:

- **Zu entscheiden** (rot): Claude braucht eine Freigabe (**Erlauben** / **Ablehnen**), stellt
  eine Frage mit Optionen (ein Klick auf die Option antwortet) oder hat mit `brain report --waiting`
  gefragt. Die Reihenfolge ist Freigabe, Optionen, Frage, darin die am längsten Wartende zuerst.
  Wartet nichts, steht dort „Nichts wartet auf dich.“
  - **Frage?** (gelber Streifen, kein Zähler): Der Turn endete ohne Meldung, der letzte Satz endet
    mit „?“. Die Karte bleibt, solange du die Session ausgewählt hast, und verschwindet beim
    Weiterklicken (mit ↓ gehst du so durch alle) oder sofort mit **Nur Bericht**.
- **Zum Lesen:** fertige Sessions, die du noch nicht angesehen hast, neueste zuerst, mit dem
  letzten Satz der Antwort. Als gesehen zählt eine Session, sobald du sie in Brains vorderem Fenster
  auswählst. Sie bleibt in der Zone, solange sie ausgewählt ist, und geht beim Weiterklicken.
  Eine Session, die schon gesehen ist oder deren Turn endet, während sie offen ist, landet nicht
  in „Zum Lesen“. **alle gesehen** (oben bei „Projekte“) macht das für alle.
- **Der Rest**, was läuft: blau arbeitet, hellblau Subagents oder Hintergrund-Shells laufen noch,
  grau ruht; „schläft“ heißt, Claude hat den Prozess nach einer Stunde Ruhe beendet, eine
  Nachricht weckt die Session wieder.

Auf dem Handy ist die Liste genauso gruppiert, mit einem Filter nach Projekt oder Konto.

Eine Session gehört woanders hin? Rechtsklick → **In ein anderes Projekt …** (oder ⌘K, „Projekt“ tippen):
ein vorhandenes Projekt, ein neues per Namen, oder zurück in ihr eigenes. Brain merkt sich das, auch
fürs Handy (⋯ → **In anderes Projekt …**, oder lange drücken in der Liste).

Angeheftete Sessions (★) stehen in ihrem Projekt; beendete angeheftete unter „Zum Fortsetzen gemerkt“.
Alles, was gerade nicht nach dir ruft, liegt am Listenende: in der Fußzeile und in einem
zugeklappten Abschnitt, jeweils mit Zahl; ein Klick (oder ⌘K) klappt auf:

- **▸ N beendet** (`E`): die des letzten Tages; ältere findet die Suche.
- **▸ N im Hintergrund** (`B`): Claudes Hintergrund-Sessions ohne Aktivität seit einem Tag.
- **▸ automatisch** (`U`): Sessions, die Programme gestartet haben, etwa Security-Reviews nach
  Commits oder `claude -p`-Skripte.
- **Pausiert und ausgeblendet** (`S`, `⌫` `⌫`): als eigener Abschnitt darunter.

Die Sessions aus Hintergrund, automatisch und pausiert melden sich nie per Mitteilung. So
verschwindet keine Session spurlos.

Oben in der Titelleiste steht die Zahl der Wartenden und der Kontofilter (umschalten mit `⇥`).
Tastenkürzel stehen nur auf der gewählten Karte.

- **Suchen:** `/` durchsucht die Liste nach Name, Ordner, Konto und Überschrift.
- **Kontofilter:** in der Titelleiste, umschalten mit `⇥`.
- **Rechtsklick** auf eine Session zeigt alle passenden Aktionen mit ihren Kürzeln.
- **Fußleiste:** zeigt die Tasten, die für die gewählte Session gerade passen; `?` zeigt alle.

## Eine Session ansehen

Rechts steht die ausgewählte Session.

- **Kopfzeile:** Name (Klick zum Umbenennen), Konto, Markierungen wie ★ angeheftet,
  „● klingelt“ (Bell) oder der Titel, den Claude Code gesetzt hat.
- **Darunter eine Zeile:** Ordner, Modell, die **Context-Anzeige** (ab 80 % gelb mit Knopf
  **/compact**, ab 90 % rot), ein abweichender Freigabe-Modus und der PR mit Checks. Kosten,
  pid und Startzeit zeigt sie beim Überfahren.
- **Wartekarte:** worauf die Session wartet, mit **Antworten** (`T`) bzw. **Erlauben**/**Ablehnen**
  (`Y`/`N`) als Hauptknopf.

Die Tabs (`←` `→`):

| Tab | Inhalt |
|---|---|
| **Nachrichten** | Die letzten Nachrichten im Terminal-Look. `⌘F` öffnet darüber die Suchleiste für die ganze Session, mit den Ansichten **Gespräch**, **Deine Prompts** (alles, was du gefragt hast), **Fragen** (jede Rückfrage von Claude mit deiner Antwort; offene sind markiert) und **Ereignisse** (Start, Prompts, Freigaben, Turn-Enden, Meldungen). `esc` schließt sie. `⌘⇧C` kopiert die letzte Antwort, ⌘K → „Gespräch als Markdown öffnen“ öffnet es in YAMV. |
| **Dateien** | Jede Datei, die die Session geschrieben, geändert, gelesen oder bekommen hat, neueste zuerst. So findest du z. B. erzeugte Markdown-Dateien. Klick öffnet sie. Der letzte Abschnitt, **Git-Änderungen**, zeigt Branch und geänderte Dateien; ein Klick zeigt den Diff, „Änderung verwerfen“ (zweimal) legt die jetzige Fassung in den Papierkorb und holt die aus HEAD zurück. |
| **Konsil** | Nur bei Sessions, die ein Konsil gehalten haben: die Beratung als Bühne, Runde für Runde. |
| **Terminal** | Die Session selbst, in Brains Terminal (siehe unten). Bei einer beendeten Session steht hier vorher ein **Steckbrief**: Anfang, letzte Frage, letzte Antwort, offene Rückfrage, geänderte Dateien. |

## Sessions in Brain laufen lassen

- **Neue Session:** `⌘N`. Ordner, Konto, Modell und erster Prompt, auf Wunsch aus einer
  Vorlage. Zuletzt benutzte Ordner stehen oben; tippst du einen Pfad (`~/`), listet Brain die
  Ordner darin, `→` geht in den gewählten hinein. `⇧⇥` wählt das Antwort-Profil (Output-Style),
  sonst gilt, was in den Einstellungen des Kontos steht. Sie läuft als Claude-Code-Hintergrund-Session
  in Brains Terminal weiter, auch wenn Brain beendet wird.
- **Antwort-Profil wechseln:** `⌘K` → „Antwort-Profil von …“ tippt `/output-style <Name>` in die
  Session; es gilt ab der nächsten Nachricht.
- **Agenten-Team:** `⌘T` im Neue-Session-Dialog startet die Session als Team wie `claude-team`
  (Agent `team_agent`, Standard `dirigent`; Plugin-Ordner `team_plugin_dir`, Standard
  `~/.claude-mods/team-chat`). Ein Team braucht eine interaktive Session: Brain startet es in einer
  unsichtbaren tmux-Session (`brain-team-…`), sein Terminal-Tab hängt sich daran, und es läuft weiter,
  wenn Brain beendet wird. `⌘K` → „Als Agenten-Team fortsetzen“ / „Ohne Team fortsetzen“ schaltet eine
  laufende Session um (sie startet dafür neu, das Gespräch bleibt). Hat eine Session Agenten, zeigt der Tab **Team** ihre Fragen an dich
  („Frage an Martin: …“, mit Mitteilung; „Antworten“ schreibt dem Lead, der gibt es weiter), wer
  arbeitet und wer wartet, was jeder verbraucht hat (Tokens rein/raus, aus den Transkripten der
  Mitglieder), den Stand der Aufgabe (die Datei des Leads unter `team_state_dir`, Standard
  `~/.claude-mods/team-state/<repo>/<branch>.md`), ihre Nachrichten mit Typ (`[BLOCKER]`, `[EINWAND]`
  rot) und die Anleitung (`team_guide`, Standard
  `~/.claude-mods/agenten-team-anleitung.md`).
- **Öffnen:** `⏎` oder Doppelklick. Läuft die Session in Brain, öffnet sie sich hier; eine beendete
  wird hier fortgesetzt. `⌥⏎` öffnet sie stattdessen in iTerm.
- **Aus iTerm holen:** `I` (dann `⏎`) holt eine Session aus ihrem iTerm-Tab nach Brain, ohne
  dass das Gespräch verloren geht.

**Tastatur im Terminal:**

- **Hinein und heraus:** `⏎`, `⌘2` oder ein Klick ins Terminal gibt ihm die Tastatur. Dann
  gehören alle Tasten Claude, Brains Kürzel ruhen. `⌘1` bringt dich zurück zur Liste.
- **Zeilen bearbeiten wie am Mac:** `⌘⌫` löscht bis zum Zeilenanfang, `⌘←`/`⌘→` springen an
  Anfang und Ende. `⇧⏎` macht eine neue Zeile im Prompt.
- **Umlaute, Tottasten und Diktat** funktionieren, auch über Karabiner.

**Dateien und Bilder:**

- **Hinein:** Dateien ins Terminal ziehen oder einen kopierten Screenshot mit `⌘V` einfügen.
  Claude liest das Bild.
- **Pfade und Links öffnen:** `⌘`-Klick auf einen Pfad oder Link in der Ausgabe. Markdown öffnet
  in YAMV, andere Dateien im Reader daneben (`⌘⏎` öffnet sie in deinem Editor), Links im
  Browser.
- **Kopieren:** Text mit der Maus markieren, Claude Code kopiert ihn in die Zwischenablage.

**Mehr Platz:**

- **Teilen:** `⌘D` im Terminal-Tab, oder Rechtsklick → „Daneben öffnen“. Zwei ganze Sessions
  nebeneinander; ein Klick in die linke macht sie zur aktiven.
- **Übersicht:** `⌘⇧A` zeigt alle Terminals als Live-Vorschau, ein Klick öffnet eine.
- **Shell:** `⌘T` öffnet eine Shell im Ordner der Session, unter ihrem Terminal.

## Antworten und steuern

- **Freigaben:** `Y` erlaubt, `N` lehnt ab.
- **Kurz antworten:** `T`, dann tippen, oder `1`–`9` übernimmt eine Schnellantwort; `⏎` schickt
  ab. Eine Ziffer allein schickt nie etwas (Claudes nummerierte Fragen).
- **Composer:** `⌘E`. Ein großes Feld für längere Prompts, mit Diktat, Vorlagen und
  eingefügten Screenshots. `⌘⏎` sendet.
- **Befehlspalette:** `⌘K`. Alle Aktionen; Slash-Befehle wie `/compact`, `/context`, `/model`
  für die Session. Freier Text geht an die Session oder an alle, die auf dich warten.
- **Benachrichtigungen:** Nur wenn eine Session fertig ist oder dich etwas fragt (eine Frage oder
  eine Freigabe). Wartet eine Session weiter auf eine Entscheidung, erinnert Brain nach
  `remind_after_minutes` (Standard 10, 0 schaltet aus), höchstens dreimal; nie bei „Frage?“
  und nie bei Berichten. Direkt in der Mitteilung kannst du
  antworten (nur am entsperrten Mac), öffnen oder 15 Minuten pausieren.
- **Quick Terminal:** Eine Taste deiner Wahl holt Brain aus jeder App mit dem Terminal der
  wartenden Session nach vorn; nochmal drücken blendet Brain aus. Ist aus, bis du sie in der
  Config setzt (`"quick_terminal": "cmd+shift+b"`). Eine globale Taste gilt überall, auch
  gegenüber macOS: ⌃⌥Space etwa wechselt dort die Eingabequelle.
- **Screenshot-Wächter:** Ein neuer Screenshot erscheint als Leiste über der Fußzeile, mit
  „In <Session>“ oder „In den Composer“.

## Ordnung halten

| Taste | Wirkung |
|---|---|
| `P` | anheften; bei einer beendeten Session: zum Fortsetzen merken |
| `S` | pausieren: 15 Min → 1 Std → bis morgen 9 Uhr → bis sich in der Session etwas tut → wieder da |
| `⌫` `⌫` | gleich „bis sich etwas tut“ (nochmal: wieder einblenden) |
| Rechtsklick → *Stummschalten* | keine Benachrichtigungen mehr für diese Session (auch in ⌘K) |
| `R` | umbenennen, solange die Session auf dich wartet |
| `X` `X` | beenden (bleibt fortsetzbar) |
| `⌘⌫` `⌘⌫` | beendete Session in den Papierkorb |
| `C` | aufräumen: alles, was seit N Tagen ruht, ausblenden oder in den Papierkorb |
| `L` | für später: Aufgaben aus `/later`, `⏎` startet eine als neue Session, `⌫` hakt ab |
| `A` | **weiterführen …**: ein Dialog für Konto (`1`, `2`, … – mit dem Verbrauch jedes Kontos), ob das Original weiterläuft (`⇥`), und Flags. Er sagt, was `⏎` tut: anderes Konto → *umziehen* (z. B. wenn ein Limit voll ist); Original läuft weiter → *forken*; eigenes Konto mit Flags (`--chrome`, `--model opus`) → *neu starten*, wo sie lief; eigenes Konto ohne Flags → aus iTerm *nach Brain holen* |
| `F` / `I` / Rechtsklick → *Neu starten mit …* | derselbe Dialog, vorbelegt zum Forken / Holen / mit Flags |

Tasten, die zweimal gedrückt werden wollen, fragen im Rechtsklick-Menü nicht nach: Der Klick
zählt als Bestätigung.

**Der Gärtner** (⌘K → „Gärtner“, oder „✂ N kaputt“ in der Titelleiste, oder `brain garden`)
findet Sessions, die Brain listet, aber nicht mehr steuern kann, und beendet sie endgültig:

- **läuft doppelt:** ein umgezogenes Gespräch, dessen Original weiterlief;
- **nicht erreichbar:** ihr Terminal ging gleich nach dem Anhängen wieder zu;
- **hängt vermutlich:** „arbeitet“ seit 3 Stunden ohne Lebenszeichen (nur ein Verdacht, darum
  nicht vorausgewählt).

Beenden heißt: `claude stop`, sonst SIGTERM, zuletzt SIGKILL, und nur bei einem Prozess, der noch
Claude Code ist. Das Gespräch bleibt fortsetzbar. Für eine einzelne Session, die sich nicht
beenden lässt: Rechtsklick → **Hart beenden**.

**Das Log** hält fest, was Brain in seiner Statuszeile gesagt hat, was fehlschlug, was das
Terminal einer Session zuletzt zeigte, als es zuging, und was Gärtner und Konten-Abgleich getan
haben: `~/.claude-brain/logs/<Datum>.log`, zwei Wochen lang (⌘K → „Log öffnen“).

## Brain auf dem iPhone (Brain Link)

Mit der App **Brain Companion** (Projekt `brain-companion` neben diesem) siehst du unterwegs,
welche Session dich braucht, liest mit, antwortest und gibst Freigaben. Brain auf dem Mac tippt es
für dich ein, auch in Hintergrund-Sessions, die gerade in keinem Terminal offen sind.

1. ⌘K → **„iPhone koppeln (Brain Link)“**. Brain schaltet Brain Link ein und zeigt einen QR-Code.
2. Kamera des iPhones auf den Code richten, „In Brain öffnen“ tippen.
3. Die App fragt nach und zeigt eine **Kennung** (z. B. `071f 6895 1f9e 01d6`). Sie muss mit der Kennung
   unter dem Code in Brain übereinstimmen – nur dann „Koppeln“.

Brain Link ist aus, bis du koppelst, und startet danach mit Brain. Die Verbindung ist
verschlüsselt; die App prüft Brains Zertifikat am Fingerabdruck aus dem Code und schickt bei jeder
Anfrage den Schlüssel mit – ohne ihn kommt niemand rein. Eine Freigabe vom Handy gilt nur für den
Dialog, den das Handy gezeigt hat: Ist inzwischen ein anderer offen, antwortet Brain nicht.

Die App erreicht Brain, solange iPhone und Mac im selben WLAN sind. Wechseln beide das WLAN, findet
sie Brain nach wenigen Sekunden wieder; eine neue Adresse des Macs übernimmt sie von selbst. Im
Kopplungsdialog: **„Alle Handys abmelden“** (neuer Schlüssel, alte Kopplungen
gelten nicht mehr) und **„Brain Link ausschalten“**.

## Ein neues Claude-Konto einrichten

⌘K → **„Neues Claude-Konto einrichten“** (oder `brain account new <name>`) legt `~/.claude-<name>`
an und übernimmt aus allen Konten Einstellungen, CLAUDE.md, Skills, Agents, Commands, Plugins,
MCP-Server und das Gedächtnis pro Projekt. Die Vorschau zeigt vorher, was woher kommt und wo sich
die Konten unterscheiden: Dann gewinnt main, bei Gedächtnis-Notizen die neuere. Die Anmeldung und
die Gespräche kommen nicht mit – melde dich in der ersten Session des neuen Kontos mit `/login` an;
Brain öffnet dafür gleich „Neue Session“ mit dem neuen Konto. Plugins werden auf APFS geklont und
belegen kaum Platz.

## Konten abgleichen

⌘K → **„Konten abgleichen“** (oder `brain sync`, mit `--yes` schreibt es) hält Einstellungen,
CLAUDE.md, Skills, Agents, Commands, MCP-Server und das Gedächtnis pro Projekt in allen Konten
gleich. Brain merkt sich nach jedem Abgleich den gemeinsamen Stand; was ein Konto seitdem ändert
oder löscht, bekommen die anderen. Haben zwei Konten dasselbe verschieden geändert, entscheidest du
im Dialog: vorgeschlagen ist die neueste Änderung, `1`–`9` wählt eine andere, `K` lässt Konten mit
anderer Version ihre behalten. Gedächtnis-Notizen warten nie: die neuere gewinnt.

Der erste Abgleich führt die Konten zusammen. Danach gleicht **jeder Session-Start** mit ab, lässt
aber Konflikte und alles, was Rechte gibt oder Code ausführen kann (Freigaben, Hooks, `env`,
Plugins, MCP-Server, das Statusline-Skript, Skills, Commands, Agents; im Dialog mit ⚑), für den
Dialog liegen – die Titelleiste zählt sie („⇄ 2 Konto-Konflikte“). Einfache Einstellungen,
CLAUDE.md und das Gedächtnis gleicht er selbst ab. Was ersetzt oder gelöscht wird, liegt 30 Tage in
`~/.claude-brain/sync/backup/`.

## Nutzung und Limits

Unter der Liste steht pro Konto, wie viel vom 5-Stunden- und vom Wochenlimit verbraucht ist und
wann es zurückgesetzt wird. Reicht das Tempo nicht bis zum Reset, steht dort **„voll ~Do 14:00“**,
und hat ein anderes Konto Luft, nennt Brain es („→ second hat Luft“). Beim Wochenlimit zählt das
Tempo der letzten 24 Stunden (Brain merkt sich dafür alle zehn Minuten den Stand); beim
5-Stunden-Limit das des laufenden Fensters.

## Einstellungen

Alles optional, in `~/.claude-brain/config.json`:

| Schlüssel | Bedeutung |
|---|---|
| `language` | `"de"` oder `"en"` |
| `chat_font`, `chat_font_size` | Schrift des Gesprächs, sonst die von iTerm |
| `quick_replies` | eigene Schnellantworten für `T` `1`–`9` |
| `quick_terminal` | Tastenkürzel des Quick Terminals, z. B. `"cmd+shift+b"`; ohne Eintrag aus |
| `screenshot_folder` | Ordner, den der Screenshot-Wächter beobachtet; `"off"` schaltet ihn ab |

**Vorlagen** liegen in `~/.claude-brain/templates/*.md`, mit Platzhaltern wie `{branch}`. Im Dialog
„Neue Session“ öffnet `⌘E` die gewählte Vorlage im Editor, `⌘⇧N` legt eine neue an.

## Alle Tastenkürzel

`?` zeigt sie auch in Brain.

| Taste | Wirkung |
|---|---|
| `↑` `↓`, `j` `k` | Session wählen |
| `⏎`, Doppelklick | öffnen (in Brain, sonst im Terminal-Programm; eine beendete: fortsetzen) |
| `⌥⏎` | in iTerm öffnen |
| `⌘2` `⌘J` / `⌘1` `⌘[` | ins Terminal / zurück zur Liste |
| `/` | Liste durchsuchen |
| `⇥` / `⇧⇥` | Kontofilter |
| `E` `B` `U` | beendete / Hintergrund- / automatische Sessions auflisten |
| `C` | aufräumen |
| `⌘N` | neue Session (darin `⌘O`: Ordner im macOS-Dialog wählen) |
| `T` | antworten (`1`–`9` übernimmt eine Schnellantwort, `⏎` schickt ab) |
| `Y` / `N` | Freigabe erlauben / ablehnen |
| `R` | umbenennen (solange die Session wartet) |
| `P` / `S` | anheften / pausieren |
| `⌫` `⌫` | ausblenden, bis sich etwas tut |
| `A` | weiterführen: Konto, Fork, Flags |
| `F` / `I` | forken / aus iTerm nach Brain holen (derselbe Dialog) |
| `X` `X`, `⌘⌫` `⌘⌫` | beenden, in den Papierkorb |
| `⌘F` | in der ganzen Session suchen (Ansichten: Prompts, Fragen, Ereignisse) |
| `⌘⇧C` | letzte Antwort kopieren |
| `⌘E` | Composer |
| `⌘T` | Shell im Projektordner |
| `⌘D` | Terminal teilen (im Terminal-Tab) |
| `⌘⇧A` | alle Terminals im Überblick |
| `⌘`-Klick | Pfad oder Link aus der Ausgabe öffnen |
| `⌘V` | Screenshot ins Terminal einfügen |
| `←` `→` | Tabs wechseln |
| `⌘K` | Befehlspalette: alles, auch was keine Taste hat (stumm, Abschnitte aufklappen, Konten abgleichen …) |
| `?` | alle Tasten |
| `esc` | Dialog oder Suchleiste schließen, Suche leeren |
| deine `quick_terminal`-Taste | Quick Terminal, aus jeder App |
