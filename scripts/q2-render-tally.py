#!/usr/bin/env python3
"""Tally the diagnostics of a `q2 render --json-errors` run of a project.

Built for bd-uk8zgkha (rendering claude-notes/ as a q2 website), where each
round fixes one class of parse error across hundreds of files and we want the
delta per round. Works on any project.

Usage:
  scripts/q2-render-tally.py claude-notes                  # summary table
  scripts/q2-render-tally.py claude-notes --save base.json # ... and save the tally
  scripts/q2-render-tally.py claude-notes --compare base.json
  scripts/q2-render-tally.py claude-notes --list Q-2-17    # file:line + source text
  scripts/q2-render-tally.py claude-notes --list uncoded   # diagnostics without a Q-code
  scripts/q2-render-tally.py --from out.jsonl ...          # reuse a captured stderr

Notes on the input format (q2 0.33.0):
- Pass-1 (parse) failures are `json-pass1-failure` records; the structured
  diagnostics are in their `diagnostics` array. Ignore their `error` field: it
  is ANSI-rendered text that shows only a subset of them.
- Standalone `json-diagnostic` records (warnings) carry no file path
  (bd-ckbqmupi), so the path is scraped from the `rendered` text.
"""

import argparse
import collections
import json
import re
import subprocess
import sys
from pathlib import Path

ANSI = re.compile(r"\x1b\[[0-9;]*m|\x1b\]8;;[^\x1b]*\x1b\\")
RENDERED_LOC = re.compile(r"\[ (\S+?):(\d+):(\d+) \]")
SUMMARY = re.compile(r"Rendered (\d+) of (\d+) files")


def run_render(project, q2):
    proc = subprocess.run(
        [q2, "render", project, "--json-errors"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    return proc.stderr + proc.stdout


def key_of(diag):
    code = diag.get("code") or "uncoded"
    title = diag.get("title", "")
    # Q-2-49 and friends embed the offending text in the title; keep only
    # the code so they aggregate into one row.
    if code != "uncoded":
        return code
    return f"uncoded: {title[:40]}"


def parse(text, project_root):
    records = []  # (kind, key, title, file, line)
    rendered = total = None
    for line in text.splitlines():
        m = SUMMARY.search(line)
        if m:
            rendered, total = int(m.group(1)), int(m.group(2))
        if not line.startswith("{"):
            continue
        d = json.loads(line)
        if "pass1-failure" in d.get("$schema", ""):
            f = d.get("source_file")
            for x in d.get("diagnostics") or []:
                records.append(
                    (x.get("kind", "error"), key_of(x), x.get("title", ""), f, x.get("start_line"))
                )
        else:
            loc = RENDERED_LOC.search(ANSI.sub("", d.get("rendered", "")))
            f = loc.group(1) if loc else None
            records.append(
                (d.get("kind", "?"), key_of(d), d.get("title", ""), f, d.get("start_line"))
            )
    root = str(Path(project_root).resolve()) + "/" if project_root else ""
    records = [(k, c, t, (f[len(root):] if f and f.startswith(root) else f), ln) for k, c, t, f, ln in records]
    return records, rendered, total


def tally(records):
    out = {}
    for kind, key, title, f, _ in records:
        e = out.setdefault(key, {"kind": kind, "title": title, "count": 0, "files": set()})
        e["count"] += 1
        if f:
            e["files"].add(f)
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("project", nargs="?", default="claude-notes")
    ap.add_argument("--q2", default="q2", help="q2 binary (default: q2 on PATH)")
    ap.add_argument("--from", dest="from_file", help="read captured --json-errors output instead of rendering")
    ap.add_argument("--save", help="write the tally as JSON")
    ap.add_argument("--compare", help="show deltas against a saved tally")
    ap.add_argument("--list", metavar="CODE", help="list occurrences of CODE (or 'uncoded') with source text")
    ap.add_argument("--limit", type=int, default=0, help="max rows for --list (0 = all)")
    args = ap.parse_args()

    text = Path(args.from_file).read_text() if args.from_file else run_render(args.project, args.q2)
    records, rendered, total = parse(text, args.project)

    if args.list:
        want = args.list
        rows = [r for r in records if r[1] == want or (want == "uncoded" and r[1].startswith("uncoded"))]
        rows.sort(key=lambda r: (r[3] or "", r[4] or 0))
        for i, (_, _, _, f, ln) in enumerate(rows):
            if args.limit and i >= args.limit:
                break
            src = ""
            try:
                src = (Path(args.project) / f).read_text().split("\n")[ln - 1].strip()[:140]
            except Exception:
                pass
            print(f"{f}:{ln}: {src}")
        print(f"-- {len(rows)} occurrences in {len({r[3] for r in rows})} files", file=sys.stderr)
        return

    t = tally(records)
    base = {}
    if args.compare:
        base = json.loads(Path(args.compare).read_text())
    base_rows = base.get("rows", {})

    failed = len({r[3] for r in records if r[0] == "error" and r[3]})
    head = f"Rendered {rendered} of {total} files; {failed} files with errors"
    if base:
        head += f" (baseline: rendered {base.get('rendered')} of {base.get('total')}, {base.get('failed_files')} with errors)"
    print(head)
    print()
    print("| diags | files | Δdiags | kind | code | title |")
    print("|------:|------:|-------:|------|------|-------|")
    keys = sorted(set(t) | set(base_rows), key=lambda k: -(t.get(k, {}).get("count", 0)))
    for k in keys:
        e = t.get(k)
        b = base_rows.get(k, {}).get("count", 0)
        n = e["count"] if e else 0
        delta = f"{n - b:+d}" if base else ""
        kind = e["kind"] if e else base_rows[k]["kind"]
        title = (e["title"] if e else base_rows[k]["title"])[:50].replace("|", "\\|")
        nfiles = len(e["files"]) if e else 0
        print(f"| {n} | {nfiles} | {delta} | {kind} | {k} | {title} |")

    if args.save:
        Path(args.save).write_text(json.dumps({
            "rendered": rendered, "total": total, "failed_files": failed,
            "rows": {k: {"kind": v["kind"], "title": v["title"], "count": v["count"], "files": len(v["files"])}
                     for k, v in t.items()},
        }, indent=1))


if __name__ == "__main__":
    main()
