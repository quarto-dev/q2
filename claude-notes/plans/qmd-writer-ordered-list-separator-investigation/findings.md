---
title: 'Findings: ordered-list separator round trip (bd-2bcre8cr)'
---

Probed on `main` @ `665f9b588` (2026-11-10) with `target/debug/pampa` and
pandoc 3.12.1. The probe is `roundtrip.sh` in this directory, and its
output is in `roundtrip-output.txt`. `pandoc-writer-output.txt` records
what pandoc's own markdown writer emits for the same inputs.

## Round trips through the qmd writer

| # | Input | Written | Re-read | Verdict |
|---|---|---|---|---|
| 1 | `- 1.\n  1)` | `* 1.\n  <!-- -->\n  1)` | `1)` becomes `Plain [Str "1)"]` | **broken** (the strand's case) |
| 2 | `- 1. a\n  1) b` | `* 1.  a\n  <!-- -->\n  1)  b` | the comment becomes a lazy-continuation `RawInline` inside `a` | **broken**, and invisible to the shape-only round-trip test |
| 3 | `- x\n  1. a\n  1)` | blank lines around the separator | `Plain x` becomes `Para x` (the item turned loose) | **broken** |
| 4 | `- x\n  1. a\n  1. b` | tight, no separator | unchanged | ok |
| 5 | `1.\n1)` (top level) | `1.\n\n<!-- -->\n\n1)` | gains a `RawBlock` the source did not have | stable, but not identical |
| 6 | `1. a\n\n<!-- -->\n\n1.` (top level, separator authored) | unchanged | unchanged | ok |
| 7 | `- 1. a\n\n  <!-- -->\n\n  1.` (separator authored inside an item) | `* 1.  a\n  <!-- -->\n  1.` (tight) | **the two lists merge into one**, and the comment becomes inline | **broken, with data loss** |
| 8 | `a\n<!-- c -->\nb` | unchanged | one `Para` with a `RawInline` | ok (by design) |

Case 1 is the strand. Cases 2, 3 and 7 are the same defect class:
writing a bare HTML `RawBlock` tight against a list inside a list item.
The generated corpus can't see case 2, because `check_roundtrip`
compares block *shapes* only, and an absorbed `RawInline` doesn't change
the shape.

## Why: two models meet

- **HTML extent follows pandoc markdown.** The reader's HTML block
  extent is the paragraph (`2026-09-04-block-html-adjacent-markdown-unparsed.md`,
  "Writer separator rule"), and naked HTML is not an authoring form
  (`dev-docs/syntax-notes.md`, "No naked HTML support"). A
  `<!-- -->` line directly after list-item text is therefore a lazy
  continuation line. Pandoc's **markdown** reader agrees on cases 2, 3
  and 8; CommonMark disagrees (it reads type-2 HTML blocks). For the
  empty item in case 1, pandoc markdown reads
  `[OL [[RawBlock]], OL [[]]]` (the comment becomes the first item's
  content), pandoc commonmark reads `[OL, RawBlock, OL]`, and pampa
  reads `[OL, RawBlock, Plain "1)"]`. All three differ.
- **Looseness follows CommonMark.** bd-az53ojvo decided that "CommonMark
  is the target over pandoc-markdown" for loose and tight lists. A blank
  line between two blocks of an item makes the enclosing list loose.

Pandoc's markdown writer separates adjacent ordered lists with a bare
`<!-- -->` surrounded by **blank lines**, even inside tight items (see
`pandoc-writer-output.txt`). That works for pandoc because its markdown
reader's looseness rule leaves `Plain x` alone. In q2 those blank lines
flip the enclosing list to loose (case 3). So no bare-comment separator
is both tight and robust in q2.

A fenced `{=html}` separator is:

````
* 1.  a
  ```{=html}
  <!-- -->
  ```
  1.
````

pampa and `pandoc -f markdown` both read this as
`[OL [[Plain a]], RawBlock, OL [[]]]`.

## When the separator is needed at all

- qmd ordered lists are decimal only (`a. x` is a paragraph), and a
  delimiter change (`1.` → `1)`) starts a new list, as in CommonMark. So
  **a qmd-read AST only ever has adjacent `OrderedList`s whose
  delimiters differ.** The reader splits these again by itself, with no
  separator.
- Adjacent ordered lists with the **same** delimiter come only from
  imported, pandoc-shaped ASTs. Those still need a separator.
- `separate_ordered_lists` in `crates/pampa/src/writers/qmd_prepass.rs`
  inserts the separator between *any* two adjacent ordered lists. Its
  header comment claims that "a qmd-read AST never contains either
  shape", which is false for differing delimiters. The copy-on-trigger
  fast path therefore also fires on ordinary qmd documents.
