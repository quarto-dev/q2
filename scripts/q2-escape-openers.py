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
import re
import subprocess
import sys
from pathlib import Path

# code -> (character to escape, where the position comes from)
#   "opener": the detail whose content mentions "opening"; for "unclosed"
#             errors the main start position is the end of the block.
#   "start":  the diagnostic's own start position.
#   "start-1": one column before the diagnostic's start.
RULES = {
    "Q-2-17": ("~", "opener"),   # Unclosed Subscript: `~16`
    "Q-2-16": ("^", "opener"),   # Unclosed Superscript
    "Q-2-7": ("'", "opener"),    # Unclosed Single Quote: `a.rs`'s
    "Q-2-12": ("*", "opener"),   # Unclosed Star Emphasis: O(N * D)
    "Q-2-13": ("**", "opener"),  # Unclosed Strong Star Emphasis
    "Q-2-10": ("'", "opener"),   # Closed Quote Without Matching Open Quote: engines'
    # Uncoded "Parse error" reported just after a bare `@` (`main` @ `sha`);
    # bd-bare-at-literal-w3ytmu8e will make it a literal. Other uncoded
    # parse errors are not this rule's business and are ignored silently.
    "bare-@": ("@", "start-1"),
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
            if where == "start-1":
                if x.get("code") is None:
                    yield d["source_file"], x["start_line"], x["start_column"] - 1
                continue
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


def locate(line, col, delim):
    """0-based index of `delim` at 1-based `col`, skipping leading whitespace
    (some opener spans, e.g. Q-2-17's and Q-2-13's, start at the whitespace
    before the delimiter)."""
    if col < 0:
        return None
    i = col - 1
    while 0 <= i < len(line) and line[i] in " \t":
        i += 1
    if 0 <= i and line[i:i + len(delim)] == delim and (i == 0 or line[i - 1] != "\\"):
        return i
    return None


def escaped(delim):
    return "".join("\\" + c for c in delim)


CODE_SPAN = re.compile(r"(`+)(?:.*?[^`])?\1(?!`)")
CODE_SPAN_BLOCK = re.compile(CODE_SPAN.pattern, re.DOTALL)  # spans may wrap lines


def block_bounds(lines, ln):
    """1-based (start, end) of the block (run of non-blank lines) that
    contains, or else ends before, 1-based line `ln`."""
    e = min(ln, len(lines))
    while e > 1 and lines[e - 1].strip() == "":
        e -= 1
    s = e
    while s > 1 and lines[s - 2].strip() != "":
        s -= 1
    while e < len(lines) and lines[e].strip() != "":
        e += 1
    return s, e


def in_code_span(lines, ln, i):
    """Is 0-based column `i` of 1-based line `ln` inside a code span?

    Code spans are matched over the whole block, not line by line: a span
    that wraps across lines would otherwise pair its closing backtick with
    the next one on that line, so `` `x`'s `` at the start of a line looks
    like code."""
    s, e = block_bounds(lines, ln)
    text = "\n".join(lines[s - 1:e])
    off = sum(len(l) + 1 for l in lines[s - 1:ln - 1]) + i
    return any(a <= off < b for a, b in (m.span() for m in CODE_SPAN_BLOCK.finditer(text)))


def block_escape(lines, ln, pattern):
    """Escape every unescaped match of `pattern` outside code spans in the
    block (run of non-blank lines) that ends at or before 1-based `ln`.

    Fallback for diagnostics with no opener position: the error is reported
    at the end of the block (typical in pipe tables), so the opener is
    somewhere above. Returns the number of escapes."""
    s, e = block_bounds(lines, ln)
    text = "\n".join(lines[s - 1:e])
    spans = [m.span() for m in CODE_SPAN_BLOCK.finditer(text)]
    hits = [m.start() for m in pattern.finditer(text)
            if not any(a <= m.start() < b for a, b in spans)
            and (m.start() == 0 or text[m.start() - 1] != "\\")]
    for j in reversed(hits):
        text = text[:j] + "\\" + text[j:]
    lines[s - 1:e] = text.split("\n")
    return len(hits)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("project")
    ap.add_argument("code", choices=sorted(RULES))
    ap.add_argument("--q2", default="q2")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--max-rounds", type=int, default=10)
    ap.add_argument("--only-context", metavar="REGEX",
                    help="escape only where REGEX matches at the delimiter's position "
                         "(lookbehind sees the rest of the line); other occurrences are "
                         "listed for manual review instead")
    ap.add_argument("--block-pattern", metavar="REGEX",
                    help="for diagnostics without an opener position, escape every match of "
                         "REGEX (outside code spans) in the enclosing block; the match start is "
                         "the escaped character. E.g. '~' for Q-2-17, \"(?<=`)'(?=\\w)\" for Q-2-7")
    args = ap.parse_args()
    block_re = re.compile(args.block_pattern) if args.block_pattern else None
    only_re = re.compile(args.only_context) if args.only_context else None
    review = []
    seen_review = set()
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
                if i is not None and in_code_span(lines, ln, i):
                    # inside a code span a backslash would render literally;
                    # the diagnostic is a knock-on from an earlier error
                    i = None
                if i is None and col < 0 and block_re is not None:
                    n = block_escape(lines, ln, block_re)
                    if n:
                        if args.dry_run:
                            print(f"{f}:{ln}: [block fallback: {n} escapes]")
                        changed = True
                        edits += n
                        continue
                if i is None and RULES[args.code][1] == "start-1":
                    continue
                if i is None:
                    if (f, ln, col) not in seen_skips:
                        seen_skips.add((f, ln, col))
                        skipped.append(f"{f}:{ln}:{abs(col)}: {line.strip()[:120]}")
                    continue
                if only_re is not None and not only_re.match(line, i):
                    if (f, ln, i) not in seen_review:
                        seen_review.add((f, ln, i))
                        review.append(f"{f}:{ln}:{i + 1}: {line[max(0, i - 50):i]}[{ch}]{line[i + len(ch):i + 50]}")
                    continue
                if args.dry_run:
                    print(f"{f}:{ln}: {line[max(0, i - 40):i]}[{escaped(ch)}]{line[i + len(ch):i + 40]}")
                lines[ln - 1] = line[:i] + escaped(ch) + line[i + len(ch):]
                changed = True
                edits += 1
            if changed and not args.dry_run:
                p.write_text("\n".join(lines), newline="")
        total += edits
        print(f"round {rnd}: {edits} escapes in {len(by_file)} files", file=sys.stderr)
        if args.dry_run or edits == 0:
            break

    print(f"total escapes: {total}", file=sys.stderr)
    if review:
        print(f"needs review ({len(review)}; --only-context did not match):", file=sys.stderr)
        for r in review:
            print("  " + r, file=sys.stderr)
    if skipped:
        print(f"skipped (expected {ch!r} not found at reported position; fix by hand):", file=sys.stderr)
        for s in skipped:
            print("  " + s, file=sys.stderr)


if __name__ == "__main__":
    main()
