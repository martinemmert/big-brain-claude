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

## Der Baum

Links steht der **Baum**: die Projekte als feste Überschriften, darunter ihre Sessions. Die
Projekte stehen alphabetisch und bleiben dort, auch wenn eine Session zu fragen beginnt: nichts
springt unter der Maus. Welche Projekte im Baum stehen, bestimmst du:

- **+ Neues Projekt …** (ganz unten) fragt nach einem Ordner (ein Worktree zählt als sein Haupt-Ordner).
- Startest du in Brain eine Session in einem Ordner (`⌘N`, das **+** am Projekt, eine Aufgabe „für
  später“), kommt sein Projekt dazu. So auch **In ein anderes Projekt …** mit einem neuen Namen.
- Beim ersten Start nach dem Update übernimmt Brain einmal die Projekte der Sessions, die gerade in
  Brain laufen. Sonst nichts.

Ein Projekt bleibt, bis du es ausblendest (Rechtsklick auf den Projektkopf, oder `⌘K`); unten holt
„N Projekte ausgeblendet“ es zum Nachsehen zurück. Ein Klick auf den Kopf klappt es zu oder auf; ein
zugeklapptes zeigt rote und gelbe Zähler. „ruht“ steht an einem Projekt ohne lebende Session. Daneben
steht das Konto.

Unter dem Projekt stehen die Sessions in der Reihenfolge, in der sie gestartet wurden, jede mit
einem Zustandspunkt (rot fragt, gelb fertig und ungesehen, blau arbeitet, hellblau Subagents oder
Hintergrund-Shells laufen noch, Ring: angesehen oder ruht), dem Namen und der Zeit seit dem
Zustandswechsel. Eine zweite Zeile gibt es nur, wenn die Session dich braucht oder fertig ist:

- **Rot, mit Ring:** Claude braucht eine Freigabe („Freigabe: Bash(npm run e2e)“, mit **Erlauben** und
  **Ablehnen** gleich darunter), stellt eine Frage oder hat mit `brain report --waiting` gefragt. Rot
  bleibt, bis du geantwortet hast. Optionen einer Frage beantwortest du im Terminal.
- **Gelb:** fertig und noch nicht angesehen, mit dem letzten Satz der Antwort. Es verschwindet, sobald
  du die Session in Brains vorderem Fenster auswählst. Auch **Frage?** (der Turn endete ohne Meldung,
  der letzte Satz endet mit „?“) ist gelb und geht beim Ansehen; **Nur Bericht, keine Frage** im
  Rechtsklick-Menü nimmt es sofort.
- Alle anderen Sessions sind einzeilig; Details (Überschrift, Ordner, PR, Worktree …) erscheinen beim
  Überfahren.

Beendete Sessions des letzten Tages stehen gedimmt unter ihrem Projekt, ältere findest du über die
Suche oder „▸ N älter“. Angeheftete (★) bleiben stehen. **Für später · N** ist eine zuklappbare Zeile
mit den Aufgaben aus `/later` dieses Projekts: ein Klick startet eine als neue Session dort, das ✓ hakt sie ab.

**Weitere** am Ende des Baums sammelt Sessions, deren Ordner kein Projekt im Baum ist (iTerm,
`claude --bg` aus einer Shell, von Programmen gestartete). Rechtsklick → **Als Projekt behalten**
nimmt ihr Projekt in den Baum.

Alles, was gerade nicht nach dir ruft, liegt am Ende in einer Zeile mit Zahlen; ein Klick klappt auf:
**pausiert** (Menü *Session → Pausieren*, `⌫`), **im Hintergrund** (Menü *Darstellung*, ohne Aktivität seit einem Tag), **automatisch** (Menü *Darstellung*:
Programme wie Security-Reviews nach Commits oder `claude -p`-Skripte) und **älter**. Diese Sessions
melden sich nie per Mitteilung. So verschwindet keine Session spurlos.

Auf dem Handy ist die Liste weiter nach Zustand und Projekt gruppiert, mit einem Filter nach Projekt oder Konto.

