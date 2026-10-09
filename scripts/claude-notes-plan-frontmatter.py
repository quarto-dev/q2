#!/usr/bin/env python3
"""Give claude-notes plans `title:` and `date:` front matter.

Most plans start with an ATX `# Heading` and carry their date only in the
filename (`YYYY-MM-DD-slug.md`). Listings need both in front matter, and
q2 does not fall back to the first heading for a listing title
(bd-pnajor0b), so this script lifts them there (bd-fvcip3t5):

- `title:` is the text of the leading `# H1`, written as a single-quoted
  YAML scalar so backslash escapes (`\\@`, `\\*`) survive unchanged. The
  H1 line (and the blank lines after it) is removed: the title block
  renders the title, and keeping the heading would show it twice.
- `date:` is the filename's `YYYY-MM-DD` prefix, or, for an undated
  filename, the date of the commit that added the file.

Existing front matter is kept; only missing keys are added. A file whose
first block is not a plain `# H1` (no H1, something before it, attributes
on the heading) or that already has `title:` keeps its body unchanged
and is reported, so a human can decide.

With `--descriptions JSON`, it instead adds `description:` to each file
named in JSON (a list of {"path", "description"} objects; descriptions are
written by a model, see the plan for bd-fvcip3t5). The key goes right after
`date:` (or at the end of the front matter); a file that already has one
is reported and left alone.

With `--header-meta JSON --skein SKEIN.json`, it moves the plan header
lines `**Status:**`, `**Braid:**`/`**Strand:**`/`**Beads:**`/`**Issue:**`,
and `**Date:**`/`**Created:**`/`**Updated:**` into front matter. JSON is a
list of objects a model extracted from each plan (see the plan for
bd-fvcip3t5):

    {"path", "status": {"value", "original", "lines"},
     "strand": {"id", "note", "lines"},
     "created": {"value", "lines"}, "updated": {"value", "lines"}}

Every field is checked before anything is written: `status.value` must be
in STATUSES, `strand.id` must be an exact id in SKEIN (the output of
`braid list --all --json`), dates must be ISO `YYYY-MM-DD`, `created`
must equal the front matter `date:`, and every line in `lines` must occur
verbatim in the body header (before the first `## `). A field that fails
is reported and its lines stay in the body. Priority and labels come from
the skein, not from the plan text. The result:

    status: done  # <the original Status text>
    braid:
      strand: bd-xxxx  # <note>
      priority: P2
      labels: [ci, release]

Usage:
  scripts/claude-notes-plan-frontmatter.py FILE...          # edit in place
  scripts/claude-notes-plan-frontmatter.py --dry-run FILE...
  scripts/claude-notes-plan-frontmatter.py --descriptions items.json [--dry-run]
  scripts/claude-notes-plan-frontmatter.py --header-meta items.json --skein skein.json [--dry-run]
"""
import json
import argparse
import os
import re
import subprocess
import sys

DATE_RE = re.compile(r"^(\d{4}-\d{2}-\d{2})-")
H1_RE = re.compile(r"^# +(.*?)(?: +#+)? *$")


def split_front_matter(text):
    """Return (front-matter lines without delimiters, or None; body)."""
    lines = text.split("\n")
    if not lines or lines[0].rstrip() != "---":
        return None, text
    for i in range(1, len(lines)):
        if lines[i].rstrip() in ("---", "..."):
            return lines[1:i], "\n".join(lines[i + 1:])
    return None, text


def has_key(fm_lines, key):
    return any(re.match(rf"^{re.escape(key)}\s*:", l) for l in fm_lines)


def yaml_single_quote(s):
    return "'" + s.replace("'", "''") + "'"


def git_added_date(path):
    out = subprocess.run(
        ["git", "log", "--diff-filter=A", "--follow", "--format=%as", "--", path],
        capture_output=True, text=True, check=True,
    ).stdout.split()
    return out[-1] if out else None


