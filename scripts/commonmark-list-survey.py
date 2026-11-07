#!/usr/bin/env python3
"""Cross-check list shapes against pandoc's CommonMark reader (bd-az53ojvo).

The in-tree regression tests (crates/pampa/tests/integration/list_looseness.rs)
use comrak as the CommonMark oracle. This script is the dev-time check that
the oracle agrees with pandoc, and a survey of pampa's qmd reader against
pandoc on the spec's own examples. pandoc 3.x must be on PATH.

  scripts/commonmark-list-survey.py spec
      Every spec.txt example whose pandoc AST contains a list: compare the
      list shape of `pampa -t json` (the qmd reader) with
      `pandoc -f commonmark -t json`, bucketed as agree / looseness mismatch
      / other mismatch / pampa parse error.

  scripts/commonmark-list-survey.py corpus DIR [--jobs N]
      DIR is a dump of the enumerated corpus (run the enumerated test with
      LIST_LOOSENESS_DUMP_DIR=DIR). For each document, on its oracle text
      (the `.sub.md` quote substitute when the document has fenced divs):
      compare `pampa -f commonmark -t json` (comrak) with pandoc, and
      pampa's qmd reader with pandoc.

The shape is the block kinds of the document, recursively, with list items
as nested sequences; Plain vs Para inside items carries the loose/tight
verdict. Inlines and attributes are ignored.
"""

import argparse
import json
import os
import subprocess
import sys
from collections import Counter, defaultdict
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SPEC = REPO / "external-sources" / "commonmark-spec" / "spec.txt"


def shape(blocks):
    out = []
    for b in blocks:
        t = b["t"]
        if t == "BulletList":
            out.append(("BulletList", [shape(i) for i in b["c"]]))
        elif t == "OrderedList":
            out.append(("OrderedList", [shape(i) for i in b["c"][1]]))
        elif t == "BlockQuote":
            out.append(("BlockQuote", shape(b["c"])))
        elif t == "Div":
            out.append(("Div", shape(b["c"][1])))
        elif t == "Figure":
            out.append(("Figure", shape(b["c"][2])))
        else:
            out.append(t)
    return out


def fmt(s):
    parts = []
    for x in s:
        if isinstance(x, tuple):
            name, inner = x
            if name in ("BulletList", "OrderedList"):
                parts.append(f"{name}[{', '.join('[' + fmt(i) + ']' for i in inner)}]")
            else:
                parts.append(f"{name}[{fmt(inner)}]")
        else:
            parts.append(x)
    return ", ".join(parts)


def has_list(s):
    return any(
        isinstance(x, tuple) and (x[0] in ("BulletList", "OrderedList") or has_list(x[1]))
        for x in s
    )


def run_json(cmd):
    """The parsed JSON of a reader command, or None when it fails."""
    p = subprocess.run(cmd, capture_output=True, text=True)
    if p.returncode != 0:
        return None
    try:
        return json.loads(p.stdout)
    except json.JSONDecodeError:
        return None


def pandoc_shape(path):
    doc = run_json(["pandoc", "-f", "commonmark", "-t", "json", str(path)])
    return None if doc is None else shape(doc["blocks"])


def pampa_shape(pampa, path, reader):
    doc = run_json([pampa, "-f", reader, "-t", "json", str(path)])
    return None if doc is None else shape(doc["blocks"])


def classify(expected, actual):
    if actual is None:
        return "parse error"
    if expected == actual:
        return "agree"
    strip = lambda s: fmt(s).replace("Plain", "Para")  # noqa: E731
    return "looseness (Plain vs Para only)" if strip(expected) == strip(actual) else "other shape"


def report(title, rows):
    """rows: (label, text, expected, actual)."""
    counts = Counter()
    examples = defaultdict(list)
    for label, text, expected, actual in rows:
        c = classify(expected, actual)
        counts[c] += 1
        if c != "agree":
            key = (c, fmt(expected), "-" if actual is None else fmt(actual))
            examples[key].append((label, text))
    print(f"== {title}: " + ", ".join(f"{k}: {v}" for k, v in sorted(counts.items())))
    for (c, exp, act), cases in sorted(examples.items()):
        print(f"   {c} x{len(cases)}")
        print(f"      pandoc: {exp}")
        print(f"      other : {act}")
        for label, text in cases[:2]:
            print(f"      e.g. {label}: {text!r}")


def spec_examples():
    fence = "`" * 32 + " example"
    lines = SPEC.read_text(encoding="utf-8").split("\n")
    out, i, n = [], 0, 0
    while i < len(lines):
        if lines[i] == fence:
            n += 1
            i += 1
            md = []
            while lines[i] != ".":
                md.append(lines[i].replace("→", "\t"))
                i += 1
            out.append((n, "\n".join(md) + "\n"))
        i += 1
    return out


def cmd_spec(args, tmp):
    rows = []
    for number, text in spec_examples():
        path = tmp / f"spec-{number}.md"
        path.write_text(text, encoding="utf-8")
        expected = pandoc_shape(path)
        if expected is None or not has_list(expected):
            continue
        rows.append((f"example {number}", text, expected, pampa_shape(args.pampa, path, "markdown")))
    report("spec examples, qmd reader vs pandoc", rows)


def cmd_corpus(args, tmp):
    corpus = Path(args.dir)
    docs = sorted(p for p in corpus.glob("*.md") if not p.name.endswith(".sub.md"))

    def one(path):
        sub = path.with_name(path.stem + ".sub.md")
        oracle_text = sub if sub.exists() else path
        text = oracle_text.read_text(encoding="utf-8")
        expected = pandoc_shape(oracle_text)
        comrak = pampa_shape(args.pampa, oracle_text, "commonmark")
        qmd = pampa_shape(args.pampa, oracle_text, "markdown")
        return (path.stem, text, expected, comrak, qmd)

    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        results = list(pool.map(one, docs))
    missing = [r[0] for r in results if r[2] is None]
    if missing:
        print(f"pandoc failed on {len(missing)} documents: {missing[:5]}")
    results = [r for r in results if r[2] is not None]
    report(
        f"corpus ({len(results)} documents), comrak vs pandoc",
        [(label, text, exp, comrak) for label, text, exp, comrak, _ in results],
    )
    report(
        f"corpus ({len(results)} documents), qmd reader vs pandoc",
        [(label, text, exp, qmd) for label, text, exp, _, qmd in results],
    )


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--pampa", default=str(REPO / "target" / "debug" / "pampa"), help="pampa binary")
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("spec")
    c = sub.add_parser("corpus")
    c.add_argument("dir")
    c.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    args = ap.parse_args()
    import tempfile

    with tempfile.TemporaryDirectory() as d:
        tmp = Path(d)
        if args.cmd == "spec":
            cmd_spec(args, tmp)
        else:
            cmd_corpus(args, tmp)


if __name__ == "__main__":
    sys.exit(main())
