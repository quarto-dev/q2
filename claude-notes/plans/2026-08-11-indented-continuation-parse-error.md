# Fix indented continuation-line parse errors (bd-indented-continuation-parse-error-j7be7kuc)

## Overview

92737cdd (v0.18.0) fixed digit/dash/plus-leading continuation lines
terminating paragraphs, but introduced a P0 regression: the same
continuation lines with *leading indentation* became hard parse errors
that drop the entire file from a render. A 280-cell characterization
sweep (leader × indent 0–10 × context top/bullet/ordered/quote,
pandoc 3.9 as reference) shows 118 regressed cells plus a family of
semantic disagreements. Strand:
bd-indented-continuation-parse-error-j7be7kuc (comment c-4oz3w0zf has
the verified mechanism analysis).

**Mechanism (verified via tree-sitter trace):** when a SOFT_LINE_ENDING
gate peek judges an indented line "prose", the peeked path skips
`mark_end`, leaving the line's leading whitespace unconsumed. If the
next scan's first token is internal (plain text), the internal lexer
produces `_whitespace` — and the grammar state after `_soft_line_ending`
cannot shift `_whitespace` → hard error. Backtick/star peeks survive by
accident (their continuations start with external-scanner tokens that
absorb the whitespace); `block_continuation` rescues in-list cases only
when the indent equals the content column exactly (zero residue).

**Secondary defects (verified):**
- `peek_ordered_marker`'s `indentation > 3` guard reads *raw* columns at
  gate 1 (pre-`match_line`), misjudging legitimate nested markers
  (`1. one` + 4-space `1. nested` must nest per pandoc; it errors).
- `peek_dash_plus_opens_block` has no indent guard at all, so
  over-indented bullets (pandoc: lazy prose continuation) are declared
  block openers and then fail to form a block.
- The gate-1 digit branch sets `first_peeked = true` even when the peek
  bailed without advancing.

## Fix design (two-part, agreed with user 2026-08-11)

**Part A — grammar (the crash class):** `_soft_line_break` in
`grammar.js` gains an optional trailing `_whitespace`:
`seq($._soft_line_ending, optional($.block_continuation), optional($._whitespace))`.
This makes residual continuation-line indentation shiftable everywhere a
soft break can occur, matching pandoc's stripping of continuation-line
leading whitespace (the whitespace lands inside the aliased
`pandoc_soft_break` node → still a single `SoftBreak`). Requires
`tree-sitter generate` + `build` AND regenerating the error-message
table (`crates/pampa/scripts/build_error_table.ts`) because parser
state numbers shift.

**Part B — scanner (the semantics):** make the marker peeks
position-independent at gate 1 and apply the indent guard at gate 2,
where `s->indentation` is post-`match_line` (relative):
- Remove the `s->indentation > 3` early-out from `peek_ordered_marker`
  (pure character-shape peek).
- At gate 2, combine the shape verdict with the *residual* indent:
  marker-opens-block only if shape says so AND residual indent ≤ 3.
  (Both the `first_peeked` shortcut path and gate 2's own peek
  branches.)
- Gate 1 keeps suppressing its soft break whenever the shape says
  "well-formed marker" (it cannot judge indentation), deferring the
  interrupt decision to gate 2 — same division of labor the
  92737cdd commit message already documents.
- Verify `match_line` actually leaves `s->indentation` as the residual
  (post-prefix) count before relying on it; adjust if not.

Expected behavioral outcomes (pandoc parity, from the sweep):
- prose leaders (`-5`, `--`, `+5`, `30 minutes`) at any indent, any
  context → soft-break continuation (fixes 88 of the 118 error cells);
- `- item`/`+ item`/`1. nested` at relative indent ≤ 3 → nested/sibling
  list (fixes e.g. ordered ctx indent 4/6, the Connect-docs case);
- `- item`/`+ item`/`1. nested` at relative indent > 3 → lazy prose
  continuation, NOT a Q-2-35 indented-code error (fixes top ctx
  indent 4/6/10 and friends);
- `*5 stars` cells keep erroring (deliberate qmd unclosed-emphasis
  strictness, all 28 cells, verified standalone).

## Risks / watch items

- Grammar change may create conflicts with `_shortcode_sep` /
  `_attr_ws` / `_inline_whitespace`, which build `choice`/`seq` shapes
  over `_soft_line_break` and `_whitespace`; resolve at generate time,
  simplify those rules if they become ambiguous.
