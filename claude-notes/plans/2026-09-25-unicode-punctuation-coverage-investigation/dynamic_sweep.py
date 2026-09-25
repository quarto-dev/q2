#!/usr/bin/env python3
"""Dynamic sweep: parse 'a X b' for every assigned non-ASCII code point with
pampa (tree-sitter's own Unicode tables), bisecting batches that fail.
(Pandoc -f markdown never fails; it folds all of these into Str.)

Usage: dynamic_sweep.py PAMPA_BIN
"""
import subprocess, sys, unicodedata, collections

PAMPA = sys.argv[1]
SKIP = {"Cn", "Co", "Cs", "Cc"}

def doc(cps):
    return "\n\n".join(f"a {chr(cp)}b and a{chr(cp)} b" for cp in cps) + "\n"

def ok(cps):
    r = subprocess.run([PAMPA, "-t", "native"], input=doc(cps).encode(),
                       capture_output=True)
    return r.returncode == 0 and b"Error" not in r.stderr

def bisect(cps, out):
    if ok(cps):
        return
    if len(cps) == 1:
        out.append(cps[0]); return
    m = len(cps) // 2
    bisect(cps[:m], out); bisect(cps[m:], out)

cps = [cp for cp in range(0x80, 0x110000)
       if not (0xD800 <= cp <= 0xDFFF)
       and unicodedata.category(chr(cp)) not in SKIP]
fails = []
for i in range(0, len(cps), 2000):
    bisect(cps[i:i+2000], fails)

by = collections.defaultdict(list)
for cp in fails:
    by[unicodedata.category(chr(cp))].append(cp)
print(f"unicodedata {unicodedata.unidata_version}; tested {len(cps)}; failed {len(fails)}")
for k, v in sorted(by.items(), key=lambda kv: -len(kv[1])):
    print(f"\n## {k}: {len(v)}")
    for cp in v:
        print(f"U+{cp:04X} {chr(cp)} {unicodedata.name(chr(cp), '?')}")
