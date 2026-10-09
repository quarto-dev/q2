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

Usage:
  scripts/claude-notes-plan-frontmatter.py FILE...          # edit in place
  scripts/claude-notes-plan-frontmatter.py --dry-run FILE...
"""
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


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("files", nargs="+")
    args = ap.parse_args()
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
