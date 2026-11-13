---
title: "Fenced code bodies: strip fence indentation, keep blank lines in list items"
date: 2026-11-06
---

**Strand:** `bd-fenced-code-list-indent-wsirymk4` (P1 bug)
**Source:** two reports from the Connect docs migration (50 blocks on 22 pages; 5 blocks on 5 pages).

## Summary

Two separate defects in how pampa extracts the body of a fenced code block.
Both are in the reader's Rust layer, in
`crates/pampa/src/pandoc/treesitter_utils/code_fence_content.rs`.
I plan to fix both there, without changing the tree-sitter scanner.

1. **Fence indentation is never stripped.** CommonMark and pandoc say: if
   the opening fence is indented N spaces past where its container's
   content starts, remove up to N spaces from each body line. pampa removes
   nothing.
2. **Blank lines inside a fenced block in a list item are dropped.** The
   scanner emits one `block_continuation` token that runs over the blank
   line *and* the next line's indentation. The body extractor throws away
   every continuation token, so it also throws away that line break.

## Reproduction

Every case below was run with `target/debug/pampa -t native` and checked
against `pandoc -f markdown -t native` (pandoc 3.x; Quarto 1 uses the same
reader).

| # | Input (body shown with `·` = space) | pampa | pandoc |
|---|---|---|---|
| A | `1. Run:` / blank / `····` + fence + `bash` / `····chmod 750 x.sh` / `····` + fence | `" chmod 750 x.sh"` | `"chmod 750 x.sh"` |
| C | top level: `··` + fence / `··indented` / `····more` / `··` + fence | `"  indented\n    more"` | `"indented\n  more"` |
| B | `1. The output:` / blank / `···` + fence / `···Authentication provider: ldap` / blank / `···GUID  USERNAME` / `···` + fence | `"Authentication provider: ldap\nGUID  USERNAME"` | `"…ldap\n\nGUID  USERNAME"` |
| E | like B with **two** blank lines | `"a\nb"` | `"a\n\n\nb"` |
| N | B nested in a blockquote (`>` on the blank line) | `"a\nb"` | `"a\n\nb"` |
| P | B with an executable `{python}` cell | `"a = 1\nprint(a)"` | blank line kept |
| G | B with a `-` bullet item | `"a\nb"` | `"a\n\nb"` |
| D | top-level fence with a blank line (control) | `"x\n\ny"` ✓ | `"x\n\ny"` |
| F | blockquote fence with a `>` blank line (control) | `"a\n\nb"` ✓ | `"a\n\nb"` |

Case P matters most: blank lines also disappear from **executable cells**
inside list items, so the AST changes the code itself, not only how it looks.

### What the parse trees show

For case B (`pampa -v`):

```
pandoc_code_block (2,3)-(7,0)
  fenced_code_block_delimiter (2,3)-(2,6)
  block_continuation         (3,0)-(3,3)
  code_fence_content         (3,3)-(6,3)
    block_continuation       (4,0)-(5,3)   <- covers the "\n" of blank row 4
    block_continuation       (6,0)-(6,3)
```

The cause is the `LIST_ITEM` arm of `match()` in
`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c` (around line 641).
When it reaches a line ending, it returns `2`, "start again from the next
line", and the lexer has already advanced past the `\n`. The
`BLOCK_CONTINUATION` token emitted afterward therefore contains the line
break. In paragraphs and other blocks the line break carries no meaning,
which is why this went unnoticed. In code it is content.

For case A, the opening delimiter token is `(2,3)-(2,7)`. It covers
the extra space plus the three backticks, so **the fence's extra indentation sits inside the delimiter
token**. The body lines start at column 3, right after the list item's
continuation. That leftover space is never removed. Top level (case C) works
the same way: the delimiter is `(0,0)-(0,5)` and the continuations have zero
width.

## Fix design

All changes go in `process_code_fence_content`. It already walks the
`block_continuation` children and builds the body from the bytes between
them.

### Part 1: keep the line endings inside continuations (bug 2)

When the walk skips a `block_continuation`, copy the line-terminator bytes
inside its range (`\n` or `\r\n`, as written) into the body. Copy nothing
else. Per CommonMark, a blank or whitespace-only line shorter than the
container indent becomes an empty body line, which this produces.

- This fixes B, E, N, P and G. It changes nothing for D and F, because their
  continuations contain no line breaks.
