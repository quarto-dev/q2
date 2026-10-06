#!/usr/bin/env python3
"""For every claude-notes line that holds a code span with an inner backtick
run at least as long as its delimiter (same detection as scan-corpus.py),
parse that single line with two builds of the qmd grammar and report how
many lines carry a tree-sitter ERROR node under each.

Usage: python3 compare-corpus-lines.py <corpus-dir> <old-grammar-dir> <new-grammar-dir>
Each grammar dir is compiled by the tree-sitter CLI into its own parser
cache (TREE_SITTER_LIBDIR), because the CLI otherwise shares one cache
entry per grammar name and both dirs would load the same build. Lines are
parsed with their indentation stripped, so list-continuation lines are
not misread as indented code blocks.
"""
import re, sys, subprocess, tempfile, pathlib, collections, os

root, old_dir, new_dir = (pathlib.Path(p) for p in sys.argv[1:4])
span_re = re.compile(r'(?<!`)(`+)(?!`)(.+?)(?<!`)\1(?!`)', re.S)
run_re = re.compile(r'`+')
fence_re = re.compile(r'^(\s{0,3})(`{3,}|~{3,})')

def prose_lines(text):
    out, fence = [], None
    for line in text.split('\n'):
        m = fence_re.match(line)
        if fence is None and m:
            fence = m.group(2); out.append(''); continue
        if fence is not None:
            if m and m.group(2)[0] == fence[0] and len(m.group(2)) >= len(fence) and line.strip() == m.group(2):
                fence = None
            out.append(''); continue
        out.append(line)
    return out

lines = []  # (path, lineno, text)
for path in sorted(root.rglob('*.md')):
    pl = prose_lines(path.read_text(encoding='utf-8', errors='replace'))
    for i, line in enumerate(pl, 1):
        hit = False
        for m in span_re.finditer(line):
            runs = [len(r.group(0)) for r in run_re.finditer(m.group(2))]
            if runs and max(runs) >= len(m.group(1)):
                hit = True
        if hit:
            lines.append((str(path), i, line))

def has_error(grammar_dir, text):
    with tempfile.NamedTemporaryFile('w', suffix='.md', delete=False) as f:
        f.write(text.lstrip() + '\n'); name = f.name
    env = dict(os.environ, TREE_SITTER_LIBDIR=str(grammar_dir / '.ts-lib'))
    out = subprocess.run(['tree-sitter', 'parse', name], cwd=grammar_dir,
                         capture_output=True, text=True, env=env).stdout
    pathlib.Path(name).unlink()
    return 'ERROR' in out

tally = collections.Counter()
still, regressed = [], []
for path, i, line in lines:
    o, n = has_error(old_dir, line), has_error(new_dir, line)
    tally[(o, n)] += 1
    if n:
        still.append((path, i, line))
    if n and not o:
        regressed.append((path, i, line))
print(f'affected lines: {len(lines)}')
print(f'ERROR under old grammar: {sum(v for (o, _), v in tally.items() if o)}')
print(f'ERROR under new grammar: {sum(v for (_, n), v in tally.items() if n)}')
print(f'fixed (old ERROR, new ok): {tally[(True, False)]}')
print(f'regressed (old ok, new ERROR): {tally[(False, True)]}')
print('\nregressed lines (old ok, new ERROR):')
for path, i, line in regressed:
    print(f'  {path}:{i}: {line.strip()[:110]}')
print('\nlines still erroring under the new grammar (other causes, e.g. bare @, unclosed *):')
for path, i, line in still:
    print(f'  {path}:{i}: {line.strip()[:110]}')
