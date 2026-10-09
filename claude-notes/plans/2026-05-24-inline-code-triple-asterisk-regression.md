---
title: 'Inline code span with `***` content — parse-error regression'
date: 2026-05-24
description: 'Fixes a parse error where inline code beginning with three asterisks, such as `***`, is rejected because the external scanner emits a token that the grammar does not allow at that position.'
---

**Beads:** bd-qhb2o
**Discovered from:** bd-ilv8p (multi-line inline code spans + math)
**Date:** 2026-05-24

## Symptom

`` `***` `` inside a paragraph fails to parse:

```
triple `***` is broken.
```

```
Error: Parse error
1 │ triple `***` is broken.
  │         ─┬─
  │          ╰─── unexpected character or token here
```

Pandoc (`pandoc -f markdown -t native`) accepts the input and produces
`Code "" [] [] "***"`. Pampa accepted it before commit `38e889ad` (the
bd-ilv8p multi-line code span change) and rejects it after. **This is a
regression introduced by bd-ilv8p, landed on 2026-05-24.**

User encounter: `~/Desktop/daily-log/2026/05/24/test-2.qmd` — note the
filename suggests the user was likely writing prose explaining the
`Q-2-32` "triple star emphasis disallowed" diagnostic and tried to put a
literal `***` inside an inline code span. That is exactly the construct
the parser rejects.

## Reproduction

Minimal:

```bash
echo 'a `***` z' > /tmp/repro.qmd
cargo run --bin pampa -- /tmp/repro.qmd
```

Expected: `[ Para [Str "a", Space, Code ( "" , [] , [] ) "***", Space, Str "z"] ]`
Actual: parse error pointing at the first `*` inside the backticks.

## Characterization

The trigger is *narrow*. Across `` `<content>` `` probes (file per case):

| Inline-code content | Result |
|---|---|
| `*` | OK |
| `**` | OK |
| `***` | **FAIL** |
| `****` | OK |
| `*****` | OK |
| `***x` | **FAIL** |
| `x***` | OK |
| `***x***` | **FAIL** |
| `*x***` | OK |
| `*** ` (trailing space) | **FAIL** |
| ` ***` (leading space) | **FAIL** |
| `_`, `__`, `___` | OK |
| `~~strike~~`, `$x$` | OK |

Pattern: a run of exactly 3 consecutive asterisks at the **start** of
inline-code content (after the opening backtick, possibly with a
leading space) trips the failure. Four or more asterisks are fine.
Triple underscores are fine. A `***` that follows other non-asterisk
content inside the code span (`x***`, `*x***`) is fine.

## Root cause (confirmed by `pampa -v`)

`pampa -v` on ``a `***` z`` shows:

```
process version:0, version_count:1, state:2931, row:0, col:3
lex_external state:53, row:0, column:3
lexed_lookahead sym:_triple_star_error, size:3
detect_error lookahead:_triple_star_error
```

At column 3 (just past the opening backtick) the parser is in state
2931 — the state inside `pandoc_code_span` content. The external
scanner is called, emits `_triple_star_error` for the `***`, and the
parser immediately rejects it (`detect_error lookahead`). TRIPLE_STAR
is *not* in the valid-symbols set for state 2931; the scanner emitted
it anyway. That is a tree-sitter external-scanner contract violation.

Code path: `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`,
`parse_star` (entered at the `*` dispatch in `tree_sitter_qmd_external_scanner_scan`).
Lines 847–850 today:

```c
if (star_count == 3 && !line_end && no_spaces) {
    mark_end(s, lexer);
    EMIT_TOKEN(TRIPLE_STAR);
}
```

This emission is unconditional on `valid_symbols[TRIPLE_STAR]`.
Compare neighbouring branches in the same function (lines 856–864
for THEMATIC_BREAK and LIST_MARKER_STAR) which are correctly gated
on `valid_symbols[…]`.

### Why this didn't fire before bd-ilv8p

Tree-sitter only invokes the external scanner at states where *some*
external token is in the valid set. Before bd-ilv8p, the
`pandoc_code_span` content rule was

