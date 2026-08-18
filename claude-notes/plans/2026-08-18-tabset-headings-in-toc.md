# Headings nested inside tabset panels leak into the TOC and point at hidden content (bd-tabset-headings-in-toc-t04ie7f7)

**Date:** 2026-08-18
**Braid:** bd-tabset-headings-in-toc-t04ie7f7
**Branch:** `main` @ `5b6774d1` (investigated in the main checkout, per `/investigate-beads`; no worktree created)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design — but the strand's stated root cause is wrong, and correcting it opens a
materially better fix than either option the strand proposes.** The bug reproduces exactly as
described at HEAD; what does not hold up is the explanation of *why Q1 differs*, and the
"two-line skip on `.panel-tabset`" that explanation motivates.

> **Update 2026-08-18, after the recursion probe** (`tabset-headings-in-toc-investigation/div-recursion-probe/`).
> Direction (B) is not merely "the principled option" — it is the only one that reaches parity,
> and it is cheaper than this plan first estimated:
>
> - `.callout` **bodies leak too** — a third container beyond tabset panes and blockquotes. A
>   narrow `.panel-tabset` skip does not close the bug.
> - Recursion costs **zero test failures** (12306 passed on the spike). The blast radius feared
>   in "Risks" below did not materialize.
> - The three parts are **coupled**: recurse + restrict *without* pandoc's attribute-merge rule
>   under-collects (`.content-visible`, `.column-margin`, `layout-ncol` entries vanish). All
>   three must land together.
> - Non-recursion is **not** what protects callout/tab titles — the transforms consume those
>   Headers before sectionize runs. Q1 recurses and gets them right by the same mechanism.

## Issue context

Filed 2026-08-18 by Carlos Scheidegger; `bug`, priority 2, label `html`, `open`. Follow-up to
bd-toc-tabset-titles-zq93gjvf (panel-tabset support, landed in 0.23.0), which fixed the *tab
title* leak. Headings written **inside a tab body** still reach the TOC.

Reproduced at HEAD (`5b6774d1`), repro at
`/Users/cscheid/repos/github/cscheid/q2-connect-docs/llms-info/repros/tabset-headings-in-toc/`:

```
Q1: #configuration, #next-steps
q2: #configuration, #create-the-integration, #create-the-integration-1, #next-steps
```

and `#create-the-integration-1` lives in `<div id="tabset-1-2" class="tab-pane">` — no `active`
class, so `display:none`. All three reader-visible problems in the strand check out.

Real-world impact per the strand: 25 of 451 Connect-docs pages, 44 leaked entries, 18 pointing at
an inactive pane; the entire residual `toc:N` bucket of the 0.23.0 chrome sweep.

## Dependency graph

Thin — one edge out, none in.

- **discovered-from → bd-toc-tabset-titles-zq93gjvf** (`in_progress`). The tabset-support strand,
  landed on `main` via PR #543 (`5b6774d1`). Its plan is
  `claude-notes/plans/2026-08-17-tabset-panel-tabset.md`; the markup contract it implements is
  `claude-notes/plans/tabset-panel-tabset-investigation/q1-target-markup.html`. Nothing since
  0.23.0 touches this area.
- **Siblings** (same parent, both `open`, both informational here):
  - bd-47afd5ro — q2-preview renders tabsets via a React `Tabset` component. Preview currently
    excludes the transform pair entirely, so preview's TOC still shows *tab titles*. Any fix
    placed in the **transform** (rather than the collector) is invisible to preview until that
    strand lands; a fix in the collector is not.
  - bd-y5j0m776 — revealjs tabset support. `PanelTabsetTransform` self-gates to non-reveal
    Bootstrap HTML, so on reveal (and on non-HTML formats) the `.panel-tabset` Div passes through
    with its headers intact.
- No incoming `blocks`. Urgency comes from the Connect-docs port, not from a dependent strand.
- **Filed during this investigation** (both `discovered-from` this strand):
  - bd-8yjvs3bj — headings inside a blockquote leak into the TOC (same root cause, different
    container).
  - bd-26nryuwh — `sectionize_blocks` does not recurse into Divs, so q2 emits `<div><h4>` where Q1
    emits `<section class="level4 …">`. This is the second half of the cancelling pair below, and
    a prerequisite for direction (B).

## What the code looks like today

Every path in the description still exists and is accurate:

