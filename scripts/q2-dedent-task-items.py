#!/usr/bin/env python3
"""Re-indent task-list items whose continuation lines sit under the text.

Notes here often write

    - [x] Did the thing, and the explanation continues on the next
          line, indented six spaces to line up under the text.

          ```sh
          a fenced block, also at six
          ```
          * a sub-list, also at six

The item's content column is 2 (after `- `; the `[x]` is content), so
anything indented six is four past it: a lazy continuation line is fine,
but a paragraph after a blank line, a fenced block or a sub-list becomes
an *indented code block*, which q2 rejects (Q-2-35, or a parse error at
the fence). CommonMark and Pandoc read it the same way; q2 just says so.

This script finds every top-level `- [ ]` / `- [x]` item whose non-blank
continuation lines are ALL indented six or more, and dedents those lines
by four, so the item's content lines up at column 2. Items with any
continuation line below six are left alone (bd-uk8zgkha).

Usage:
  scripts/q2-dedent-task-items.py FILE...          # edit in place
  scripts/q2-dedent-task-items.py --dry-run FILE...
"""
import argparse
import re
import sys
from pathlib import Path

ITEM = re.compile(r"^- \[[ xX]\] ")
FENCE = re.compile(r"^\s*(```|~~~)")


def process(lines):
    """Return (new_lines, number_of_dedented_lines)."""
    out = list(lines)
    i = 0
    n = 0
    while i < len(out):
        if not ITEM.match(out[i]):
            i += 1
            continue
        # The item's continuation block: until the next non-blank line at
        # column 0. A fenced block inside the item is part of it.
        j = i + 1
        fence = False
        while j < len(out):
            line = out[j]
            if FENCE.match(line) and (line.startswith("      ") or fence):
                fence = not fence
            elif not fence and line.strip() and not line.startswith(" "):
                break
            j += 1
        block = out[i + 1:j]
        nonblank = [l for l in block if l.strip()]
        if nonblank and all(l.startswith("      ") for l in nonblank):
            for k in range(i + 1, j):
                if out[k].strip():
                    out[k] = out[k][4:]
                    n += 1
        i = j
    return out, n


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("files", nargs="+")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()
    total = 0
    for f in args.files:
        p = Path(f)
        text = p.read_text(encoding="utf-8")
        lines = text.split("\n")
        new, n = process(lines)
        if n:
            print(f"{f}: {n} lines")
            total += n
            if not args.dry_run:
                p.write_text("\n".join(new), encoding="utf-8")
    print(f"total: {total} lines{' (dry run)' if args.dry_run else ''}")


if __name__ == "__main__":
    main()
