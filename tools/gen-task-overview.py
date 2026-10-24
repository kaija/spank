#!/usr/bin/env python3
"""Regenerate the overview (wave table, mermaid graph, dependency table, critical path) in docs/tasks.md.

Reads each "### Txx" block ("- 依賴：" deps, "- 狀態：✅" = done) and fails if a task depends on a task
in the same or a later wave. Usage: python3 -I tools/gen-task-overview.py [docs/tasks.md]
"""
import re, functools, sys
p=sys.argv[1] if len(sys.argv) > 1 else "docs/tasks.md"; s=open(p).read()
waves={}; deps={}; titles={}; done=set(); cur=None; t=None
for line in s.splitlines():
    m=re.match(r'## Wave (\d+) — (.*)',line)
    if m: cur=int(m.group(1)); continue
    m=re.match(r'### (T\d+) (.*)',line)
    if m: t=m.group(1); waves[t]=cur; titles[t]=m.group(2).replace('⛳ ','').replace('`','')
    m=re.match(r'- 依賴：(.*)',line)
    if m: deps[t]=re.findall(r'T\d+',m.group(1))
    if line.startswith('- 狀態：✅'): done.add(t)
wtitle={int(m.group(1)):m.group(2) for m in re.finditer(r'## Wave (\d+) — (.*)',s)}
bad=[(t,d) for t in deps for d in deps[t] if d not in waves or waves[d]>=waves[t]]
assert not bad, bad
@functools.lru_cache(None)
def lp(t): return max([lp(d) for d in deps[t]], key=len, default=[])+[t]
crit=max((lp(t) for t in deps), key=len)
mark=lambda t: t+(" ✅" if t in done else "")
tbl=["| Wave | 主題 | 任務（同 Wave 可平行） |","|------|------|------|"]+[f"| {w} | {wtitle[w]} | "+" ".join(mark(t) for t in waves if waves[t]==w)+" |" for w in sorted(wtitle)]
mer=["```mermaid","flowchart LR"]
for w in sorted(wtitle):
    mer.append(f'  subgraph W{w}["Wave {w}"]')
    mer += [f'    {t}["{mark(t)} {titles[t]}"]' for t in waves if waves[t]==w]
    mer.append('  end')
mer += [f'  {d} --> {t}' for t in deps for d in deps[t]]
mer += [f'  style {t} fill:#d4edda' for t in sorted(done)]
mer.append("```")
dt=["| 任務 | Wave | 依賴 |","|------|------|------|"]+[f"| {mark(t)} {titles[t]} | {waves[t]} | {', '.join(deps[t]) or '—'} |" for t in deps]
head=("## 總覽\n\n"+"\n".join(tbl)+"\n\n> T25（spank-ipc）、T29（spank-helper）已依 ADR-0014 移除（加速度計不需 root）。\n\n"
      +"\n".join(mer)+"\n\n**Critical path**："+" → ".join(crit)+"\n\n### 依賴表\n\n"+"\n".join(dt)+"\n\n---\n\n## Wave 0")
s=re.sub(r'## 總覽.*?---\n\n## Wave 0', lambda _: head, s, count=1, flags=re.S)
open(p,'w').write(s); print("critical:", " → ".join(crit)); print("tasks:", len(deps), "done:", sorted(done))
