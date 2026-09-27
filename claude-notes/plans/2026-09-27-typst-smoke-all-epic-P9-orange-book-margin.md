# P9 — Port `orange-book-margin` (book-context margin notes)

**Date:** 2026-09-27
**Epic:** [`2026-09-27-typst-smoke-all-epic.md`](2026-09-27-typst-smoke-all-epic.md) —
Decided item 4: in scope for this epic, not deferred to a follow-on. Sequenced after
P5 specifically so the struct-tree implementation is already proven at scale.
**Depends on:** P5, P8.
**Worktree:** the convergence point — either `workspace-2` or `workspace-5`,
whichever is free first (see epic's "Parallel development plan").

## Worktree & git workflow

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3
git show feature/typst-testing:claude-notes/plans/2026-09-27-typst-smoke-all-epic-P5-margin-layout.md \
  | grep -q '^Complete\.' && echo "P5 merged" || echo "STOP: P5 not yet merged, wait"
git show feature/typst-testing:claude-notes/plans/2026-09-27-typst-smoke-all-epic-P8-orange-book-base.md \
  | grep -q '^Complete\.' && echo "P8 merged" || echo "STOP: P8 not yet merged, wait"
```

Only proceed once **both** print "merged". **Which worktree does this phase**:
whichever of `workspace-2`/`workspace-5` finished its own track first and is sitting
idle. If both are idle when you check, default to `workspace-2`. There's no
correctness difference between the two — this phase's own topic branch starts from
`feature/typst-testing`'s tip either way, and neither worktree's prior branch state
matters once its own last phase has merged.

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-2   # or workspace-5 — see above
git checkout -B typst-testing/p9-orange-book-margin feature/typst-testing
```

Implement the checklist below, gating on `cargo clippy -p quarto --all-targets --
-D warnings` + `cargo nextest run -p quarto`. When done, flip this doc's checklist
to `[x]` and `## Status` to `Complete.`, commit, then:

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-2   # whichever you used above
git rebase feature/typst-testing
cd /Users/gordon/src/q2/.worktrees/workspace-3
git checkout feature/typst-testing
git merge --ff-only typst-testing/p9-orange-book-margin   # retry from rebase if not a fast-forward
cargo nextest run --workspace                  # phase-boundary gate
```

Continue directly to P10 in the same worktree you just used for P9.

## Scope

`external-sources/quarto-cli/tests/docs/smoke-all/typst/orange-book-margin/` — same
shape as `orange-book` (P8) but adds `reference-location: margin`,
`citation-location: margin`, `suppress-bibliography: true`,
`grid.margin-width/gutter-width`. `index.qmd`'s front matter is the heaviest of the
four `orange-book*` fixtures: ~170 `ensureTypstFileRegexMatches`, ~180
`ensurePdfRegexMatches` (body text uses invented Latin-ish anchor words —
"Heliocircula", "Ankylosaura" — to avoid ambiguity in narrow-margin text), and **~24
`ensurePdfTextPositions` assertions**, almost all plain-string `rightOf`/`leftOf` pairs
keyed to specific page numbers (recto/verso alternation), plus exactly one
`granularity`-based check (`{text, granularity: "Div"|"P"}` object form, same
code-listing check as `orange-book` but using the object form here).

## First task: confirm book-level margin wiring

P8 proves single-document-scoped margin notes work (via the already-vendored
`marginalia` wiring, `typst_compile.rs:123-155`). Book-level margin options split
into two genuinely different mechanisms — **verified during plan review, one
correction to the original analysis**:

- **`reference-location` (confirmed, code-verified):** read by
  `FootnotesResolveTransform::get_reference_location`
  (`crates/quarto-core/src/transforms/footnotes_resolve.rs:82-91`), a
  Normalization-phase transform that runs during each chapter's own *per-chapter
  paused render* (`render_chapter_paused` in `single_file_render.rs` pauses right
  after Normalization) — using the same project-metadata-merged context every
  single-document render gets. Unlike citeproc, this isn't deferred past the merge
  point, so it needs no merge-specific machinery.
- **`citation-location`/`grid.margin-width`/`grid.gutter-width` (a *different*
  mechanism — the original "read by `FootnotesResolveTransform`" claim was wrong for
  these): that transform doesn't read `citation-location` at all — grep confirms the
  only other hit in `crates/quarto-core/src/` is an unrelated comment in
  `transforms/appendix.rs:44`. These are pure **Pandoc template variables**,
  consumed only inside `resources/pandoc-filters/typst-template/{biblio.typ,page.typ,
  definitions.typ}` and the vendored `orange-book` extension's own
  `_extensions/orange-book/typst-show.typ` (`$if(margin-geometry)$` passthrough).
  The real question for these three isn't transform timing, it's whether
  project-level `_quarto.yml` metadata reaches the *merged* document's template
  context — a metadata-merge question, not the transform-ordering argument that
  covers `reference-location`. P8 already proves other project-level metadata
  (`crossref.custom`) reaches chapters, so this is likely fine, but hasn't been
  empirically confirmed for these three keys specifically — that's what the spike
  below checks.
- **Recto/verso needs no detection code at all** — verified against the real
  fixture (`index.qmd:349-455`, all 24 position assertions read in full). Every
  assertion is a plain `rightOf`/`leftOf` comparison between two unique search
  strings; "recto"/"verso" appear *only* as human-readable YAML comments explaining
  why the fixture author picked `rightOf` on odd pages vs. `leftOf` on even (duplex
  margin-note alternation) — the words never appear in the assertion schema. There
  is no page-side/parity detection to build; P3's existing relation vocabulary
  already covers 100% of this fixture's position assertions.
- **A related, separate finding worth flagging for P1/P3's parser**: each of these
  24 assertions also carries a `page: N` sibling key (e.g. `page: 9`) alongside
  `subject`/`relation`/`object`. This is *not* part of Q1's actual schema —
  `DirectionalAssertionSchema` (`verify-pdf-text-position.ts`) has no top-level
  `page` field at all (only `TextSelector.page`, used exclusively for `role: "Page"`
  selectors). Since that schema isn't `.strict()`, Zod silently ignores the extra
  key — Q1's real page-correctness comes entirely from unique-text search plus the
  implicit same-page check between resolved subject/object bboxes, not from this
  annotation. **Q2's port needs to tolerate this decorative extra key** rather than
  reject it as unrecognized input — worth an explicit check if `spec.rs`'s
  deserializer uses anything like `#[serde(deny_unknown_fields)]` on this assertion
  shape (P1/P3's concern, flagging here since this fixture is where it'd first bite).

This lowers this phase from "does this work at all" to "port + one real empirical
spike" (same shape as P8), covering both the `citation-location`/`margin-geometry`
propagation question above and a confirming render — see checklist below.

## Checklist

- [ ] Spike: render `orange-book-margin` with today's (post-P8) harness. Two things
      to specifically check, not just "does it render": (a) does `reference-location`
      margin placement work as the code-reading above predicts (should — treat a
      failure here as a real finding); (b) does `citation-location: margin`/
      `suppress-bibliography: true`/`grid.margin-width`/`grid.gutter-width` actually
      reach the merged document's Typst template context (**not yet confirmed by
      code-reading alone** — this is the part of the spike that's a genuine unknown,
      not a formality).