Eine Session gehört woanders hin? Rechtsklick → **In ein anderes Projekt …** (oder ⌘K, „Projekt“ tippen):
ein vorhandenes Projekt, ein neues per Namen, oder zurück in ihr eigenes. Brain merkt sich das, auch
fürs Handy (⋯ → **In anderes Projekt …**, oder lange drücken in der Liste).

Oben in der Titelleiste steht der Kontofilter (umschalten mit `⇥`).

- **Suchen:** `⌥⌘F` filtert den Baum nach Name, Ordner, Konto und Überschrift; `⌘O` sucht auch unter den beendeten.
- **Klick auf eine Session** gibt die Tastatur an ihr Terminal.
- **Rechtsklick** auf eine Session zeigt alle passenden Aktionen mit ihren Kürzeln.
- **Menüleiste:** alle Befehle stehen in den Menüs oben (*Brain, Ablage, Bearbeiten, Session, Darstellung,
  Fenster, Hilfe*), mit ihren Kürzeln. Einbuchstaben-Tasten gibt es in der Liste nicht mehr; was gerade nicht passt, ist
  ausgegraut. Im Menü *Fenster* stehen die Sessions, die dich brauchen, als `⌘1` bis `⌘9`.
- **Statuskarte:** der Knopf in der Titelleiste rechts neben dem Konto-Filter (er zeigt, wie viele dich brauchen, und den
  Verbrauch des vollsten Kontos) öffnet eine Karte oben rechts im Fenster: wer dich braucht (bei einer Freigabe mit
  *Erlauben*, *Ablehnen* und *Ansehen*; *Ansehen* wählt die Session), wer fertig ist und noch nicht angesehen wurde, und je
  Konto das 5-h- und Wochenlimit mit Prognose und Empfehlung, wo eine neue Session besser läuft. Ein Klick aufs Zeichen in
  der Menüleiste holt Brain nach vorn und öffnet die Karte. `esc`, ein Klick daneben oder ein zweiter Klick auf den Knopf
  schließt sie. Neue Session und Beenden stehen in der Menüleiste.
- **Größe:** `⌘+`, `⌘-` und `⌘0` skalieren das ganze Fenster samt Terminalschrift; die Statuszeile zeigt die Stufe,
  wenn sie nicht 100 % ist.
- **Tasten:** `⌘?` zeigt alle.

Über dem Terminal steht eine schmale Zeile: Projekt › Session (ein Klick benennt um), Branch, Konto,
wie voll der Kontext ist (ab 80 % mit **/compact**) und ein ⚠, wenn die Session dieselben Dateien
bearbeitet wie eine andere. Modell, Kosten, pid und Ordner erscheinen beim Überfahren.

Unten steht eine Zeile mit „N braucht dich“, „N fertig“, „N arbeiten“, der Meldung des letzten
Befehls und rechts der Nutzung pro Konto (siehe unten).

## Eine Session ansehen

Rechts steht die ausgewählte Session.

- **Kopfzeile:** Name (Klick zum Umbenennen), Konto, Markierungen wie ★ angeheftet,
  „● klingelt“ (Bell) oder der Titel, den Claude Code gesetzt hat.
- **Darunter eine Zeile:** Ordner, Modell, die **Context-Anzeige** (ab 80 % gelb mit Knopf
  **/compact**, ab 90 % rot), ein abweichender Freigabe-Modus und der PR mit Checks. Kosten,
  pid und Startzeit zeigt sie beim Überfahren.
- **Das Terminal:** darunter läuft die Session selbst, in Brains Terminal (siehe unten). Bei einer
  beendeten Session steht dort vorher ein **Steckbrief**: Anfang, letzte Frage, letzte Antwort,
  offene Rückfrage, geänderte Dateien. Läuft die Session in einem iTerm-Tab, steht dort ein Hinweis;
  „Nach Brain holen“ (Rechtsklick oder `⌥⌘O`) holt sie herüber.
- **Suchen in der Session:** `⌘F` öffnet über dem Terminal eine Suchleiste für das ganze Gespräch;
  mit Text darin ersetzen die Treffer das Terminal, ein Klick öffnet einen ganz. `esc` schließt sie.
  `⌘⇧C` kopiert die letzte Antwort, ⌘K → „Gespräch als Markdown öffnen“ öffnet es in YAMV.
