# The Konsil tab

Date: 2026-10-10.

**What.** The konsil skill (`phoenix-framework:konsil`) seats the lazy senior and his pragmatic
buddy at a table as two subagents; the session narrates. Its output was one long reply. Brain now
shows each council in a **Konsil** tab, as a stage: the two figures facing each other (🦥 the lazy
senior in amber, 🚀 the buddy in blue) with their latest first sentence, the topic and the latest
scene line between them; below, what they agree on and what they still dispute; then the rounds,
the newest unfolded, with each statement in full (Markdown) and the user's words in a green card;
at the end "Was tust du?" or the decision.

**Data.** The skill marks the council with three calls; everything else comes from the transcript.

- `brain konsil start "<topic>"`, `brain konsil stand --einig "a; b" --strittig "c (faul: ja,
  kumpel: nein)"`, `brain konsil end "<decision>"`. `start` and `end` also report (`--doing`,
  `--done`), so the timeline and Today show the council; `stand` only marks.
- Brain finds these Bash calls in the transcript (`brain_core::konsil`), not in the event log: the
  timeline keeps only recent events, the transcript keeps the whole session.
- Who speaks: the Agent call's description (`Konsil: lazy senior …` / `Konsil: pragmatic buddy …`)
  or the persona file in its prompt; the agent id from `toolUseResult.agentId`. Statements come
  from hand-backs (`origin.handback`, else `<agent-message>`), foreground tool results, or task
  notifications that carry the report.
- Rounds: each figure speaks once per round, so a second word opens the next one; the user's
  words open the next round once the table has spoken; `stand` attaches to the round it follows.
- Transcripts without `brain konsil` are only searched for the marker, not parsed. The tab shows
  only when the selected session has a council. `BRAIN_DEMO=1 BRAIN_DEMO_KONSIL=<transcript>`
  opens the tab on a transcript, for screenshots.

**Left out.** Buttons that write into the session ("another round", "ask the lazy one"); other
messages during the council (e.g. a note from another session) stay in Messages.

## The skill patch

Not applied here: the skill lives in the `phoenix-framework` plugin. Against
`skills/konsil/SKILL.md` of version 3.5.0:

```diff
@@ ## 1. The topic
 No topic anywhere? Ask the user for one before you start.
 
+## Brain
+
+If the `brain` CLI is there, mark the council for Brain's Konsil tab. Run each call as
+`brain konsil … >/dev/null 2>&1 || true`, so a missing Brain never shows:
+
+- before step 2: `brain konsil start "<the topic in one line>"`
+- in step 4, before you narrate: `brain konsil stand --einig "<point>; <point>" --strittig
+  "<point> (faul: <position>, kumpel: <position>)"` — the same agreement and dispute you name in
+  your neutral sentences, points separated by `;`, `--strittig ""` when nothing is left
+- when the user decides: `brain konsil end "<the decision in one line>"`
+
 ## 2. Opening statements
 
-Start both subagents **in parallel** (general-purpose), and wait for both:
+Start both subagents **in parallel** (general-purpose), and wait for both. Their descriptions
+start with `Konsil: lazy senior` and `Konsil: pragmatic buddy` (Brain tells them apart by that):
```
