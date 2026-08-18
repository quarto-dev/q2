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
> - The three parts are **coupled**: recurse + restrict *without* pandoc's absorb rule
>   under-collects. All three must land together.
>
> **Fallout measured 2026-08-18** — full report:
> `tabset-headings-in-toc-investigation/FALLOUT-B.md`; measured diff:
> `…/spike-B.patch` (spike reverted). On the Connect docs port (451 pages):
> **35 HTML pages change, 46 TOC entries removed, 0 added, and exact TOC parity
> with Q1 goes 421 → 444.** Workspace tests: 12306 passed, 0 failed, no snapshot
> churn. `docs/` (247 pages): 0 files changed.
>
> One new blocker surfaced: **2 pages regress**, and the cause is not (B). Q1
> *unwraps* `.content-visible` Divs; q2 keeps them (10 pages corpus-wide, Q1: 0),
> so the restricted walk stops at the Div. **(B) must be preceded by matching
> Q1's conditional-content unwrapping**, or those 2 pages lose entries.
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
- **Phase 1.5 — Unwrap conditional-content Divs to match Q1** — a prerequisite discovered by the
  fallout measurement; without it 2 pages lose TOC entries. Needs its own strand.
- **Phase 2 — Reconcile `sectionize_blocks`** (bd-26nryuwh) *(direction B only)* — recurse into Divs,
  apply pandoc's absorb rule (empty Div id + header-led run), do not descend into `BlockQuote`.
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
3. ~~**Is the blockquote leak (bd-8yjvs3bj) in scope here?**~~ **Settled:** it comes for free with
   (B); bd-8yjvs3bj closes as absorbed once (B) lands.
4. ~~**Non-HTML and reveal formats.**~~ **Settled: uniform.** The rule lives in pampa
   (`sectionize_blocks` + `collect_toc_entries`), so it is format-agnostic by construction —
   which also keeps the door open for filters that render tabsets for PDF targets. One trap to
   carry into the plan: `SectionizeTransform` is pushed only in the non-reveal branch
   (`pipeline.rs:1332`) while `TocGenerateTransform` is ungated (`:1387`), so a future revealjs
   TOC would find no section tree. Verified q2 emits no `nav#TOC` for revealjs today, so this is
   latent, not live — but sectionize should run for reveal too before anyone adds one.
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

---

# Implementation plan (approved 2026-08-18)

**Direction: (B).** Branch `braid/bd-tabset-headings-in-toc-t04ie7f7` off `main` @ `0aa7cb7c`.

Design questions 1–5 are settled (see above). Phases land in dependency order; each is
independently green, so the tree is never in a state where the TOC is wrong in a *new* way.

**Ordering constraint.** Phase 3 (restrict the walk) must come last: it is the only phase that can
*remove* a TOC entry, and it is correct only once Phases 1 and 2 guarantee that every heading Q1
lists lives in the section tree.

## Phase 0 — Test harness and characterization tests

- [ ] Promote `div-toc-probe` / `div-recursion-probe` fixtures into the workspace as a container
      matrix: for each of {plain div, `content-visible`, `content-hidden`, `column-margin`,
      `layout-ncol`, callout title, callout body, tabset pane, blockquote}, assert the TOC entries
      and the section/div shape.
- [ ] Write these as **characterization tests first** (asserting today's behavior, with the
      divergent rows marked), so each later phase flips a known set of assertions rather than
      landing untested behavior.
- [ ] Route through an end-to-end entry point (`render_document_to_file` or the tabset-pipeline
      integration style), not `render_qmd_to_html` with defaults — per CLAUDE.md's end-to-end rule.

## Phase 1 — Conditional-content: unwrap the resolved wrapper (prerequisite)