- Error-table regeneration must be re-run after BOTH parts (state
  numbers move again after scanner edits are compiled? scanner is
  runtime — only grammar regen moves states; still regenerate once
  after both parts are in).
- Corpus churn: existing corpus cases that encode the buggy behavior
  must be updated deliberately, and only ones added by 92737cdd or ones
  whose expectations the fix legitimately changes.
- `cargo xtask verify` FULL (WASM leg) required — pampa feeds
  wasm-quarto-hub-client.

## Work items

### Phase 0 — setup
- [x] Bugfix branch `bugfix/bd-indented-continuation-parse-error-j7be7kuc`
- [x] Process note `claude-notes/instructions/scanner-indentation-contexts.md`
- [x] Characterization sweep (280 cells, pandoc vs pampa) — results
      summarized above
- [x] Strand → in_progress; commit setup artifacts (e63f5831)

### Phase 1 — tests first (TDD)
- [x] Table-driven integration test in
      `crates/pampa/tests/integration/test_indented_continuation.rs`:
      6 test fns, 138 cells (prose × indent × context, marker ×
      relative-indent bands, backtick/star controls, `*5` deliberate
      errors)
- [x] tree-sitter corpus cases: new
      `test/corpus/indented-continuation.txt` (10 cases; trees
      generated post-fix via `--update` and hand-reviewed — the
      integration table carried the fail-first burden)
- [x] Failing set recorded pre-fix: 72 prose + 27 over-indent marker +
      3 nesting cells, matching the sweep prediction cell-for-cell;
      controls green pre-fix
- [x] DISCOVERED: pandoc merges `- a` + `+ item` into one list; qmd
      starts a new list (CommonMark). Pre-existing deliberate
      deviation, control only.

### Phase 2 — grammar fix (Part A)
- [x] `_soft_line_break` gains `optional($._whitespace)` under
      `prec.right` (conflict vs `_attr_ws`/`_shortcode_sep` resolved
      toward absorption); regenerated + built
- [x] `tree-sitter test` 572/572 with NO existing-test changes
- [x] Post-Part-A state: all 96 prose cells pass; 18 marker cells
      still error, 3 digit cells parse-as-prose instead of nesting —
      exactly the predicted Part B residue

### Phase 3 — scanner fix (Part B)
- [x] Verified `match` subtracts `list_item_indentation` from
      `s->indentation` → gate 2 sees the residual
- [x] `peek_ordered_marker` now shape-only; gate 1 guards both
      dash/plus and digit branches with
      `s->indentation <= claimable_list_indentation(s) + 3` (new
      helper: leading LIST_ITEM* run of the stack); gate 2 guards with
      residual `<= 3` on both the first_peeked shortcut and its own
      peek branches (skipping the peek on over-indent so the existing
      mark_end absorbs the residue); `first_peeked` now always means
      "the peek advanced"
- [x] `tree-sitter test` 582/582 + full integration table green

### Phase 4 — regeneration + verification
- [x] Regenerated error table (`build_error_table.ts`); only
      `_autogen-table.json` changed (state renumbering); `deno.lock`
      artifact deleted (matches prior cleanup convention)
- [x] `cargo nextest run --workspace`: 11727/11727 passed, ZERO
      snapshot changes
- [ ] `cargo xtask verify` (full, WASM leg)
- [ ] E2E: scratch repros + the Connect-docs repro project
      (`q2 render` on
      `~/repos/github/cscheid/q2-connect-docs/llms-info/repros/indented-continuation-parse-error/`,
      expect 3/3 files, then the real `api/index.qmd` project if
      feasible)
- [ ] Close strand with summary; PR

## Reference: sweep failure structure (pre-fix)

- 146/280 cells PARSE-ERROR; 28 are `*5 stars` (deliberate strictness),
  118 regression.
- Prose leaders: error at every indent ≥ 1 in top/quote; every indent
  ≥ 1 except the exact content column in bullet/ordered.
- Marker leaders: `- item`/`+ item` error at top/quote indent 4+,
  bullet indent 6+, ordered indent 10; `1. nested` additionally errors
  at bullet/ordered indent 4/6 (where pandoc nests).
- Full table: session scratchpad `sweep-results.txt` (regenerate with
  `sweep.sh` if needed).