```js
alias(repeat1(choice(/[^`]+/, /[`]/)), $.content)
```

— two pure-regex alternatives, no external tokens. The scanner was
never called inside code-span content, so the unconditional
TRIPLE_STAR emission could not fire.

bd-ilv8p added a third alternative,
`alias($._soft_line_break, $.pandoc_soft_break)`. `_soft_line_break`
is an external-scanner token, so SOFT_LINE_BREAK joined the
valid-symbols set in code-span content states, and tree-sitter now
calls the scanner there. The scanner's first `*`-dispatch hits
`parse_star`, sees three asterisks, and emits TRIPLE_STAR —
illegally, because TRIPLE_STAR is not valid in those states.

This explains the characterization table exactly: only the *first*
position after the opening backtick can hit the contract violation,
because that is the only place the external scanner is asked for a
lookahead before any content has been consumed by the regex. Three
consecutive asterisks downstream (`x***`, `*x***`) are absorbed by
the ``/[^`\n\r]+/`` regex during ordinary tokenization, never reaching
the external dispatch on `*`. Four or more consecutive asterisks
(`****`, `*****`) take the same path as three at the start — but
`parse_star` only triggers TRIPLE_STAR for `star_count == 3 &&
no_spaces`. A run of 4+ stars falls through that branch.

### Why the original two-grammar-option plan would not have worked

Both Option A and Option B in the earlier draft of this plan would
have left the `_soft_line_break` alternative in place (it is required
for the multi-line code-span feature). So the external scanner would
still be invoked in code-span content states, and the unconditional
TRIPLE_STAR emission would still fire. Tweaking the *regex* in the
first content alternative does not change which external tokens are
in the valid-symbols set.

### Chosen fix — Option D (scanner-level)

Gate `EMIT_TOKEN(TRIPLE_STAR)` on `valid_symbols[TRIPLE_STAR]`. This
restores the tree-sitter external-scanner contract: the scanner must
only emit tokens the parser asked for. No grammar change required;
the grammar's external-tokens list and the `pandoc_code_span`
content rule both stay as bd-ilv8p left them.

Same fix should be audited for the other emissions in `parse_star`
(LIST_MARKER_STAR is gated; STRONG_EMPHASIS_CLOSE_STAR and
EMPHASIS_CLOSE_STAR are gated; THEMATIC_BREAK is gated; TRIPLE_STAR
is the outlier) and for the EQUALS_THREE Q-2-style emission at
line 2386 which the comment claims uses the "same emission pattern
as TRIPLE_STAR".

## Test plan (TDD, write tests first)

### 1. Failing unit test in pampa pandoc-match corpus

File: `crates/pampa/tests/test_documents/markdown/inline-code-triple-star.qmd`

```
triple `***` inside code span.
```

This should be asserted to match pandoc's `markdown` reader output by
the existing `unit_test_corpus_matches_pandoc_markdown` harness.

Add at least three more fixtures to lock in the surrounding surface:

- `inline-code-triple-star-only.qmd` — content is exactly `` `***` ``
- `inline-code-triple-star-prefix.qmd` — content is `` `***hello` ``
- `inline-code-triple-star-suffix.qmd` — content is `` `hello***` ``
  (already OK today; regression guard)
- `inline-code-quad-star.qmd` — content is `` `****` ``
  (regression guard for the "4+ stars is OK" boundary)

### 2. Tree-sitter corpus case

Add to `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/code_span.txt`:

```
================
Inline code containing triple asterisk
================
a `***` b

----

(document
  (section
    (paragraph
      (inline))))
```

(Update with actual node shape once we re-run `tree-sitter test`. The
key assertion is that the paragraph parses to a single `inline`
without an `ERROR` node.)

### 3. Run the tests, confirm they fail in the expected way

```bash
cd crates/tree-sitter-qmd/tree-sitter-markdown && tree-sitter test
cargo nextest run -p pampa inline-code-triple-star
```

Expected failures:
- Tree-sitter corpus: ERROR node where the `***` appears.
- Pampa fixture: pampa returns a parse error, not the pandoc-matched AST.

## Fix — Option D (scanner-level)

In `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`,
`parse_star`, change

```c
if (star_count == 3 && !line_end && no_spaces) {
    mark_end(s, lexer);
    EMIT_TOKEN(TRIPLE_STAR);
}
```

to

```c
if (valid_symbols[EMPHASIS_OPEN_STAR] &&
    star_count == 3 && !line_end && no_spaces) {
    mark_end(s, lexer);
    EMIT_TOKEN(TRIPLE_STAR);
}
```

### Why `EMPHASIS_OPEN_STAR`, not `TRIPLE_STAR`

The first attempt at this fix gated on `valid_symbols[TRIPLE_STAR]`
and broke Q-2-32 entirely (paragraph-level `***hello` started
producing the generic "unexpected character or token here" instead
of the structured "Triple star emphasis disallowed" diagnostic).

The reason: TRIPLE_STAR is *deliberately* an error-trigger token.
It is **never** declared valid in any parser LR state — its whole
role is to provoke a parse error that the merr-style state→Q-code
table (`resources/error-corpus/_autogen-table.json`) maps to Q-2-32.
Gating on `valid_symbols[TRIPLE_STAR]` therefore suppresses every
emission and turns Q-2-32 off.

A `printf` of `valid_symbols[]` in `parse_star` showed the actual
discriminator. Inside `pandoc_code_span` content:

```
TRIPLE_STAR=0 CODE_SPAN_START=0 CODE_SPAN_CLOSE=0
EMPHASIS_OPEN_STAR=0 STRONG_EMPHASIS_OPEN_STAR=0
SOFT_LINE_ENDING=1 THEMATIC_BREAK=0 LIST_MARKER_STAR=0
```

At document / paragraph start:

```
TRIPLE_STAR=0 CODE_SPAN_START=1 CODE_SPAN_CLOSE=0
EMPHASIS_OPEN_STAR=1 STRONG_EMPHASIS_OPEN_STAR=1
SOFT_LINE_ENDING=1 THEMATIC_BREAK=1 LIST_MARKER_STAR=1
```

`EMPHASIS_OPEN_STAR` is the cleanest discriminator semantically too:
TRIPLE_STAR's whole point is to diagnose an *attempted* `***foo***`
(strong+emphasis), and that interpretation is only meaningful in
states where emphasis can plausibly open. Inside code-span content
emphasis cannot open, so `EMPHASIS_OPEN_STAR` is not in the valid
set; at all other inline-receptive sites (paragraph, span text,
list item, blockquote, pipe-table cell, etc.) it is. The same gate
also harmlessly suppresses TRIPLE_STAR in any future state that
does not admit emphasis opens.

### Why CODE_SPAN_CLOSE doesn't work either

A second attempt gated on `!valid_symbols[CODE_SPAN_CLOSE]` (logic:
"we're not currently inside a code span"). Tracing showed
`CODE_SPAN_CLOSE` is in fact *not* set inside code-span content —
it's a pipe-table-cell-only token here, set only when scanning the
table's `|`-delimited cell. So that gate behaved the same as the
unconditional emission and kept failing.

### Out of scope (audit)

* `INDENTED_CODE_BLOCK_DISALLOWED` (scanner.c around line 2415,
  comment claims "same emission pattern as TRIPLE_STAR") is already
  gated on `valid_symbols[ATX_H1_MARKER] || valid_symbols[BLANK_LINE_START]`,
  which restricts it to block-start positions. No analogous bug.
* No other unconditional `EMIT_TOKEN(_*_ERROR)` patterns in
  scanner.c were found by inspection.

## Work items

- [x] **Reproduction recorded** in this plan.
- [x] **Diagnose with `-v`**: ran `cargo run --bin pampa -- -v
  /tmp/repro.qmd`; confirmed scanner emits TRIPLE_STAR at column 3
  inside `pandoc_code_span` content (LR state 2931) where the token
  is not in valid_symbols. Root cause section updated above.
- [x] **Write failing tree-sitter corpus test** for `` `***` ``,
  `` `***hello` ``, and a `` `****` `` regression guard in
  `tree-sitter-markdown/test/corpus/code_span.txt` (cases 11/12/13).
  `tree-sitter test` reported the two regression cases failing with
  ERROR nodes before the fix.
- [x] **Write failing pampa fixtures**: added
  `inline-code-triple-star-only.qmd`,
  `inline-code-triple-star-prefix.qmd`,
  `inline-code-triple-star-suffix.qmd` (regression guard), and
  `inline-code-quad-star.qmd` (boundary guard) under
  `tests/pandoc-match-corpus/markdown/`. Confirmed
  `unit_test_corpus_matches_pandoc_markdown` panicked on the new
  fixtures pre-fix (offset 3, "unexpected character or token here").
- [x] **Apply Option D fix** in `scanner.c`: gate
  `EMIT_TOKEN(TRIPLE_STAR)` on `valid_symbols[EMPHASIS_OPEN_STAR]`.
  Initial attempt with `valid_symbols[TRIPLE_STAR]` broke Q-2-32 —
  see "Why EMPHASIS_OPEN_STAR" subsection above.
- [x] **No `parser.c` / `_autogen-table.json` regeneration needed**:
  the fix touches `scanner.c` only, not `grammar.js`. Parser-state
  numbers are unchanged, so the autogen table and parser.c stay
  current.
- [x] **Verify tests pass**: tree-sitter corpus (524/524) + pampa
  nextest (3775/3775) + workspace nextest (9425/9425) + `cargo xtask
  verify --skip-hub-build` (all 12 steps green) + full `cargo xtask
  verify` (all 12 steps green; hub-client WASM build succeeded,
  hub-client tests 553+66+60+79 passed, trace-viewer + preview-*
  packages all green). The fix flows cleanly through the WASM
  dependency chain.
- [x] **End-to-end check**: ran pampa on
  `~/Desktop/daily-log/2026/05/24/test-2.qmd` → clean
  `Code "***"` output. Also confirmed:
  - `***hello`, `***foo***`, `hello ***foo***` all still fire
    structured Q-2-32 diagnostics.
  - `a *foo* z` → `Emph`, `a **foo** z` → `Strong` unchanged.
  - bd-ilv8p multi-line code-span fixtures still parse correctly
    (`code\nspan` → `Code "code span"`, blockquote and list
    variants verified).
- [x] **No regression in pipe-table cells**: confirmed by stash-
  compare that the pre-existing "pipe-table cell `***hello` produces
  generic parse error instead of Q-2-32" behavior is unchanged.
  That gap exists in Q-2-32's `prefixesAndSuffixes` corpus and is
  out of scope here.

## Out of scope

- Q-2-32 ("triple star emphasis disallowed") at the *paragraph* level
  should continue to fire — the bug is specifically the bleed of that
  recognition into inline-code-span content. Do not touch top-level
  Q-2-32 behavior.
- Multi-line inline code spans (bd-ilv8p's main feature) should keep
  working. The multi-line fixtures added in bd-ilv8p are the
  regression guard.