Q1's `content-hidden.lua` (`customnodes/content-hidden.lua:56`) resolves a **visible Div** by
returning `el.content` — the wrapper disappears — after `clearHiddenVisibleAttributes` strips both
marker classes and the condition attributes. Spans/CodeBlocks keep their element ("we keep the
scaffolding element, as opposed to in the Div where we return the inlined content", `:154`).
q2 keeps the marker class *and* the Div, so 10 Connect-docs pages carry a
`<div class="content-visible">` Q1 does not emit.

- [ ] Failing test: a visible `::: {.content-visible when-format="html"}` leaves no Div in the
      output; a visible `[x]{.content-visible …}` Span keeps its Span; marker classes are gone in
      both cases.
- [ ] Strip the marker classes on surviving elements (Q1's `clearHiddenVisibleAttributes`).
- [ ] Unwrap a resolved **Div** — but only what the feature itself contributed. After stripping the
      marker class and condition attributes, unwrap iff nothing remains (empty id, no classes, no
      attributes); otherwise keep a plain Div carrying the user's own attributes.
      **Divergence from Q1, deliberate:** Q1 unconditionally returns `el.content`, discarding a
      user's `#id` and extra classes. Preserving them is not lossy and costs nothing — pandoc's
      absorb rule (Phase 2) merges an empty-id wrapper into the section anyway, so the TOC outcome
      is identical. All 33 real uses in the Connect corpus are bare markers, where the two rules
      coincide exactly.
- [ ] Keep the llms two-view path intact (`.quarto-llms-omit` / `.quarto-llms-keep` markers are
      applied *instead of* resolving; unwrapping must not fire on a marked element).
- [ ] Update the module docs — the current text says surviving elements "keep their classes", which
      is the bug.

## Phase 2 — `sectionize_blocks`: recurse into Divs + pandoc's absorb rule (bd-26nryuwh)

- [ ] Failing test: a heading inside a plain Div is wrapped in a section; a Div with an empty id
      wrapping a single header-led run is absorbed, merging its classes/attrs into the section
      (Q1: `class="level4 my-wrapper"`); a Div with a non-empty id keeps the Div and nests the
      section inside; `BlockQuote` content is *not* sectionized.
- [ ] Recurse into non-section Divs.
- [ ] Implement the absorb rule. The spike's version (`spike-B.patch`) required `content.len() == 1`;
      pandoc's real condition is a header-led run, so verify against a Div holding a section
      *followed by* trailing blocks, and a Div holding two sibling sections.
- [ ] Confirm the consume-first transforms still work: `CalloutTransform` / `PanelTabsetTransform`
      run before sectionize and depend on **flat** Headers as direct Div children. Nothing here
      changes that, but the tabset/callout tests must stay green.

## Phase 3 — `collect_toc_entries`: walk only the section tree

- [ ] Failing test: headings inside a tabset pane, a callout body, and a blockquote are absent from
      the TOC; headings inside `content-visible` / `column-margin` / `layout-ncol` / a plain Div are
      still present.
- [ ] A non-section Div terminates the walk (pandoc's `sectionToListItem`).
- [ ] Remove the `BlockQuote` arm.
- [ ] Decide the un-sectionized fallback. `SectionizeTransform` is skipped for revealjs
      (`pipeline.rs:1332`) while `TocGenerateTransform` is ungated (`:1387`). q2 emits no `nav#TOC`
      for reveal today, so nothing breaks — but leave the code honest: either run sectionize for
      reveal too, or document the precondition at `generate_toc`.
- [ ] `document_profile.rs::extract_outline` calls `generate_toc` on the **pre-transform** AST
      (`DocumentProfileStage` at `pipeline.rs:314` runs before `AstTransformsStage` at `:353`), so
      its outline is un-sectionized. Check what the new rule does to it and record the answer.

## Phase 4 — End-to-end verification

- [ ] `cargo nextest run --workspace`, then `cargo xtask verify` (full — pampa/quarto-core are in
      hub-client's WASM closure).
- [ ] Re-render the Connect corpus with the clean double-render method (see `FALLOUT-B.md` — an
      incremental baseline invalidates the diff) and confirm: exact TOC parity with Q1 rises from
      **421 → 444 of 451**, TOC entries added **= 0**, and the 2 `content-visible` regressions are
      gone (target ≥ 446 once Phase 1 lands).
- [ ] Review every changed page against Q1, not just the counts.
- [ ] Record the invocations and observed output in this plan.

## Phase 5 — Bookkeeping

- [ ] Close bd-8yjvs3bj (blockquote leak) as absorbed by Phase 3.
- [ ] Close bd-26nryuwh (sectionize recursion) as delivered by Phase 2.
- [ ] File the conditional-content unwrap as its own strand (Phase 1) so the fix is attributable.
- [ ] Docs: user-visible behavior change is "headings inside tabsets/callouts no longer appear in
      the TOC" — check whether `docs/` says anything about TOC contents that needs updating.
- [ ] Commit at each phase boundary; do not push without approval.
