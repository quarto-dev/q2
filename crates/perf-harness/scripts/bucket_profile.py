#!/usr/bin/env python3
"""Bucket samply samples by crate/module and by nearest quarto_core stage; report callers of a target.

Companion to analyze_profile.py (which ranks individual symbols by self time).
This script answers the coarser questions a whole-project profile raises:

  * which *crate / subsystem* owns the self time (grass, tree-sitter, pampa,
    malloc, memmove, syscalls, ...),
  * which *pipeline stage* the samples fall under (leaf-most
    `quarto_core::stage::stages::<name>` / `pipeline::` / `project::` frame),
  * for one target substring (default `grass_compiler`), its inclusive share
    and the nearest non-plumbing caller frames.

Usage:
  bucket_profile.py <profile.json.gz> [target-substring]

Requires the `--unstable-presymbolicate` sidecar next to the profile, like
analyze_profile.py. Written for bd-fq44dlnm (2026-09-13 Connect-docs profile).
"""
import gzip, json, re, sys
from collections import Counter
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
from analyze_profile import load_syms, resolve_address, default_syms_path

prof = Path(sys.argv[1]); target = sys.argv[2] if len(sys.argv) > 2 else "grass_compiler"
modules = load_syms(default_syms_path(prof))
profile = json.load(gzip.open(prof, "rt"))

BUCKETS = [("grass (SCSS)", r"grass_compiler|quarto_sass|codemap::"), ("tree-sitter", r"^ts_|tree_sitter"),
  ("pampa", r"^pampa::|<pampa::"), ("quarto_core", r"quarto_core"), ("quarto_yaml", r"quarto_yaml"),
  ("quarto_source_map", r"quarto_source_map"), ("quarto_pandoc_types", r"quarto_pandoc_types"),
  ("scraper/html5ever", r"scraper|html5ever|ego_tree|markup5ever"), ("regex", r"regex_automata|regex::|aho_corasick"),
  ("serde/json", r"serde_json|serde::"), ("hash", r"core::hash|sip::|sha2|RandomState|hash_one"),
  ("alloc/clone/drop", r"alloc::|drop_in_place|__rdl_alloc|RawVec|finish_grow|String as core::clone|Vec<.*> as core::clone"),
  ("libsystem_malloc", r"^\[libsystem_malloc"), ("memmove/memset/memcmp", r"_platform_mem|__bzero|memchr"),
  ("kernel (syscalls)", r"^\[libsystem_kernel"), ("lua/mlua", r"mlua|lua_|luaV_|luaH_")]
STAGE_RE = re.compile(r"quarto_core::(?:stage::stages::|pipeline::|project::)([A-Za-z0-9_]+)")

self_b = Counter(); incl_target = 0; callers = Counter(); stage_incl = Counter(); total = 0
for th in profile["threads"]:
    strings = th["stringArray"]; sframe = th["stackTable"]["frame"]; sprefix = th["stackTable"]["prefix"]
    ffunc = th["frameTable"]["func"]; fname = th["funcTable"]["name"]; fres = th["funcTable"].get("resource")
    rname = th.get("resourceTable", {}).get("name", [])
    def sym(fi):
        func = ffunc[fi]; raw = strings[fname[func]]; mod = ""
        if fres is not None:
            r = fres[func]
            if r is not None and 0 <= r < len(rname): mod = strings[rname[r]]
        if raw.startswith("0x") and mod in modules:
            s, ok = resolve_address(modules, mod, raw)
            if ok: return s
            return f"[{mod}] {raw}"
        if raw.startswith("0x"): return f"[{mod}] {raw}"
        return raw
    for s in th["samples"]["stack"]:
        if s is None: continue
        total += 1
        stack = []
        while s is not None:
            stack.append(sym(sframe[s])); s = sprefix[s]
        leaf = stack[0]
        for name, pat in BUCKETS:
            if re.search(pat, leaf): self_b[name] += 1; break
        else: self_b["other"] += 1
        if any(target in f for f in stack):
            incl_target += 1
            # nearest caller outside the target and generic plumbing
            for f in stack:
                if target not in f and not re.search(r"^(alloc::|core::|<alloc|<core|std::|__rustc|\[libsystem|_platform|hashbrown|codemap|quarto_sass::|<grass)", f):
                    callers[f[:150]] += 1; break
        for f in stack:
            m = STAGE_RE.search(f)
            if m: stage_incl[m.group(1)] += 1; break
        else: stage_incl["(no quarto_core frame)"] += 1

print(f"total samples: {total}")
print(f"\n== self-time by bucket ==")
for k, v in self_b.most_common(): print(f"{100*v/total:6.2f}%  {v:6d}  {k}")
print(f"\n== inclusive: any '{target}' frame on stack: {incl_target} ({100*incl_target/total:.1f}%) ==")
print(f"nearest non-{target} caller frames:")
for k, v in callers.most_common(12): print(f"{100*v/total:6.2f}%  {v:6d}  {k}")
print(f"\n== inclusive by nearest quarto_core stage/module frame (leaf-most) ==")
for k, v in stage_incl.most_common(25): print(f"{100*v/total:6.2f}%  {v:6d}  {k}")
