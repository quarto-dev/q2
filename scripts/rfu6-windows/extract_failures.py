#!/usr/bin/env python3
"""RFU-6: turn the raw run logs into a compact "why did these fail" report.

Run on the Windows machine, after run.ps1 has produced its logs:

    uv run scripts/rfu6-windows/extract_failures.py

It reads the newest results/logs/<run>/ directory (or --logs DIR) and writes
results/failure-details-<run>.md, which is small enough to commit and push. It:
  * probes which tools are on PATH (pandoc, typst, quarto, ...) with their versions;
  * groups every failing test in i8-nextest.log by its first panic message, so
    "336 failures" becomes a handful of causes with counts;
  * counts failures per test module;
  * dumps the full (line-clipped) output of the failing tests in the i1, i4 and i6 logs,
    which are the three RFU-6 findings;
  * with --show SUBSTR (repeatable), dumps the full output of any failing test whose
    name contains SUBSTR.

Standard library only; it never runs the tests again.
"""
import argparse
import collections
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
RESULTS = HERE / "results"

# "        FAIL [   1.241s] ( 30/31) crate::binary test::name"
HEADER = re.compile(r"^\s+(FAIL|SIGABRT|SIGSEGV|TIMEOUT|EXECFAIL)\b[^\[]*\[[^\]]*\]\s+\(\s*\d+/\d+\)\s+(\S+)\s+(\S+)")
# a plain header with a leak marker: " FAIL + LEAK [ ..."
HEADER2 = re.compile(r"^\s+(?:FAIL \+ LEAK|TIMEOUT \+ LEAK)\s+\[[^\]]*\]\s+\(\s*\d+/\d+\)\s+(\S+)\s+(\S+)")
ANY_RESULT = re.compile(r"^\s+(?:PASS|SLOW|LEAK|SKIP|FAIL|SIGABRT|SIGSEGV|TIMEOUT|EXECFAIL|RETRY|FLAKY)\b[^\[]*\[")
PANICKED = re.compile(r"panicked at (.+?):(\d+):(\d+):?\s*$")
SUMMARY = re.compile(r"^\s*Summary \[")
CLIP = 300


def clip(s: str, n: int = CLIP) -> str:
    s = s.rstrip("\r\n")
    return s if len(s) <= n else s[:n] + " ...[clipped %d chars]" % (len(s) - n)


def normalize(msg: str) -> str:
    msg = re.sub(r"[A-Za-z]:[\\/][^\s\"']*?\.tmp[A-Za-z0-9]+", "<tmp>", msg)
    msg = re.sub(r"\.tmp[A-Za-z0-9]{4,}", ".tmp<id>", msg)
    msg = re.sub(r"\(\d{3,}\)", "(<tid>)", msg)
    msg = re.sub(r"[A-Za-z]:\\Users\\[^\\]+", r"C:\\Users\\<user>", msg)
    msg = re.sub(r"[0-9a-f]{12,}", "<hash>", msg)
    return msg


def read_lines(path: Path):
    with open(path, "r", encoding="utf-8-sig", errors="replace", newline=None) as f:
        for line in f:
            yield line.rstrip("\n").rstrip("\r")


def parse_nextest(path: Path):
    """Return (blocks, summary). blocks: {testid: {"module", "lines", "msg"}} for the first
    occurrence of each failing test that has output; the repeated FAIL lines nextest prints in
    its final summary have no output and are ignored."""
    blocks = {}
    seen = set()
    summary = []
    cur = None
    for line in read_lines(path):
        m = HEADER.match(line) or HEADER2.match(line)
        if m and (line.lstrip().startswith(("FAIL", "SIGABRT", "SIGSEGV", "TIMEOUT", "EXECFAIL"))):
            tid = m.group(m.lastindex - 1) + " " + m.group(m.lastindex)
            if tid in seen:
                cur = None  # the end-of-run repeat of a failure: no output follows
            else:
                seen.add(tid)
                cur = {"id": tid, "lines": [], "msg": None, "want": 0, "msg_lines": []}
                blocks[tid] = cur
            continue
        if SUMMARY.match(line):
            summary.append(line.strip())
            cur = None
            continue
        if ANY_RESULT.match(line):
            cur = None  # a PASS/SLOW/... line ends the failing test's output
            continue
        if cur is not None:
            cur["lines"].append(line)
            if cur["want"] > 0:
                cur["want"] -= 1
                st = line.strip()
                if st and not st.startswith("note:"):
                    cur["msg_lines"].append(st)
                if cur["want"] == 0:
                    cur["msg"] = " | ".join(cur["msg_lines"])
            elif cur["msg"] is None:
                pm = PANICKED.search(line)
                if pm:
                    cur["where"] = pm.group(1) + ":" + pm.group(2)
                    cur["want"] = 3
    # fall back for failures with no panic line
    for b in blocks.values():
        if b["msg"] is None:
            if b["msg_lines"]:
                b["msg"] = " | ".join(b["msg_lines"])
            else:
                cands = [l.strip() for l in b["lines"]
                         if re.search(r"error|Error|STATUS_|exit code|not found|os error|No such file", l)]
                b["msg"] = ("(no panic line) " + cands[0]) if cands else "(no panic line, no error text)"
        b["module"] = module_of(b["id"])
    return blocks, summary


