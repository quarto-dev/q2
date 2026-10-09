---
title: 'P10 — Reconcile with the six existing Rust book integration tests'
date: 2026-09-27
---

**Date:** 2026-09-27
**Epic:** [`2026-09-27-typst-smoke-all-epic.md`](2026-09-27-typst-smoke-all-epic.md) —
Decided item 6: coexist, not replace. This phase documents and confirms that decision
once real smoke-all coverage exists to compare against — it is analysis/docs, not new
test code.
**Depends on:** P8, P9 (do this once real, running smoke-all coverage exists, not
against a hypothetical).
**Worktree:** the same worktree (`workspace-2` or `workspace-5`) that just did P9 —
see epic's "Parallel development plan". This is the epic's last phase.

## Worktree & git workflow

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3
git show feature/typst-testing:claude-notes/plans/2026-09-27-typst-smoke-all-epic-P9-orange-book-margin.md \
  | grep -q '^Complete\.' && echo "P9 merged" || echo "STOP: P9 not yet merged, wait"
```

Once it prints "merged" (P8 is already implied, since P9 depended on it):

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-2   # or workspace-5 — whichever just did P9
git checkout -B typst-testing/p10-reconcile-rust-tests feature/typst-testing
```

Implement the checklist below — this phase edits docs/module comments, not new
production code, so its own gate is lighter (no new `cargo clippy` target beyond
what the workspace run below already covers). When done, flip this doc's checklist
to `[x]` and `## Status` to `Complete.`, commit, then:

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-2   # whichever you used above
git rebase feature/typst-testing
cd /Users/gordon/src/q2/.worktrees/workspace-3
git checkout feature/typst-testing
git merge --ff-only typst-testing/p10-reconcile-rust-tests   # retry from rebase if not a fast-forward
```

The `cargo nextest run --workspace` this phase's own checklist already calls for
**is** the final phase-boundary gate for the whole epic — no separate one needed
after this merge. Push `feature/typst-testing` and open the PR into `main` once
green.

## Scope

Six existing files, all on `main`, all in
`crates/quarto-core/tests/integration/`: `book_numbering_torture.rs` (381 lines),
`book_appendix_letter_parity.rs` (252), `book_part_appendix.rs` (152),
`orange_book_lua.rs` (186), `book_numbering_lua.rs` (156),
`book_numbering_pipeline.rs` (444) — 1,571 lines total (all confirmed current via
`wc -l` during plan review — no drift), RED→GREEN-proven. **Not all six use
`pdf_extract::extract_text()`** — `orange_book_lua.rs`/`book_numbering_lua.rs` don't
call it at all (they check `.typ`/Lua-filter output, not extracted PDF text), while
two files *outside* this six (`book_theorem_crossref.rs`, `book_citations.rs`) do
call it but test different mechanisms and are out of this phase's coexistence scope.
This phase's cross-reference sweep covers exactly the six named above; P2's
`pdf_extract` regression-verification checklist covers a different, overlapping six
(see P2.md) — don't conflate the two lists.

**Decided rationale (coexist)**: the Rust tests test *mechanisms* (e.g. "appendix
theorem numbering resets") — fast, targeted, no full-fixture dependency. The smoke-all
port tests *end-to-end fidelity against a foreign, unmodified real-world extension* —
slower, but catches integration-level drift the mechanism tests can't. Different
failure modes, worth keeping both.

## Checklist

- [x] Cross-reference every assertion in the six Rust files against P8/P9's smoke-all
  coverage — confirm no *unique* mechanism-level check exists only in the Rust
  tests without an equivalent smoke-all assertion covering the same behavior (not
  byte-identical, just equivalent coverage). **Concrete method** (the goal above
  was previously stated with no method, an under-specified gap caught during plan
  review): all six files use `assert!`/`assert_eq!` with descriptive panic
  messages that already read as natural-language mechanism descriptions (e.g.
  `"chapter one's custom-kind float"`, `"expected the unpatched extension's own
  #part[...] emission"`) — extract each assertion's message string, grep the
  corresponding smoke-all fixture's front matter for the same literal text or
  equivalent behavior, and tabulate \{file:line, panic-message, mechanism,
  ported/gap\} for every assertion. Handle the six as **two different kinds of
  check, not one uniform sweep**: `book_numbering_torture.rs`,
  `book_appendix_letter_parity.rs`, `book_part_appendix.rs`, and
  `book_numbering_pipeline.rs` assert on extracted PDF text (cross-reference
  against `ensurePdfRegexMatches`/`ensurePdfTextPositions` coverage);
  `orange_book_lua.rs`/`book_numbering_lua.rs` assert on `.typ`/Lua-filter
  output (cross-reference against `ensureTypstFileRegexMatches` coverage
  instead). Document any gap found — either port the missing check into a
  fixture's front matter, or explicitly accept the gap with a one-line reason.
  **Result**: 43 mechanism-level assertions tabulated across the six files;
  ~27 already had equivalent smoke-all coverage, 16 gaps found. One gap was
  real and worth closing: no smoke-all fixture asserted the appendix chapters\'
  own *heading* text with its letter prefix (only body-construct numbering like
  "Theorem A.1" was checked) — confirmed by compiling `orange-book`'s `.typ`
  output and extracting PDF text with `pdftotext`, which showed the compiled
  heading renders as "A. Additional Resources" / "B. Supplementary Data" (the
  `.typ` source heading text itself carries no letter — `appendices()`'s show
  rule adds it at compile time). Ported: added
  `"A\\.\\s+Additional\\s+Resources"` / `"B\\.\\s+Supplementary\\s+Data"` to
  `ensurePdfRegexMatches` in both `orange-book/index.qmd` and
  `orange-book-margin/index.qmd`; verified both pass against a real render. The
  other 15 gaps are accepted as-is: Rust-internal API checks with no rendered
  surface (`book_render_items`/`chapter_label_prefix`), synthetic negative/
  isolation-control tests no book fixture can express by construction, or
  docx/epub3-specific mechanisms orthogonal to this Typst-only epic.
