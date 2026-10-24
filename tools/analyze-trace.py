#!/usr/bin/env python3
"""Per-segment stats for trace format v1 (fixtures/traces/README.md).

Usage: python3 -I tools/analyze-trace.py fixtures/traces/<dir> [--trigger 0.2] [--refractory-ms 300]

For each labeled segment it prints the gravity-removed |a| (p99 / max), how many impacts a naive
detector (per-axis 2 Hz high-pass, threshold, 80 ms window, refractory) finds with their peaks,
and the lid angle range. Slap segments should detect the requested slaps; others should detect 0.
"""
import json
import math
import sys
from pathlib import Path


def arg(flag, default):
    return float(sys.argv[sys.argv.index(flag) + 1]) if flag in sys.argv else default


def analyze(path, trigger, refractory_ms):
    lines = path.read_text().splitlines()
    header = json.loads(lines[0])
    accel, lid = [], []
    for line in lines[1:]:
        o = json.loads(line)
        (accel if "a" in o else lid).append(o)
    labels = json.loads(path.with_suffix("").with_suffix(".labels.json").read_text())["labels"]

    fs = (len(accel) - 1) / (accel[-1]["t"] - accel[0]["t"]) if len(accel) > 1 else 800.0
    alpha = 1.0 / (1.0 + 2 * math.pi * 2.0 / fs)
    prev, hp = None, [0.0, 0.0, 0.0]
    for r in accel:
        a = r["a"]
        if prev is not None:
            hp = [alpha * (hp[i] + a[i] - prev[i]) for i in range(3)]
        prev = a
        r["m"] = math.sqrt(sum(v * v for v in hp))

    # Skip the first second while the high-pass filter settles.
    usable = [r for r in accel if r["t"] > accel[0]["t"] + 1.0]
    events, last, i = [], -1e9, 0
    while i < len(usable):
        r = usable[i]
        if r["m"] > trigger and (r["t"] - last) * 1000 > refractory_ms:
            j, peak = i, 0.0
            while j < len(usable) and usable[j]["t"] - r["t"] < 0.08:
                peak = max(peak, usable[j]["m"])
                j += 1
            events.append((r["t"], peak))
            last, i = r["t"], j
        else:
            i += 1

    print(f"\n== {path.name}  ({header.get('model')}, macOS {header.get('os')}, accel {fs:.0f} Hz, "
          f"{len(lid)} lid samples, dropped {header.get('accel_dropped')})")
    print(f"   {'segment':<16} {'p99 g':>6} {'max g':>6}  {'impacts':<8} peaks / lid")
    for lb in labels:
        seg = sorted(r["m"] for r in usable if lb["start"] <= r["t"] < lb["end"])
        ev = [p for t, p in events if lb["start"] <= t < lb["end"]]
        angles = [r["lid"] for r in lid if lb["start"] <= r["t"] < lb["end"]]
        p99 = seg[int(len(seg) * 0.99)] if seg else 0.0
        mx = seg[-1] if seg else 0.0
        lid_rng = f"lid {min(angles)}–{max(angles)}°" if angles else ""
        peaks = ", ".join(f"{p:.2f}" for p in ev[:10]) + (" …" if len(ev) > 10 else "")
        print(f"   {lb['label']:<16} {p99:6.3f} {mx:6.3f}  {len(ev):<8} [{peaks}] {lid_rng}")


def main():
    d = Path(sys.argv[1])
    trigger, refractory = arg("--trigger", 0.2), arg("--refractory-ms", 300)
    print(f"detector: trigger {trigger} g, refractory {refractory:.0f} ms")
    for p in sorted(d.glob("*.ndjson")):
        analyze(p, trigger, refractory)


if __name__ == "__main__":
    main()
