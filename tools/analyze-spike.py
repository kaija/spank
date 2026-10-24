#!/usr/bin/env python3
"""Analyze a spike recording (accel.ndjson + lid.ndjson + labels.ndjson).

Usage: python3 -I tools/analyze-spike.py fixtures/traces/<dir>
Prints per-segment accelerometer stats (gravity removed) and a naive impact detector run,
plus lid-angle push-rate stats.
"""
import json
import math
import sys
from pathlib import Path


def load(path):
    header, rows = None, []
    for line in Path(path).read_text().splitlines():
        if not line.startswith("{"):
            continue
        o = json.loads(line)
        if o.get("header"):
            header = o
        elif "t" in o:
            rows.append(o)
    return header, rows


def main(d):
    d = Path(d)
    ah, acc = load(d / "accel.ndjson")
    lh, lid = load(d / "lid.ndjson")
    labels = [json.loads(l) for l in (d / "labels.ndjson").read_text().splitlines()]
    a0, l0 = ah["t0_unix_ms"], lh["t0_unix_ms"]

    # Per-axis one-pole high-pass (fc ~ 2 Hz at ~800 Hz) to remove gravity.
    fs = (len(acc) - 1) / (acc[-1]["t"] - acc[0]["t"])
    alpha = 1.0 / (1.0 + 2 * math.pi * 2.0 / fs)
    px = py = pz = None
    hx = hy = hz = 0.0
    for r in acc:
        x, y, z = r["x"], r["y"], r["z"]
        if px is not None:
            hx = alpha * (hx + x - px)
            hy = alpha * (hy + y - py)
            hz = alpha * (hz + z - pz)
        px, py, pz = x, y, z
        r["m"] = math.sqrt(hx * hx + hy * hy + hz * hz)
        r["abs_ms"] = a0 + r["t"] * 1000
    print(f"accel: {len(acc)} samples, fs≈{fs:.1f} Hz")

    # Naive impact detector: m > thr, refractory 150 ms; report peak in an 80 ms window.
    def impacts(rows, thr):
        out, i = [], 0
        while i < len(rows):
            if rows[i]["m"] > thr:
                t_start = rows[i]["abs_ms"]
                j, peak = i, 0.0
                while j < len(rows) and rows[j]["abs_ms"] - t_start < 80:
                    peak = max(peak, rows[j]["m"])
                    j += 1
                out.append((t_start, peak))
                while j < len(rows) and rows[j]["abs_ms"] - t_start < 150:
                    j += 1
                i = j
            else:
                i += 1
        return out

    idle = [r["m"] for r in acc if labels[0]["start_unix_ms"] + 500 < r["abs_ms"] < labels[0]["end_unix_ms"]]
    floor = sorted(idle)[int(len(idle) * 0.99)]
    print(f"idle noise p99 |a_hp| = {floor:.4f} g\n")
    print(f"{'segment':<12} {'p50':>7} {'p99':>7} {'max':>7}  impacts@0.05g (peaks g)")
    for lb in labels:
        seg = [r for r in acc if lb["start_unix_ms"] <= r["abs_ms"] < lb["end_unix_ms"]]
        ms = sorted(r["m"] for r in seg)
        if not ms:
            continue
        imp = impacts(seg, 0.05)
        peaks = ", ".join(f"{p:.2f}" for _, p in imp[:12])
        more = f" …(+{len(imp) - 12})" if len(imp) > 12 else ""
        print(f"{lb['label']:<12} {ms[len(ms)//2]:7.4f} {ms[int(len(ms)*0.99)]:7.4f} {ms[-1]:7.3f}  "
              f"n={len(imp):<3} [{peaks}{more}]")

    print("\nlid reports:")
    for lb in labels:
        seg = [r for r in lid if lb["start_unix_ms"] <= l0 + r["t"] * 1000 < lb["end_unix_ms"]]
        angles = [r["angle"] for r in seg if r.get("angle") is not None]
        dur = (lb["end_unix_ms"] - lb["start_unix_ms"]) / 1000
        rng = f"{min(angles)}–{max(angles)}°" if angles else "-"
        print(f"  {lb['label']:<12} reports={len(seg):<4} rate={len(seg)/dur:5.1f} Hz  angle {rng}")
    if lid:
        ts = [r["t"] for r in lid]
        gaps = sorted(b - a for a, b in zip(ts, ts[1:]))
        print(f"  min gap between lid reports: {gaps[0]*1000:.1f} ms" if gaps else "")


if __name__ == "__main__":
    main(sys.argv[1])