def module_of(tid: str) -> str:
    binary, name = tid.split(" ", 1)
    head = name.split("::")[0] if "::" in name else name
    return binary + " " + head


def probe_tools():
    rows = []
    for tool, args in [("pandoc", ["--version"]), ("typst", ["--version"]), ("quarto", ["--version"]),
                       ("deno", ["--version"]), ("node", ["--version"]), ("julia", ["--version"]),
                       ("Rscript", ["--version"]), ("python", ["--version"]), ("python3", ["--version"]),
                       ("uv", ["--version"]), ("git", ["--version"]), ("sh", ["--version"]),
                       ("bash", ["--version"]), ("cmd", None)]:
        path = shutil.which(tool)
        ver = ""
        if path and args:
            try:
                r = subprocess.run([path] + args, capture_output=True, text=True, timeout=15)
                ver = (r.stdout or r.stderr).strip().split("\n")[0]
            except Exception as e:  # noqa: BLE001
                ver = "(probe failed: %s)" % e
        rows.append((tool, path or "NOT FOUND", ver))
    return rows


def newest_logs() -> Path:
    base = RESULTS / "logs"
    dirs = sorted([p for p in base.iterdir() if p.is_dir()], key=lambda p: p.stat().st_mtime) if base.exists() else []
    if not dirs:
        sys.exit("no results/logs/<run>/ directory found; run scripts/rfu6-windows/run.ps1 first")
    return dirs[-1]


def dump_block(out, b, limit=80):
    out.append("#### `%s`" % b["id"])
    out.append("")
    out.append("```text")
    for l in b["lines"][:limit]:
        out.append(clip(l))
    if len(b["lines"]) > limit:
        out.append("... [%d more lines]" % (len(b["lines"]) - limit))
    out.append("```")
    out.append("")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--logs", help="a results/logs/<run> directory (default: the newest)")
    ap.add_argument("--show", action="append", default=[], metavar="SUBSTR",
                    help="also dump the full output of failing tests whose name contains SUBSTR")
    a = ap.parse_args()
    logs = Path(a.logs) if a.logs else newest_logs()
    run_id = logs.name
    out = ["# RFU-6 failure details, run `%s`" % run_id, ""]

    out += ["## Tools on PATH", "", "| tool | path | version |", "|---|---|---|"]
    for tool, path, ver in probe_tools():
        out.append("| %s | %s | %s |" % (tool, path.replace("|", "/"), ver.replace("|", "/")))
    interesting = {k: v for k, v in os.environ.items()
                   if re.match(r"(QUARTO|PANDOC|TYPST|CARGO_TARGET|RUST|TMP|TEMP)", k, re.I)}
    out += ["", "Environment variables of interest: " +
            (", ".join("%s=%s" % kv for kv in sorted(interesting.items())) or "none"), ""]

    nextest = logs / "i8-nextest.log"
    if nextest.exists():
        blocks, summary = parse_nextest(nextest)
        out += ["## i8 nextest", ""] + ["- " + s for s in summary] + [""]
        groups = collections.OrderedDict()
        for b in blocks.values():
            groups.setdefault(normalize(b["msg"])[:200], []).append(b)
        out += ["### Failures grouped by first panic message (%d failing tests with output, %d causes)"
                % (len(blocks), len(groups)), ""]
        for key, bs in sorted(groups.items(), key=lambda kv: -len(kv[1])):
            mods = collections.Counter(b["module"] for b in bs)
            out.append("- **%d x** `%s`" % (len(bs), clip(key, 200).replace("`", "'")))
            out.append("  - modules: " + ", ".join("%s (%d)" % (m, n) for m, n in mods.most_common(6)))
            out.append("  - example: `%s`" % bs[0]["id"])
            if bs[0].get("where"):
                out.append("  - at: `%s`" % bs[0]["where"])
        out += ["", "### Failures per module", "", "| failing | module |", "|---:|---|"]
        mc = collections.Counter(b["module"] for b in blocks.values())
        for m, n in mc.most_common():
            out.append("| %d | %s |" % (n, m))
        out.append("")
        if a.show:
            out += ["### Requested tests (--show)", ""]
            for b in blocks.values():
                if any(s in b["id"] for s in a.show):
                    dump_block(out, b)
    else:
        out += ["## i8 nextest", "", "no i8-nextest.log in %s" % logs, ""]

    out += ["## The RFU-6 findings: full output of the failing tests", ""]
    for item in ("i1", "i4", "i6"):
        p = logs / (item + ".log")
        out += ["### %s" % item, ""]
        if not p.exists():
            out += ["no %s.log" % item, ""]
            continue
        blocks, summary = parse_nextest(p)
        out += ["- " + s for s in summary] + [""]
        if not blocks:
            out += ["no failing tests with output in this log", ""]
        for b in blocks.values():
            dump_block(out, b)

    RESULTS.mkdir(exist_ok=True)
    dest = RESULTS / ("failure-details-%s.md" % run_id)
    dest.write_text("\n".join(out) + "\n", encoding="utf-8")
    print("wrote %s (%d bytes)" % (dest, dest.stat().st_size))
    print("next: git add scripts/rfu6-windows/results && git commit -m 'RFU-6: failure details' && "
          "git push origin HEAD:feature/rfu-6-windows-run")
    return 0


if __name__ == "__main__":
    sys.exit(main())
