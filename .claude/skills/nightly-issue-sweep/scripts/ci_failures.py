#!/usr/bin/env python3
"""Find CI failures shared across unrelated PRs ("shared failures").

When one bad or flaky test breaks many PRs at once, the test is the
problem, not the PRs. This script samples finished, failed CI runs from:

  - the open `autofix` PRs (always),
  - a few recently updated non-autofix PRs, and
  - the latest few commits on `main`,

extracts the failing tests from each run's `--log-failed` output, and
marks a failure *suspect* when:

  - it fails on `main` and on at least one PR, or
  - it is a nextest test and fails on >= 2 PRs whose diffs don't touch the
    test's crate, or
  - it is a coarse job/step key (non-nextest job) and fails on >= 3 PRs.

Failure keys:
  - nextest:  "<crate>::<binary> <test path>", e.g.
              "quarto-core::integration docs_examples::docs_jupyter_span_sees_cell_state"
  - other:    "job: <job name> / step: <failed step>"

Used as a library by pr_status.py (`collect()`), or run directly to print
the analysis as JSON:

    ci_failures.py [--max-runs 20] [--other-prs 8] [--main-commits 3]
"""

import argparse
import glob
import json
import os
import re
import subprocess
import sys

REPO = "quarto-dev/q2"
ANSI_RE = re.compile(r"(\x1b|\^\[)\[[0-9;]*m")
# nextest result lines: "FAIL [  30.412s] (15309/20431) crate::bin test::path"
NEXTEST_RE = re.compile(
    r"^\s*(?:TRY \d+ )?(FAIL|SIGSEGV|SIGABRT|SIGKILL|TIMEOUT|LEAK-FAIL|ABORT) "
    r"\[\s*[\d.]+s\]\s+(?:\(\s*\d+/\d+\)\s+)?(\S+)\s+(\S+)\s*$")

# CI step names that run nextest (.github/workflows/test-suite.yml).
RUST_TEST_STEP_RE = re.compile(r"Test Rust|nextest", re.I)


def gh(args, parse=True):
    out = subprocess.run(["gh", *args], check=True, capture_output=True, text=True).stdout
    return json.loads(out) if parse else out


def crate_dirs(root):
    """Map crate name -> repo-relative dir, from crates/*/Cargo.toml."""
    out = {}
    for toml in glob.glob(os.path.join(root, "crates", "*", "Cargo.toml")):
        with open(toml) as f:
            m = re.search(r'^name\s*=\s*"([^"]+)"', f.read(), re.M)
        if m:
            out[m.group(1)] = os.path.relpath(os.path.dirname(toml), root) + "/"
    return out


def failed_runs_for_sha(sha):
    runs = gh(["api", f"repos/{REPO}/actions/runs?head_sha={sha}&per_page=50",
               "--jq", "[.workflow_runs[] | {id, status, conclusion, name}]"])
    return [r for r in runs if r["status"] == "completed" and r["conclusion"] == "failure"]


def run_failures(run_id):
    """Failure keys for one finished, failed run."""
    jobs = gh(["run", "view", str(run_id), "--repo", REPO, "--json", "jobs"])["jobs"]
    failed_jobs = {j["name"]: [s["name"] for s in j.get("steps", []) if s.get("conclusion") == "failure"]
                   for j in jobs if j.get("conclusion") == "failure"}
    if not failed_jobs:
        return set()
    try:
        log = gh(["run", "view", str(run_id), "--repo", REPO, "--log-failed"], parse=False)
    except subprocess.CalledProcessError:
        log = ""
    tests_by_job = {}
    for line in log.splitlines():
        parts = line.split("\t", 2)
        if len(parts) < 3:
            continue
        job, _, rest = parts
        rest = ANSI_RE.sub("", rest)
        rest = re.sub(r"^\S+Z\s", "", rest)  # timestamp
        m = NEXTEST_RE.match(rest)
        if m:
            tests_by_job.setdefault(job, set()).add(f"{m.group(2)} {m.group(3)}")
    # One key per failed step. The log doesn't say which step a line came
    # from, so when nextest FAIL lines were found, they stand in for the Rust
    # test step(s) ("Test Rust code") and the job's other failed steps keep
    # their own keys.
    keys = set()
    for job, steps in failed_jobs.items():
        tests = tests_by_job.get(job, set())
        keys |= tests
        for step in steps or ["?"]:
            if tests and RUST_TEST_STEP_RE.search(step):
                continue
            keys.add(f"job: {job} / step: {step}")
    return keys