def process(path, dry_run):
    """Return (status, message). status is 'changed', 'unchanged' or 'report'."""
    with open(path, encoding="utf-8") as f:
        text = f.read()
    fm, body = split_front_matter(text)
    fm = fm if fm is not None else []
    notes = []
    add = []

    # Title: the leading H1, if the body's first block is one.
    body_lines = body.split("\n")
    first = next((i for i, l in enumerate(body_lines) if l.strip()), None)
    h1 = H1_RE.match(body_lines[first]) if first is not None else None
    if has_key(fm, "title"):
        notes.append("already has title:")
    elif h1 is None:
        what = body_lines[first][:60] if first is not None else "<empty body>"
        notes.append(f"first block is not a '# H1': {what!r}")
    elif re.search(r"\{[^}]*\}\s*$", h1.group(1)):
        notes.append(f"H1 has attributes: {h1.group(1)!r}")
    else:
        add.append(f"title: {yaml_single_quote(h1.group(1).strip())}")
        rest = first + 1
        while rest < len(body_lines) and not body_lines[rest].strip():
            rest += 1
        body_lines = body_lines[rest:]

    if not has_key(fm, "date"):
        m = DATE_RE.match(os.path.basename(path))
        date = m.group(1) if m else git_added_date(path)
        if date:
            add.append(f"date: {date}")
            if not m:
                notes.append(f"date {date} from git (undated filename)")
        else:
            notes.append("no date in filename and none in git")

    if not add:
        return ("report" if notes else "unchanged"), "; ".join(notes)

    # New keys go first so title/date lead the block.
    new = "---\n" + "\n".join(add + fm) + "\n---\n\n" + "\n".join(body_lines)
    if not dry_run:
        with open(path, "w", encoding="utf-8") as f:
            f.write(new)
    msg = ", ".join(a.split(":")[0] for a in add)
    if notes:
        return "report", f"added {msg}; " + "; ".join(notes)
    return "changed", f"added {msg}"


def add_description(path, description, dry_run):
    with open(path, encoding="utf-8") as f:
        text = f.read()
    fm, body = split_front_matter(text)
    if fm is None:
        return "report", "no front matter"
    if has_key(fm, "description"):
        return "report", "already has description:"
    desc = " ".join(description.split())
    line = f"description: {yaml_single_quote(desc)}"
    at = next((i + 1 for i, l in enumerate(fm) if re.match(r"^date\s*:", l)), len(fm))
    fm = fm[:at] + [line] + fm[at:]
    if not dry_run:
        with open(path, "w", encoding="utf-8") as f:
            f.write("---\n" + "\n".join(fm) + "\n---\n" + body)
    return "changed", "added description"


STATUSES = ("draft", "approved", "in-progress", "blocked", "done",
            "superseded", "abandoned")
ISO_RE = re.compile(r"^\d{4}-\d{2}-\d{2}$")


def yaml_plain_ok(s):
    return re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", s) is not None


def comment(text):
    text = " ".join((text or "").split())
    return f"  # {text}" if text else ""


