#!/usr/bin/env python3
"""Generate built-in sound candidates with ElevenLabs Sound Effects, audition them, import the picks.

Prompts, lengths and profiles live in assets/sounds/elevenlabs.json.

  python3 -I tools/elevenlabs-sounds.py generate [--candidates 3] [--only STEM ...] [--force]
  python3 -I tools/elevenlabs-sounds.py page       # target/sound-candidates/index.html
  python3 -I tools/elevenlabs-sounds.py import picks.json

The API key is read from --key, $ELEVENLABS_API_KEY or signing/elevenlabs.key (gitignored) and is
never printed. Candidates go to target/sound-candidates/<profile>/<stem>/cN.wav (48 kHz WAV) with
the exact request next to them; `import` copies the chosen ones to assets/sounds/source/<stem>.wav,
which tools/gen-builtin-sounds packs.
"""

import argparse
import concurrent.futures
import html
import json
import math
import os
import shutil
import struct
import sys
import time
import urllib.error
import urllib.request
import wave
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SPEC = ROOT / "assets/sounds/elevenlabs.json"
OUT = ROOT / "target/sound-candidates"
SOURCE = ROOT / "assets/sounds/source"
API = "https://api.elevenlabs.io/v1/sound-generation?output_format=wav_48000"

EVENT_ZH = {
    "lid_opened": "打開螢幕（喚醒）",
    "lid_closing": "闔上螢幕",
    "lid_overbent": "開啟中（30–120°）",
    "slap_light": "輕拍",
    "slap_medium": "中拍",
    "slap_hard": "重拍",
    "slap_combo": "連擊",
}
EVENT_ORDER = list(EVENT_ZH)


def load_spec():
    return json.loads(SPEC.read_text(encoding="utf-8"))


def all_sounds(spec):
    for p in spec["profiles"]:
        for s in p["sounds"]:
            yield p, s


def request_body(spec, s):
    d = spec["defaults"]
    return {
        "text": f"{s['prompt']}, {d['style']}",
        "duration_seconds": s.get("duration", spec["durations"][s["event"]]),
        "prompt_influence": s.get("prompt_influence", d["prompt_influence"]),
        "model_id": d["model_id"],
    }


def read_key(path):
    key = os.environ.get("ELEVENLABS_API_KEY", "").strip()
    if not key:
        p = Path(path) if path else ROOT / "signing/elevenlabs.key"
        if not p.is_file():
            sys.exit(f"no API key: pass --key, set ELEVENLABS_API_KEY or create {p}")
        key = p.read_text().strip()
    return key


