# P8 — Port `orange-book` (base), full predicates

**Date:** 2026-09-27
**Epic:** [`2026-09-27-typst-smoke-all-epic.md`](2026-09-27-typst-smoke-all-epic.md) —
Decided item 5: full predicate scope, not the reduced "one assertion" scope floated
during research.
**Depends on:** P1, P3, P6, P7.
**Worktree:** `workspace-5` (Track B, sequential after P7 — see epic's "Parallel
development plan"). **This is the one cross-track blocking point in the whole
epic**: P3 is done in `workspace-2` (Track A), not this worktree — don't just check
P7, check P3 too.

## Worktree & git workflow

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3
git show feature/typst-testing:claude-notes/plans/2026-09-27-typst-smoke-all-epic-P7-override-and-lang.md \
  | grep -q '^Complete\.' && echo "P7 merged" || echo "STOP: P7 not yet merged, wait"
git show feature/typst-testing:claude-notes/plans/2026-09-27-typst-smoke-all-epic-P3-struct-tree-walk.md \
  | grep -q '^Complete\.' && echo "P3 merged" || echo "STOP: P3 not yet merged — this is the cross-track wait, check back later, don't start P8 without it"
```

Only proceed once **both** print "merged". If P3 isn't merged yet, there is no more
Track-B-only work to do before P8 — stop here and re-run this check later (e.g. at
the start of a new session) rather than guessing at P3's interface.

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-5
git checkout -B typst-testing/p8-orange-book-base feature/typst-testing
```

Implement the checklist below, gating on `cargo clippy -p quarto --all-targets --
-D warnings` + `cargo nextest run -p quarto`. When done, flip this doc's checklist
to `[x]` and `## Status` to `Complete.`, commit, then:

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-5
git rebase feature/typst-testing
cd /Users/gordon/src/q2/.worktrees/workspace-3
git checkout feature/typst-testing
git merge --ff-only typst-testing/p8-orange-book-base   # retry from rebase (in workspace-5) if not a fast-forward
cargo nextest run --workspace                  # phase-boundary gate
```

`workspace-5` is now free — check whether `workspace-2` still needs help finishing
P5 (see P9's doc for the exact "is P5 merged yet" check) before picking up P9.

## Scope

`external-sources/quarto-cli/tests/docs/smoke-all/typst/orange-book/` — the deep
torture-test fixture. Full multi-file book project: `_quarto.yml`
(`project: type: book`, 2 parts/3 chapters + 2 appendices + references,
`bibliography: references.bib`, `citeproc: true`, custom crossref kind `dino`,
`format.typst.keep-typ/toc-depth/theorem-appearance`), `_brand.yml` (color + logo).

`index.qmd` carries all assertions (~270 lines of front matter):
`ensureTypstFileRegexMatches` (~140 patterns: labels, `#part[...]`, appendix
`#show: appendices.with(...)`, brand-color/logo injection, custom-crossref-kind
counters), `ensurePdfRegexMatches` (~110 patterns: cross-chapter numbering,
appendix-A/B independent lettering, citeproc author-date bibliography, sub-figure
panels, custom "dinosaur" crossref kind), and exactly one `ensurePdfTextPositions`
assertion (plain-string `leftAligned`, no `granularity`/`role` — the simplest possible
case, already proven by P4/P5 before this phase needs it). Chapters/appendices/
references carry `_quarto.tests.run.skip: "Book chapter - only renders as part of full
book"`.

This is where P6's `render-project: true` dedup and P1's PDF-regex assertions get
exercised at full depth for the first time, and where P3's struct-tree walk gets
proven inside a real book-merge render (as opposed to P5's simpler independent-file
renders).

**The custom crossref kind and brand injection are lower-risk than they look —
verified during plan review, not just plausible.** A custom `crossref.custom` kind
needs zero Lua-filter-level extension work: it's Q2's native mechanism
(`crates/quarto-core/src/crossref/metadata.rs`'s `read_custom`, registered via
`RefTypeRegistry::register_custom`, `registry.rs:198`) declared purely in
`_quarto.yml`, not in the vendored extension's own Lua. More importantly, **this
exact scenario is already proven through the book-merge path on `main` today**:
`crates/quarto-core/tests/integration/book_numbering_torture.rs` declares a
`crossref.custom` kind (`key: dino`, `reference-prefix: Dinosaur`) in its own test
project and asserts `"Dinosaur 1.1: This is the first dinosaur."` against a real
compiled, book-merged PDF (`:64-65,288`) — currently green. Treat dino-through-merge
as confirmed, not a risk to re-derive. Brand injection is similarly lower-risk:
`resolve_typst_brand_param` (`pandoc_write.rs:248`) recomputes the brand fresh from
the merged document's own metadata at the finishing-stage tail (not a stateful field
needing book-merge carry-forward the way `ref_type_registry` is), and the metadata
merge's retain predicate only strips `"format"` (`metadata_merge.rs:471`), so
`brand:` survives into the merged document the same way `crossref` does. The one
genuine gap: nobody has actually rendered a book fixture with `_brand.yml`
end-to-end yet — this phase is the first real spike for that, specifically for
logo-path resolution against a synthetic merged document's directory context.

## Checklist

- [ ] Copy the fixture directory's **tracked source files** into
      `crates/quarto/tests/smoke-all/typst/orange-book/` (all `.qmd`/`.bib`/
      `_brand.yml`/`logo.svg`/`notebooks/` files) — not a literal directory copy.
      Q1's checkout carries generated/local cruft alongside the source (`.quarto/`
      caches, `_book/` pre-rendered output, `index.typ`, and in at least one sibling
      `orange-book*` fixture a stray `.claude/settings.local.json` accidentally
      committed into the `quarto-cli` checkout) — each fixture's own `.gitignore`
      lists what's generated; copy everything except those.
- [ ] Set `index.qmd`'s `render-project: true` (confirm it's already set in Q1's
      fixture; add if not, matching Q1's own convention).
- [ ] `chapter1.qmd` (carries `_quarto.tests.run.skip`) uses
      `{{< embed notebooks/computations.ipynb#fig-visualization >}}`, and neither it
      nor `index.qmd` declares `requires: jupyter`. Confirm whether the book-merge
      path needs a live Jupyter runtime to process this embed, or only reads
      pre-baked notebook outputs — if the former, this fixture needs the same
      `requires: jupyter` gate `crates/quarto-test/src/runner.rs:100-101,137-152`
      already provides for other fixtures, or it will hard-fail (not cleanly skip)
      on machines without Jupyter installed.
- [ ] Render the whole book via P6's harness; confirm all ~140
      `ensureTypstFileRegexMatches`, ~110 `ensurePdfRegexMatches`, and the one
      `ensurePdfTextPositions` assertion pass against Q2's real, unmodified vendored
      `orange-book` extension. Treat this as the first real end-to-end exercise of
      `_brand.yml` through the book-merge path (see note above) — a logo-path
      mismatch here is a genuine finding, not expected to be pre-ruled-out.
- [ ] Cross-check against the six existing Rust integration tests
      (`book_numbering_torture.rs` et al.) — any assertion that fails here but passes
      there points at a smoke-all-harness gap, not a rendering regression; triage
      accordingly before assuming a Q2 bug.
- [ ] `cargo clippy -p quarto --all-targets -- -D warnings` + `cargo nextest run
      -p quarto`.

## Status

Not started.
