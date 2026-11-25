#!/usr/bin/env python3
"""Pick the open quarto-dev/q2 issues worth gating tonight.

Deterministic pre-filter for the nightly-autofix workflow: no model
judgement happens here. Prints one JSON object on stdout:

    {"gate_version": 1,
     "candidates": [{"number", "title", "url", "reason"}],
     "skipped":    [{"number", "title", "reason"}]}

`reason` on a candidate says why it is (re-)eligible: "never gated",
"activity since gate marker", or "gate version bumped".

An issue is skipped when it is assigned, carries an excluded label,
has an open PR that closes it, has a non-closed braid strand that
references it, or carries a current gate marker comment with no human
activity after it. The marker is written by autonomously-fix-small-issue
(see its "Recording a gate failure" section):

    <!-- q2-autofix gate v=<N> verdict=fail checks=a,b -->

Usage: select_candidates.py --gate-version N [--limit K]
"""

import argparse
import json
import re
import subprocess
import sys

REPO_OWNER, REPO_NAME = "quarto-dev", "q2"
EXCLUDED_LABELS = {"question", "wontfix", "duplicate", "invalid", "cannot-repro"}
MARKER_RE = re.compile(r"<!--\s*q2-autofix gate v=(\d+)\s+verdict=(\w[\w-]*)")
ANY_MARKER_RE = re.compile(r"<!--\s*q2-autofix ")

QUERY = """
query($owner: String!, $name: String!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    issues(states: OPEN, first: 50, after: $cursor,
           orderBy: {field: CREATED_AT, direction: ASC}) {
      pageInfo { hasNextPage endCursor }
      nodes {
        number title url lastEditedAt
        assignees(first: 5) { nodes { login } }
        labels(first: 20) { nodes { name } }
        closedByPullRequestsReferences(first: 10, includeClosedPrs: false) {
          nodes { number state }
        }
        comments(last: 100) { nodes { createdAt body } }
      }
    }
  }
}
"""


def run(cmd):
    return subprocess.run(cmd, check=True, capture_output=True, text=True).stdout


def fetch_issues():
    issues, cursor = [], None
    while True:
        cmd = ["gh", "api", "graphql", "-f", f"query={QUERY}",
               "-f", f"owner={REPO_OWNER}", "-f", f"name={REPO_NAME}"]
        if cursor:
            cmd += ["-f", f"cursor={cursor}"]
        page = json.loads(run(cmd))["data"]["repository"]["issues"]
        issues += page["nodes"]
        if not page["pageInfo"]["hasNextPage"]:
            return issues
        cursor = page["pageInfo"]["endCursor"]


def strand_refs():
    """Map issue number -> ids of non-closed braid strands that cite it."""
    refs = {}
    pat = re.compile(r"(?:GH #|q2/issues/)(\d+)\b")
    for line in run(["braid", "export"]).splitlines():
        if not line.strip():
            continue
        s = json.loads(line)
        if s.get("status") == "closed":
            continue
        text = " ".join(str(s.get(k) or "") for k in ("title", "description", "notes"))
        for n in set(pat.findall(text)):
            refs.setdefault(int(n), []).append(s["id"])
    return refs


def classify(issue, gate_version, refs):
    """Return (eligible, reason)."""
    if issue["assignees"]["nodes"]:
        return False, "assigned"
    labels = {l["name"] for l in issue["labels"]["nodes"]}
    if labels & EXCLUDED_LABELS:
        return False, "label " + ",".join(sorted(labels & EXCLUDED_LABELS))
    prs = [p["number"] for p in issue["closedByPullRequestsReferences"]["nodes"]
           if p["state"] == "OPEN"]
    if prs:
        return False, "open PR " + ",".join(f"#{p}" for p in prs)
    if issue["number"] in refs:
        return False, "braid strand " + ",".join(refs[issue["number"]])

    comments = issue["comments"]["nodes"]
    last_marker = None
    for i, c in enumerate(comments):
        m = MARKER_RE.search(c["body"] or "")
        if m:
            last_marker = (i, int(m.group(1)), c["createdAt"])
    if last_marker is None:
        return True, "never gated"
    idx, version, marked_at = last_marker
    human_after = any(not ANY_MARKER_RE.search(c["body"] or "")
                      for c in comments[idx + 1:])
    edited_after = (issue["lastEditedAt"] or "") > marked_at
    if human_after or edited_after:
        return True, "activity since gate marker"
    if version < gate_version:
        return True, f"gate version bumped ({version} -> {gate_version})"
    return False, f"gated v{version}, no activity since"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--gate-version", type=int, required=True)
    ap.add_argument("--limit", type=int, default=0,
                    help="cap the number of candidates (0 = no cap)")
    args = ap.parse_args()

    refs = strand_refs()
    out = {"gate_version": args.gate_version, "candidates": [], "skipped": []}
    for issue in fetch_issues():
        ok, reason = classify(issue, args.gate_version, refs)
        entry = {"number": issue["number"], "title": issue["title"], "reason": reason}
        if ok:
            out["candidates"].append({**entry, "url": issue["url"]})
        else:
            out["skipped"].append(entry)
    if args.limit and len(out["candidates"]) > args.limit:
        dropped = out["candidates"][args.limit:]
        out["candidates"] = out["candidates"][:args.limit]
        out["skipped"] += [{**{k: d[k] for k in ("number", "title")},
                            "reason": "over --limit"} for d in dropped]
    json.dump(out, sys.stdout, indent=1)
    print()


if __name__ == "__main__":
    main()