- **Freigaben** beantwortest du im Baum (**Erlauben**/**Ablehnen**, `⌘Y`/`⇧⌘Y`),
  Fragen mit Optionen mit einem Klick.

## Sessions in Brain laufen lassen

- **Neue Session:** `⌘N`. Ordner, Konto, Modell und erster Prompt, auf Wunsch aus einer
  Vorlage. Zuletzt benutzte Ordner stehen oben; tippst du einen Pfad (`~/`), listet Brain die
  Ordner darin, `→` geht in den gewählten hinein. `⇧⇥` wählt das Antwort-Profil (Output-Style),
  sonst gilt, was in den Einstellungen des Kontos steht. Sie läuft als Claude-Code-Hintergrund-Session
  in Brains Terminal weiter, auch wenn Brain beendet wird.
- **Antwort-Profil wechseln:** `⌘K` → „Antwort-Profil von …“ tippt `/output-style <Name>` in die
  Session; es gilt ab der nächsten Nachricht.
- **Vom Handy in einem neuen Ordner starten:** Trägt das Konto den Ordner noch nicht als vertraut ein,
  stellt das Handy Claudes Frage „Vertraust du diesem Ordner?“; erst nach „Vertrauen und starten“
  trägt Brain ihn ein und startet (sonst startet Claude im Hintergrund gar nicht).
- **Öffnen:** `⏎` oder Doppelklick. Läuft die Session in Brain, öffnet sie sich hier; eine beendete
  wird hier fortgesetzt. `⌥⏎` öffnet sie stattdessen in iTerm.
- **Aus iTerm holen:** `⌥⌘O` holt eine Session aus ihrem iTerm-Tab nach Brain, ohne dass das
  Gespräch verloren geht. Das ist kein Kontowechsel und hat deshalb keinen Dialog.

**Tastatur im Terminal:**

- **Hinein und heraus:** `⏎`, `⌘J` oder ein Klick ins Terminal gibt ihm die Tastatur. Dann
  gehören die Tasten Claude, die das Terminal braucht (`⌘⌫`, `⌘←`, `⌘→`, `⌘C`, `⌘V`); Brains Menükürzel liegen auf `⌘` und `⌥⌘`
  und stehen ihm nicht im Weg. `⌘[` bringt dich zurück in den Baum.
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

- **Teilen:** `⌘D`, oder Rechtsklick → „Daneben öffnen“. Zwei ganze Sessions
  nebeneinander; ein Klick in die linke macht sie zur aktiven.
- **Übersicht:** `⌘⇧A` zeigt alle Terminals als Live-Vorschau, ein Klick öffnet eine.
- **Shell:** `⌘T` öffnet eine Shell im Ordner der Session, unter ihrem Terminal.

## Antworten und steuern

- **Freigaben:** `⌘Y` erlaubt, `⇧⌘Y` lehnt ab.
- **Composer:** `⌘E`. Ein großes Feld für längere Prompts, mit Diktat, Vorlagen und
  eingefügten Screenshots. `⌘⏎` sendet.
- **Befehlspalette:** `⌘K`. Alle Aktionen; Slash-Befehle wie `/compact`, `/context`, `/model`
  für die Session. Freier Text geht an die Session oder an alle, die auf dich warten.
- **Benachrichtigungen:** Nur wenn eine Session dich direkt fragt (eine Frage, eine Freigabe oder
  ein `brain report --waiting`). Eine fertige Runde („du bist dran“)
  steht nur in der Liste. Auf dem iPhone gilt dasselbe. Wartet eine Session weiter auf eine Entscheidung, erinnert Brain nach
  `remind_after_minutes` (Standard 10, 0 schaltet aus), höchstens dreimal; nie bei „Frage?“
  und nie bei Berichten. Direkt in der Mitteilung kannst du
  antworten (nur am entsperrten Mac), öffnen oder 15 Minuten pausieren.
- **Quick Terminal:** Eine Taste deiner Wahl holt Brain aus jeder App mit dem Terminal der
  wartenden Session nach vorn; nochmal drücken blendet Brain aus. Ist aus, bis du sie in der
  Config setzt (`"quick_terminal": "cmd+shift+b"`). Eine globale Taste gilt überall, auch
  gegenüber macOS: ⌃⌥Space etwa wechselt dort die Eingabequelle.
## Ordnung halten

| Taste | Wirkung |
|---|---|
| `⌥⌘P` | anheften; bei einer beendeten Session: zum Fortsetzen merken |
| Menü *Pausieren* | 15 Min, 1 Std, bis morgen 9 Uhr, bis sich in der Session etwas tut, Pause aufheben |
| `⌫` `⌫` | gleich „bis sich etwas tut“ (nochmal: wieder einblenden) |
| Rechtsklick → *Stummschalten* | keine Benachrichtigungen mehr für diese Session (auch in ⌘K) |
| `⌘R` | umbenennen (auch während die Session arbeitet; `/rename` geht hinein, sobald sie frei ist) |
| `⌥⌘X` `⌥⌘X` | beenden (bleibt fortsetzbar) |
| `⌥⌘⌫` `⌥⌘⌫` | beendete Session in den Papierkorb |
| Menü *Ablage → Aufräumen …* | alles, was seit N Tagen ruht, ausblenden oder in den Papierkorb |
| `⌘L` | für später: Aufgaben aus `/later`, `⏎` startet eine als neue Session, `⌫` hakt ab |
| Menü *In anderes Konto*, *Forken in* | **in ein anderes Konto oder forken …**: ein Dialog für Konto (`1`, `2`, … – mit dem Verbrauch jedes Kontos), ob das Original weiterläuft (`⇥`), und Flags. Er sagt, was `⏎` tut: anderes Konto → *umziehen* (z. B. wenn ein Limit voll ist); Original läuft weiter → *forken*. Neu starten mit Flags (`--chrome`, `--model opus`) geht über Menü *Session* oder ⌘K → „Neu starten mit …“, nach Brain holen mit `⌥⌘O` |
| Menü *Session → Neu starten mit …* | derselbe Dialog, vorbelegt mit Flags |

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
sie Brain nach wenigen Sekunden wieder; eine neue Adresse des Macs übernimmt sie von selbst.

**Unterwegs** erreicht die App Brain über [Tailscale](https://tailscale.com): die App auf Mac und
iPhone installieren und mit demselben Konto anmelden (auf dem iPhone mit „Use Tailscale DNS“, der
Voreinstellung). Brain nennt dem Handy seinen Tailscale-Namen (`….ts.net`); das Handy lernt ihn,
sobald es Brain einmal erreicht, und behält ihn.

Tailscale läuft am Mac nur, solange du unterwegs bist:

- **Losgehen:** die Kurzbefehl-Aktion **„Fernzugriff einschalten“** der Brain-App (noch im WLAN).
  Brain öffnet Tailscale am Mac und verbindet. Im selben Kurzbefehl danach Tailscales „Connect“, das
  WLAN aus und die Brain-App öffnen.
- **Zurück:** Meldet sich das Handy wieder über das WLAN, schaltet Brain Tailscale ab (frühestens
  3 Minuten nach dem Einschalten, damit der Kurzbefehl in Ruhe das WLAN abschalten kann). Von Hand:
  **„Fernzugriff ausschalten“**. Am iPhone schaltet eine Automation „Beim Verbinden mit <WLAN>“ →
  Tailscale „Disconnect“ Tailscale dort ab.
- **Aus** heißt: getrennt und die Tailscale-App beendet, sie hat dann keine Verbindung nach draußen.
  Beendet sich Brain, schaltet es Tailscale genauso ab. Was Brain dabei tut, steht im Log (`remote`).

Der Mac muss dafür wach sein (am Netzteil schläft er nicht, zugeklappt ohne Monitor schon). Im
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

In der Zeile unten steht pro Konto, wie viel vom 5-Stunden- und vom Wochenlimit verbraucht ist und
wann es zurückgesetzt wird. Reicht das Tempo nicht bis zum Reset, steht dort **„voll ~Do 14:00“**,
und hat ein Konto noch Platz, steht dort „hat Platz“. Beim Wochenlimit zählt das
Tempo der letzten 24 Stunden (Brain merkt sich dafür alle zehn Minuten den Stand); beim
5-Stunden-Limit das des laufenden Fensters.

## Einstellungen

Alles optional, in `~/.claude-brain/config.json`:

| Schlüssel | Bedeutung |
|---|---|
| `language` | `"de"` oder `"en"` |
| `chat_font`, `chat_font_size` | Schrift des Gesprächs, sonst die von iTerm |
| `quick_terminal` | Tastenkürzel des Quick Terminals, z. B. `"cmd+shift+b"`; ohne Eintrag aus |

**Vorlagen** liegen in `~/.claude-brain/templates/*.md`, mit Platzhaltern wie `{branch}`. Im Dialog
„Neue Session“ öffnet `⌘E` die gewählte Vorlage im Editor, `⌘⇧N` legt eine neue an.

## Alle Tastenkürzel

`⌘?` zeigt sie auch in Brain; die Menüleiste zeigt jedes Kürzel neben seinem Befehl. Alle liegen auf `⌘` oder
`⌥⌘`. Hat das Terminal die Tastatur, gehören ihm `⌘⌫`, `⌘←`, `⌘→`, `⌘C` und `⌘V`.

| Taste | Wirkung |
|---|---|
| `↑` `↓` | Session wählen (solange der Baum die Tastatur hat) |
| `⏎`, Doppelklick | öffnen (in Brain, sonst im Terminal-Programm; eine beendete: fortsetzen) |
| `⌥⏎` | in iTerm öffnen |
| `⌘J` / `⌘[` | ins Terminal / zurück in den Baum |
| `⌃⇥` `⌃⇧⇥` | nächste / vorherige Session |
| `⇧⌘U` | nächste Session, die dich braucht |
| `⌘1` … `⌘9` | die Sessions, die dich brauchen (Menü *Fenster*) |
| `⌥⌘F` | Liste durchsuchen |
| `⌘O` | Session fortsetzen: suchen, auch unter den beendeten |
| `⇥` / `⇧⇥` | Kontofilter |
| `⌘N` / `⌥⌘N` / `⇧⌘N` | neue Session / im Projekt / neues Projekt (im `⌘N`-Dialog: `⌘O` Ordner wählen) |
| `⌘L` / `⇧⌘L` | für später / Später-Notizen des Projekts |
| `⌘Y` / `⇧⌘Y` | Freigabe erlauben / ablehnen |
| `⌘R` | umbenennen |
| `⌥⌘P` / `⌥⌘M` | anheften / stummschalten |
| `⌫` `⌫` | ausblenden, bis sich etwas tut |
| `⌥⌘O` | aus iTerm nach Brain holen (sofort, ohne Dialog) |
| `⌥⌘X` `⌥⌘X`, `⌥⌘⌫` `⌥⌘⌫` | beenden, in den Papierkorb |
| `⌘F` | in der ganzen Session suchen |
| `⌘⇧C` | letzte Antwort kopieren |
| `⌘C` | im Terminal: was Claude Code nach deiner Maus-Auswahl kopiert hat, sonst Brains Auswahl |
| `⌘V` / `⌃⌘V` | ins Terminal einsetzen (ein Screenshot geht als Pfad hinein) / nur den Screenshot einsetzen |
| `⌘E` | Composer |
| `⌘T` | Shell im Projektordner |
| `⌘D` | Terminal teilen |
| `⌘⇧A` | alle Terminals im Überblick |
| `⌃⌘S` / `⌥⌘0` | Seitenleiste / Änderungen unter der Session ein- und ausblenden |
| `⌘+` `⌘-` `⌘0` | Fenster größer / kleiner / Originalgröße (die Stufe bleibt gemerkt) |
| `⌘`-Klick | Pfad oder Link aus der Ausgabe öffnen |
| `⌘K` | Befehlspalette: alles, auch was keine Taste hat (Abschnitte aufklappen, Konten abgleichen …) |
| `⌘,` | Einstellungen: `~/.claude-brain/config.json` im Texteditor |
| `⌘?` | alle Tasten |
| `esc` | Dialog oder Suchleiste schließen, Suche leeren |
| deine `quick_terminal`-Taste | Quick Terminal, aus jeder App |
