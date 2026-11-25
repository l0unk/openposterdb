#!/usr/bin/env python3
"""Print a markdown comparison table of burst result files.

  python3 -I scripts/louk/results-table.py [~/opdb-dev/results] [label ...]
"""
import glob, json, os, sys

d = os.path.expanduser(sys.argv[1]) if len(sys.argv) > 1 else os.path.expanduser("~/opdb-dev/results")
want = set(sys.argv[2:])
rows = []
for p in sorted(glob.glob(os.path.join(d, "*.json"))):
    o = json.load(open(p))
    if want and o["label"] not in want:
        continue
    rows.append(o)
print("| label | peak RSS MB | VmHWM MB | idle RSS MB | p50 ms | p95 ms | max ms | errors | wall s | poster p95 | backdrop p95 | logo p95 | episode p95 |")
print("|---|---|---|---|---|---|---|---|---|---|---|---|---|")
for o in rows:
    k = o.get("kinds", {})
    kp = lambda n: k.get(n, {}).get("p95_ms", "-")
    print(f"| {o['label']}{' (DIED)' if o.get('server_died') else ''} | {o['peak_rss_mb']} | {o['vmhwm_mb']} | {o['idle_rss_mb']} | "
          f"{o['p50_ms']} | {o['p95_ms']} | {o['max_ms']} | {o['errors']} | {o['wall_s']} | "
          f"{kp('poster')} | {kp('backdrop')} | {kp('logo')} | {kp('episode')} |")
