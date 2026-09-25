#!/usr/bin/env python3
"""Scan a directory of markdown for inline code spans (CommonMark rule:
closer = run of exactly the opener's length) whose content contains a
backtick run at least as long as the delimiter. Those are the spans the
pre-fix grammar mis-parses. Prints a (delimiter, longest inner run)
histogram plus one example line per class.

Usage: python3 scan-corpus.py claude-notes
"""
import re, sys, collections, pathlib

root = pathlib.Path(sys.argv[1])
span_re = re.compile(r'(?<!`)(`+)(?!`)(.+?)(?<!`)\1(?!`)', re.S)
run_re = re.compile(r'`+')
fence_re = re.compile(r'^(\s{0,3})(`{3,}|~{3,})')

hist = collections.Counter()
example = {}
for path in sorted(root.rglob('*.md')):
    text = path.read_text(encoding='utf-8', errors='replace')
    # Drop fenced code blocks (block structure wins over inline).
    out, fence, lines = [], None, text.split('\n')
    for line in lines:
        m = fence_re.match(line)
        if fence is None and m:
            fence = m.group(2)[0] * len(m.group(2)); out.append(''); continue
        if fence is not None:
            if m and m.group(2)[0] == fence[0] and len(m.group(2)) >= len(fence) and line.strip() == m.group(2):
                fence = None
            out.append(''); continue
        out.append(line)
    prose = '\n'.join(out)
    for m in span_re.finditer(prose):
        delim, body = len(m.group(1)), m.group(2)
        if '\n\n' in body:
            continue
        runs = [len(r.group(0)) for r in run_re.finditer(body)]
        longest = max(runs) if runs else 0
        if longest >= delim:
            key = (delim, longest)
            hist[key] += 1
            if key not in example:
                lineno = prose.count('\n', 0, m.start()) + 1
                example[key] = (str(path), lineno, m.group(0).replace('\n', '\\n')[:100])

print('delim  longest-inner  count  example')
for key in sorted(hist):
    p, l, s = example[key]
    print(f'{key[0]:5}  {key[1]:13}  {hist[key]:5}  {p}:{l}  {s}')
print(f'\ntotal spans affected: {sum(hist.values())}')
