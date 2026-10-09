#!/usr/bin/env python3
"""Simulate the bd-fvcip3t5 plans listing on main's plans (2026-10-09).

Writes OUT/plans/<name>.qmd per top-level plan (title from its H1, date
from the filename, trivial body) plus a table listing over all of them,
so `q2 render --strict` in OUT exercises every real plan title through
the listing round trip.

Usage: python3 plans-listing-sim.py OUT  (run from the repo root)
"""
import glob, os, re, sys

out = sys.argv[1]
os.makedirs(os.path.join(out, "plans"), exist_ok=True)
n = 0
for f in sorted(glob.glob("claude-notes/plans/*.md")):
    body = open(f, encoding="utf-8").read()
    if body.startswith("---\n"):
        continue
    m = re.search(r"^# (.+)$", body, re.M)
    if not m:
        continue
    title = m.group(1).strip().replace("'", "''")
    d = re.match(r"(\d{4}-\d{2}-\d{2})", os.path.basename(f))
    fm = f"---\ntitle: '{title}'\n" + (f"date: {d.group(1)}\n" if d else "") + "---\n\nBody.\n"
    stem = os.path.splitext(os.path.basename(f))[0]
    open(os.path.join(out, "plans", stem + ".qmd"), "w").write(fm)
    n += 1
open(os.path.join(out, "_quarto.yml"), "w").write("project:\n  type: website\n")
open(os.path.join(out, "index.qmd"), "w").write(
    '---\ntitle: Plans\nlisting:\n  id: plans\n  type: table\n  contents: "plans/*.qmd"\n'
    '  fields: [date, title]\n  sort: "date desc"\n---\n'
)
print(n, "plans")
