#!/usr/bin/env python3
"""Builds the Brain explainer video from script.json, the narration in audio/ and the app
screenshots in build/shots/ (see capture.sh).

    python3 video/build.py            # → video/out/brain-explainer.mp4

Cards, the framed app shots and the captions are HTML pages rendered by headless Chrome; ffmpeg
zooms into the part of the app each line talks about, crossfades between shots and lays the
narration underneath.
"""

import html
import os
import signal
import tempfile
import json
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent
BUILD = ROOT / "build"
OUT = ROOT / "out" / "brain-explainer.mp4"
CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
ICON = (ROOT.parent / "assets" / "icon.png").as_uri()

FPS = 30
W, H = 3840, 2160          # working canvas (2× of 1080p)
LEAD = 0.35                # silence before a line starts
TAIL = 0.75                # hold after a line ends
XFADE = 0.45               # crossfade between different shots

# The app window inside a framed shot, in CSS px of a 1920×1080 page.
WIN_PT = (1440, 860)       # the captured window, in points
WIN_H = 940
WIN_W = WIN_H * WIN_PT[0] / WIN_PT[1]
WIN_X = (1920 - WIN_W) / 2
WIN_Y = 70

# Where each line looks, as a rectangle in window points (x, y, w, h).
FOCUS = {
    "full": (0, 0, 1440, 860),
    "cards": (8, 100, 470, 280),
    "callout": (500, 100, 930, 140),
    "lower-list": (8, 370, 470, 300),
    "messages": (500, 230, 930, 600),
    "detail": (490, 40, 950, 420),
    "jump": (500, 40, 930, 200),
    "left": (0, 40, 490, 330),
    "header": (490, 40, 950, 260),
}

INK = "#0f1322"
BASE_CSS = f"""
* {{ box-sizing: border-box; margin: 0; }}
html, body {{ width: 1920px; height: 1080px; overflow: hidden; }}
body {{ font-family: -apple-system, "SF Pro Display", sans-serif; color: #e7e9f1;
  -webkit-font-smoothing: antialiased; }}
.stage {{ width: 1920px; height: 1080px; position: relative;
  background: radial-gradient(1200px 700px at 30% 20%, #1c2440 0%, {INK} 60%, #0b0e1a 100%); }}
.dots {{ position: absolute; inset: 0; opacity: .5;
  background-image: radial-gradient(#2a3252 1.6px, transparent 1.7px); background-size: 44px 44px; }}
.mono {{ font-family: "SF Mono", Menlo, monospace; }}
"""


REUSE_CLIPS = "--reuse-clips" in sys.argv


def run(cmd, **kw):
    subprocess.run(cmd, check=True, **kw)