def is_nextest_key(key):
    return not key.startswith("job: ")


def key_crate(key):
    return key.split(" ", 1)[0].split("::", 1)[0] if is_nextest_key(key) else None


def collect(root, max_runs=20, other_prs=8, main_commits=3):
    """Return the shared-failure analysis as a dict (see module docstring)."""
    crates = crate_dirs(root)
    notes, budget = [], [max_runs]

    def sample(sha):
        keys = set()
        for r in failed_runs_for_sha(sha):
            if budget[0] <= 0:
                notes.append(f"run cap {max_runs} reached; later runs not sampled")
                break
            budget[0] -= 1
            keys |= run_failures(r["id"])
        return keys

    fields = "number,title,headRefOid,labels,files,isDraft,updatedAt"
    autofix = gh(["pr", "list", "--repo", REPO, "--state", "open", "--label", "autofix",
                  "--limit", "50", "--json", fields])
    others = [p for p in gh(["pr", "list", "--repo", REPO, "--state", "open", "--limit", "40",
                             "--json", fields])
              if not any(l["name"] == "autofix" for l in p["labels"])]
    others = sorted(others, key=lambda p: p["updatedAt"], reverse=True)[:other_prs]

    prs = {}
    for p in autofix + others:  # autofix first, so they get the run budget
        prs[p["number"]] = {
            "autofix": p in autofix, "title": p["title"],
            "files": [f["path"] for f in p.get("files") or []],
            "failures": sorted(sample(p["headRefOid"])),
        }

    main_shas = []
    for r in gh(["run", "list", "--repo", REPO, "--branch", "main", "--limit", "40",
                 "--json", "headSha,status,event"]):
        if r["event"] == "push" and r["status"] == "completed" and r["headSha"] not in main_shas:
            main_shas.append(r["headSha"])
        if len(main_shas) >= main_commits:
            break
    main_fail = set()
    for sha in main_shas:
        main_fail |= sample(sha)

    suspects = classify(prs, main_fail, crates)
    for p in prs.values():
        del p["files"]
    return {
        "suspects": suspects,
        "prs": {str(n): p for n, p in prs.items()},
        "main": {"commits": main_shas, "failures": sorted(main_fail)},
        "runs_sampled": max_runs - budget[0],
        "notes": sorted(set(notes)),
    }


def classify(prs, main_fail, crates):
    """Pure part of collect(): mark suspects, and split each PR's failures
    into `shared` and `own` in place. prs: {number: {autofix, files,
    failures}}; main_fail: set of keys; crates: {name: dir/}."""
    by_key = {}
    for n, p in prs.items():
        for k in p["failures"]:
            by_key.setdefault(k, []).append(n)
    for k in main_fail:
        by_key.setdefault(k, [])

    suspects = []
    for k, nums in sorted(by_key.items()):
        on_main = k in main_fail
        crate = key_crate(k)
        cdir = crates.get(crate) if crate else None
        untouched = [n for n in nums
                     if cdir is None or not any(f.startswith(cdir) for f in prs[n]["files"])]
        if is_nextest_key(k):
            suspect = (on_main and bool(nums)) or len(untouched) >= 2
        else:
            suspect = (on_main and bool(nums)) or len(nums) >= 3
        if suspect:
            suspects.append({
                "key": k, "kind": "nextest" if is_nextest_key(k) else "job",
                "prs": sorted(nums), "autofix_prs": sorted(n for n in nums if prs[n]["autofix"]),
                "untouched_prs": sorted(untouched), "on_main": on_main,
            })
    suspect_keys = {s["key"] for s in suspects}
    for p in prs.values():
        p["shared"] = [k for k in p["failures"] if k in suspect_keys]
        p["own"] = [k for k in p["failures"] if k not in suspect_keys]
    return suspects


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--max-runs", type=int, default=20)
    ap.add_argument("--other-prs", type=int, default=8)
    ap.add_argument("--main-commits", type=int, default=3)
    a = ap.parse_args()
    root = subprocess.run(["git", "rev-parse", "--show-toplevel"], check=True,
                          capture_output=True, text=True).stdout.strip()
    json.dump(collect(root, a.max_runs, a.other_prs, a.main_commits), sys.stdout, indent=1)
    print()


if __name__ == "__main__":
    main()
