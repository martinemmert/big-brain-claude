# Brain – kleine Anleitung

Brain behält alle deine Claude-Code-Sessions im Blick, über alle Konten (`~/.claude`,
`~/.claude-second`, …). Es zeigt, wer auf dich wartet, lässt Sessions in seinem eigenen
Terminal laufen und hilft, die Liste kurz zu halten.

## Einrichten

- **Installieren:** `./scripts/install.sh` baut Brain, legt `Brain.app` nach `~/Applications`, den
  `brain`-Befehl nach `~/.cargo/bin` und richtet jedes Konto ein. Danach gilt alles für jede neue
  Claude-Session.
- **Wie Sessions melden:** über den **Brain-Mod** (läuft in Claude Code selbst) und, als
  Rückfallebene, über Hooks. Laufende Sessions laden den Mod mit `/reload-plugins`;
  `/plugin` zeigt „1 mod active · brain“.
- **Berechtigungen:** Brain ist mit deiner Apple-Development-Identität signiert. macOS fragt jede
  Berechtigung einmal und merkt sich die Antwort auch über Updates.

## Die Liste

Links stehen die Sessions, wer auf dich wartet zuerst, am längsten Wartende oben:

- **Rot:** Claude fragt etwas oder braucht eine Freigabe.
- **Gelb:** Der Turn ist fertig, du bist dran.
- **Blau:** Claude arbeitet; **hellblau:** fertig, aber Subagents oder Hintergrund-Shells laufen noch.

Oben stehen Angeheftete (★), darunter „Braucht dich“ und „Arbeitet“. Alles, was gerade nicht
nach dir ruft, liegt in **zugeklappten Abschnitten am Listenende**, jeweils mit Zahl; ein Klick
(oder ⌘K) klappt einen auf:

- **Ruht seit über 2 h**
- **Pausiert und ausgeblendet** (`S`, `⌫` `⌫`)
- **Ruhig im Hintergrund:** Claudes Hintergrund-Sessions ohne Aktivität seit einem Tag
- **Automatisch gestartet:** Security-Reviews nach Commits, `claude -p`-Skripte und Ähnliches
- **Beendet:** die des letzten Tages; ältere findet die Suche

Die aus den drei mittleren melden sich nie per Mitteilung. So verschwindet keine Session spurlos.

- **Suchen:** `/` durchsucht die Liste nach Name, Ordner, Konto und Überschrift.
- **Nach Projekt gruppieren:** `G` oder „Projekte“ neben dem Suchfeld.
- **Heute:** `D` (oder ⌘K) zeigt, was jede Session heute erledigt hat; `⌘C` kopiert das als
  Markdown, `esc` schließt.
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
  Vorlage. Sie läuft als Claude-Code-Hintergrund-Session in Brains Terminal weiter, auch wenn
  Brain beendet wird.
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
- **Benachrichtigungen:** Wartet eine Session, meldet macOS das. Direkt in der Mitteilung kannst du
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
| `A` | **weiterführen …**: ein Dialog für Konto (`1`, `2`, … – mit dem Verbrauch jedes Kontos), ob das Original weiterläuft (`⇥`), und Flags. Er sagt, was `⏎` tut: anderes Konto → *umziehen* (z. B. wenn ein Limit voll ist); Original läuft weiter → *forken*; eigenes Konto mit Flags (`--chrome`, `--model opus`) → *neu starten*, wo sie lief; eigenes Konto ohne Flags → aus iTerm *nach Brain holen* |
| `F` / `I` / Rechtsklick → *Neu starten mit …* | derselbe Dialog, vorbelegt zum Forken / Holen / mit Flags |

Tasten, die zweimal gedrückt werden wollen, fragen im Rechtsklick-Menü nicht nach: Der Klick
zählt als Bestätigung.

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
| `remind_after_minutes` | nach wie vielen Minuten Warten Brain erneut erinnert (`0` = nie) |
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
| `1`–`9` | n-te Session öffnen |
| `⏎`, Doppelklick | öffnen (in Brain, sonst im Terminal-Programm; eine beendete: fortsetzen) |
| `⌥⏎` | in iTerm öffnen |
| `⌘2` `⌘J` / `⌘1` `⌘[` | ins Terminal / zurück zur Liste |
| `/` | Liste durchsuchen |
| `⇥` / `⇧⇥` | Kontofilter |
| `G` / `D` | nach Projekten / Heute |
| `C` | aufräumen |
| `⌘N` | neue Session |
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
| `esc` | Dialog, Suchleiste oder Heute schließen, Suche leeren |
| deine `quick_terminal`-Taste | Quick Terminal, aus jeder App |