def render_html(name: str, body: str, transparent: bool = False) -> Path:
    page = BUILD / "pages" / f"{name}.html"
    png = BUILD / "pages" / f"{name}.png"
    page.parent.mkdir(parents=True, exist_ok=True)
    page.write_text(f"<!doctype html><meta charset=utf-8><style>{BASE_CSS}</style>{body}")
    profile = tempfile.mkdtemp(prefix="chrome-", dir=BUILD)
    cmd = [CHROME, "--headless=new", "--disable-gpu", "--hide-scrollbars", "--force-device-scale-factor=2",
           "--window-size=1920,1080", f"--user-data-dir={profile}", "--no-first-run",
           "--no-default-browser-check", "--disable-extensions", "--disable-background-networking",
           "--disable-sync", "--allow-file-access-from-files", f"--screenshot={png}", page.as_uri()]
    if transparent:
        cmd.insert(1, "--default-background-color=00000000")
    png.unlink(missing_ok=True)
    # Headless Chrome writes the screenshot but does not always exit afterwards: wait for the
    # file to settle, then end it.
    # Its own profile and process group per page, so no instance lingers and swallows the next one.
    chrome = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    deadline, last = time.time() + 30, -1
    while time.time() < deadline:
        if chrome.poll() is not None and png.exists():
            break
        size = png.stat().st_size if png.exists() else -1
        if size > 0 and size == last:
            break
        last = size
        time.sleep(0.4)
    try:
        os.killpg(chrome.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    chrome.wait()
    shutil.rmtree(profile, ignore_errors=True)
    if not png.exists():
        raise RuntimeError(f"Chrome did not render {page.name}")
    return png


# ---- pages --------------------------------------------------------------------------------

def framed_shot(name: str) -> Path:
    shot = (BUILD / "shots" / f"{name}.png").as_uri()
    return render_html(f"shot-{name}", f"""
<div class=stage><div class=dots></div>
<img src="{shot}" style="position:absolute; left:{WIN_X}px; top:{WIN_Y}px; height:{WIN_H}px;
  filter: drop-shadow(0 30px 60px rgba(0,0,0,.55)) drop-shadow(0 0 1px rgba(255,255,255,.12));">
</div>""")


CARDS = {
    "intro": f"""
<div class=stage><div class=dots></div>
<div style="position:absolute; left:220px; top:300px; display:flex; align-items:center; gap:70px;">
  <img src="{ICON}" style="width:340px; filter: drop-shadow(0 30px 60px rgba(0,0,0,.5));">
  <div>
    <div style="font-size:150px; font-weight:800; letter-spacing:-4px; line-height:1;">Brain</div>
    <div style="font-size:48px; color:#aab1c6; margin-top:26px; max-width:900px; line-height:1.25;">
      One window for all your Claude&nbsp;Code sessions</div>
  </div>
</div></div>""",
    "protocol": """
<div class=stage><div class=dots></div>
<style>
 .box { position:absolute; background:#161b2c; border:1px solid #2e3654; border-radius:22px; padding:34px 40px; }
 .h { font-size:32px; font-weight:700; color:#f3f4f9; }
 .sub { font-size:23px; color:#8c93aa; margin-top:6px; }
 .chip { display:inline-block; white-space:nowrap; font-size:22px; padding:8px 14px; border-radius:10px;
   margin:14px 10px 0 0; background:#1d2338; border:1px solid #2e3654; color:#d5d9e6; }
 .arrow { position:absolute; height:4px; background:#46507a; border-radius:2px; }
 .arrow:after { content:""; position:absolute; right:-6px; top:-9px; border:11px solid transparent; border-left:16px solid #46507a; }
 .line { font-size:20px; color:#aab1c6; white-space:nowrap; margin-top:16px; }
</style>
<div class=box style="left:100px; top:150px; width:760px; height:300px;">
  <div class=h>Claude Code hooks</div><div class=sub>automatic, in every session</div>
  <span class="chip mono">SessionStart</span><span class="chip mono">UserPromptSubmit</span><span class="chip mono">Notification</span>
  <span class="chip mono">Stop</span><span class="chip mono">SessionEnd</span>
</div>
<div class=box style="left:100px; top:630px; width:760px; height:300px;">
  <div class=h>brain report</div><div class=sub>written by Claude, as CLAUDE.md asks</div>
  <div class="chip mono" style="color:#ff8f9a">brain report --waiting "Per tenant or global?"</div><br>
  <div class="chip mono" style="color:#45d19a">brain report --done "Search indexes changelogs"</div>
</div>
<div class=arrow style="left:880px; top:298px; width:110px;"></div>
<div class=arrow style="left:880px; top:778px; width:110px;"></div>
<div class=box style="left:1010px; top:150px; width:560px; height:780px;">
  <div class=h>One event log</div><div class="sub mono" style="font-size:21px">~/.claude-brain/events/</div>
  <div style="margin-top:28px">
  <div class="line mono">{"kind":"prompt", …}</div>
  <div class="line mono">{"kind":"permission", …}</div>
  <div class="line mono" style="color:#ff8f9a">{"kind":"waiting", …}</div>
  <div class="line mono">{"kind":"stop", …}</div>
  <div class="line mono">{"kind":"prompt", …}</div>
  <div class="line mono" style="color:#45d19a">{"kind":"done", …}</div>
  </div>
  <div class=sub style="position:absolute; bottom:34px; left:40px; right:40px;">one line per event, from every account</div>
</div>
<div class=arrow style="left:1590px; top:538px; width:90px;"></div>
<div style="position:absolute; left:1700px; top:440px; text-align:center; width:180px;">
  <img src=\"""" + ICON + """\" style="width:180px;">
  <div class=h style="margin-top:10px">Brain</div>
</div>
</div>""",
    "notify": f"""
<div class=stage><div class=dots></div>
<div style="position:absolute; right:90px; top:80px; width:760px; display:flex; gap:24px; padding:26px 30px;
  background:rgba(48,52,66,.92); border:1px solid rgba(255,255,255,.12); border-radius:30px;
  box-shadow: 0 30px 80px rgba(0,0,0,.5);">
  <img src="{ICON}" style="width:76px; height:76px;">
  <div style="flex:1; min-width:0;">
    <div style="display:flex; justify-content:space-between; font-size:26px; font-weight:700;">api-gateway
      <span style="font-weight:400; color:#a7adbd; font-size:22px">now</span></div>
    <div style="font-size:24px; color:#d9dce5; margin-top:2px">main · braucht dich</div>
    <div style="font-size:24px; color:#c3c7d2; margin-top:4px">Sollen die Limits pro Mandant oder global gelten?</div>
  </div>
</div>
<div style="position:absolute; left:220px; top:520px;">
  <div style="font-size:84px; font-weight:800; letter-spacing:-2px; line-height:1.05; max-width:1100px;">Never keep Claude waiting.</div>
  <div style="font-size:38px; color:#aab1c6; margin-top:24px;">A notification as soon as a session needs you.</div>
</div></div>""",
    "install": """
<div class=stage><div class=dots></div>
<div style="position:absolute; left:260px; top:130px; width:1400px; height:820px; border-radius:22px; overflow:hidden;
  background:#0b0e18; border:1px solid #2e3654; box-shadow:0 40px 90px rgba(0,0,0,.6);">
  <div style="height:56px; background:#161b2c; display:flex; align-items:center; gap:12px; padding-left:22px;">
    <span style="width:16px;height:16px;border-radius:50%;background:#ff5f57"></span>
    <span style="width:16px;height:16px;border-radius:50%;background:#febc2e"></span>
    <span style="width:16px;height:16px;border-radius:50%;background:#28c840"></span>
  </div>
  <pre class=mono style="font-size:27px; line-height:1.6; padding:34px 44px; color:#d5d9e6;">\
<span style="color:#5aa9ff">$</span> git clone https://github.com/martinemmert/big-brain-claude.git
<span style="color:#5aa9ff">$</span> cd big-brain-claude
<span style="color:#5aa9ff">$</span> ./scripts/install.sh
<span style="color:#8c93aa">→ brain CLI
→ Brain.app
→ Hooks &amp; Protokoll</span>
main (~/.claude)
  settings.json  <span style="color:#45d19a">✓</span> updated (backup: settings.json.brain-backup-…)
  CLAUDE.md      <span style="color:#45d19a">✓</span> updated (backup: CLAUDE.md.brain-backup-…)
second (~/.claude-second)
  settings.json  <span style="color:#45d19a">✓</span> updated
  CLAUDE.md      <span style="color:#45d19a">✓</span> updated</pre>
</div></div>""",
    "outro": f"""
<div class=stage><div class=dots></div>
<div style="position:absolute; inset:0; display:flex; flex-direction:column; align-items:center; justify-content:center;">
  <img src="{ICON}" style="width:300px; filter: drop-shadow(0 30px 60px rgba(0,0,0,.5));">
  <div style="font-size:120px; font-weight:800; letter-spacing:-3px; margin-top:20px;">Brain</div>
  <div class=mono style="font-size:40px; color:#5aa9ff; margin-top:18px;">github.com/martinemmert/big-brain-claude</div>
  <div style="font-size:30px; color:#8c93aa; margin-top:18px;">Open source, MIT licensed</div>
</div></div>""",
}


def caption(name: str, text: str) -> Path:
    return render_html(f"cap-{name}", f"""
<div style="position:absolute; left:0; right:0; bottom:56px; display:flex; justify-content:center;">
  <div style="max-width:1500px; padding:16px 30px; border-radius:16px; background:rgba(9,12,22,.78);
    font-size:36px; line-height:1.35; text-align:center; color:#f3f4f9;">{html.escape(text)}</div>
</div>""", transparent=True)


def caption_chunks(say: str) -> list[str]:
    """Sentences, longer ones split at commas, about two caption lines each."""
    chunks = []
    for sentence in re.split(r"(?<=[.?!])\s+", say.strip()):
        if len(sentence) <= 90:
            chunks.append(sentence)
            continue
        current = ""
        for part in re.split(r"(?<=,)\s+", sentence):
            if current and len(current) + len(part) > 90:
                chunks.append(current)
                current = part
            else:
                current = f"{current} {part}".strip()
        if current:
            chunks.append(current)
    return chunks


# ---- motion -------------------------------------------------------------------------------

def focus_rect(name: str) -> tuple[float, float, float]:
    """The 16:9 crop (x, y, w) on the canvas that shows a focus area with some air around it."""
    if name == "full":
        return (0.0, 0.0, float(W))
    px, py, pw, ph = FOCUS[name]
    scale = 2 * WIN_W / WIN_PT[0]
    x, y, w, h = WIN_X * 2 + px * scale, WIN_Y * 2 + py * scale, pw * scale, ph * scale
    pad = 1.12
    w, h = w * pad, h * pad
    if w / h < 16 / 9:
        w = h * 16 / 9
    else:
        h = w * 9 / 16
    w = min(w, W)
    h = w * 9 / 16
    cx, cy = x + pw * scale / 2, y + ph * scale / 2
    x = min(max(cx - w / 2, 0), W - w)
    y = min(max(cy - h / 2, 0), H - h)
    return (x, y, w)


def clip(index: int, image: Path, start, end, duration: float, captions: list[tuple[Path, float, float]]) -> Path:
    """One scene: glide from `start` to `end` crop over 1.4 s, then drift slowly; captions on top."""
    out = BUILD / "clips" / f"{index:02d}.mp4"
    out.parent.mkdir(parents=True, exist_ok=True)
    if REUSE_CLIPS and out.exists():
        return out
    frames = int(round(duration * FPS))
    glide = 1.4 * FPS
    (x0, y0, w0), (x1, y1, w1) = start, end
    e = f"(st(0,min(on/{glide},1))*0+ld(0)*ld(0)*(3-2*ld(0)))"
    drift = f"(1+0.025*on/{frames})"
    w_expr = f"(({w0})+(({w1})-({w0}))*{e})/{drift}"
    zoom = f"{W}/({w_expr})"
    cx = f"(({x0}+{w0}/2)+(({x1}+{w1}/2)-({x0}+{w0}/2))*{e})"
    cy = f"(({y0}+{w0}*9/32)+(({y1}+{w1}*9/32)-({y0}+{w0}*9/32))*{e})"
    x = f"max(0,min({W}-{W}/zoom,{cx}-{W}/zoom/2))"
    y = f"max(0,min({H}-{H}/zoom,{cy}-{H}/zoom/2))"

    inputs = ["-loop", "1", "-framerate", str(FPS), "-i", str(image)]
    graph = [f"[0:v]scale={W}:{H},zoompan=z='{zoom}':x='{x}':y='{y}':d=1:s=1920x1080:fps={FPS},format=yuv420p[v0]"]
    last = "v0"
    for n, (png, t0, t1) in enumerate(captions, start=1):
        inputs += ["-loop", "1", "-framerate", str(FPS), "-i", str(png)]
        graph.append(f"[{n}:v]scale=1920:1080,format=rgba[c{n}]")
        graph.append(f"[{last}][c{n}]overlay=0:0:enable='between(t,{t0:.2f},{t1:.2f})'[v{n}]")
        last = f"v{n}"
    run(["ffmpeg", "-loglevel", "error", "-y", *inputs, "-filter_complex", ";".join(graph),
         "-map", f"[{last}]", "-frames:v", str(frames), "-c:v", "libx264", "-preset", "medium",
         "-crf", "18", "-pix_fmt", "yuv420p", "-r", str(FPS), str(out)])
    return out


# ---- assembly -----------------------------------------------------------------------------

def main():
    script = json.loads((ROOT / "script.json").read_text())
    manifest = json.loads((ROOT / "audio" / "manifest.json").read_text())["lines"]
    if not REUSE_CLIPS:
        shutil.rmtree(BUILD / "clips", ignore_errors=True)

    images: dict[str, Path] = {}
    scenes = []
    for scene in script["scenes"]:
        kind, name = scene["shot"].split(":")
        if scene["shot"] not in images:
            images[scene["shot"]] = framed_shot(name) if kind == "app" else render_html(f"card-{name}", CARDS[name])
        audio = manifest[scene["id"]]["duration"]
        scenes.append({**scene, "image": images[scene["shot"]], "audio": audio,
                       "duration": LEAD + audio + TAIL})

    # Clips, with crossfades only where the picture changes.
    clips, offsets, t, previous = [], [], 0.0, None
    for i, scene in enumerate(scenes):
        same = previous is not None and previous["shot"] == scene["shot"]
        if i > 0:
            t -= 0.0 if same else XFADE
        scene["start"] = t
        end_rect = focus_rect(scene.get("focus", "full"))
        start_rect = focus_rect(previous.get("focus", "full")) if same else (
            end_rect if scene["shot"].startswith("card") else focus_rect("full"))
        if scene["shot"].startswith("card"):
            start_rect = (0.0, 0.0, float(W))
            end_rect = (0.0, 0.0, float(W))

        chunks = caption_chunks(scene["say"])
        total = sum(len(c) for c in chunks)
        caps, ct = [], LEAD
        for n, chunk in enumerate(chunks):
            share = scene["audio"] * len(chunk) / total
            caps.append((caption(f"{scene['id']}-{n}", chunk), ct, ct + share))
            ct += share
        clips.append(clip(i, scene["image"], start_rect, end_rect, scene["duration"], caps))
        offsets.append((same, t))
        t += scene["duration"]
        previous = scene
        print(f"  {scene['id']:<14} {scene['duration']:5.1f}s")

    # Video: chain the clips, xfading at shot changes and concatenating otherwise.
    inputs, graph = [], []
    for i, c in enumerate(clips):
        inputs += ["-i", str(c)]
        # One time base for every stream, or xfade refuses concat's output.
        graph.append(f"[{i}:v]settb=AVTB,setpts=PTS-STARTPTS[in{i}]")
    label, length = "in0", scenes[0]["duration"]
    for i in range(1, len(clips)):
        same, start = offsets[i]
        nxt = f"x{i}"
        if same:
            graph.append(f"[{label}][in{i}]concat=n=2:v=1:a=0,settb=AVTB[{nxt}]")
        else:
            graph.append(f"[{label}][in{i}]xfade=transition=fade:duration={XFADE}:offset={length - XFADE:.3f},settb=AVTB[{nxt}]")
            length -= XFADE
        length += scenes[i]["duration"]
        label = nxt

    # Audio: every line at its scene's start plus the lead-in.
    for scene in scenes:
        inputs += ["-i", str(ROOT / "audio" / f"{scene['id']}.m4a")]
    n_video = len(clips)
    for j, scene in enumerate(scenes):
        delay = int((scene["start"] + LEAD) * 1000)
        graph.append(f"[{n_video + j}:a]aresample=48000,adelay={delay}|{delay}[a{j}]")
    graph.append("".join(f"[a{j}]" for j in range(len(scenes)))
                 + f"amix=inputs={len(scenes)}:normalize=0,apad,atrim=0:{length:.3f}[aout]")

    OUT.parent.mkdir(parents=True, exist_ok=True)
    run(["ffmpeg", "-loglevel", "error", "-y", *inputs, "-filter_complex", ";".join(graph),
         "-map", f"[{label}]", "-map", "[aout]", "-c:v", "libx264", "-preset", "medium", "-crf", "19",
         "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "160k", "-movflags", "+faststart", str(OUT)])
    print(f"→ {OUT.relative_to(ROOT.parent)}  ({length:.1f}s)")


if __name__ == "__main__":
    main()
