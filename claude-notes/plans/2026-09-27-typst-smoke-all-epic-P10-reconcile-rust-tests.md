# P10 — Reconcile with the six existing Rust book integration tests

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

- [ ] Cross-reference every assertion in the six Rust files against P8/P9's smoke-all
      coverage — confirm no *unique* mechanism-level check exists only in the Rust
      tests without an equivalent smoke-all assertion covering the same behavior (not
      byte-identical, just equivalent coverage). **Concrete method** (the goal above
      was previously stated with no method, an under-specified gap caught during plan
      review): all six files use `assert!`/`assert_eq!` with descriptive panic
      messages that already read as natural-language mechanism descriptions (e.g.
      `"chapter one's custom-kind float"`, `"expected the unpatched extension's own
      #part[...] emission"`) — extract each assertion's message string, grep the
      corresponding smoke-all fixture's front matter for the same literal text or
      equivalent behavior, and tabulate {file:line, panic-message, mechanism,
      ported/gap} for every assertion. Handle the six as **two different kinds of
      check, not one uniform sweep**: `book_numbering_torture.rs`,
      `book_appendix_letter_parity.rs`, `book_part_appendix.rs`, and
      `book_numbering_pipeline.rs` assert on extracted PDF text (cross-reference
      against `ensurePdfRegexMatches`/`ensurePdfTextPositions` coverage);
      `orange_book_lua.rs`/`book_numbering_lua.rs` assert on `.typ`/Lua-filter
      output (cross-reference against `ensureTypstFileRegexMatches` coverage
      instead). Document any gap found — either port the missing check into a
      fixture's front matter, or explicitly accept the gap with a one-line reason.
- [ ] Add a short note to each of the six Rust files' module doc comment pointing at
      the smoke-all fixtures that now provide overlapping end-to-end coverage, so a
      future reader doesn't assume these are the only book-numbering tests.
- [ ] Add a short section to this epic's `claude-notes/plans/2026-09-27-typst-smoke-all-epic.md`
      "Definition of done" confirming both surfaces are green in the same workspace
      run.
- [ ] Before this final gate, confirm CI's actually-pinned Typst version is
      compatible with the struct-tree assumptions P3 depends on — the epic's own
      "Known limitations" flags this as a caveat on the struct-tree *shape* itself
      (only verified against Typst 0.14.2 locally), but no phase before this one
      owns re-checking it against CI's real pinned version. Also spot-check whether
      `cargo nextest run --workspace`'s wall-clock time moved materially given this
      epic adds ~90 new PDF-rendering/Typst-compiling fixtures plus several
      whole-book renders to the existing single-process `smoke_all()` test — nothing
      earlier in the epic owns this either.
- [ ] `cargo nextest run --workspace` — full phase-boundary run, per the standard
      per-phase-boundary gate. Report the delta against the current live baseline.

## Status

Not started.
