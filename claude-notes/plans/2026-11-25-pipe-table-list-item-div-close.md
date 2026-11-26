---
title: 'tree-sitter-qmd: pipe table ending a list item before a closing div fence'
date: 2026-11-25
description: 'A pipe table that ended a list item inside a fenced div, directly followed by the closing colon fence, gave a parse error. The scanner now re-matches the open blocks on the fence line, so the list item closes before the div does.'
status: in-progress
---

**Braid:** bd-imcsjv3l · **Issue:** <https://github.com/quarto-dev/q2/issues/1333>

## Overview

Current behaviour: this input fails with a parse error at 5:4.

```markdown
::: {.note}
- | c | d |
  |---|---|
  | e | f |
:::
```

The cause is the #206 peek in `scan()` in
`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`. When the line after a
pipe-table row starts with a `:::` run, the scanner emits `LINE_ENDING` early so
that the table terminates. That early exit skips the bookkeeping that the normal
`LINE_ENDING` path at the bottom of `scan()` does: resetting `s->matched` and
entering `STATE_MATCHING`. Without the re-match, the list item stays open, and the
`:::` that should close the div is read as content inside the item.

New behaviour: when the innermost open block is not the fenced div (a list item
in this case), the peek branch does the same bookkeeping before it emits
`LINE_ENDING`. The list item closes, and `:::` closes the div. When the div is
the innermost block (the #206 case), nothing needs re-matching, so that path
does not change and the existing #206 corpus tree stays the same.

## Work items

- [x] Corpus test in `test/corpus/pipe_table.txt` (fails before the fix)
- [x] pampa native snapshot `issue-1333-fenced-div-close-after-list-item-table`
- [x] Scanner fix in the #206 peek branch
- [x] `cargo xtask ts-test` (full corpus), `cargo xtask verify --skip-hub-build`, lint, note render
