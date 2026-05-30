# bd-1lpkx — Fix Q-2-12 "opening '*' mark" diagnostic pointing at the preceding word

**Issue:** bd-1lpkx
**Date:** 2026-05-30
**Status:** implemented — verification in progress

## Overview

For an unclosed single-star emphasis, the `This is the opening '*' mark.`
diagnostic detail underlines the **word preceding** the `*`, not the `*`
delimiter itself. Example:

```
 1 │ hello world *baz
   │       ──┬──     ┬
   │         ╰──────── This is the opening '*' mark.   ← points at "world"
   │                 ╰── I reached the end of the block …
```

The main / closing location is correct; only the opening-mark detail is
mislocated.

## Root cause (confirmed by evidence, not the original triage guess)

This is a **corpus-data bug**, not a code bug in `find_matching_token`.

The error-message machinery (Jeffery-style "errors from examples") records,
for each capture, the `(lr_state, sym)` of the tree-sitter token that sits at
the capture's `(row, column, size)` in the example. At runtime,
`find_matching_token` (crates/quarto-parse-errors/src/error_generation.rs:78)
walks the consumed tokens in reverse and picks the most recent token with that
exact `(lr_state, sym)`.

The template `crates/pampa/resources/error-corpus/Q-2-12.json` `"simple"` case
has:

```json
{ "name": "simple", "content": "a *",
  "captures": [{ "label": "emphasis-start", "row": 0, "column": 0, "size": 1 }] }
```

But in `"a *"` the tokens are:

| token                | row | col | size | lrState | sym                  |
| -------------------- | --: | --: | ---: | ------: | -------------------- |
| `a`                  |   0 |   0 |    1 |    1179 | `pandoc_str_token1`  |
| ` *` (leading space) |   0 |   1 |    2 |     799 | `emphasis_delimiter` |

So the capture at `column 0, size 1` matched the **`a` text-run token**, and
the generated table recorded `(lrState 1179, pandoc_str_token1)` for the
`emphasis-start` capture — across **every** `simple-N` prefix/suffix variant
(verified in `_autogen-table.json`). At runtime `find_matching_token` therefore
resolves `emphasis-start` to the most recent text run before the error — i.e.
the preceding word. That is exactly the observed off-by-`len(preceding word)`
pattern.

### Why the other constructs are fine (corrects the issue's scope guesses)

Verified end-to-end with `printf … | pampa -t html`:

- **Q-2-13 (`**`) WORKS.** `foo **bar` correctly underlines `**`. Its template
  content is `"**"`, so the capture at column 0 lands on the
  `strong_emphasis_delimiter`, and the runtime error state (2351) is covered.
  The issue's "likely same machinery" is **incorrect** — no change needed.
- **Q-2-5 (`_`) WORKS in the simple case.** `hello world _baz` correctly emits
  and points the `_` opening note (it records `emphasis_delimiter`). The issue's
  "emits NO opening detail" did **not** reproduce on a plain paragraph; if a
  real gap exists it is a context-specific *coverage* problem, tracked
  separately — out of scope here.
- **Q-2-17 (`~`), Q-2-16 (`^`), Q-2-7/10/11 (quotes)** all record the actual
  delimiter token and are accurate.

The single working Q-2-12 case, `stray-ending-star` (content `"foo* a "`,
capture `column 3, size 2` = `"* "`), already demonstrates the correct pattern:
point the capture at the 2-char `emphasis_delimiter` token (delimiter + adjacent
space) and let the note's `trimLeadingSpace`/`trimTrailingSpace` flags collapse
the span onto the `*`.

## The fix

In `crates/pampa/resources/error-corpus/Q-2-12.json`, change the `"simple"`
case capture to point at the `emphasis_delimiter` token instead of the
preceding `a`:

```diff
   "name": "simple",
   "content": "a *",
   "captures": [
-    { "label": "emphasis-start", "row": 0, "column": 0, "size": 1 }
+    { "label": "emphasis-start", "row": 0, "column": 1, "size": 2 }
   ],
```

The note already declares `trimLeadingSpace: true` and `trimTrailingSpace: true`,
so the displayed span trims the leading space of `" *"` and the caret lands on
the `*`. This mirrors the already-correct `stray-ending-star` case exactly.

