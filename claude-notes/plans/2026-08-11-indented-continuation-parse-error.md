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
- [ ] Strand → in_progress; commit setup artifacts

### Phase 1 — tests first (TDD)
- [ ] Table-driven integration test in
      `crates/pampa/tests/integration/` covering the sweep's meaningful
      cells (prose × indent × context, marker × relative-indent bands,
      backtick/star controls), expectations transcribed from pandoc
      with deliberate-deviation cells (`*5`) encoded as expected errors
- [ ] tree-sitter corpus cases for representative cells (indent
      dimension added to paragraph.txt or a new corpus file)
- [ ] Run both; record the failing set matches the sweep (118 cells)

### Phase 2 — grammar fix (Part A)
- [ ] `_soft_line_break` optional `_whitespace`; `tree-sitter generate`
      + `tree-sitter build`; resolve conflicts
- [ ] `tree-sitter test` green (update only tests whose expectations
      the fix legitimately changes; document each)
- [ ] Prose cells of the integration test pass; marker cells still fail
      (expected — Part B)

### Phase 3 — scanner fix (Part B)
- [ ] Verify `s->indentation` residual semantics at gate 2
- [ ] Move indent guard: shape-only peeks at gate 1, residual-indent
      guard at gate 2 (both paths); fix `first_peeked` semantics
- [ ] `tree-sitter test` + full integration table green

### Phase 4 — regeneration + verification
- [ ] Regenerate error table (`build_error_table.ts`), commit autogen
      changes
- [ ] `cargo nextest run --workspace`; review snapshot churn and
      document per snapshot policy
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
