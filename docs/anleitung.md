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

Oben in der Titelleiste:

- **Ansichten:** Status, Projekte (`G`) und Heute (`D`, was jede Session heute erledigt hat;
  `⌘C` kopiert das als Markdown).
- **Kontofilter:** umschalten mit `⇥`.
- **„◌ N im Hintergrund“ (`B`)** blendet Claudes Hintergrund-Sessions ein.
- **„⚙ N automatisch“ (`U`)** blendet Sessions ein, die Programme gestartet haben, etwa
  Security-Reviews nach Commits oder `claude -p`-Skripte. Standardmäßig ausgeblendet.

Weitere Gruppen: Angeheftete (★), Ruhende (`H` klappt sie auf), Beendete (`E`).

- **Suchen:** `/` durchsucht die Liste nach Name, Ordner, Konto und Überschrift.
- **Rechtsklick** auf eine Session zeigt alle passenden Aktionen mit ihren Kürzeln.

## Eine Session ansehen

Rechts steht die ausgewählte Session.

- **Kopfzeile:** Name (Klick zum Umbenennen), Konto, Markierungen wie ★ angeheftet,
  „● klingelt“ (Bell) oder der Titel, den Claude Code gesetzt hat.
- **Infos:** Ordner, Modell, Kosten, PR mit Checks und die **Context-Anzeige**: ein Balken, ab
  80 % gelb mit Knopf **/compact**, ab 90 % rot.

Die Tabs (`←` `→`):

| Tab | Inhalt |
|---|---|
| **Nachrichten** | Die letzten Nachrichten im Terminal-Look. `⌘F` sucht in der ganzen Session, „Deine Prompts“ listet alles, was du gefragt hast. „Antwort kopieren“ (`⌘⇧C`), „Als Markdown“ öffnet das Gespräch in YAMV. |
| **Verlauf** | Jedes Ereignis der Session: Start, Prompts, Freigaben, Turn-Enden, Meldungen. |
| **Änderungen** | Branch und geänderte Dateien. Klick auf eine Datei zeigt den Diff. „Änderung verwerfen“ (zweimal) legt die jetzige Fassung in den Papierkorb und holt die aus HEAD zurück. |
| **Dateien** | Jede Datei, die die Session geschrieben, geändert, gelesen oder bekommen hat, neueste zuerst. So findest du z. B. erzeugte Markdown-Dateien. Klick öffnet sie. |
| **Fragen** | Jede Rückfrage von Claude mit deiner Antwort; offene sind markiert. |
| **Terminal** | Die Session selbst, in Brains Terminal (siehe unten). Bei einer beendeten Session steht hier vorher ein **Steckbrief**: Anfang, letzte Frage, letzte Antwort, offene Rückfrage, geänderte Dateien. |

## Sessions in Brain laufen lassen

- **Neue Session:** `⌘N`. Ordner, Konto, Modell und erster Prompt, auf Wunsch aus einer
  Vorlage. Sie läuft als Claude-Code-Hintergrund-Session in Brains Terminal weiter, auch wenn
  Brain beendet wird.
- **Öffnen:** `⏎` oder Doppelklick. Läuft die Session in Brain, öffnet sie sich hier; eine beendete
  wird hier fortgesetzt. `⌥⏎` öffnet sie stattdessen in iTerm.
- **Aus iTerm holen:** `I` `I` holt eine Session aus ihrem iTerm-Tab nach Brain, ohne dass das
  Gespräch verloren geht.

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

- **Teilen:** `⌘D`, oder Rechtsklick → „Daneben öffnen“. Zwei ganze Sessions nebeneinander; ein
  Klick in die linke macht sie zur aktiven.
- **Übersicht:** `⌘⇧A` zeigt alle Terminals als Live-Vorschau, ein Klick öffnet eine.
- **Shell:** `⌘T` öffnet eine Shell im Ordner der Session, unter ihrem Terminal.

## Antworten und steuern

- **Freigaben:** `Y` erlaubt, `N` lehnt ab.
- **Kurz antworten:** `T`, dann tippen oder `1`–`9` für eine Schnellantwort.
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
| `M` | keine Benachrichtigungen mehr für diese Session |
| `S` | pausieren: 15 Min → 1 Std → bis morgen 9 Uhr → aus |
| `R` | umbenennen |
| `X` `X` | beenden (bleibt fortsetzbar) |
| `⌫` `⌫` | ausblenden, bis sich in der Session wieder etwas tut |
| `⌘⌫` `⌘⌫` | beendete Session in den Papierkorb |
| `C` | aufräumen: alles, was seit N Tagen ruht, ausblenden oder in den Papierkorb |
| `A` `A` | im anderen Konto fortsetzen, z. B. wenn ein Limit voll ist |

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

## Nutzung und Limits

Unter der Liste steht pro Konto, wie viel vom 5-Stunden- und vom Wochenlimit verbraucht ist und
wann es zurückgesetzt wird. Reicht das Tempo nicht bis zum Reset, steht dort **„voll ~Do 14:00“**,
und hat ein anderes Konto Luft, nennt Brain es („→ second hat Luft“).

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

| Taste | Wirkung |
|---|---|
| `↑` `↓`, `j` `k` | Session wählen |
| `1`–`9` | n-te Session öffnen |
| `⏎`, Doppelklick | öffnen (in Brain, sonst im Terminal-Programm) |
| `⌥⏎` | in iTerm öffnen |
| `⌘2` / `⌘1` | ins Terminal / zurück zur Liste |
| `⌘N` | neue Session |
| `⌘K` | Befehlspalette |
| `⌘E` | Composer |
| `⌘F` | in der ganzen Session suchen |
| `/` | Liste durchsuchen |
| `⌘⇧C` | letzte Antwort kopieren |
| `⌘D` | Terminal teilen |
| `⌘⇧A` | alle Terminals im Überblick |
| `⌘T` | Shell im Projektordner |
| deine `quick_terminal`-Taste | Quick Terminal, aus jeder App |
| `⌘`-Klick | Pfad oder Link aus der Ausgabe öffnen |
| `⌘V` | Screenshot ins Terminal einfügen |
| `I` `I` | Session aus iTerm nach Brain holen |
| `Y` / `N` | Freigabe erlauben / ablehnen |
| `T` | antworten |
| `P` `M` `S` `R` | anheften, stumm, pausieren, umbenennen |
| `X` `X`, `⌫` `⌫`, `⌘⌫` `⌘⌫` | beenden, ausblenden, in den Papierkorb |
| `A` `A` | ins andere Konto |
| `C` | aufräumen |
| `G` / `D` | nach Projekten / Heute |
| `B` / `U` / `E` / `H` | Hintergrund / automatische / beendete / ruhende Sessions |
| `⇥` | Kontofilter |
| `←` `→` | Tabs wechseln |
| `esc` | Dialog oder Ansicht schließen, Suche leeren |
