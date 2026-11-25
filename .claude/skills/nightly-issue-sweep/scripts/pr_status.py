#!/usr/bin/env python3
"""Classify the open `autofix`-labelled PRs for the Repair phase.

Prints one JSON object:

    {"needs_repair": [{"number", "title", "url", "head", "state", "attempts"}],
     "pending": [...], "green": [...], "needs_human": [...]}

`state` is one of: pending, green, red, conflicting. A PR needs repair
when it is red or conflicting, has fewer than MAX_ATTEMPTS repair markers,
and is not labelled autofix:needs-human. Repair agents leave one marker
comment per attempt on the PR:

    <!-- q2-autofix repair n=<K> -->

Usage: pr_status.py [--max-attempts 3]
"""

import argparse
import json
import re
import subprocess
import sys

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
    args = ap.parse_args()

    prs = json.loads(run([
        "gh", "pr", "list", "--repo", REPO, "--state", "open", "--label", "autofix",
        "--limit", "100", "--json",
        "number,title,url,headRefName,labels,mergeable,statusCheckRollup,comments"]))
    out = {"needs_repair": [], "pending": [], "green": [], "needs_human": []}
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
        elif state in ("red", "conflicting"):
            if attempts >= args.max_attempts:
                out["needs_human"].append({**entry, "note": "attempt cap reached"})
            else:
                out["needs_repair"].append(entry)
        else:
            out[state].append(entry)
    json.dump(out, sys.stdout, indent=1)
    print()


if __name__ == "__main__":
    main()
