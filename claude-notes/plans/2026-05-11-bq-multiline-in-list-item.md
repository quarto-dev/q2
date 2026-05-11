# Multi-line block quote inside a list item — parser bug

## Overview

The tree-sitter qmd parser fails on a Pandoc-valid construct: a list item
containing a block quote whose paragraph spans multiple lines using the `>`
continuation marker.

Minimal failing input (`test-2.qmd`):

```
- > a
  > b
```

Pandoc reference:

```
[ BulletList
    [ [ BlockQuote [ Para [ Str "a" , SoftBreak , Str "b" ] ] ]
    ]
]
```

Our parser produces a parse error at line 2, column 6 (just past `b`).

`tree-sitter parse` against the raw grammar confirms the failure is in the
grammar layer, not in pampa downstream code:

```
(ERROR [0, 0] - [2, 0]
  (list_marker_minus [0, 0] - [0, 2])
  (block_quote_marker [0, 2] - [0, 4])
  (pandoc_str [0, 4] - [0, 5])
  (pandoc_str [1, 4] - [1, 5]))
```

## Scope

The bug fires for any list marker (`-`, `*`, `+`, `1.`, etc.) when:

- the list item contains a block quote, and
- the block quote's paragraph continues on a second (or later) line, and
- the continuation line uses the explicit `>` continuation marker
  (not lazy continuation).

These all *work* and are sanity checks for the eventual fix:

| Input | Behaviour |
|---|---|
| `- > a` (single-line bq in list) | OK |
| `> > a\n> > b\n` (bq-in-bq, multi-line) | OK |
| `- > a\n  b\n` (lazy continuation) | OK |
| `- > a\n  > # b\n` (heading on line 2) | OK |
| `- > a\n  > > b\n` (extra nesting on line 2) | OK |
| `- > a\n  \n  > b\n` (blank-line-separated paragraphs) | OK |

These all fail:

| Input | Behaviour |
|---|---|
| `- > a\n  > b\n` | parse error at 2:6 |
| `* > a\n  > b\n` | parse error at 2:6 |
| `1. > a\n   > b\n` | parse error at 2:7 |
| `- > a\n  > b\n  > c\n` | parse error at 2:6 |

## Root cause

In `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`:

After line 1 ends, the scanner emits `SOFT_LINE_ENDING` and sets
`STATE_MATCHING | STATE_WAS_SOFT_LINE_BREAK`. The parser then consumes `b`
via the internal lexer. When the scanner is called again at the trailing
`\n` of line 2, `STATE_MATCHING` is still set (tree-sitter rolls back
scanner-state mutations from `scan()` calls that return `false`, so any
intermediate unset during the `b`-lookahead scan does not persist).

The `STATE_MATCHING` block (scanner.c:2040) calls `match_line()`. The first
open block is `LIST_ITEM_2_INDENTATION`. The `match()` function for list
items hits this branch (scanner.c:537–540):

```c
if (lexer->lookahead == '\n' || lexer->lookahead == '\r') {
    s->indentation = 0;
    return 2;   // "blank-line in list item" — keep matching past the \n
}
```

The `case 2` handler in `match_line` (scanner.c:1991–1996) calls
`advance(s, lexer)`, **consuming the `\n` that we needed to recognize as
end-of-line-2**. After `match_line` returns, lookahead is EOF. The
line-ending gate at scanner.c:2233 requires `lookahead == '\n' || '\r'`,
so it does not fire. `scan()` falls through and returns `false`. Tree-sitter
retries with a different lex-state; the scanner emits `_close_block`
(size 0), which has no valid shift at parser state 1381 → `detect_error`.

**Why bq-in-bq works but bq-in-list-item does not.** The `match()` case for
`BLOCK_QUOTE` (scanner.c:553–565) has no `case 2` / no newline branch — it
returns 0 when `>` is absent **without advancing**. So for
`> > a\n> > b\n`, lookahead stays at `\n` after `match_line`, the
line-ending gate fires, `_line_ending` is emitted, and the parse completes.

The LIST_ITEM blank-line branch is a real feature (`- a\n\n  b` needs it),
but it mis-fires when re-matching the LIST_ITEM context at the trailing
`\n` of a *content* line under a leftover `STATE_MATCHING` set by a soft
line break.

## Approach

Option 2 from the assessment: bail out of `STATE_MATCHING` re-entry when
`STATE_WAS_SOFT_LINE_BREAK` is set and lookahead is `\n`. Rationale: the
soft-line-break already accounted for the continuation prefix; re-running
`match_line` against the trailing `\n` of the same logical line is wrong.

The fix is in scanner.c around the `STATE_MATCHING` block (line 2040) —
likely a guard before calling `match_line` that skips the matching pass
when `STATE_WAS_SOFT_LINE_BREAK` is set and lookahead is `\n`/`\r`/EOF, so
the line-ending gate at line 2233 can handle it instead.

## Work items

### Phase 1 — failing corpus tests (TDD)

- [x] Add failing corpus test to
      `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/list.txt`:
      `- > a\n  > b` → expected tree with `pandoc_list > list_item >
      pandoc_block_quote > pandoc_paragraph` containing two `pandoc_str`
      separated by `pandoc_soft_break`.
