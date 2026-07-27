#!/usr/bin/env python3
"""Extract RESULT[0]/AFTER[0] Debug dumps from divergence-debug.txt and strip
source-tracking fields (source_info, attr_source, key_source) so a plain diff
shows only structural differences. Investigation artifact for bd-9fwn1504."""
import re

lines = open('divergence-debug.txt').read().splitlines()
res_start, aft_start = 5218, 10480  # 1-indexed marker lines found by grep
res = lines[res_start - 1:aft_start - 1]
aft = lines[aft_start - 1:]

CLOSERS = {'},', '}', '),', ')', '],', ']'}


def clean(dump):
    out = []
    skip_stack_indent = None
    for l in dump:
        ind = len(l) - len(l.lstrip())
        s = l.strip()
        if skip_stack_indent is not None:
            if ind > skip_stack_indent:
                continue
            if ind == skip_stack_indent and s in CLOSERS:
                skip_stack_indent = None
                continue
            skip_stack_indent = None
        if re.match(r'^(source_info|attr_source|key_source):', s):
            balanced = (s.count('{') == s.count('}')
                        and s.count('(') == s.count(')')
                        and s.count('[') == s.count(']'))
            if balanced:
                continue
            skip_stack_indent = ind
            continue
        out.append(l)
    return out


rc, ac = clean(res), clean(aft)
open('result0-clean.txt', 'w').write('\n'.join(rc) + '\n')
open('after0-clean.txt', 'w').write('\n'.join(ac) + '\n')
print(len(rc), len(ac))
