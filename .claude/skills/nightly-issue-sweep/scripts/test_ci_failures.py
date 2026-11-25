#!/usr/bin/env python3
"""Checks for ci_failures.classify() and its log parsing. Run directly:
python3 -I -B test_ci_failures.py (no network, no gh)."""
import importlib.util, sys
import os
spec = importlib.util.spec_from_file_location("cf", os.path.join(os.path.dirname(os.path.abspath(__file__)), "ci_failures.py")); cf = importlib.util.module_from_spec(spec); spec.loader.exec_module(cf)
crates = {"quarto-core": "crates/quarto-core/", "pampa": "crates/pampa/"}
T = "quarto-core::integration marimo::live"
J = "job: Run test suite (ubuntu-latest) / step: Run hub-client tests"
def prs(**kw): return {n: {"autofix": True, "files": f, "failures": list(fl)} for n, (f, fl) in kw.items()}
# 1. same test fails on two PRs that don't touch quarto-core -> suspect
p = {1: {"autofix": True, "files": ["crates/pampa/x.rs"], "failures": [T]},
     2: {"autofix": True, "files": ["crates/quarto-citeproc/y.rs"], "failures": [T, "pampa::integration own::t"]}}
s = cf.classify(p, set(), crates); assert [x["key"] for x in s] == [T], s
assert p[2]["shared"] == [T] and p[2]["own"] == ["pampa::integration own::t"]
# 2. two PRs, but one touches the test's crate -> only 1 untouched -> not suspect
p = {1: {"autofix": True, "files": ["crates/quarto-core/a.rs"], "failures": [T]},
     2: {"autofix": True, "files": ["crates/pampa/x.rs"], "failures": [T]}}
assert cf.classify(p, set(), crates) == []
# 3. on main + one PR (even one touching the crate) -> suspect
p = {1: {"autofix": True, "files": ["crates/quarto-core/a.rs"], "failures": [T]}}
assert len(cf.classify(p, {T}, crates)) == 1
# 4. on main only, no PR -> not reported
assert cf.classify({}, {T}, crates) == []
# 5. coarse job key: 2 PRs not enough, 3 enough
p = {n: {"autofix": False, "files": [], "failures": [J]} for n in (1, 2)}
assert cf.classify(p, set(), crates) == []
p[3] = {"autofix": True, "files": [], "failures": [J]}
s = cf.classify(p, set(), crates); assert s[0]["autofix_prs"] == [3] and s[0]["kind"] == "job"
# 6. nextest log line parsing (ANSI-coloured, as in real logs)
line = "\x1b[31;1m        FAIL\x1b[0m [  30.412s] (15309/20431) \x1b[35;1mquarto-core::integration\x1b[0m \x1b[36mdocs::x\x1b[0m"
m = cf.NEXTEST_RE.match(cf.ANSI_RE.sub("", line)); assert m and m.group(2) == "quarto-core::integration" and m.group(3) == "docs::x"
assert cf.NEXTEST_RE.match("  TRY 1 FAIL [   1.0s] (1/2) pampa::integration a::b")
assert not cf.NEXTEST_RE.match("        PASS [   0.007s] (  443/20431) pampa json::t")
print("all classify tests passed")