- [x] Add a short note to each of the six Rust files\' module doc comment pointing at
  the smoke-all fixtures that now provide overlapping end-to-end coverage, so a
  future reader doesn't assume these are the only book-numbering tests.
- [x] Add a short section to this epic's `claude-notes/plans/2026-09-27-typst-smoke-all-epic.md`
  "Definition of done" confirming both surfaces are green in the same workspace
  run.
- [x] Before this final gate, confirm CI's actually-pinned Typst version is
  compatible with the struct-tree assumptions P3 depends on — the epic's own
  "Known limitations" flags this as a caveat on the struct-tree *shape* itself
  (only verified against Typst 0.14.2 locally), but no phase before this one
  owns re-checking it against CI's real pinned version. Also spot-check whether
  `cargo nextest run --workspace`'s wall-clock time moved materially given this
  epic adds ~90 new PDF-rendering/Typst-compiling fixtures plus several
  whole-book renders to the existing single-process `smoke_all()` test — nothing
  earlier in the epic owns this either.
  **Result**: downloaded the real CI-pinned Typst 0.15.1 binary and re-ran
  smoke-all against it via `QUARTO_TYPST=<path>` — `margin-layout`'s full
  86-fixture/76-positional-assertion set passed identically to the 0.14.2
  baseline (81 passed, 5 pre-existing skips, 0 failed), and `orange-book`/
  `orange-book-margin` reproduced the identical single pre-existing failure
  (the embed gap) under both versions. No struct-tree regression from 0.14.2 to
  0.15.1; closed out P3's own deferred checklist item with this finding. Wall-
  clock: this branch's full workspace run took 718.929s vs. `main`\'s CI
  `macos-latest` baseline of 391.512s — a ~84% increase, disproportionate to
  the +107 test count (+0.7%) and attributable almost entirely to `smoke_all`
  itself (162.5s) compiling far more real PDF/Typst fixtures. Accepted as the
  inherent cost of real end-to-end coverage — see epic doc's "Definition of
  done" for the full numbers.
- [x] `cargo nextest run --workspace` — full phase-boundary run, per the standard
  per-phase-boundary gate. Report the delta against the current live baseline.
  **Result**: 15295 tests run, 15294 passed, 1 failed (`smoke_all` — the
  accepted bd-gak8uiza embed gap, not a regression), 201 skipped, 718.929s.
  Delta vs. `main` @ `e8379cfe1` (CI run 36479624208, `macos-latest`): +107
  tests, 0→1 failed (expected/accepted), skip count unchanged. See epic doc's
  "Definition of done" for full detail.

## Status

Complete.
