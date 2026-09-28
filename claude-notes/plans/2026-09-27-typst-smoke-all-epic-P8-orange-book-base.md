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

- [x] Copy the fixture directory's **tracked source files** into
      `crates/quarto/tests/smoke-all/typst/orange-book/` (all `.qmd`/`.bib`/
      `_brand.yml`/`logo.svg`/`notebooks/` files) — not a literal directory copy.
      Q1's checkout carries generated/local cruft alongside the source (`.quarto/`
      caches, `_book/` pre-rendered output, `index.typ`, and in at least one sibling
      `orange-book*` fixture a stray `.claude/settings.local.json` accidentally
      committed into the `quarto-cli` checkout) — each fixture's own `.gitignore`
      lists what's generated; copy everything except those.

      **Reconciled 2026-09-28.** The fixture had already been copied in an earlier
      session but had drifted from Q1's tracked source with a mix of intentional
      and stray changes; diffed every file against
      `/Users/gordon/src/quarto-cli/tests/docs/smoke-all/typst/orange-book/`
      (Q1's tracked `git ls-files`, not a literal directory listing) to sort them
      out:
      - Removed generated cruft not in Q1's tracked source: `.quarto/render-manifest.json`
        (empty Q2 render-cache artifact), `chapter1_files/` (stale Q2 render output,
        matches Q1's own `*_files/` gitignore pattern — Q1's real `chapter1_files/`
        has different contents, `fig-cars-1.svg` + a notebook PNG, neither of which
        this stray copy had), `chapter2.qmd.bak`/`chapter3.qmd.bak` (byte-identical
        backups of the already-edited files, safe to drop).
      - Reverted `_quarto.yml`'s `citeproc: false` back to Q1's `citeproc: true` —
        this was a leftover from earlier debugging that silently routed around the
        BibTeX bibliography-parse failure (see item 4) rather than exposing it;
        the plan's own Scope section calls for `citeproc: true`.
      - **Did NOT revert** the escaped apostrophes in `chapter2.qmd`/`chapter3.qmd`
        (`@hoare1978's` → `@hoare1978\'s`, etc.) — initially mistook these for
        spike debris and reverted them too, which immediately reproduced a real,
        documented Q2 parser limitation: `[Q-2-7] Unclosed Single Quote` (an
        unescaped `'` right before Markdown syntax, e.g. before a citation's
        trailing text, is misparsed as an opening smart-quote with no close). This
        is a known, tooled port-time fix — see
        `crates/qmd-syntax-helper/src/conversions/q_2_7.rs` — not a workaround for
        anything BibTeX/citeproc-specific. Re-applied the escapes; this is now the
        one intentional content deviation from Q1's tracked source, required for
        this fixture to parse under Q2 at all.
      - `index.qmd`'s only diff from Q1 is the `requires: jupyter` block added in
        an earlier session (see item 3 — since superseded by a `run.skip`, see
        item 4).
- [x] Set `index.qmd`'s `render-project: true` (confirm it's already set in Q1's
      fixture; add if not, matching Q1's own convention). Already set; unchanged.
- [x] `chapter1.qmd` (carries `_quarto.tests.run.skip`) uses
      `{{< embed notebooks/computations.ipynb#fig-visualization >}}`, and neither it
      nor `index.qmd` declares `requires: jupyter`. Confirm whether the book-merge
      path needs a live Jupyter runtime to process this embed, or only reads
      pre-baked notebook outputs — if the former, this fixture needs the same
      `requires: jupyter` gate `crates/quarto-test/src/runner.rs:100-101,137-152`
      already provides for other fixtures, or it will hard-fail (not cleanly skip)
      on machines without Jupyter installed.

      **Resolved 2026-09-28: neither — `{{< embed >}}` is not implemented in Q2 at
      all, so jupyter-availability is moot.** `dispatch_shortcode` in
      `crates/quarto-core/src/transforms/shortcode_resolve.rs` only has three
      built-in Rust handlers (`meta`/`env`/`var`) and falls through to Lua
      handlers; there is no `embed.lua` anywhere in the tree and no Rust handler
      named `embed`. `claude-notes/plans/2026-07-31-shortcode-extensions-port.md`
      confirms this explicitly (D6, confirmed 2026-07-31): "`embed` is out of
      scope for this plan... needs a more [substantial] redesign... own epic,
      engine-dependent." An unresolved shortcode call produces a Q-16-3
      "Shortcode handler not found" warning + an inline `**?embed**` marker, not a
      hard failure — engine-resolution for chapter1.qmd is driven only by its own
      `” `{r}` code chunk (the `fig-cars` plot), which knitr/R (available on this
      dev machine) handles fine regardless. Verified empirically: with the
      `requires: jupyter` gate removed, `SMOKE_FILTER=typst/orange-book/index.qmd`
      got past R-engine execution (`processing file: chapter1.rmarkdown` succeeded)
      and ran until the real, pre-existing blocker — the BibTeX bibliography-parse
      failure (see item 4) — with no jupyter involved at any point. Removed the
      `requires: jupyter` gate as based on an incorrect premise.

      Net effect on scope: `embed`'s absence will cost a handful of assertions
      once item 4 unblocks — only 3 lines in `index.qmd` reference
      `fig-visualization`/`<sec-embedded-notebooks>` — not the whole fixture. Not
      a P8 task to fix (own epic, per D6); tracked here as an accepted, understood
      gap to revisit once that epic lands, not a new strand (per this branch's
      "P8-scoped fixes and findings stay in the P8 plan" constraint).
- [ ] Render the whole book via P6's harness; confirm all ~140
      `ensureTypstFileRegexMatches`, ~110 `ensurePdfRegexMatches`, and the one
      `ensurePdfTextPositions` assertion pass against Q2's real, unmodified vendored
      `orange-book` extension. Treat this as the first real end-to-end exercise of
      `_brand.yml` through the book-merge path (see note above) — a logo-path
      mismatch here is a genuine finding, not expected to be pre-ruled-out.

      **Blocked 2026-09-28 on BibTeX bibliography support (external dependency,
      not listed in this plan's original "Depends on" line).** Reproduced the
      exact baseline the handoff described: `citeproc failed for merged book:
      Failed to parse bibliography '.../references.bib': expected value at line 1
      column 1` — `load_bibliography` in `crates/pampa/src/citeproc_filter.rs`
      only parses CSL-JSON today; `references.bib` is BibTeX and isn't valid JSON.
      The fix (biblatex-based `.bib` parsing) is in active, separate development
      in `workspace-6` on branch `braid/bd-l6eh1635-bibtex-citeproc` (WIP, not yet
      committed/merged there either) — per this branch's handoff constraints, that
      work stays out of workspace-5 entirely; it is not to be cherry-picked or
      reproduced here.

      Because `smoke_all::smoke_all` (`crates/quarto/tests/integration/smoke_all.rs`)
      is a single test that walks every smoke-all fixture and panics if **any**
      fails, leaving this fixture red broke `cargo nextest run -p quarto`
      workspace-wide (602 tests → 1 failure) the moment the fixture was
      reconciled to a real (non-`citeproc:false`-masked) state. Added a
      `_quarto.tests.run.skip: "Blocked on BibTeX bibliography support
      (bd-l6eh1635)"` gate on `index.qmd`, mirroring the existing per-chapter
      skip convention, with a comment pointing at this plan section and the
      workspace-6 branch. `cargo nextest run -p quarto` is green again (602
      passed, 2 skipped) with this fixture visibly, reasonedly skipped rather than
      silently masked.

      **Next step once bd-l6eh1635 merges into `feature/typst-testing`:** rebase
      this branch onto it, remove the `run.skip`, and actually run this item —
      render the whole book and reconcile all ~250 assertions against Q2's real
      output. Expect the `fig-visualization`/`sec-embedded-notebooks` assertions
      (3 lines, see item 3) to still fail on `embed`'s absence; everything else is
      new ground.
- [x] Cross-check against the six existing Rust integration tests
      (`book_numbering_torture.rs` et al.) — any assertion that fails here but passes
      there points at a smoke-all-harness gap, not a rendering regression; triage
      accordingly before assuming a Q2 bug.

      **Baseline confirmed green 2026-09-28**, unaffected by this branch's fixture
      work: `cargo nextest run -p quarto-core -E 'test(book_numbering_torture) +
      test(book_multifile_bibliography) + test(book_appendix_letter_parity) +
      test(orange_book_lua) + test(book_citations) + test(book_theorem_crossref)'`
      → 15/15 passed (1 leaky, pre-existing). Real cross-checking against
      smoke-all assertion failures can't happen until item 4 unblocks and the
      fixture actually renders.
- [x] `cargo clippy -p quarto --all-targets -- -D warnings` + `cargo nextest run
      -p quarto`.

      Both green as of 2026-09-28 (clippy clean; nextest 602 passed, 2 skipped —
      see item 4 for why orange-book's own test is one of the skips).

## Status

**Blocked on bd-l6eh1635 (BibTeX bibliography support, workspace-6/
`braid/bd-l6eh1635-bibtex-citeproc`) for the one remaining item (rendering the
whole book and reconciling assertions).** Everything else in this branch's scope
is done: fixture reconciled to Q1's tracked source (plus one required Q-2-7
apostrophe-escape deviation), `render-project: true` confirmed, the
`requires: jupyter` question resolved (not needed — `embed` is unimplemented,
own epic, per D6), existing Rust integration-test baseline confirmed green,
`cargo clippy`/`cargo nextest -p quarto` green with this fixture's own test
visibly skipped pending the BibTeX dependency. Not merging to
`feature/typst-testing` yet — nothing here is ready to flip to `Complete.` until
item 4 actually runs.