- Copying CRLF as written keeps internal CRLF intact, which is what
  `bd-xyn4kk3k` asks for. That strand's trailing-CR pop in
  `fenced_code_block.rs` is separate and stays out of scope.

**Why not fix the scanner?** The return-2 path is shared by every block
type, and a comment there marks it as the fix for earlier broken behavior. A
scanner change would ripple through the paragraph, list-looseness, and
tiling logic, and through the whole tree-sitter test corpus. The body
extractor is the only consumer where the line break inside the token
carries meaning. I'll leave a comment at the scanner site pointing to this
plan.

### Part 2: strip the fence indentation (bug 1)

1. Find the fence indentation N. Take the opening
   `fenced_code_block_delimiter`, the first such child of
   `node.parent()`, and count the leading spaces in its bytes. A tab counts
   up to the next multiple of 4 columns, matching the scanner's `advance()`.
2. After building the body as in Part 1, remove up to N columns of leading
   whitespace from each line (CommonMark §4.5: "up to N spaces … if
   present"). Lines with less indentation lose what they have. Empty lines
   stay empty.
3. Raw blocks (` ```{=html} `) use the same function, so they get the same
   fix.

Edge cases the tests must cover: N=0 (no change; fast path), tab
indentation, a body line indented less than N, a fence inside a nested
container (the delimiter's leading bytes count only the excess past the
container, which is the N we want), and a body line made only of
whitespace.

### Open question: whitespace-only lines deeper than the content column

In the table below, `w.qmd` is a list item with content column 3 whose body
has a line of 5 spaces.

| pampa (after the fix) | pandoc |
|---|---|
| `"a\n··\nb"` (CommonMark-correct) | `"a\n\nb"` |

Pandoc's markdown reader blanks that line. Strict CommonMark keeps the 2
spaces. I propose **not** copying pandoc here: the difference can't be seen
in rendered output, and keeping the bytes is the safer default for code.
Tell me if you'd rather match Quarto 1 exactly.

## Downstream consumers to check

- **`quarto-core` `crossref/codeblock_shorthand.rs::body_source_for`** finds
  the body by searching for `cb.text` in the block's source. Inside list
  items this search already fails today, because continuations are removed.
  When it fails, it falls back to the whole block's span. After the fix it
  will also fail for top-level indented fences, and fall back the same way.
  That's acceptable. The proper fix is `bd-lm75ion7`, which is to carry body
  provenance on `CodeBlock`. Add a test that shows the fallback is still
  sound.
- **qmd writer round-trip.** A body that now contains blank lines must write
  back inside a list item and parse again to the same AST. The roundtrip
  tests cover this; add a case for it.
- **Engines.** Engine cell extraction works on raw text (see the
  `bd-ts-engine-display-fence` and `bd-breakquartomd-*` strands), so the
  engines' view of a cell doesn't change. What changes is the AST
  `CodeBlock` used for echo and display. I'll confirm with a `q2 render` of a
  list-item `{python}` cell, or note it if the engine path is unaffected.
- **Source-info tiling auditor.** The `CodeBlock` `source_info` still covers
  the whole block node, so nothing changes. I'll still run the
  tiling-corpus tests.
- **Snapshots.** Some existing snapshots may encode the buggy output. Review
  each change one by one. Don't blanket-accept them.

## Implementation steps (TDD)

1. **Failing tests first.** Add cases A, B, C, E, N and G to
   `crates/pampa/tests/pandoc-match-corpus/markdown/`. That corpus asserts
   pampa matches pandoc's reader. Leave the open-question case out of the
   corpus. Add unit tests in `code_fence_content.rs` (or
   `tests/integration/`) for tabs, a body line indented less than N, CRLF,
   and a raw block. Confirm they fail.
2. Implement Part 1, then confirm B, E, N and G pass.
3. Implement Part 2, then confirm A and C pass.
4. Add a qmd round-trip case and a `body_source_for` fallback test.
5. Add a short comment at the scanner's return-2 site pointing to this plan.
6. Run `cargo xtask verify` bare (no pipe). Run
   `q2 render --strict` on this note before committing.

## Out of scope / noticed in passing

- **Tilde fences (`~~~`) are not parsed as code blocks at all.** The
  `~~` is read as a strikeout opener and raises Q-2-18. That could be a
  deliberate qmd restriction. I haven't checked, so I'm not filing it here
  until I do.
- `bd-xyn4kk3k`: a lone trailing CR on CRLF fences. It's related but
  separate.
- `bd-lm75ion7`: body provenance on `CodeBlock`.
