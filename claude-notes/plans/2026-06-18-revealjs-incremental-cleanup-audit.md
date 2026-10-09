# Audit: can list-item `itemAttr` simplify the revealjs incremental-list `fragment` machinery?

**Strand:** bd-34vf6fpr (discovered-from bd-aeyss6p5)
**Date:** 2026-06-18
**Status:** audit + recommendation (no code yet)
**Related:** bd-aeyss6p5 (list-item block attrs, merged PR #314),
`claude-notes/plans/2026-06-08-revealjs-presentations.md` (Phase 2d, the
incremental-lists feature, bd-fy793w6i)

## Question

bd-aeyss6p5 gave list items an attribute channel (`itemAttr`). The revealjs
"incremental lists" feature (`<li class="fragment">`) predates it and uses a
*different*, contextual mechanism. Can the new machinery clean up / unify the
fragment work?

**Short answer: partially, and the appealing full unification is blocked by a
real obstacle. The most valuable win (a single per-`<li>` attr-composition
point) already landed for free with bd-aeyss6p5. The remaining duplication is
the contextual *flip* logic, which a transform could centralize — but only if we
first generalize the per-item attr carrier, because the current carrier cannot
mark structurally-complex items (nested lists), which decks use constantly.**

## Audit: how `fragment` reaches an `<li>` today (post-bd-aeyss6p5)

Two parallel renderers, each computing the fragment flag from a contextual
"incremental" state and composing it with the item's own attr.

### Rust (`q2 render`)
| Concern | Location |
| --- | --- |
| Config flags `incremental_lists`, `incremental_default` | `pampa/src/writers/html.rs` `HtmlConfig` |
| Traversal state `HtmlWriterContext.incremental` | `html.rs` (init from `incremental_default`) |
| **Compose** fragment + item classes | `html.rs` `composed_list_item_attr` (prepends `"fragment"`) |
| Emit `<li …>` | `html.rs` `write_list_item` |
| **Contextual flip** on `.incremental`/`.nonincremental`, force-off in `aside` | `html.rs` Div arm (save/restore `ctx.incremental`) |
| Origin: set flags for revealjs from `meta.incremental` | `quarto-core/src/stage/stages/render_html.rs` (gated on `FormatIdentifier::Revealjs`) |

### React (`q2 preview` / `q2-slides`)
| Concern | Location |
| --- | --- |
| `IncrementalContext { enabled, incremental }` | `preview-renderer/src/q2-preview/IncrementalContext.tsx` |
| **Compose** fragment + item classes | `framework/listItemAttr.ts` `liItemAttrProps(attr, fragment)` (mirror of Rust) |
| Consume → emit `<li>` | `blocks/BulletList.tsx`, `blocks/OrderedList.tsx` (the `enabled` branch) |
| Provide `enabled`+global `incremental` | `RevealDeck.tsx` (deck root; reads `meta.incremental`) |
| **Contextual flip** at slide level (`## S {.incremental}`) | `RevealDeck.tsx` `SlideBody` |
| **Contextual flip** on `.incremental`/`.nonincremental`/`.notes` Divs | `blocks/Div.tsx` |

**`IncrementalContext` exists *solely* for this feature** — its only consumers
are the two list components (fragment) and the Div/RevealDeck providers. (Caveat:
the `enabled` field also gates the list components\' *deck-vs-registry* render
branch, which carries an editing distinction — see Obstacle 2.)

### What's duplicated
1. The **compose** rule (`fragment` first, then item classes) — Rust
   `composed_list_item_attr` ≈ React `liItemAttrProps`. *Already a clean
   one-function mirror on each side, introduced by bd-aeyss6p5.*
2. The **contextual flip** rule (global default; `.incremental`/`.nonincremental`
   nesting; `.notes`/aside force-off; slide-heading class) — implemented in
   Rust (`html.rs` Div arm) **and** React (`Div.tsx` + `SlideBody`).

## The realization that motivated this audit

The 2026-06-08 plan classified incremental lists as **"class 3 — structural
generation, needs writer-level support"**, explicitly *because* "list items have
no `Attr`." **bd-aeyss6p5 invalidated that premise** — list items now have an
attr channel. So in principle the feature could be reclassified to **"class 2 —
an AST transform"**, joining `RevealColumnsTransform` / `RevealAutoStretchTransform`
in the `pipeline.rs` `is_revealjs` branch (which runs for **both** render and
preview, unlike the writer-level path which only fires for render). A
`RevealIncrementalListsTransform` would inject the fragment marker once, in the
AST, single-sourcing the rule and letting us delete the writer state **and** the
React context.

`auto_stretch.rs` is the precedent: it already injects a trailing
`Inline::Attr{.caption}` that the writer hoists — the exact mechanism we'd reuse.

## The obstacle (verified empirically 2026-06-18)

Pandoc's `writerIncremental` puts `fragment` on **every** `<li>`, including an
item whose last block is a **nested list**. Confirmed on a real render:

```
- outer item          ->  <li class="fragment">  (last block = nested <ul>)
    - nested item     ->  <li class="fragment">
- second outer        ->  <li class="fragment">
```
All three `<li>` carry `fragment`.

But `split_list_item_attr` (the `itemAttr` carrier) **only hoists a trailing
`Inline::Attr` from a `Para`/`Plain` last block** — by design, because that is
where the parser places an *authored* `- item {.foo}`. For the outer item above,
the last block is a `BulletList`, so the carrier finds nothing and the outer
`<li>` would get **no** `fragment`.

**→ A naive transform-based replacement regresses nested incremental lists**,
which decks use constantly. The carrier that's perfect for *authored* attrs (only
present where there's inline content) is insufficient for *injected* attrs that
must mark *arbitrary* items.

This is the crux: `itemAttr`-via-trailing-`Inline::Attr` is an
*authoring-shaped* channel, while `fragment` is an *every-item, structure-blind*
decoration. They don't align without generalizing the carrier.

## Options

### Option A — Full unification via `RevealIncrementalListsTransform` (+ carrier generalization)
A revealjs AST transform computes the per-item fragment flag once (replicating
the contextual flip in one place) and marks each item; both writers then just
read the marker. Delete `HtmlWriterContext.incremental` + the html.rs Div-flip +
the `incremental_*` config, and the entire React `IncrementalContext`
(BulletList/OrderedList/Div/RevealDeck/SlideBody) fragment plumbing.

**Blocked on** first generalizing the per-item attr carrier so it can mark items
whose last block is structural. Sub-options:
- **A1.** Extend `split_list_item_attr` to also accept a trailing `Inline::Attr`
  on the item's **first** `Para`/`Plain` (every item has a leading one) when the
  last block carries none. Small, but mixes a second position into the carrier's
  contract and risks interaction with authored attrs.
- **A2.** Give list items a more first-class per-item attr (e.g. the transform
  populates an explicit AST-level item-attr list that the json writer projects to
  `itemAttr` directly, not via a trailing inline). Cleanest semantics; largest
  type/plumbing change; effectively "list items get an `Attr`."

**Cost:** new transform replicating the flip traversal (incl. slide-heading
classes + `.notes` force-off + nesting), carrier generalization, migration of
both renderers, re-verify both paths in a browser. **Benefit:** the
`incremental → fragment` rule and the contextual flip live in exactly one place;
the cross-renderer divergence risk the project explicitly fears goes away.

### Option B — Modest consolidation + golden parity (recommended)
Accept that the two-renderer architecture means the *flip* lives in two languages,
and instead minimize drift:
- Extract the flip rule into a tiny, documented spec (the class names
  `incremental`/`nonincremental`/`notes`, the global default, the force-off
  conditions) and reference it from both sides; share a `FRAGMENT_CLASS`/class-name
  constant per language.
- Add a **golden parity test** asserting Rust `q2 render` and React preview emit
  identical `<li class="fragment …">` sets for a fixture matrix (global on/off,
  `.incremental`/`.nonincremental` Divs, slide-heading classes, **nested lists**,
  notes asides, authored item attrs composing with fragment).
- Document in `html.rs`/`IncrementalContext.tsx` the relationship to `itemAttr`
  and *why* fragment is not carried through `itemAttr` (the structural-item
  obstacle), so a future reader doesn't re-attempt Option A without the carrier
  work.

**Cost:** small. **Benefit:** locks the two implementations together against
drift; records the design rationale; no regression risk.

### Option C — Document-only
Note the relationship and stop. bd-aeyss6p5 already unified the per-`<li>`
*compose* step; the rest is inherent to the two-renderer split.

## Recommendation

**Option B now; keep Option A as a documented future direction gated on carrier
generalization (A2).**

Rationale:
- The highest-value, lowest-risk win — a single per-`<li>` attr-composition
  function on each side — **already shipped** with bd-aeyss6p5. Before it,
  fragment and an item class would have collided; now they compose. That was the
  real cleanup, and it's done.
- The remaining duplication (the contextual flip) cannot be deleted without
  Option A, and Option A regresses nested decks unless we first generalize the
  carrier (A2). That is a meaningful, separable piece of work with its own risk —
  not a "clean up a bit" change.
- Option B captures the durable value (anti-drift parity test + rationale) at a
  fraction of the cost and risk, and leaves a clear, correct breadcrumb for
  Option A if/when list items earn a first-class attr.

If the user wants the full unification, the right sequencing is: **(1)** land
A2 (first-class per-item attr / generalized `itemAttr` injection that can mark
any item) with its own tests, **then** **(2)** the `RevealIncrementalListsTransform`
+ deletion of the writer/React contextual machinery.

## Proposed work (Option B) — TDD

- [ ] **Golden parity fixture matrix.** A shared set of revealjs list fixtures
      (global incremental on/off; `.incremental`/`.nonincremental` Div nesting;
      `## Slide {.incremental}`; nested lists; `.notes` aside; an authored
      `- item {.foo}` composing with fragment). Assert the multiset of
      `<li class="…">` is identical between `q2 render` HTML and the React
      preview DOM. (Rust integration test in `revealjs_features.rs` + a
      preview-renderer integration test reading the same fixtures.)
- [ ] **Shared constants + spec.** `FRAGMENT_CLASS` and the
      `incremental`/`nonincremental`/`notes` class names as named constants on
      each side (today they are string literals); a short doc block stating the
      flip rule once.
- [ ] **Rationale comments.** In `composed_list_item_attr` / `liItemAttrProps`
      and `IncrementalContext.tsx`, note the `itemAttr` relationship and the
      structural-item obstacle (link this plan + bd-aeyss6p5).
- [ ] Full `cargo xtask verify`; browser spot-check unchanged.

## Future (Option A) — sketch, not scheduled
- [ ] A2: generalize the per-item attr carrier so an injected attr can mark any
      item (incl. structural last block); tests incl. nested lists.
- [ ] `RevealIncrementalListsTransform` in the `pipeline.rs` `is_revealjs`
      branch, replicating the contextual flip once.
- [ ] Delete `HtmlWriterContext.incremental` + html.rs Div-flip + `incremental_*`
      config; delete React `IncrementalContext` fragment plumbing
      (BulletList/OrderedList/Div/RevealDeck/SlideBody).
- [ ] Re-run the Option-B golden parity matrix (now the regression guard for the
      migration) + browser verify both paths.

## Open questions for the user
1. Is the **single-sourcing** of the contextual flip worth the Option-A cost
   (carrier generalization + transform + dual-renderer migration), or is the
   Option-B parity-test-and-document approach the right altitude for now?
2. If Option A later: prefer **A1** (extend the trailing-attr scan to the first
   block — small, slightly ad hoc) or **A2** (first-class per-item attr — bigger,
   cleaner)?
3. Does anything beyond fragment rely on React `IncrementalContext.enabled` (the
   deck-vs-registry editing branch)? If so, Option A keeps `enabled` and removes
   only the `incremental` half. (Audit suggests `enabled`\'s editing role is
   incidental, but confirm before deleting.)
