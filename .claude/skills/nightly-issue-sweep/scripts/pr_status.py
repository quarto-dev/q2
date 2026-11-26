#!/usr/bin/env python3
"""Classify the open `autofix`-labelled PRs for the Repair phase.

Prints one JSON object:

    {"needs_repair": [{"number", "title", "url", "head", "state", "attempts",
                       "ignore_failures"}],
     "blocked_shared": [...], "pending": [...], "green": [...], "needs_human": [...],
     "shared_failures": [...], "ci_notes": [...]}

`state` is one of: pending, green, red, conflicting. A PR needs repair
when it is red or conflicting, has fewer than MAX_ATTEMPTS repair markers,
and is neither labelled autofix:needs-human nor a draft. A maintainer
turns a PR back into a draft to park it; drafts go to `needs_human`
(with note "draft") and the sweep doesn't touch them. Repair agents leave one marker
comment per attempt on the PR:

    <!-- q2-autofix repair n=<K> -->

With --shared, CI failures are also compared across PRs and `main`
(ci_failures.py). `shared_failures` lists the suspect failures. A red PR
whose failures are *all* shared goes to `blocked_shared` instead: repairing
it can't help, and it doesn't use up an attempt. A red PR with some shared
failures gets them in `ignore_failures`, so the repair agent leaves them
alone.

Usage: pr_status.py [--max-attempts 3] [--shared]
"""

import argparse
import json
import os
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ci_failures  # noqa: E402

REPO = "quarto-dev/q2"
REPAIR_RE = re.compile(r"<!--\s*q2-autofix repair n=(\d+)")
FAIL = {"FAILURE", "ERROR", "CANCELLED", "TIMED_OUT", "ACTION_REQUIRED", "STARTUP_FAILURE"}


def run(cmd):
    return subprocess.run(cmd, check=True, capture_output=True, text=True).stdout


def check_state(rollup):
    """pending / green / red from statusCheckRollup (checks + statuses)."""
    if not rollup:
        return "pending"
    states = []
    for c in rollup:
        if c.get("__typename") == "StatusContext":
            states.append({"SUCCESS": "ok", "PENDING": "run", "EXPECTED": "run"}
                          .get(c.get("state"), "fail"))
        elif c.get("status") != "COMPLETED":
            states.append("run")
        elif c.get("conclusion") in FAIL:
            states.append("fail")
        else:
            states.append("ok")  # SUCCESS, NEUTRAL, SKIPPED
    # Wait for every check to finish before calling a PR red: repair needs
    # `gh run rerun --failed` and `--log-failed`, which only work on a
    # finished run, and a merge-and-push mid-run wastes an attempt.
    if "run" in states:
        return "pending"
    return "red" if "fail" in states else "green"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--max-attempts", type=int, default=3)
    ap.add_argument("--shared", action="store_true",
                    help="detect failures shared across PRs and main (slower: reads CI logs)")
    args = ap.parse_args()

    prs = json.loads(run([
        "gh", "pr", "list", "--repo", REPO, "--state", "open", "--label", "autofix",
        "--limit", "100", "--json",
        "number,title,url,headRefName,labels,mergeable,statusCheckRollup,comments,isDraft"]))
    out = {"needs_repair": [], "blocked_shared": [], "pending": [], "green": [],
           "needs_human": [], "shared_failures": [], "ci_notes": []}
    shared = {}
    if args.shared and any(check_state(p["statusCheckRollup"]) == "red" for p in prs):
        root = run(["git", "rev-parse", "--show-toplevel"]).strip()
        analysis = ci_failures.collect(root)
        out["shared_failures"] = analysis["suspects"]
        out["ci_notes"] = analysis["notes"]
        shared = analysis["prs"]
    for pr in prs:
        labels = {l["name"] for l in pr["labels"]}
        attempts = max([int(m) for c in pr["comments"]
                        for m in REPAIR_RE.findall(c["body"] or "")] or [0])
        state = check_state(pr["statusCheckRollup"])
        if pr["mergeable"] == "CONFLICTING":
            state = "conflicting"
        elif pr["mergeable"] == "UNKNOWN" and state == "green":
            state = "pending"  # GitHub hasn't computed mergeability yet
        entry = {"number": pr["number"], "title": pr["title"], "url": pr["url"],
                 "head": pr["headRefName"], "state": state, "attempts": attempts}
        if "autofix:needs-human" in labels:
            out["needs_human"].append(entry)
        elif pr.get("isDraft"):
            out["needs_human"].append({**entry, "note": "draft"})
        elif state in ("red", "conflicting"):
            ci = shared.get(str(pr["number"]), {})
            if ci.get("shared"):
                entry["ignore_failures"] = ci["shared"]
            if state == "red" and ci.get("shared") and not ci.get("own"):
                out["blocked_shared"].append(entry)
            elif attempts >= args.max_attempts:
                out["needs_human"].append({**entry, "note": "attempt cap reached"})
            else:
                out["needs_repair"].append(entry)
        else:
            out[state].append(entry)
    json.dump(out, sys.stdout, indent=1)
    print()


if __name__ == "__main__":
    main()