### Why this resolves the runtime case

`hello world *baz` reaches error **state 1671**; its `*` is the
`emphasis_delimiter` token at `(row 0, col 11, size 2)` with `lrState 799` —
the *same* `lrState 799` seen in the `"a *"` example. After the fix, the
regenerated table entry for state 1671 carries capture
`(lrState 799, emphasis_delimiter)`, so `find_matching_token` resolves to the
`" *"` token, and `trimLeadingSpace` yields the caret at column 12 — the `*`.
Both the example and the real input share `lrState 799`, which is what makes the
example-based match transfer.

## Test plan (TDD — write first, watch it fail)

Add a focused regression test (it does not depend on snapshot regeneration) in
`crates/pampa/tests/integration/` (register in `main.rs`, keep alphabetized).
Drive the real reader path used by the binary:

```rust
// parse "hello world *baz\n", expect Err(diagnostics)
let diags = pampa::readers::qmd::read(b"hello world *baz\n", false, "t.qmd",
                                      &mut std::io::sink(), true, None).unwrap_err();
let q2_12 = diags.iter().find(|d| d.code.as_deref() == Some("Q-2-12")).unwrap();
let opening = &q2_12.details[0];               // "This is the opening '*' mark."
let loc = opening.location.as_ref().unwrap();
assert_eq!(loc.start_offset(), 12);            // the '*', not "world"
```

Also assert a second input to lock the pattern, e.g. `foo *bar\n` →
`start_offset == 4`. (Both currently land inside the preceding word and will
fail pre-fix; `12`/`4` are the `*` byte positions.)

Verify it FAILS first (`details[0]` start offset will be inside the preceding
word), then apply the corpus fix + regenerate, then confirm it PASSES.

## Work items

- [x] Add failing regression test (`hello world *baz` → offset 12; `foo *bar` →
      4) routed through `pampa::readers::qmd::read`. Registered in
      `tests/integration/main.rs` (`test_emphasis_opening_mark`). Confirmed it
      failed pre-fix (reported byte 6 = start of "world").
- [x] Edit `Q-2-12.json` `"simple"` capture: `column 0, size 1` → `column 1, size 2`.
- [x] Regenerate via `./scripts/build_error_table.ts` (clean; only pre-existing
      Q-2-7 single-quote duplicate warnings).
- [x] **Diff scope verified:** only `Q-2-12` entries changed in
      `_autogen-table.json` (per-code shasum); entry count stable at 680; no
      `case-files/` or `snapshots/` churn.
- [x] All 32 regenerated `Q-2-12` captures now record `emphasis_delimiter`
      (0 `pandoc_str_token1` left); state 1671 carries `lrState 799`.
- [x] Regression test passes.
- [x] End-to-end via binary — caret underlines the `*` for `hello world *baz`
      and `foo *bar`.
- [x] No error-corpus snapshots changed (Q-2-12 is `.json`; snapshot tests glob
      top-level `*.qmd` only).
- [x] `cargo nextest run -p pampa` — 3776 passed, 2 skipped.
- [x] `cargo nextest run --workspace` — 9484 passed, 196 skipped.
- [x] `cargo xtask verify --skip-hub-build` — "All verification steps passed!"
- [ ] `br close bd-1lpkx`, `br sync --flush-only`, commit `.beads/`.

## Open questions / scope guardrails

- **Out of scope:** Q-2-13 (works), Q-2-5 underscore coverage (works in the
  simple case; any real gap is a separate coverage issue). Note this explicitly
  in the close-out so the narrowed scope is on record.
- If regeneration surfaces broad table drift, that is its own problem — flag it,
  don't fold it into this fix.

## Key references

- `crates/pampa/resources/error-corpus/Q-2-12.json` — the template to edit
- `crates/quarto-parse-errors/src/error_generation.rs:78-89` — `find_matching_token`
- `crates/quarto-parse-errors/src/error_generation.rs:159-228` — note resolution + trim logic
- `crates/pampa/scripts/build_error_table.ts` — corpus → `_autogen-table.json` builder
- `crates/pampa/resources/error-corpus/_autogen-table.json` — generated table (do not hand-edit)
- `crates/pampa/CLAUDE.md` — "Error messages" / corpus regeneration instructions
