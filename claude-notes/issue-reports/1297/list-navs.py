"""List the landmark-relevant attributes of every <nav> in an HTML file.

Usage: python3 -I list-navs.py <file.html> [...]

Prints one row per <nav> (id, class, role, aria-label, aria-labelledby),
then every element carrying role="doc-toc". Used for issue #1297.
"""

import sys
from html.parser import HTMLParser


class NavLister(HTMLParser):
    def __init__(self):
        super().__init__()
        self.navs = []
        self.doc_toc = []

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        row = (
            tag,
            a.get("id") or "-",
            (a.get("class") or "-")[:40],
            a.get("role") or "-",
            a.get("aria-label") or "-",
            a.get("aria-labelledby") or "-",
        )
        if tag == "nav":
            self.navs.append(row)
        if a.get("role") == "doc-toc":
            self.doc_toc.append(row)


for path in sys.argv[1:]:
    p = NavLister()
    with open(path, encoding="utf-8") as f:
        p.feed(f.read())
    print(f"== {path}")
    fmt = "{:<4} {:<15} {:<41} {:<8} {:<12} {}"
    print(fmt.format("tag", "id", "class", "role", "aria-label", "aria-labelledby"))
    for row in p.navs:
        print(fmt.format(*row))
    print(f"   role=doc-toc elements: {len(p.doc_toc)}")
