#!/usr/bin/env python3
"""Backslash-escape every match of a pattern in the prose of markdown files.

Companion to q2-escape-openers.py for bd-uk8zgkha. That script fixes only
what q2 reports; this one sweeps by pattern, for delimiters that q2 pairs up
SILENTLY when they occur an even number of times in a paragraph (e.g.
`~5 and ~10` renders as a subscript; bd-whitespace-flanked-delimiters-0ncy8bgq).

Prose only. Never touched: YAML front matter, fenced code blocks (``` and
~~~ fences), $$ display math, HTML comments, inline code spans, URLs,
autolinks and link destinations, and characters already escaped.

The match start is the character that gets escaped; use lookarounds for
context. Examples:
  scripts/q2-escape-literal.py claude-notes '(?<!~)~(?!~)'           # single ~ (keeps ~~strike~~)
  scripts/q2-escape-literal.py claude-notes '(?<=\\s)\\*(?=\\s)' --dry-run  # a * b
Files: every *.md under the directory (or the given files); pass --exclude
globs to skip paths, e.g. the nested repro projects.
"""

import argparse
import fnmatch
import re
import sys
from pathlib import Path

FENCE = re.compile(r"^(\s*)(`{3,}|~{3,})")
# Regions inside a line that must not be edited.
PROTECTED = re.compile(
    r"(`+)(?:.*?[^`])?\1(?!`)"          # code span
    r"|<!--.*?-->"                       # one-line HTML comment
    r"|<https?://[^>]*>"                 # autolink
    r"|\]\([^)]*\)"                      # link destination
    r"|https?://\S+"                     # bare URL
)


def sweep(text, pattern):
    lines = text.split("\n")
    out = []
    edits = []
    fence = None      # (indent, fence run) of the open fenced block, if any
    in_front = lines and lines[0].strip() == "---"
    in_math = False
    in_comment = False
    for n, line in enumerate(lines, 1):
        if in_front:
            out.append(line)
            if n > 1 and line.strip() in ("---", "..."):
                in_front = False
            continue
        m = FENCE.match(line)
        if fence is not None:
            # a closer is the same character, at least as long, nothing else on
            # the line, and indented at most 3 columns past the opener
            indent, run = fence
            if m and m.group(2)[0] == run[0] and len(m.group(2)) >= len(run) \
                    and line.strip() == m.group(2) and len(m.group(1)) <= indent + 3:
                fence = None
            out.append(line)
            continue
        if m:
            fence = (len(m.group(1)), m.group(2))
            out.append(line)
            continue
        if line.strip().startswith("$$"):
            if line.strip() != "$$" and line.strip().endswith("$$") and len(line.strip()) > 2:
                out.append(line)      # one-line display math
                continue
            in_math = not in_math
            out.append(line)
            continue
        if in_math:
            out.append(line)
            continue
        if in_comment:
            out.append(line)
            if "-->" in line:
                in_comment = False
            continue
        if "<!--" in line and "-->" not in line[line.index("<!--"):]:
            in_comment = True
            out.append(line)
            continue
        spans = [s.span() for s in PROTECTED.finditer(line)]
        hits = [h.start() for h in pattern.finditer(line)
                if not any(a <= h.start() < b for a, b in spans)
                and (h.start() == 0 or line[h.start() - 1] != "\\")]
        for j in reversed(hits):
            edits.append((n, line[max(0, j - 40):j] + "[\\" + line[j] + "]" + line[j + 1:j + 40]))
            line = line[:j] + "\\" + line[j:]
        out.append(line)
    return "\n".join(out), edits


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("root", help="directory to sweep (all *.md), or a single file")
    ap.add_argument("pattern")
    ap.add_argument("--exclude", action="append", default=[], help="glob relative to root (repeatable)")
    ap.add_argument("--project-excludes", action="store_true",
                    help="also skip the `!` patterns of project.render in <root>/_quarto.yml")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()
    if args.project_excludes:
        cfg = Path(args.root) / "_quarto.yml"
        args.exclude += re.findall(r'^\s*-\s*"!([^"]+)"', cfg.read_text(), re.M)
    pattern = re.compile(args.pattern)
    root = Path(args.root)
    files = [root] if root.is_file() else sorted(root.rglob("*.md"))
    total = 0
    nfiles = 0
    for p in files:
        rel = p.relative_to(root).as_posix() if root.is_dir() else p.name
        if p.is_symlink() or any(fnmatch.fnmatch(rel, g) for g in args.exclude):
            continue
        text = p.read_text(newline="")
        new, edits = sweep(text, pattern)
        if not edits:
            continue
        nfiles += 1
        total += len(edits)
        if args.dry_run:
            for n, ctx in edits:
                print(f"{rel}:{n}: {ctx}")
        else:
            p.write_text(new, newline="")
    print(f"{total} escapes in {nfiles} files" + (" (dry run)" if args.dry_run else ""), file=sys.stderr)


if __name__ == "__main__":
    main()