- `PanelTabsetResolveTransform` — `crates/quarto-core/src/transforms/panel_tabset_resolve.rs`
- pushed at `crates/quarto-core/src/pipeline.rs:1246` / `:1248`, well before
  `SectionizeTransform` (`:1333`) and `TocGenerateTransform` (`:1387`)
- `collect_toc_entries` — `crates/pampa/src/toc.rs:341`
- `sectionize_blocks` — `crates/pampa/src/transforms/sectionize.rs:75`

`cargo xtask verify` is green at `5b6774d1` (pre-flight, full run including the hub leg).

### The strand's root cause is wrong about Q1

The strand says Q1 excludes in-tab headings because
`src/resources/filters/customnodes/panel-tabset.lua` "returns raw HTML for the whole tabset, so
pandoc's TOC pass sees an opaque blob." It does not. `render_tabset`
(`external-sources/quarto-cli/src/resources/filters/customnodes/panel-tabset.lua:72`) returns
**real AST** — `Div(.panel-tabset)[ Plain(nav RawInlines), Div(.tab-content)[ Div(.tab-pane)[…] ] ]`
— which is the *same shape* q2 emits. And Q1's render of the strand's own repro proves headings
inside the panes survive to sectionizing time:

```html
<div id="tabset-1-1" class="tab-pane active" role="tabpanel" …>
<section id="create-the-integration" class="level4">
```

The section exists in Q1's output. It is simply not in Q1's TOC.

### The actual rule

Probe fixture + captured results:
`claude-notes/plans/tabset-headings-in-toc-investigation/div-toc-probe/`.

Pandoc's TOC is **exactly the section tree**. `makeSections` recurses into Divs and, when the Div's
id is empty, *merges* the Div's attributes into the section it wraps; it does **not** descend into
`BlockQuote`. `sectionToListItem` then matches only `Div(_, _, Header : rest)` — a Div whose first
child is not a `Header` ends the walk, with **no recursion past it**.

One rule, four confirmed predictions (Q1 vs q2 at HEAD):

| heading inside…            | Q1       | q2       |
| -------------------------- | -------- | -------- |
| plain `::: {.my-wrapper}`  | included | included |
| `::: {.callout-note}`      | excluded | excluded |
| `> blockquote`             | excluded | **included** ← q2-only leak (bd-8yjvs3bj) |
| `.panel-tabset` pane       | excluded | **included** ← this strand |

- plain div → absorbed into a genuine section → in the TOC;
- callout → the filter replaced it with non-section structure → gone;
- blockquote → never sectionized → a bare `Header`, never matched → gone;
- tabset → the section *is* there, but buried under `Div(.panel-tabset)`, whose first child is
  `Plain(nav)`; the walk stops at that Div.

### Where q2 diverges — two divergences, currently cancelling

1. **`collect_toc_entries` over-collects.** It recurses into *every* non-section Div, and into
   `BlockQuote`, and picks up bare `Header`s wherever it finds them.
2. **`sectionize_blocks` under-sectionizes** (filed as bd-26nryuwh). It never recurses into Divs at all. q2 emits
   `<div class="my-wrapper"><h4 id=…>` where Q1 emits
   `<section id="…" class="level4 my-wrapper">`.

For a plain div the two errors cancel and q2 matches Q1 by accident. For a tabset pane and a
blockquote they compound and q2 leaks. **This is why the strand's option (a) is not simply "two
lines":** the narrow skip fixes the tabset, leaves the blockquote leak, and leaves the
sectionize divergence — which is itself DOM noise for the port beyond the `toc:N` bucket.

### The layering wrinkle the strand flags is real, and has a second edge

The strand worries that pampa should not know the name of a quarto-core construct. There is a
second, sharper problem with pushing the marker into the transform: **`DocumentProfileStage`
(`pipeline.rs:314`) runs before `AstTransformsStage` (`:353`)**. `document_profile.rs:1017`
(`extract_outline`) calls the same `generate_toc` on the *pre-transform* AST, so its outline today
contains both the tab titles *and* the in-tab headings. A marker applied by
`PanelTabsetTransform`/`…Resolve` never reaches it; a rule in the collector, or a check on the
source `panel-tabset` class, covers both. (No in-tree consumer reads `profile.outline` yet, so
this is correctness-in-waiting, not a live symptom.)

## Proposed phases (draft)

Skeleton only — which of the three directions below we take is the first design question, and the
phases differ substantially between them.