def add_header_meta(it, skein, dry_run):
    path = it["path"]
    with open(path, encoding="utf-8") as f:
        text = f.read()
    fm, body = split_front_matter(text)
    if fm is None:
        return "report", ["no front matter"]
    blines = body.split("\n")
    header_end = next((i for i, l in enumerate(blines) if l.startswith("## ")), len(blines))
    header = blines[:header_end]
    notes, remove, add = [], [], []

    def lines_ok(field, lines):
        missing = [l for l in lines if l not in header]
        if missing:
            notes.append(f"{field}: line not found verbatim: {missing[0][:60]!r}")
            return False
        return True

    fm_date = next((l.split(":", 1)[1].strip() for l in fm if l.startswith("date:")), None)
    created = it.get("created") or {}
    if created.get("lines"):
        if created.get("value") != fm_date:
            notes.append(f"created {created.get('value')!r} != date {fm_date!r}; kept in body")
        elif lines_ok("created", created["lines"]):
            remove += created["lines"]

    updated = it.get("updated") or {}
    if updated.get("value"):
        if has_key(fm, "date-modified"):
            notes.append("already has date-modified:")
        elif not ISO_RE.match(updated["value"]):
            notes.append(f"updated {updated['value']!r} is not ISO")
        elif lines_ok("updated", updated.get("lines", [])):
            add.append(f"date-modified: {updated['value']}")
            remove += updated.get("lines", [])

    status = it.get("status") or {}
    if status.get("value"):
        if has_key(fm, "status"):
            notes.append("already has status:")
        elif status["value"] not in STATUSES:
            notes.append(f"status {status['value']!r} not in vocabulary")
        elif lines_ok("status", status.get("lines", [])):
            orig = status.get("original") or ""
            same = " ".join(orig.split()).strip(" .").lower() == status["value"]
            add.append(f"status: {status['value']}" + ("" if same else comment(orig)))
            remove += status.get("lines", [])

    strand = it.get("strand") or {}
    if strand.get("id"):
        sid = strand["id"]
        if has_key(fm, "braid") or has_key(fm, "beads"):
            notes.append("already has braid:/beads:")
        elif sid not in skein:
            notes.append(f"strand {sid!r} not in skein")
        elif lines_ok("strand", strand.get("lines", [])):
            s = skein[sid]
            add.append("braid:")
            add.append(f"  strand: {sid}" + comment(strand.get("note")))
            if s.get("priority") is not None:
                add.append(f"  priority: P{s['priority']}")
            labels = s.get("labels") or []
            if labels:
                add.append("  labels: [" + ", ".join(
                    l if yaml_plain_ok(l) else yaml_single_quote(l) for l in labels) + "]")
            remove += strand.get("lines", [])

    if not add and not remove:
        return ("report" if notes else "unchanged"), notes
    # Insert after description: (or date:), keep the rest of the block.
    at = next((i + 1 for i, l in enumerate(fm) if l.startswith("description:")), None)
    if at is None:
        at = next((i + 1 for i, l in enumerate(fm) if l.startswith("date:")), len(fm))
    dm = [a for a in add if a.startswith("date-modified:")]
    rest = [a for a in add if not a.startswith("date-modified:")]
    if dm:
        d_at = next(i + 1 for i, l in enumerate(fm) if l.startswith("date:"))
        fm = fm[:d_at] + dm + fm[d_at:]
        at += 1 if d_at < at else 0
    fm = fm[:at] + rest + fm[at:]
    drop = set(remove)
    header = [l for l in header if l not in drop]
    # Collapse the blank runs the removals leave behind.
    out = []
    for l in header:
        if not l.strip() and (not out or not out[-1].strip()):
            continue
        out.append(l)
    new_body = "\n".join(out + blines[header_end:])
    if not dry_run:
        with open(path, "w", encoding="utf-8") as f:
            f.write("---\n" + "\n".join(fm) + "\n---\n\n" + new_body.lstrip("\n"))
    return ("report" if notes else "changed"), notes


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--descriptions", metavar="JSON")
    ap.add_argument("--header-meta", metavar="JSON")
    ap.add_argument("--skein", metavar="JSON")
    ap.add_argument("files", nargs="*")
    args = ap.parse_args()
    if args.header_meta:
        with open(args.skein, encoding="utf-8") as f:
            skein = {s["id"]: s for s in json.load(f)}
        with open(args.header_meta, encoding="utf-8") as f:
            items = json.load(f)
        counts = {"changed": 0, "report": 0, "unchanged": 0}
        for it in items:
            status, notes = add_header_meta(it, skein, args.dry_run)
            counts[status] += 1
            for n in notes:
                print(f"{it['path']}: {n}")
        print(", ".join(f"{k} {v}" for k, v in counts.items()), file=sys.stderr)
        return
    if args.descriptions:
        with open(args.descriptions, encoding="utf-8") as f:
            items = json.load(f)
        counts = {"changed": 0, "report": 0}
        for it in items:
            status, msg = add_description(it["path"], it["description"], args.dry_run)
            counts[status] += 1
            if status == "report":
                print(f"{it['path']}: {msg}")
        print(f"changed {counts['changed']}, reported {counts['report']}", file=sys.stderr)
        return
    counts = {"changed": 0, "unchanged": 0, "report": 0}
    for path in args.files:
        if os.path.islink(path):
            continue
        status, msg = process(path, args.dry_run)
        counts[status] += 1
        if status == "report":
            print(f"{path}: {msg}")
    print(f"changed {counts['changed']}, reported {counts['report']}, "
          f"unchanged {counts['unchanged']}", file=sys.stderr)


if __name__ == "__main__":
    main()
