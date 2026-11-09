#!/usr/bin/env python3
"""RFU-6 results parser (macOS side).

Reads a results JSON written by run.ps1 and updates the RFU-6 plan file:
  * ticks each checklist box with an evidence mark (ran on Windows / read / not run);
  * rewrites the "Notes from the run" section.

Usage:  uv run scripts/rfu6-windows/parse_results.py [results.json] [--plan PLAN.md] [--dry-run]
Default results file: the newest scripts/rfu6-windows/results/rfu6-*.json.
Standard library only. It never sets `status:`; do that by hand after reading the output.
"""
import argparse
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
DEFAULT_PLAN = REPO / "claude-notes/plans/2026-11-08-rfu-6-windows-run.md"

# Checklist box number (1-based, in file order) -> result item ids.
# Box 3 (the --source-uri default) was ticked by RFU-1 and is left alone.
BOX_ITEMS = {
    1: ["i1"],
    2: ["i2", "i2b"],
    4: ["i4"],
    5: ["i5"],
    6: ["i6"],
    7: ["i7"],
    8: ["i8-clippy", "i8-nextest", "i8-named"],
}
# Box 2 is complete on i2 alone; i2b is optional and only adds evidence.
OPTIONAL = {"i2b"}


def load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8-sig"))  # PowerShell 5.1 writes a BOM


def verdict(items: dict, ids: list) -> tuple:
    """(box_char, mark, detail) for one checklist box."""
    got = [items[i] for i in ids if i in items]
    required = [items[i] for i in ids if i in items and i not in OPTIONAL]
    if not required:
        return " ", "not run", "no result for " + ", ".join(ids)
    real_fail = [i for i in required if i["status"] == "FAIL" and not i.get("known_unrelated")]
    known_fail = [i for i in required if i["status"] == "FAIL" and i.get("known_unrelated")]
    skipped = [i for i in required if i["status"] == "SKIP"]
    caveats = [i for i in required if i["status"] == "PASS" and i.get("caveat")]
    if real_fail:
        return " ", "ran on Windows: FAIL", "; ".join(f"{i['id']}: {i['reason']}" for i in real_fail)
    if skipped:
        return " ", "not run", "; ".join(f"{i['id']}: {i['reason']}" for i in skipped)
    if caveats:
        return " ", "ran on Windows, partial", "; ".join(f"{i['id']}: {i['caveat']}" for i in caveats)
    if known_fail:
        return "x", "ran on Windows", "only known unrelated failures: " + "; ".join(i["id"] for i in known_fail)
    return "x", "ran on Windows", ""


def tick_boxes(plan: str, items: dict) -> tuple:
    lines = plan.split("\n")
    n = 0
    notes = {}
    in_checklist = False
    for idx, line in enumerate(lines):
        if line.startswith("## Checklist"):
            in_checklist = True
        elif line.startswith("## ") and in_checklist:
            in_checklist = False
        m = re.match(r"^- \[([ xX])\] (.*)$", line)
        if not (in_checklist and m):
            continue
        n += 1
        if n not in BOX_ITEMS:
            continue
        ch, mark, detail = verdict(items, BOX_ITEMS[n])
        text = re.sub(r"^\((?:ran on Windows|read|not run)[^)]*\) ", "", m.group(2))
        lines[idx] = f"- [{ch}] ({mark}) {text}"
        notes[n] = (ch, mark, detail)
    return "\n".join(lines), notes


def notes_section(data: dict, items: dict, boxes: dict) -> str:
    out = []
    out.append(f"Run `{data.get('run_id')}` on {data.get('os') or 'Windows'} ({data.get('host_triple')}), "
               f"branch `{data.get('branch')}` at `{str(data.get('commit'))[:9]}`, git `core.autocrlf` = "
               f"`{data.get('autocrlf')}`, python3 with ipykernel on PATH: {data.get('python3_with_ipykernel')}.")
    out.append("")
    out.append("Evidence marks: *ran on Windows* = the item executed on the machine above; *read* = checked "
               "by reading only; *not run* = no result.")
    out.append("")
    out.append("| item | status | what it ran |")
    out.append("|---|---|---|")
    for i in data["items"]:
        st = i["status"] + (" (known unrelated)" if i.get("known_unrelated") else "") + (" (caveat)" if i.get("caveat") else "")
        out.append(f"| {i['id']} | {st} | {str(i['reason']).replace('|', '/').replace(chr(10), ' ')} |")
    out.append("")
    out.append("### What each result proves, and what it does not")
    out.append("")
    for i in data["items"]:
        if i.get("proves") or i.get("does_not_prove"):
            out.append(f"- **{i['id']}**, {i['title']}. Proves: {i.get('proves') or 'n/a'} Does not prove: {i.get('does_not_prove') or 'n/a'}")
    findings = [i for i in data["items"] if i["status"] == "FAIL" and not i.get("known_unrelated")]
    known = [i for i in data["items"] if i["status"] == "FAIL" and i.get("known_unrelated")]
    caveats = [i for i in data["items"] if i.get("caveat")]
    out.append("")
    out.append("### Findings (failures; product code is not fixed in this plan)")
    out.append("")
    if not findings:
        out.append("None beyond the known unrelated failures below.")
    for i in findings:
        out.append(f"- **{i['id']}**, {i['title']}: {i['reason']}")
        out.append("")
        out.append("  ```text")
        for l in (i.get("output_tail") or "").split("\n")[-40:]:
            out.append("  " + l)
        out.append("  ```")
        out.append("")
        out.append("  Next step: *(a plan or strand is proposed for this finding; fill in)*")
    if known:
        out.append("")
        out.append("Known unrelated failures seen (ignored per the instructions): "
                   + "; ".join(f"{i['id']} ({i['reason']})" for i in known))
    if caveats:
        out.append("")
        out.append("Caveats: " + "; ".join(f"{i['id']}: {i['caveat']}" for i in caveats))
    out.append("")
    out.append("### Boxes")
    out.append("")
    for n, (ch, mark, detail) in sorted(boxes.items()):
        out.append(f"- Box {n}: [{ch}] {mark}" + (f". {detail}" if detail else ""))
    return "\n".join(out)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("results", nargs="?")
    ap.add_argument("--plan", default=str(DEFAULT_PLAN))
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()
    if a.results:
        path = Path(a.results)
    else:
        found = sorted((HERE / "results").glob("rfu6-*.json"))
        if not found:
            print("no results/rfu6-*.json found; fetch the branch Gordon pushed first", file=sys.stderr)
            return 2
        path = found[-1]
    data = load(path)
    if "items" not in data:
        print(f"{path}: no items ({data.get('error')}; missing: {data.get('missing')})", file=sys.stderr)
        return 2
    items = {i["id"]: i for i in data["items"]}
    plan_path = Path(a.plan)
    plan = plan_path.read_text(encoding="utf-8")
    plan, boxes = tick_boxes(plan, items)
    head = "## Notes from the run"
    k = plan.index(head)
    plan = plan[:k] + head + "\n\n" + notes_section(data, items, boxes) + "\n"
    complete = all(boxes.get(n, (" ",))[0] == "x" for n in BOX_ITEMS)
    if a.dry_run:
        print(plan)
    else:
        plan_path.write_text(plan, encoding="utf-8")
        print(f"updated {plan_path}")
    print("\nAll boxes ticked: " + ("yes; status: complete is allowed" if complete
          else "NO; leave status: in-progress and hand off (see the unticked boxes above)"), file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