- [ ] Copy the fixture directory's **tracked source files** into
      `crates/quarto/tests/smoke-all/typst/orange-book-margin/` — not a literal
      directory copy; exclude generated/local cruft (`.quarto/` caches, `_book/`
      pre-rendered output, `index.typ`, stray local dotfiles) per the fixture's own
      `.gitignore`. `external-sources/quarto-cli` is a symlink to Gordon's own
      sibling checkout (`.gitignore`d, per-checkout) — already symlinked into this
      worktree; if working from a different worktree, recreate it first
      (`ln -s /Users/gordon/src/quarto-cli external-sources/quarto-cli`).
- [ ] Confirm recto/verso-labeled position assertions (24 total, all plain
      `rightOf`/`leftOf`, no new relation types) are correctly resolved by P3's
      `/StructTreeRoot` page-scoping work. P3's map is keyed by `(PageRef, Mcid)`,
      not bare `Mcid` — this fixture is the first real (non-synthetic) exercise of
      that page-keying. Confirm P1/P3's assertion parser tolerates each assertion's
      decorative `page: N` sibling key (see note above) without rejecting the
      fixture as malformed.
- [ ] Confirm the one `granularity`-based assertion (object form
      `{text, granularity: "Div"|"P"}`) round-trips correctly through P1's assertion
      parser (P8's equivalent check uses plain strings; this is the first fixture to
      exercise the object form for real).
- [ ] `cargo clippy -p quarto --all-targets -- -D warnings` + `cargo nextest run
      -p quarto`.

## Status

Not started.