- [x] Add variants for `*`, `+`, `1.`, `1)` markers.
- [x] Add 3-line case (`- > a\n  > b\n  > c`).
- [x] Run `tree-sitter test` from the
      `crates/tree-sitter-qmd/tree-sitter-markdown` directory and confirm
      each new test fails.
- [x] Add pampa-level integration test (qmd → native AST) that mirrors the
      Pandoc reference output.
- [x] Run `cargo nextest run -p pampa` for the new test and confirm it
      fails.

#### Phase 1 — completion notes

- Tests 24–28 (marker variants) were originally written as bare
  2-line inputs `- > a\n  > b`. They unexpectedly *passed* under
  `tree-sitter test`. Investigation showed the corpus runner does not
  append a trailing newline to the source block, so the scanner takes
  the EOF path (which emits BLOCK_CLOSE cleanly) rather than the
  end-of-line `\n` path that triggers the bug. Pampa always appends a
  trailing newline (Q-7-1), which is why the bug shows up at the user
  level.
- Re-shaped tests 24–28 to follow the multi-line block quote with a
  blank line and a paragraph (`- > a\n  > b\n\nc\n`): that forces the
  scanner through the end-of-line `\n` path. All 6 new corpus tests now
  fail. Total corpus: 476 tests, 470 passing, 6 failing (the new ones).
- pampa fixtures at
  `crates/pampa/tests/pandoc-match-corpus/markdown/bq-multiline-in-list-*.qmd`
  panic at `readers::qmd::read(...).unwrap()` because the parse fails.
  The panic stops the test on the first file, but that is enough to
  confirm pampa-level reproduction; once the fix lands all six
  fixtures will be exercised.
- **Refined bug scope:** the bug requires a trailing `\n` after the
  second block-quote-marked line. With no trailing newline (EOF
  immediately after content), the scanner takes the EOF path before
  the buggy STATE_MATCHING block and emits BLOCK_CLOSE cleanly. The
  user-reported repro hits this because pampa auto-appends a newline
  (Q-7-1). Three-line variants fail regardless of trailing newline,
  because the second line's `\n` triggers the bug before EOF.

### Phase 2 — neighborhood characterization (before touching scanner.c)

- [ ] Enumerate every place where `STATE_MATCHING` is set/unset and every
      call site of `match_line`. Document the invariants the scanner expects
      around `STATE_MATCHING` + `STATE_WAS_SOFT_LINE_BREAK` at each call
      site.
- [ ] Verify the bq-in-bq case-2 absence is genuinely the difference
      (vs. some other coincidence) by adding `SCAN_DEBUG` traces for both
      working and failing inputs at the relevant scan call.
- [ ] Survey existing corpus tests that depend on the LIST_ITEM `case 2`
      behaviour (blank-line-in-list-item) so we have a regression baseline.
- [ ] Write down the proposed guard precisely (condition + which block it
      gates) in this plan document before editing scanner.c.

### Phase 3 — implementation

- [ ] Apply the guard in
      `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`.
- [ ] Confirm the failing corpus tests from Phase 1 now pass.
- [ ] Run the full `tree-sitter test` suite from
      `crates/tree-sitter-qmd/tree-sitter-markdown` and check for
      regressions. Investigate any newly-failing tests case-by-case — do
      *not* update snapshots reflexively.
- [ ] Run `cargo nextest run -p pampa` and check for regressions.
- [ ] Run `cargo nextest run --workspace` to check for cross-crate
      regressions (per CLAUDE.md monorepo rule).

### Phase 4 — end-to-end verification

- [ ] `cargo run --bin pampa -- -i test-2.qmd -t native` on the original
      reporter file. Confirm output matches Pandoc.
- [ ] Same with the marker variants (`*`, `+`, `1.`, ordered with `)`).
- [ ] Same with 3-line and 4-line cases.
- [ ] Compare against `pandoc -t native -i ...` on a handful of
      hand-picked inputs.
- [ ] Record the exact invocations + observed outputs in this plan doc
      before declaring done.

### Phase 5 — close-out

- [ ] `cargo xtask verify --skip-hub-build` (Rust-only changes).
- [ ] If any snapshot files changed, audit them per the CLAUDE.md snapshot
      policy (count, summary, surprising changes).
- [ ] Commit with reference to this beads issue. Do not push.

## Test artifacts

Repro file in repo: `test-2.qmd` (already exists in working tree under
`/Users/cscheid/Desktop/daily-log/2026/05/11/`).

Pandoc reference outputs to assert against:

```
$ printf '%s\n%s\n' '- > a' '  > b' | pandoc -f markdown -t native
[ BulletList
    [ [ BlockQuote [ Para [ Str "a" , SoftBreak , Str "b" ] ] ]
    ]
]
```

## Non-goals

- Not fixing the broader question of how `STATE_MATCHING` interacts with
  speculative scans. The narrow guard is enough for this bug; a wider
  refactor of the scanner state machine is out of scope.
- Not extending lazy-continuation support or any Pandoc-vs-CommonMark
  semantic changes.

## References

- Issue: bd-vet6
- Reporter file: `/Users/cscheid/Desktop/daily-log/2026/05/11/test-2.qmd`
- Original session transcript: 2026-05-11