def call(key, body, dest):
    """POST one generation; retry on rate limits and server errors."""
    data = json.dumps(body).encode()
    for attempt in range(6):
        req = urllib.request.Request(
            API, data=data, method="POST",
            headers={"xi-api-key": key, "Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(req, timeout=120) as r:
                audio = r.read()
            tmp = dest.with_suffix(".part")
            tmp.write_bytes(audio)
            tmp.replace(dest)
            return None
        except urllib.error.HTTPError as e:
            detail = e.read()[:300].decode("utf-8", "replace")
            if e.code in (429, 500, 502, 503, 504) and attempt < 5:
                time.sleep(2 ** attempt * 2)
                continue
            return f"HTTP {e.code}: {detail}"
        except (urllib.error.URLError, TimeoutError) as e:
            if attempt < 5:
                time.sleep(2 ** attempt * 2)
                continue
            return str(e)
    return "gave up"


def generate(args):
    spec = load_spec()
    key = read_key(args.key)
    jobs = []
    for p, s in all_sounds(spec):
        if args.only and s["stem"] not in args.only:
            continue
        body = request_body(spec, s)
        d = OUT / p["id"] / s["stem"]
        d.mkdir(parents=True, exist_ok=True)
        for n in range(1, args.candidates + 1):
            dest = d / f"c{n}.wav"
            if dest.exists() and not args.force:
                continue
            (d / f"c{n}.json").write_text(json.dumps(body, ensure_ascii=False, indent=2))
            jobs.append((s["stem"], n, body, dest))
    secs = sum(b["duration_seconds"] for _, _, b, _ in jobs)
    print(f"{len(jobs)} generations, {secs:.1f} s of audio")
    failed = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        futs = {pool.submit(call, key, b, dest): (stem, n) for stem, n, b, dest in jobs}
        for i, f in enumerate(concurrent.futures.as_completed(futs), 1):
            stem, n = futs[f]
            err = f.result()
            print(f"[{i}/{len(jobs)}] {stem} c{n}: {'ok' if err is None else err}", flush=True)
            if err:
                failed.append(f"{stem} c{n}: {err}")
    if failed:
        print(f"{len(failed)} failed:\n  " + "\n  ".join(failed))
        sys.exit(1)


def measure(path):
    """Duration, peak and RMS (dBFS) and leading silence (ms below -40 dBFS) of a 16-bit WAV."""
    with wave.open(str(path)) as w:
        ch, rate, n = w.getnchannels(), w.getframerate(), w.getnframes()
        raw = w.readframes(n)
    xs = struct.unpack(f"<{len(raw) // 2}h", raw)
    mono = [sum(xs[i:i + ch]) / ch / 32768 for i in range(0, len(xs), ch)] or [0.0]
    peak = max(abs(v) for v in mono) or 1e-9
    rms = math.sqrt(sum(v * v for v in mono) / len(mono)) or 1e-9
    lead = next((i for i, v in enumerate(mono) if abs(v) > 0.01), len(mono))
    # Runs of 3+ full-scale samples on any channel: a flat-topped (clipped, distorted) waveform.
    clips, run = 0, 0
    for v in xs:
        run = run + 1 if abs(v) >= 32766 else 0
        clips += run == 3
    return {
        "clips": clips,
        "ms": round(n * 1000 / rate),
        "peak": round(20 * math.log10(peak), 1),
        "rms": round(20 * math.log10(rms), 1),
        "lead_ms": round(lead * 1000 / rate),
    }


def page(args):
    spec = load_spec()
    rows = []
    for p in spec["profiles"]:
        sounds = sorted(p["sounds"], key=lambda s: EVENT_ORDER.index(s["event"]))
        rows.append(f'<section><h2>{html.escape(p["zh"])} <small>{html.escape(p["en"])}</small></h2>'
                    f'<p class="desc">{html.escape(p["desc_zh"])}</p>')
        for event, stems in p.get("keep", {}).items():
            for stem in stems:
                src = next((f for f in SOURCE.glob(f"{stem}.*")), None)
                if src:
                    rel = os.path.relpath(src, OUT)
                    rows.append(f'<div class="snd keep"><div class="head"><b>{stem}</b> · {EVENT_ZH[event]} · '
                                f'沿用（已選定）</div><audio controls preload="none" src="{html.escape(rel)}"></audio></div>')
        for s in sounds:
            d = OUT / p["id"] / s["stem"]
            target = s.get("duration", spec["durations"][s["event"]])
            cands = sorted(d.glob("c*.wav")) if d.is_dir() else []
            opts = []
            for c in cands:
                m = measure(c)
                n = c.stem[1:]
                warn = " warn" if m["lead_ms"] > 80 or m["peak"] < -30 or m["clips"] > 20 else ""
                opts.append(
                    f'<label class="cand{warn}"><input type="radio" name="{s["stem"]}" value="{n}">'
                    f'<span>候選 {n}</span><audio controls preload="none" src="{p["id"]}/{s["stem"]}/{c.name}"></audio>'
                    f'<span class="m">{m["ms"]} ms · peak {m["peak"]} dB · rms {m["rms"]} dB · 前導靜音 {m["lead_ms"]} ms · 削波 {m["clips"]}</span></label>')
            opts.append(f'<label class="cand"><input type="radio" name="{s["stem"]}" value="redo"><span>都不要，重做</span></label>')
            rows.append(
                f'<div class="snd"><div class="head"><b>{html.escape(s["zh"])}</b> <code>{s["stem"]}</code> · '
                f'{EVENT_ZH[s["event"]]} · 目標 {target} s</div>'
                f'<div class="prompt">{html.escape(s["prompt"])}</div>{"".join(opts)}</div>')
        rows.append("</section>")
    doc = f"""<!doctype html><html lang="zh-Hant"><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1"><title>Spank 音效試聽</title>
<style>
:root {{ --bg:#f7f7fb; --card:#fff; --text:#1a1a2e; --muted:#6b6b80; --accent:#5856d6; --warn:#c2410c; --line:#e4e4ee; }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg:#15151d; --card:#1f1f2b; --text:#ececf6; --muted:#a0a0b8; --accent:#7472f5; --warn:#fb923c; --line:#33334a; }} }}
body {{ font: 15px/1.5 -apple-system, system-ui, sans-serif; background:var(--bg); color:var(--text); margin:0; padding:16px; }}
main {{ max-width: 980px; margin: 0 auto; }}
h1 {{ margin: 0 0 4px; }} h2 {{ margin: 28px 0 4px; }} h2 small, .desc, .prompt, .m {{ color: var(--muted); }}
.bar {{ position: sticky; top: 0; background: var(--bg); padding: 8px 0; display:flex; gap:12px; align-items:center; flex-wrap:wrap; border-bottom:1px solid var(--line); z-index:1; }}
button {{ font: inherit; padding: 6px 14px; border-radius: 8px; border: 0; background: var(--accent); color: #fff; cursor: pointer; }}
.snd {{ background: var(--card); border:1px solid var(--line); border-radius: 12px; padding: 12px; margin: 10px 0; }}
.snd.keep {{ opacity: .75; }} .head code {{ font-size: 12px; color: var(--muted); }}
.prompt {{ font-size: 13px; margin: 2px 0 8px; }}
.cand {{ display:flex; gap:10px; align-items:center; flex-wrap:wrap; padding:4px 0; }}
.cand audio {{ height: 32px; max-width: 100%; }} .cand .m {{ font-size: 12px; }} .cand.warn .m {{ color: var(--warn); }}
textarea {{ width: 100%; height: 120px; font: 12px ui-monospace, monospace; }}
</style>
<main><h1>Spank 音效試聽</h1>
<p class="desc">每個音效挑一個候選（或「重做」）。選擇存在這個瀏覽器裡；挑完按「複製選擇」，貼回對話。橘色＝前導靜音超過 80 ms（打包時會自動裁掉）、幾乎沒聲音（peak 低於 -30 dB），或削波失真（超過 20 處，建議別選）。</p>
<div class="bar"><button id="copy">複製選擇</button><span id="count"></span></div>
{''.join(rows)}
<h2>選擇 JSON</h2><textarea id="out" readonly></textarea></main>
<script>
const KEY = "spank-sound-picks";
let picks = {{}};
try {{ picks = JSON.parse(localStorage.getItem(KEY) || "{{}}"); }} catch (e) {{}}
const radios = [...document.querySelectorAll('input[type=radio]')];
const names = [...new Set(radios.map(r => r.name))];
function render() {{
  radios.forEach(r => r.checked = picks[r.name] === r.value);
  document.getElementById("count").textContent = `已選 ${{Object.keys(picks).length}} / ${{names.length}}`;
  document.getElementById("out").value = JSON.stringify(picks, null, 1);
}}
radios.forEach(r => r.addEventListener("change", () => {{
  picks[r.name] = r.value;
  try {{ localStorage.setItem(KEY, JSON.stringify(picks)); }} catch (e) {{}}
  render();
}}));
document.getElementById("copy").onclick = async () => {{
  const t = JSON.stringify(picks);
  try {{ await navigator.clipboard.writeText(t); }} catch (e) {{ const o = document.getElementById("out"); o.select(); document.execCommand("copy"); }}
}};
render();
</script></html>"""
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "index.html").write_text(doc, encoding="utf-8")
    print(OUT / "index.html")


def import_picks(args):
    spec = load_spec()
    picks = json.loads(Path(args.picks).read_text())
    where = {s["stem"]: p["id"] for p, s in all_sounds(spec)}
    redo, ignored = [], []
    for stem, choice in sorted(picks.items()):
        if stem not in where:
            ignored.append(stem)  # e.g. from a profile that was dropped or parked
            continue
        if choice == "redo":
            redo.append(stem)
            continue
        src = OUT / where[stem] / stem / f"c{choice}.wav"
        shutil.copyfile(src, SOURCE / f"{stem}.wav")
        print(f"{stem} <- {src.relative_to(ROOT)}")
    missing = sorted(set(where) - set(picks))
    if ignored:
        print(f"ignored {len(ignored)} picks not in the spec")
    if redo:
        print("redo:", " ".join(redo))
    if missing:
        print("not picked yet:", " ".join(missing))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("generate")
    g.add_argument("--key")
    g.add_argument("--candidates", type=int, default=3)
    g.add_argument("--only", nargs="*")
    g.add_argument("--force", action="store_true")
    g.add_argument("--jobs", type=int, default=2)
    sub.add_parser("page")
    i = sub.add_parser("import")
    i.add_argument("picks")
    a = ap.parse_args()
    {"generate": generate, "page": page, "import": import_picks}[a.cmd](a)


if __name__ == "__main__":
    main()
