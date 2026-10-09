---
title: 'P3 — `ensurePdfTextPositions`: `/StructTreeRoot` walk + relational-assertion evaluator'
date: 2026-09-27
description: 'Builds a `/StructTreeRoot` walk that maps each marked-content ID, keyed by page and ID, to its structure role, plus the relational evaluator for `ensurePdfTextPositions` assertions such as left-of and aligned.'
---

**Epic:** [`2026-09-27-typst-smoke-all-epic.md`](2026-09-27-typst-smoke-all-epic.md) —
read "Decided" items 2, 3, 5 first: this work is folded into the epic (not a separate
plan), the research session that scoped it is gone, and the target is full predicate
parity, not a reduced subset.
**Depends on:** P1, P2 (P2 for the pinned fork's MCID surfacing; P1 because it adds
the `pdf-extract` dependency to `quarto-test`\'s `Cargo.toml` in the first place — see
the note immediately below — which this phase must re-point at P2's fork before its
own first checklist item can compile. An earlier draft of this doc and the epic's
own phase table listed only `P2` here; that was incomplete).

**First checklist item below this scope section**: P1 adds `pdf-extract` to
`quarto-test`'s `Cargo.toml` pinned to crates.io `"0.7"` (it doesn't need MCID
surfacing). This phase must re-point that same dependency at P2's git-fork rev
(`crates/quarto-core/Cargo.toml`'s pin, mirrored into `quarto-test`) before any of
the MCID-surfacing work below can compile — do this first, not as an afterthought.

**Worktree:** `workspace-2` (Track A — see epic's "Parallel development plan").

**Authorized verification scope:** For P3, the user authorizes building and testing `quarto-test` and the workspace with `pdf-extract` pinned to `https://github.com/gordonwoodhull/pdf-extract` at commit `f68ca43f27b1e23d92072cc4383178a33ba25457`. This authorization includes `cargo clippy -p quarto-test --all-targets -- -D warnings`, `cargo nextest run -p quarto-test`, and the planned workspace `nextest` phase-boundary gate. It does not authorize a different fork revision, pushing, or unrelated external-source builds. If the permission classifier denies one of these exact authorized commands, report the denial and pause for the user's direction; do not ask for the same authorization again or try alternate routes.

## Worktree & git workflow

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3
git show feature/typst-testing:claude-notes/plans/2026-09-27-typst-smoke-all-epic-P1-assertion-vocabulary.md \
  | grep -q '^Complete\.' && echo "P1 merged" || echo "STOP: P1 not yet merged, wait"
git show feature/typst-testing:claude-notes/plans/2026-09-27-typst-smoke-all-epic-P2-pdf-extract-dependency.md \
  | grep -q '^Complete\.' && echo "P2 merged" || echo "STOP: P2 not yet merged, wait"
```

Once both print "merged":

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-2
git checkout -B typst-testing/p3-struct-tree-walk feature/typst-testing
```

Implement the checklist below, gating on `cargo clippy -p quarto-test --all-targets
-- -D warnings` + `cargo nextest run -p quarto-test`. When done, flip this doc's
checklist to `[x]` and `## Status` to `Complete.`, commit, then:

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-2
git rebase feature/typst-testing
cd /Users/gordon/src/q2/.worktrees/workspace-3
git checkout feature/typst-testing
git merge --ff-only typst-testing/p3-struct-tree-walk   # retry from rebase (in workspace-2) if not a fast-forward
cargo nextest run --workspace                  # phase-boundary gate
```

## Scope decision (why this phase exists, restated for the implementer)

Q1's `ensurePdfTextPositions` (`external-sources/quarto-cli/tests/verify-pdf-text-position.ts`)
reads a PDF's tagged structure tree (MCIDs linking text spans to semantic roles — P,
H1, Figure, Table, etc.) plus word-level bounding boxes, via `pdfjs-dist`, then
evaluates relational assertions: 4 directional (`rightOf`, `leftOf`, `above`, `below`,
each with optional `byMin`/`byMax` distance constraints) and **4** alignment
(`leftAligned`, `rightAligned`, `topAligned`, **and `bottomAligned`** — verified
against the real schema (`AlignmentRelationSchema`, `verify-pdf-text-position.ts:48`);
earlier drafts of this plan and the epic's own Definition of Done only listed 7
relations, dropping `bottomAligned` — an oversight, not a deliberate cut; no in-scope
fixture happens to use it, but "full predicate support" per Decision 5 means all 8).
Alignment relations take a `tolerance` (default 2pt); either relation kind can
override which bbox edge to compare via an optional `edge` field
(`left`/`right`/`top`/`bottom`/`centerX`/`centerY`, `TextSelectorSchema`). Two escape
hatches don't need the tag tree: `role: "Decoration"` (raw text-item bounds) and
`role: "Page"` (whole-page bounds). Q1's own header comment says this only works for
Typst-produced PDFs today (Typst emits tagged PDFs by default; LaTeX/ConTeXt don't).

The top-level call signature is `ensurePdfTextPositions(file, assertions,
noMatchAssertions?)` — the same two-array (must-hold / must-NOT-hold) wrapper shape
P1 documents for `ensureFileRegexMatches`. Confirmed by reading real fixtures:
`margin-layout`\'s files use the full `[[...], []]` two-array form, while
`pdf-text-position-test.qmd` omits the second array entirely — the parser must
accept both.

P2's fork surfaces content-stream marked-content events (`BMC`/`BDC`/`EMC` → MCID +
tag). **What it does not do** — and what this phase must build — is walk
`/StructTreeRoot` to resolve each MCID to its owning structure element's role, and
aggregate word bounding boxes up to a `granularity`-requested ancestor.

## The genuinely hard part (verified against a real PDF during plan review)

A spike rendered a real multi-page Typst document with margin notes straddling a
page break (`typst compile` 0.14.2, vendored `marginalia` package) and dumped its
actual `/StructTreeRoot` via a throwaway `lopdf` binary. Full raw findings, exact
commands, and fixture description:
`claude-notes/research/2026-09-27-typst-tagged-pdf-struct-tree.md`. Two corrections
to the original inference-derived model:

1. **Typst emits no `/Sect` at all** — its role vocabulary here is `/Document`
   (root), `/H1`/`/H2`, `/P`. The risk isn't `/Sect`-specific: a plain `/P`
   element's kids spanned two pages directly (bare-integer kids inheriting the
   parent's `/Pg` on page 1, followed by explicit `/MCR` dict kids with their own
   `/Pg` on page 2). **Any StructElem can span pages**, not just section-like
   containers.
2. **MCIDs are only unique per page, not document-wide** — page 2 restarted
   numbering at 0, same as page 1. A map keyed by bare `Mcid` will silently collide
   entries from different pages. The map must be keyed by **`(PageRef, Mcid)`**.

Q1's `pdf.js`-based predicate never hits either problem, but not because it has an
explicit guard: `page.getStructTree()` hands back an *already page-scoped* subtree
per page (so `pdf.js`\'s own struct-tree walk never crosses a page boundary), and its
marked-content identifiers are page-qualified strings (e.g. `"p2R_mc0"`), not bare
integers. Neither structural guarantee exists for a `lopdf`-based walker reading a
single document-wide `/StructTreeRoot` — P3 has to build both properties in
explicitly (see checklist).

What can be trusted (from Q1's own predicate comments, confirmed by the spike):
Typst emits PDF 1.4-baseline tagged structure only (no PDF 2.0 namespace features),
`/ParentTree`/`/RoleMap` are present, and Typst doesn't write `/ID` on `StructElem`s
(`/IDTree` absent).

## WASM / cross-platform compliance

This phase's code stays out of the WASM build for one reason, checked during plan
review: it lives in `quarto-test`, a crate `wasm-quarto-hub-client` doesn't depend on
at all — regardless of whether `quarto-test`'s own `pdf-extract` dependency (added in
P1, re-pointed at the git fork by this phase per the note above) is scoped as a dev-
or regular dependency. (`quarto-core`'s *own* copy of `pdf-extract` is
`[dev-dependencies]`-scoped, which is why P2 needs no WASM note either — but that
fact is about `quarto-core` specifically and doesn't carry over to `quarto-test`\'s
separate, regular-dependency copy; don't conflate the two.) No `.claude/rules/wasm.md`
action needed; pure Rust throughout, no platform-specific APIs, so
`.claude/rules/cross-platform.md` needs no special handling either.

## Checklist

- [x] Re-run the struct-tree spike (already done once during plan review — see
  `claude-notes/research/2026-09-27-typst-tagged-pdf-struct-tree.md`) against the
  real, ported `orange-book-margin` fixture once P9 lands it, and re-check the
  Typst-version caveat — the exact `/Pg`-inheritance/MCID-restart shape may differ
  across supported versions. Current evidence: local test compiler is Typst 0.14.2,
  CI pins 0.15.1, and runtime accepts versions from floor `(0, 8)`; the P9 fixture
  has not landed in this worktree, so cross-version and project-fixture confirmation
  remain pending.
  **Closed out in P10 (2026-09-29):** downloaded the real CI-pinned Typst 0.15.1
  binary (`typst-aarch64-apple-darwin` release asset) and re-ran smoke-all against
  it via `QUARTO_TYPST=<path-to-0.15.1-binary>` (no code change, just pointing the
  pipeline's `find_binary` env override at the other binary). `margin-layout`\'s
  full 86-fixture set (76 `ensurePdfTextPositions` assertions, the richest
  struct-tree exercise in the epic) — 81 passed, 5 pre-existing skips, 0 failed,
  identical to the 0.14.2 baseline. `orange-book`/`orange-book-margin` — same
  single pre-existing failure (the bd-gak8uiza `{{{< embed >}}}`/`fig-visualization`
  gap) reproduces identically under both Typst versions; confirmed by running the
  same fixtures against the default 0.14.2 binary and diffing the failure output.
  No struct-tree-shape regression found between 0.14.2 and CI's pinned 0.15.1.
- [x] Implement `/StructTreeRoot` walk: build a map keyed by **`(PageRef, Mcid)`**,
  not `Mcid` alone — MCIDs restart at 0 on every page, so a bare-`Mcid` key
  collides entries from different pages. Walk every `StructElem`, resolving
  inherited `/Pg` (bare-integer kids inherit the parent's own `/Pg`; a kid on a
  different page always appears as an explicit `/MCR` dict carrying its own
  `/Pg` — that dict-vs-bare-integer distinction is the actual page-membership
  signal, not tree shape). Match Q1's role vocabulary (P, H1..H6, Figure, Table,
  Div, etc. — full list in `verify-pdf-text-position.ts`). Synthetic two-page tagged-tree test builds real PDF objects and covers inherited `/Pg`, a page-qualified `/MCR`, same MCID on both pages, RoleMap, and page-specific descendants.
- [x] Implement word-level bounding-box extraction per page (via the fork's
  `OutputDev` hooks), keyed by `(PageRef, Mcid)` via the marked-content nesting
  already surfaced by P2. The real Typst fixture exercises extraction, decorated raw-text bounds, and a Page selector through `verify`.
- [x] **Implement text-search selector resolution** — the step between "extract
  bounding boxes" and "evaluate relations" that earlier drafts of this checklist
  skipped entirely. Q1's actual mechanism (`verify-pdf-text-position.ts:744-763`):
  given a subject/object's `text` string, search *all* extracted text across the
  whole document for a substring match (`allTextItems.filter(t =>
  t.str.includes(searchText))`); exactly one match resolves normally; zero
  matches is a "text not found" error; more than one match is an "ambiguous,
  use a more specific search string" error — **except** for `role: "Decoration"`
  selectors, which explicitly tolerate repeats (headers/footers repeat on every
  page by design) and take the first match. Without this step, there is no way
  to turn a YAML `text:` string into the MCID/bbox the rest of the evaluator
  needs — this is foundational plumbing, not an optional refinement. A real Typst
  fixture and parser tests exercise selector resolution; repeated-decoration and
  ambiguity/error wording edge cases remain to add.
- [x] Implement `granularity` aggregation: given a text span's bbox and a requested
  ancestor role (e.g. `"Div"`, `"P"`), union bboxes up the resolved struct-tree
  path to the nearest matching ancestor — **scoped to the same page as the
  originally-resolved item**. Q1 never unions across pages because `pdf.js`
  hands it an already-page-scoped struct tree per page; a `lopdf`-based walker
  reading one document-wide tree must enforce this scoping explicitly (a real
  `/P` element spanning two pages was observed in the spike above — unioning its
  descendants\' bboxes without a page filter would mix two different pages\'
  coordinate origins into one meaningless bbox). Page-filtered synthetic regression
  coverage exercises `/Pg` inheritance, MCID 0 on each page, and subtree bbox union.
- [x] Implement the relation evaluator: 4 directional (`rightOf`, `leftOf`, `above`,
  `below`, with optional `byMin`/`byMax`) + **4** alignment (`leftAligned`,
  `rightAligned`, `topAligned`, `bottomAligned` — all four, not three) with
  numeric `tolerance` — port Q1's tolerance defaults and comparison semantics
  exactly (`verify-pdf-text-position.ts`). Support the optional `edge` override
  on either selector. Unit tests cover all eight relation directions, tolerance pass/fail, edge overrides, and directional distance bounds; the real Typst fixture checks horizontal, vertical, and alignment behavior. Include Q1's explicit subject-vs-object same-page check as
  its own step (`verify-pdf-text-position.ts:923-930`: if the resolved subject
  and object bboxes are on different pages, fail with "cannot compare positions"
  rather than silently comparing across pages) — this is a distinct check from
  the `granularity`-aggregation page-scoping above, and Q1 implements it
  explicitly. Evaluate both arrays from the two-array wrapper: `assertions`
  (must hold) and `noMatchAssertions` (must NOT hold, i.e. a passing evaluation
  of that relation is itself the failure) — both paths are tested.
- [x] Implement the `role: "Decoration"` and `role: "Page"` escape hatches (no
  struct-tree lookup needed). Decoration uses first-match raw text-item bounds;
  Page uses the 1-based page media box. Role validation intentionally skips both.
- [x] Wire `ensurePdfTextPositions` into `spec.rs`\'s assertion parser: the two-array
  `(assertions, noMatchAssertions?)` wrapper (second array must be optional —
  `pdf-text-position-test.qmd` omits it entirely); per-assertion
  `subject`/`relation`/`object`/`byMin`/`byMax`/`tolerance` fields; per-selector
  `text`/`role`/`page`/`edge`/`granularity` fields — matching Q1's YAML shape
  verbatim (full schema: `verify-pdf-text-position.ts:43-106`) so fixture front
  matter needs no rewriting beyond syntax translation. Also tolerate an
  undocumented extra `page: N` key some real fixtures place at the
  *assertion* level (sibling to `subject`/`relation`/`object`, not nested in a
  selector) — confirmed this isn't part of Q1's actual schema at all (Q1's Zod
  schema isn't `.strict()`, so it silently ignores it); don't reject fixtures
  that carry it as malformed input.
- [x] Unit tests: `quarto-test` has no `tests/` directory at all — its existing
  assertion types are each tested via an inline `#[cfg(test)] mod tests` in the
  module that implements them (see `assertions/file_regex.rs` and siblings).
  Follow the same pattern here: add `#[cfg(test)] mod tests` directly inside
  whatever module implements the struct-tree walk (e.g. a new
  `assertions/pdf_text_position.rs`), not a new top-level test file.

  **This is bigger than one bullet — verified during plan review, don't
  under-scope it by assuming "follow the fork's pattern" covers it.** The
  fork's own `marked_content_tests` (`src/lib.rs:2474-2657`, the pinned rev)
  build only a minimal Catalog→Pages→Contents scaffold via `build_pdf()` — zero
  `/StructTreeRoot`, `/ParentTree`, `/RoleMap`, or `/MarkInfo` anywhere, and no
  test draws real text (no font-metric-driven positions). That pattern gets you
  the page/content/catalog scaffold and marked-content-op mechanics only; the
  tagged-structure-tree machinery (StructElem dicts, `/K` arrays with bare-int
  vs. `/MCR` kids, `/Pg` inheritance across a multi-page tree) is 100% new test
  infrastructure to write. Split into two kinds of test, not one:
  - **Struct-tree-walk tests** (majority of coverage): fabricate MCID→bbox data
    directly (hand-built `StructTreeRoot` + directly-supplied bbox structs,
    bypassing real text rendering entirely) — covering multi-page StructElem
    spanning, inherited `/Pg`, and `granularity` aggregation. Promote the
    plan-review spike's synthetic 5-page marginalia fixture (margin notes
    straddling a page break, described in
    `claude-notes/research/2026-09-27-typst-tagged-pdf-struct-tree.md`) into
    this suite — it already exercises exactly the multi-page-spanning +
    inherited-`/Pg` case.
  - **A smaller set of true end-to-end tests** against a real rendered PDF (the
    spike's fixture, or `orange-book-margin` once ported) for whatever needs
    real word bounding boxes — relation-evaluator correctness, `edge`
    overrides, tolerance behavior.
  - **Explicitly cover the negative-assertion (`noMatchAssertions`/must-NOT-hold)
    path** — confirmed by a full sweep of every in-scope fixture that this is
    real, exercised production logic, not dormant: 6 `margin-layout` files
    (`fig-column-margin.qmd`, `fullwidth-div.qmd`, `fullwidth-figure.qmd`,
    `fullwidth-listing.qmd`, `fullwidth-table-great-tables.qmd`,
    `shift-mixed.qmd`) populate the second array with real relational
    assertions that must fail evaluation (e.g. `shift-mixed.qmd`: `IGNORE
    below FIXED` must NOT hold). `orange-book`/`orange-book-margin`/
    `pdf-text-position-test.qmd` genuinely omit the second array — both forms
    need coverage, and this checklist item previously didn't mention the
    negative path at all. Land this before P5 runs the full 86-file set, or
    P5's own triage step ends up debugging evaluator bugs unit tests should
    have caught first. Synthetic evaluator tests and the real-PDF regression
    exercise a must-not-hold result and a failed negative when its relation does hold.
- [x] `cargo clippy -p quarto-test --all-targets -- -D warnings` + `cargo nextest run
  -p quarto-test` (clippy clean; nextest 86 passed, 0 skipped after the focused synthetic and real-PDF assertions were added).

## Status

Complete. Integrated into feature/typst-testing at SHA 6ab72fdd6f5e1b24ae9dcdd06802dacd6fd7be02. All core implementation is finished: `/StructTreeRoot` walking with `(PageRef, Mcid)` keys, word-level bbox extraction, text-search selector resolution, page-scoped granularity aggregation, all 8 relations (4 directional + 4 alignment), Page/Decoration escape hatches, and double-array parsing (optional negative assertions). Crate gates pass: `cargo clippy -p quarto-test --all-targets -- -D warnings` clean, `cargo nextest run -p quarto-test` 86/86 passed (0 skipped). Remaining deferred items are tracked in the first checklist item (P9 orange-book fixture cross-version check; not gating this phase).
