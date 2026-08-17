#!/usr/bin/env python3
"""Throwaway probe used while investigating bd-u2qj4y29.

Reconciles crates/quarto-error-catalog/error_catalog.json against
docs/errors/<subsystem>/<code>.qmd and reports the four problem
classes the eventual tool would report:

  missing    catalog code with no page
  orphan     page whose code is not in the catalog
  misplaced  page in a directory that is not its catalog subsystem
  drift      front-matter title/subsystem/since != catalog
  url-drift  docs_url != https://quarto.org/docs/errors/<subsystem>/<code>

Run from the repo root:  python3 <this file>

This is a *probe*, not the deliverable. The real check belongs in
Rust (see the plan); this file exists so the numbers in the plan
can be reproduced.
"""

import json
import os
import re
import sys

CATALOG = "crates/quarto-error-catalog/error_catalog.json"
DOCS = "docs/errors"
URL_PREFIX = "https://quarto.org/docs/errors"


def front_matter(text):
    m = re.match(r"^---\n(.*?)\n---\n", text, re.S)
    if not m:
        return None
    fm = {}
    for line in m.group(1).splitlines():
        mm = re.match(r"^(\w[\w-]*):\s*(.*)$", line)
        if mm:
            fm[mm.group(1)] = mm.group(2).strip().strip('"')
    return fm


def main():
    catalog = json.load(open(CATALOG))
    pages = {}
    for root, _, files in os.walk(DOCS):
        for f in files:
            if f.startswith("Q-") and f.endswith(".qmd"):
                pages[f[:-4]] = os.path.join(root, f)

    missing = [c for c in catalog if c not in pages]
    orphan = [c for c in pages if c not in catalog]
    misplaced, drift, url_drift = [], [], []

    for code, entry in catalog.items():
        expected_url = f"{URL_PREFIX}/{entry['subsystem']}/{code}"
        if entry.get("docs_url") != expected_url:
            url_drift.append((code, entry.get("docs_url"), expected_url))
        path = pages.get(code)
        if not path:
            continue
        if os.path.basename(os.path.dirname(path)) != entry["subsystem"]:
            misplaced.append((code, path, entry["subsystem"]))
        fm = front_matter(open(path).read()) or {}
        for page_key, cat_key in (
            ("title", "title"),
            ("subsystem", "subsystem"),
            ("since", "since_version"),
        ):
            if fm.get(page_key) != entry.get(cat_key):
                drift.append((code, page_key, fm.get(page_key), entry.get(cat_key)))

    print(f"catalog codes: {len(catalog)}   pages: {len(pages)}")
    for name, rows in (
        ("missing", sorted(missing)),
        ("orphan", sorted(orphan)),
        ("misplaced", misplaced),
        ("drift", drift),
        ("url-drift", url_drift),
    ):
        print(f"\n{name}: {len(rows)}")
        for r in rows:
            print("   ", r)

    return 1 if missing else 0


if __name__ == "__main__":
    sys.exit(main())
