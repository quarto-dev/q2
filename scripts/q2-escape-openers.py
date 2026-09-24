#!/usr/bin/env python3
"""Backslash-escape the delimiter a q2 parse error points at, then re-render.

For bd-uk8zgkha (claude-notes as a q2 website). In q2's markdown, `\\A`
is a literal A for any syntax character A. Many parse errors in prose come
from a character the author meant literally: `~16` (about 16), `a.rs`'s,
`O(N * D)`. This script renders the project, and for each diagnostic of the
requested code escapes exactly the character the diagnostic points at. It
repeats until the code is gone or nothing changes, because a file can hold
several errors of the same class and the parser may stop early.

It edits only positions the parser reports, and only after checking that the
expected character is there; anything else is listed as skipped for manual
review. Review the diff before committing.

Usage:
  scripts/q2-escape-openers.py claude-notes Q-2-17            # apply
  scripts/q2-escape-openers.py claude-notes Q-2-17 --dry-run  # show edits only

Supported codes: see RULES below.
"""

import argparse
import collections
import json
import subprocess
import sys
from pathlib import Path

# code -> (character to escape, where the position comes from)
#   "opener": the detail whose content mentions "opening"; for "unclosed"
#             errors the main start position is the end of the block.
#   "start":  the diagnostic's own start position.
RULES = {
    "Q-2-17": ("~", "opener"),   # Unclosed Subscript: `~16`
    "Q-2-16": ("^", "opener"),   # Unclosed Superscript
    "Q-2-7": ("'", "opener"),    # Unclosed Single Quote: `a.rs`'s
    "Q-2-12": ("*", "opener"),   # Unclosed Star Emphasis: O(N * D)
    "Q-2-5": ("_", "opener"),    # Unclosed Underscore Emphasis
    "Q-2-11": ('"', "opener"),   # Unclosed Double Quote
}


def render(project, q2):
    proc = subprocess.run([q2, "render", project, "--json-errors"],
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    return proc.stderr


def positions(text, code):
    """Yield (file, line, column) of the character each `code` diagnostic blames."""
    _, where = RULES[code]
    for raw in text.splitlines():
        if not raw.startswith("{"):
            continue
        d = json.loads(raw)
        for x in d.get("diagnostics") or []:
            if x.get("code") != code:
                continue
            if where == "start":
                yield d["source_file"], x["start_line"], x["start_column"]
                continue
            for dt in x.get("details", []):
                if "opening" in dt.get("content", "") and "start_line" in dt:
                    yield d["source_file"], dt["start_line"], dt["start_column"]
                    break
            else:
                # no opener detail: report the diagnostic's own start, which
                # locate() will fail on, so it lands in the skipped list
                yield d["source_file"], x["start_line"], -x["start_column"]


def locate(line, col, ch):
    """0-based index of `ch` at 1-based `col`, skipping leading whitespace
    (Q-2-17's opener span starts at the whitespace before the `~`)."""
    if col < 0:
        return None
    i = col - 1
    while 0 <= i < len(line) and line[i] in " \t":
        i += 1
    if 0 <= i < len(line) and line[i] == ch and (i == 0 or line[i - 1] != "\\"):
        return i
    return None


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("project")
    ap.add_argument("code", choices=sorted(RULES))
    ap.add_argument("--q2", default="q2")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--max-rounds", type=int, default=10)
    args = ap.parse_args()
    ch, _ = RULES[args.code]

    total = 0
    skipped = []
    seen_skips = set()
    for rnd in range(1, args.max_rounds + 1):
        by_file = collections.defaultdict(set)
        for f, ln, col in positions(render(args.project, args.q2), args.code):
            by_file[f].add((ln, col))
        edits = 0
        for f, locs in sorted(by_file.items()):
            p = Path(f)
            lines = p.read_text(newline="").split("\n")
            changed = False
            # right-to-left within a line so earlier inserts don't shift later ones
            for ln, col in sorted(locs, key=lambda t: (t[0], -t[1])):
                line = lines[ln - 1]
                i = locate(line, col, ch)
                if i is None:
                    if (f, ln, col) not in seen_skips:
                        seen_skips.add((f, ln, col))
                        skipped.append(f"{f}:{ln}:{abs(col)}: {line.strip()[:120]}")
                    continue
                if args.dry_run:
                    print(f"{f}:{ln}: {line[max(0, i - 40):i]}[\\{ch}]{line[i + 1:i + 40]}")
                lines[ln - 1] = line[:i] + "\\" + line[i:]
                changed = True
                edits += 1
            if changed and not args.dry_run:
                p.write_text("\n".join(lines), newline="")
        total += edits
        print(f"round {rnd}: {edits} escapes in {len(by_file)} files", file=sys.stderr)
        if args.dry_run or edits == 0:
            break

    print(f"total escapes: {total}", file=sys.stderr)
    if skipped:
        print(f"skipped (expected {ch!r} not found at reported position; fix by hand):", file=sys.stderr)
        for s in skipped:
            print("  " + s, file=sys.stderr)


if __name__ == "__main__":
    main()