- **Phase 0 — Test plan (TDD, failing first).** Unit tests in `crates/pampa/src/toc.rs` for the
  container rule; an end-to-end test in `crates/quarto-core/tests/integration/tabset_pipeline.rs`
  asserting a `####` inside a tab does not appear in `nav#TOC`; the probe fixture promoted to a
  committed regression fixture.
- **Phase 1 — Core change** (see design question 1).
- **Phase 2 — Reconcile `sectionize_blocks`** (bd-26nryuwh) *(direction B only)* — recurse into Divs, merge attrs
  when the Div id is empty, do not descend into `BlockQuote`.
- **Phase 3 — Sweep the fallout** — snapshots, `quarto-ast-reconcile` hashing, `llms.rs`,
  `idempotence.rs`, anything asserting on section structure.
- **Phase 4 — Re-measure against the port** — rerun the Connect-docs chrome sweep; expect the
  `toc:N` bucket to go to ~0.
- **Phase 5 — Docs** if reader-facing behavior changes (direction C).

## Open design questions for the user

1. **Which direction?** Three, not two:
   - **(A) Narrow.** Skip recursion into `.panel-tabset` Divs in `collect_toc_entries`. Smallest
     diff, restores Q1 parity for this bug only. Leaves the blockquote leak and the sectionize
     divergence. Needs a call on the layering wrinkle (see question 2).
   - **(B) Principled — match Pandoc's section-tree rule.** Make `sectionize_blocks` mirror
     `makeSections`, then make `collect_toc_entries` walk only the section tree. Fixes the tabset
     leak, the blockquote leak, *and* the `div`-vs-`section` DOM divergence in one stroke, with no
     `panel-tabset` knowledge in pampa. Larger blast radius (section markup changes for every
     heading inside any Div). My recommendation, if you have appetite for the fallout.
   - **(C) Keep the entries and make them work** — the strand's option (b): qualify each entry with
     its owning tab title and have the TOC link activate the pane before scrolling. Note this is a
     *divergence from Q1 by improvement*; given the port's goal is parity, I'd file it as its own
     feature strand rather than fold it in here.
2. **If (A): where does the marker live?** In `collect_toc_entries` keyed on the literal
   `panel-tabset` class (layering smell, but works pre- *and* post-transform, so it also fixes the
   profile outline), or a generic pampa-owned opt-out class that quarto-core applies (clean
   layering, but applied by the transform it misses `extract_outline`, and it misses preview until
   bd-47afd5ro lands)?
3. **Is the blockquote leak (bd-8yjvs3bj) in scope here, or does it stay its own strand?** Same root
   cause; it falls out of (B) for free, and under (A) it needs its own two lines (delete the
   `BlockQuote` arm).
4. **Non-HTML and reveal formats.** `PanelTabsetTransform` self-gates to Bootstrap HTML, so
   elsewhere the `.panel-tabset` Div passes through with headers intact. Should the TOC rule apply
   uniformly across formats (it would under both A and B), or only where tabsets actually render?
5. **How much snapshot churn is acceptable?** (B) changes rendered section markup for every heading
   nested in a Div. Worth a quick spike to count before committing to it?

## Risks / tradeoffs (draft)

- **(B)'s blast radius measured lower than feared.** `sectionize_blocks` output feeds
  `quarto-ast-reconcile`'s hashing, `llms.rs`, the idempotence tests, and the HTML writer's
  `section` detection — but the recursion spike passed all 12306 workspace tests. The remaining
  risk is the *attribute-merge* half (finding 5), which the spike did not implement.
- **The merge rule is the subtle part.** Pandoc absorbs a Div into the section it wraps only under
  specific conditions (empty Div id, header-led run). Getting it wrong silently drops TOC entries
  rather than erroring — the exact failure the second spike hit.
- **(A) is cheap but leaves known divergences on the floor** — the blockquote leak and the
  `div`-vs-`section` structure — both of which are port-visible.
- **Preview stays divergent under any transform-side fix** until bd-47afd5ro; a collector-side fix
  is inherited by preview automatically. Worth weighing in question 2.
- **The profile outline is silently wrong today** (tab titles *and* in-tab headings). No consumer
  reads it yet, so nothing is visibly broken — but a transform-side fix bakes the wrongness in.
- **Q1 comparison used a quarto-cli dev checkout** (`quarto --version` → `99.9.9`). The structural
  facts above (real AST from `render_tabset`; sections inside panes; the four probe rows) are
  stable behavior, not release-specific, but a release-Q1 spot-check is cheap if you want it.
