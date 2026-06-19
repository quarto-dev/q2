# Nesting cursor — list-item surfaces & line-anchored navigation

**Date:** 2026-06-15
**Branch:** feature/block-editing-improvements (worktree `.worktrees/block-editing`)
**Builds on:**
- `2026-06-14-nesting-cursor-ui-enhancements.md` (geometry snapshot §1, caret-aware nest-in §2,
  mode-aware highlight §3) — the substrate this plan refines.
- `2026-06-15-breadcrumb-visual-design.md` — **run that first**; this plan is a successor that
  builds on the restyled chip (see *Sequencing* below).
**Status:** Design settled across an extended review/brainstorm (2026-06-15). All cross-feature
ambiguities resolved with the user (recorded in *Design decisions* below). TDD-first; checklist
ready to execute.

> **All cited production files live under `ts-packages/preview-renderer/src/q2-preview/`** unless a
> path is given (e.g. `../framework/dispatch.tsx`, `crates/pampa/src/...`). Playwright acceptance
> specs live under `hub-client/e2e/` (that SPA bundles `preview-renderer` from source and runs the
> real WASM, so the iframe-side TS is exercised there).

## Review amendments (2026-06-16) — READ FIRST

A pre-execution review re-verified every cited `file:line` against current source (all accurate)
and parsed the actual AST for each list/def-list shape with `pampa -t json`. The factual base holds;
the items below tighten under-specified spots and correct two framings. Where an amendment changes a
settled section, the inline text there now points here.

**A1 — §0 gating predicate is `item[0].t === 'Plain'` (not the vague "element-less-or-leading").**
Scanning `blocks/*.tsx`, **`Plain` is the only block component that renders a fragment** (`<>…</>`);
every other block renders its own element and, when editable, its own `data-block-pool-id`. Parsed
shapes confirm: tight item → `[Plain]` (element-less, borrow OK); **loose** item → `[Para]`
(renders `<p data-block-pool-id>` → borrowing `item[0].s` onto `<li>` **duplicates** the id);
**sublist-leading** item (`-  - alpha`) → `[BulletList]` (own `<ul>`+id → duplicate); empty → `[]`.
So borrow the leading pool-id onto `<li>`/`<dd>` **iff**: `item.length > 0` (the §0.f guard, checked
first) **and** `item[0].t === 'Plain'` **and** `resolveSource(item[0])` non-Opaque +
`item[0].s !== undefined` + `!editingDisabled`. For any non-`Plain` leading block, leave the item
alone — its leading block already renders its own pool-id'd, activatable, measurable surface.
**New test (the §0 suite lacks it):** a *loose* list item renders `<li>` with **no**
`data-block-pool-id` and the inner `<p>` keeps the sole id (no duplicate). Added as **0.g**.

**A2 — §1 must skip *container-gap* lines (the structural-line case).** `surfaceLineSpan` trims, but
a `BulletList`'s trimmed span still covers an empty item's marker line, and a `<dl>`'s span covers
its `<dt>` term lines. So C1's "deepest surface whose trimmed span contains L" resolves those lines
to the **container** — the exact drop-into-whole-list outcome Rule B's table (row 1) was built to
prevent. (The current `outerByLine` resolver only dodges this because it iterates *blocks*, not
lines, `PreviewRoot.tsx:560-581`; §1's line walk loses that.) **Rule:** advance past line L when the
deepest surface whose trimmed span contains L is **not a leaf for L** — it is a container *and* L
lies in none of its descendant surfaces' spans (inter-item markers, blank lines inside a container,
empty items, `<dt>` term lines). Equivalently: only land when `surfaceAtLine(L)` is a leaf in the
active set; otherwise treat L as contentless and continue to L±1. **Decision recorded:** `<dt>` term
lines are non-landable (consistent with "terms are out"); the alternative (land on the whole `<dl>`)
is rejected. Folded into §1's Behavior clause and into C1's `surfaceAtLine` definition.

**A3 — DefinitionList is real, authorable, and source-tracked (so §0's `<dd>` work is legitimate).**
A `::: definition-list` fenced div is rewritten by `postprocess.rs:803` into a `DefinitionList`
carrying the div's `source_info`; the qmd grammar has no separate def-list rule. Parsed:
`DefinitionList s:0` (editable), each definition body leads with a `Plain` (`s:2`) — the A1 happy
path. The `<dt>` term is `[[Str]]` (inlines, no block node) → correctly out. **The A1 predicate and
the §0.f empty-guard apply unchanged to each `<dd>`'s leading block** (a definition body that leads
with a `Para`/sublist must not borrow).

**A4 — centralize the proxy measure in the helper (don't enumerate sites).** C3's "everywhere a
proxy's box is read" list is incomplete: §1's clean hop measures the destination live via
`openFromResolved` → `captureGeometry(r.box.measure, …)` (`PreviewRoot.tsx:607`, box from `:587`) — a
**fourth** site beyond the three C3 names. Rather than patch four call sites, put the proxy detection
**inside `measureBlockBox`/`captureGeometry`** ("element is an `<li>`/`<dd>` whose first block-pool-id
child begins a nested block → measure the leading-block Range, else the element"). Then all sites —
snapshot capture, snapshot-miss fallback, §2 outline, and the §1 live measure — are correct by
construction and the list can't go stale.

**A5 — execution DAG (only "breadcrumb-first" and "§4 with/before §6" were written).** §1/§2's
unlock-mode list behavior is impossible without §0's proxies in the C1 set, and §6's empty-item path
needs §0.f. Order:

```
breadcrumb-visual-design (prereq, ~done)
└─ §5  one-level-descent guard      (test-only; land early to pin behavior)
└─ §0  list-item surfaces           (foundational: C1 proxies + 0.f guard)
   ├─ §1  line-anchored nav         (needs §0 proxies + the A2 skip rule)
   │     └─ §2  roving + outline     (same set; parallel-OK after §0)
   └─ §6  delete-by-emptying        (needs §0.f + §4)
§3  trailing-ws descent fix         (independent bug fix; before §1's resolver refactor)
§4  dirty-column prefixWidth        (independent; MUST precede §6)
§7  expand-on-edit                  (independent editor state; any time)
```

Load-bearing edges: **§0 → {§1, §2, §6}** and **§4 → §6**; §3 and §7 float.

**A6 — emptied items are "not their own surface," not "non-refillable" (corrects §6 Consequences).**
Parsed `- foo\n-\n- bar`: the empty item is `[]` with no node, no `s`, and no pool entry on the bare
`-` line — so it is never its *own* landing/edit target (click climbs to the `<ul>`; arrow resolves
via A2 to the enclosing list). But the content **is recoverable in-preview**: activating the
enclosing `BulletList` (exactly what clicking the empty bullet does) opens its source text, which
round-trips through `apply_node_edit` replacing the whole list. Only addressing the empty *item slot*
needs `setAstRange` (out of scope). §6's wording is corrected accordingly.

**A7 — two earlier review worries were checked and dismissed (recorded so they aren't re-raised).**
(i) §6 "a second delete is a stale-AST no-op anyway" is **sound**: `lookup_block` matches
`source_info` *exactly* (`node_lookup.rs:83`), and `incremental_write` shifts all ranges after a
delete, so a stale range matches nothing → genuine no-op (it cannot hit the shifted neighbor).
(ii) §7's `editExpandedRef` preserve/reset is **sound**: self-heal re-anchors via `setEditTargetRaw`
(`PreviewRoot.tsx:355`), **not** `openEditTarget`, so the ref is preserved across self-heal and reset
only on genuine opens — 7.g/7.h hold as written. The one still-open item is §6 blur-delete undo
forwarding (needs a real browser check; can't be settled statically).

## Overview

This plan grew out of two reported problems with the nesting cursor and a review of its navigation:

1. **Sizing bug:** nest-in to a list item sizes the editor to the *container* (the whole list),
   not the item.
2. **Navigation:** up/down only travels top-level blocks; it should behave like an ordinary cursor
   (move by visible line, descending/ascending nesting as the structure dictates).

Both resolve under one principle — **follow the Pandoc/Pampa AST: every block node is a navigable,
correctly-sized, editable surface; arrays-of-blocks (list items) are *not* nodes.** That gives:

- **§0 — List-item surfaces.** Make a list item's *leading block* clickable, correctly sized, and
  roving-reachable, with **no wrapper DOM nodes and no AST changes**, by letting the `<li>`/`<dd>`
  *borrow* the leading block's pool-id and measuring the leading block's rendered extent.
- **§1 — Line-anchored navigation.** Replace top-level-only up/down with one line-anchored resolver
  over a **mode-switched surface set**; locked mode is the *same algorithm* with the coarse set, so
  it is behavior-preserving (except an intended wrap→clamp).
- **§2 — Mode-aware roving + outline** over that same surface set (finishing §3 of the prior plan
  under the new partition).
- **§3 — Q2 trailing-whitespace descent fix.** (targeted bug fix)
- **§4 — Principle-A dirty-column caret consistency.** (small refinement)
- **§5 — Q1 one-level-descent regression guard.**
- **§6 — Delete a node by emptying it.** Empty-then-commit removes the node; backend already
  supports it via an empty-blocks splice (no `setAst`/Rust changes). (new)
- **§7 — Expand-on-edit.** A third editor size state: the surface grows to fit all its text on the
  first in-surface keystroke (leave keys never expand); only-grows, stays text-fitted while open.
  (new, frontend-only)

**Out of scope (recorded, not built): `setAstRange`** — editing a multi-block list item *as a unit*.
See *Out of scope* below.

---

## Sequencing & interaction with the breadcrumb visual-design plan

**Run `2026-06-15-breadcrumb-visual-design.md` first; execute this plan on top of it.** Rationale:
the breadcrumb is a finished, isolated, low-risk visual pass that does **not** depend on our changes
for correctness (its labels/colors key on node *type* from the source index, which we don't touch;
its margin-aligned position is surface-independent). Landing the small finished change first means
this larger plan develops on top of it (absorbing its `nestingNav.ts` additions for free) rather
than rebasing a big finished plan onto a churny small one.

**No contradictions.** The breadcrumb's contract rests on `buildAncestorPath` *membership/ordering*
staying stable. We add **no source-index surfaces** (items are not nodes; the `<li>` pool-id borrow
is a DOM tag reusing an existing pool index), so `buildAncestorPath` is unchanged. ✓

**Synergies.**
- The breadcrumb already abbreviates `Para`/`Plain` → `¶`, so editing list-item text reads `• › ¶`
  (and a sublist item reads `• › • › ¶`) — no jargon "Plain" crumb, and correctly **no "item"
  crumb** (consistent with the no-item-surface decision).
- The breadcrumb's margin-aligned positioning gracefully handles the *indented* editing surfaces our
  §0 creates (it glues to the page text column, not the surface's indented left edge).

**Watch-items (carried into the breadcrumb plan as forward-compat edits):**
1. The breadcrumb's behavior-contract nav tests must assert **landed surface/range**, not caret
   column — our §3/§4 change caret placement on dirty moves.
2. The breadcrumb's e2e geometry fixture should activate a surface **we don't change** (a *loose*-list
   `Para` or a blockquote `Para`, already pool-id'd) — **not** a tight-list item (`Plain`), which §0
   newly turns into a directly-activated surface.

---

## Core concepts (shared primitives the items below reuse)

### C1 — Visible-surface line partition (replaces DOM-leaf enumeration)

For navigation and roving we need "the surface that owns visible line L." Define it over the
**visible** `[data-block-pool-id]` elements (now including the §0 `<li>`/`<dd>` proxies):

> `surfaceAtLine(set, L)` = among surfaces in `set` whose **trimmed line-span**
> (`surfaceLineSpan`, `nestingNav.ts:287`) contains L, the one of **greatest containment depth**
> (`depthOfSurface`, `nestingNav.ts:169`). **(Amendment A2)** If that surface is a *container* (it has
> a descendant surface) and L lies in **none** of its descendant surfaces' spans, L is a
> *container-gap* line (inter-item marker, empty item, blank-in-container, `<dt>` term) — return
> `null` so §1 skips it. Only a **leaf for L** is a valid landing.

This is **total over visible content lines** and correctly handles a multi-block item's proxy: the
`<li>` for `two` owns only its text line (its borrowed range is the leading block's `[8,14]`), while
its sublist owns the lines below. **This replaces `enumerateNestingLeaves`** (the DOM-leaf "no
pool-id descendant" definition, `outerBlocks.ts:214`), which wrongly classifies a multi-block `<li>`
as a non-leaf and would drop its text line from the set.

### C2 — Mode-switched surface set

The lock chooses the partition; the algorithm is identical:

| Mode | Active set | "deepest owner of line L" is… |
|---|---|---|
| **Locked** | outer blocks (`enumerateOuterBlocks`) | the whole top-level block (a list = one stop) |
| **Unlocked** | visible-surface partition (C1) | the innermost content at L |

Both up/down navigation (§1) **and** roving (§2) consume this set. Locked mode reduces to today's
"outer block to outer block" travel.

### C3 — Leading-block measurement (no wrapper nodes)

A list item is a bare `[Block]` array — no node, no source range (verified: the parser pool for a
nested list has entries only for `BulletList`/`Plain`/`Str`, never for the item). Its **leading
block** *is* a node (e.g. tight item → `Plain [8,14]`), editable via `setAst` today. It just renders
no element (`Plain.tsx:6` → `<>{inlines}</>`), so the geometry snapshot can't measure it.

Fix: the `<li>`/`<dd>` **borrows the leading block's pool-id** (`data-block-pool-id = item[0].s`
when `item[0]` is editable), making the existing element the leading block's DOM proxy. Then measure
**the leading block's rendered extent**:

> **the `<li>`/`<dd>` content up to (but not including) any nested block.**
> - Single-block item → that extent *is* the whole `<li>` → measure the element (today's
>   `measureBlockBox`).
> - Text-with-sublist item → that extent is the leading text run, ending where the sublist begins →
>   a DOM **`Range`** from the `<li>`'s start to its first child carrying a block pool-id.

**Governing invariant:** *for an `<li>`/`<dd>` proxy, the leading block's visual extent is the
Range, not the element box.* **(Amendment A4 — preferred wiring: put this detection *inside*
`measureBlockBox`/`captureGeometry` rather than enumerating call sites, so it can't go stale.)** It
must hold everywhere a proxy's box is read — snapshot capture, the snapshot-miss fallback
(`openEditTarget` box:'snapshot' else-branch, `PreviewRoot.tsx:454-464`), the §2 hover/focus outline,
**and the §1 clean-hop live measure** (`openFromResolved` → `captureGeometry`, `PreviewRoot.tsx:607`,
box from `:587` — the fourth site this list originally omitted).

---

## §0 — List-item surfaces (P)

### Goal
A list item's leading block is **clickable** (unlock mode), **correctly sized** on activation and
nest-in, and **roving-reachable** — no wrapper nodes, no AST changes, per-node editing only.

### Wiring — Option B (q2-preview owns `<li>`/`<dd>`)
The `<li>` is currently emitted by the **framework** walk (`../framework/dispatch.tsx:197-210`),
which is shared with q2-debug and lacks `PreviewContext`. So q2-preview's list components render
their **own** items (precedent: their existing incremental-revealjs branches already do, and
`DefinitionList.tsx:49-51` already renders its own `<dd>`s):

- `BulletList.tsx` / `OrderedList.tsx`: non-incremental branch stops delegating to `renderChildren`
  and maps items itself, emitting `<li data-block-pool-id={item[0].s} tabIndex={-1}>` **gated** on
  the leading block being editable (`resolveSource(item[0])` non-Opaque, `item[0].s !== undefined`,
  `!editingDisabled`) **and `item[0].t === 'Plain'`** (see **Amendment A1** — `Plain` is the only
  element-less block; loose `Para` / sublist-leading / code-leading items already render their own
  pool-id'd surface and must **not** borrow, else the id duplicates). Children render via `<Node setLocalAst={NOOP}>`
  (q2-preview commits via source-range `SET_AST`, not `setLocalAst` — verified: the only q2-preview
  `setLocalAst` users are CustomNode slots).
- **Empty-item guard (required — see §6):** branch on `item.length === 0` **first** and render a bare
  `<li>` (no pool-id, no proxy, no measure). `item[0].s` would throw on an empty item and break the
  whole list render; empty items already exist in authored source (`- ` with nothing) and **§6's
  uniform-delete makes them routine** (every "delete a bullet's text"). This must not regress the
  framework walk's current empty-item handling.
- `DefinitionList.tsx`: add the same borrow to each `<dd>`'s leading block. `<dt>` (terms are
  *inlines*, not block nodes) stays out — definitions are editable, terms are not (documented
  asymmetry).

### Measurement (C3)
- `snapshotOuterBlockGeometry` (`outerBlocks.ts:506`) and the snapshot-miss fallback must, for an
  `<li>`/`<dd>` proxy, measure the **leading-block extent** (Range when the element has a nested
  block child; whole element otherwise), keyed by the borrowed pool-id's range. Add a `Range`-aware
  measure helper beside `measureBlockBox` (a `Plain`'s box model is empty → `contentHeight` = rect
  height, trivial `boxStyle`).
- Snapshot keys are unchanged (block-relative `(r0−topR0, r1−topR0)`); the leading block's range is
  the borrowed pool-id's range, so nest-in to that block hits the snapshot. The §1 key-uniqueness
  assertion (prior plan Reflection #10) extends to include `<li>`/`<dd>` proxies.

### Activation / mode interaction (verified)
- **Locked unchanged:** `resolveOuterBlock` climbs to the outermost prefixing `<ul>/<ol>/<dl>`
  (`PREFIXING_TAGS`, `outerBlocks.ts:65`), so a click still activates the whole list. The proxy
  pool-id only affects **unlock** mode (leaf = the `<li>`). This is the *only* locked-mode-relevant
  check and it does not change.
- **Visual vs AST nesting (documented model limitation):** a sublist is the AST **sibling** of the
  item's leading text, not its child. So **nest-in from the leading text is a no-op** (it's a leaf);
  the sublist is reached by nest-**out** then nest-**in** (or, far more commonly, by §1 arrows,
  which flow `two → alpha` by line and hide the distinction). Document this so it reads as intended.

### Tests (TDD-first)
- [ ] **0.a** `nestingNav`/`outerBlocks` unit: `surfaceLineSpan`/`depthOfSurface` already covered;
  add a unit for the new `Range`-aware leading-block measure (jsdom: stub `getClientRects`).
- [ ] **0.b** Integration (jsdom): a tight single-block list renders `<li data-block-pool-id>`;
  `snapshotOuterBlockGeometry` keys the item by its leading block's range; a click in unlock mode
  activates the item (editTarget = the `Plain`), and locked mode still activates the whole list.
- [ ] **0.c** Integration: a text-with-sublist item — the `<li>` borrows the leading `Plain`'s
  pool-id; the leading-block Range measure excludes the sublist height (assert the measured height
  < the full `<li>` height when the element has a sublist child).
- [ ] **0.d** e2e (`hub-client/e2e/`, real browser, **fail-on-revert**): nest-in to a **tight
  single-block** item → the editor sizes to the item, **not** the list (assert `≈ item height`,
  `< list height`). This is the reported bug.
- [ ] **0.e** e2e: nest-in to the **leading text of a text-with-sublist** item → editor is one line
  tall (the leading-block Range), not the full `<li>` (the C3 generalization).
- [ ] **0.f** Integration (jsdom): an **empty** list item (authored `- `, or produced by a §6 delete)
  renders a bare `<li>` with **no** `data-block-pool-id` and does **not** crash the list render.
- [ ] **0.g** Integration (jsdom, **Amendment A1** — the missing predicate test): a **loose** list
  item (leading `Para`) renders `<li>` with **no** `data-block-pool-id`, and the inner `<p>` retains
  the **sole** pool-id (no duplicate). RED if the borrow is gated on anything weaker than
  `item[0].t === 'Plain'`. Pair with a `<dd>` whose definition body leads with a `Para` (same assert).

### Consequences
- **More pool-ids → cost:** one Range measure per item on activation; fine normally, watch on long
  lists (gate/lazy-measure if it ever shows in a profile).
- **Existing test-premise churn:** nesting tests that click list text expecting the *list* to
  activate now activate the *item* in unlock mode — audit + update (this is intended).
- **Inherited commit behavior:** editing item text commits to the leading block's range, which
  re-serializes the whole top-level list (loose↔tight normalization possible) — already true via
  nest-in; now reachable by direct click.

---

## §1 — Line-anchored navigation (Q3 + locked unification + clamp)

### Behavior (Rule B)
Up/down is an ordinary cursor: **down = current source line + 1; up = current source line − 1; skip
blank/contentless lines (we navigate the *visible* set, so suppressed divs/blanks aren't in it) —
**including *container-gap* lines (Amendment A2: empty-item markers, `<dt>` terms) where
`surfaceAtLine` returns `null`**; clamp at the document ends (no wrap); activate
`surfaceAtLine(set, target)` (C1/C2).** The current
surface's "exit line" is its last line (down) / first line (up), matching the existing edge
detection (`isOnLastVisualLine`/`isOnFirstVisualLine`, `dispatchers.tsx:312-314`).

Worked example (`- one / - two / [sub] alpha / beta / - three`), **unlock**: `two → alpha → beta →
three` (depth changes fall out of "the deepest surface at the next line"). **Locked**: `para → whole
list → blockquote` (the list is one stop) — *identical to today*.

**Why Rule B and not depth-preserving:** a container always shares its lines with its content at a
shallower depth, so any "prefer same/shallower depth" rule grabs the container — e.g. ↓ from a
top-level paragraph would drop you into editing the *whole bullet list*. The only coherent
line-based rule is "deepest visible surface at the adjacent line." (Full example table in *Design
decisions* below.)

### Implementation
Generalize `resolveLanding`'s `outerByLine` kind (`PreviewRoot.tsx:554-587`) into a
`lineSurface` resolver parameterized by the surface set:
- **Locked** passes `enumerateOuterBlocks` → behavior-preserving (the up/down edge asymmetry pinned
  by prior-plan Reflection #21 is just "exit from bottom edge / top edge" and survives).
- **Unlock** passes the C1 partition → Rule B.
- Box source: the destination on a clean hop is rendered clean (only the *source* surface is a
  textarea), so **measure the destination live** (via the C3 proxy-aware measure when it's an
  `<li>`/`<dd>`); dirty moves use the existing commit-then-reland. Caret lands via the §2 widened
  `{line, column}` hint at `target − destStartLine`, preserving `exitColumn` (this *is* Principle B).
- **Clamp at ends** in both modes (remove the wrap branches at `:569`/`:580`).

### Tests (TDD-first)
- [ ] **1.a** Pure unit for `surfaceAtLine(set, L)`: the worked-example table (unlock leaves);
  blank-line skip; clamp at ends; container-start drops to the leaf at the adjacent line.
- [ ] **1.b** Characterization (**before** refactor): pin current **locked** up/down outer-block
  travel so the `lineSurface` generalization is proven behavior-preserving (extends Reflection #21's
  test).
- [ ] **1.c** Integration (jsdom): unlock up/down flows leaf-to-leaf across a sublist boundary
  (`two ↓ alpha ↓ beta ↓ three`; reverse for ↑); clamp at top/bottom (no wrap).
- [ ] **1.d** e2e (real browser): unlock arrow-down from a list item into its sublist's first item,
  caret column preserved; arrow at the document end no-ops.
- [ ] **1.e** Update the existing locked-mode up/down tests for **wrap→clamp** (the one intended
  locked change); all other locked assertions stay green.

### Consequences
- **The one locked change:** wrap→clamp at the ends (you called wrapping a misfeature). Outer-block
  travel itself is unchanged.
- **Flat docs unaffected** (a top-level paragraph's leaf is itself); behavior changes only around
  nested structures (the intended fix).

---

## §2 — Mode-aware roving + outline (over the shared set)

### Roving (your #2: roving follows the interface's level of detail)
`onKeyDown` roving (`useBlockEditHover.tsx:227`) focuses the **mode-switched set** (C2): locked →
`enumerateOuterBlocks` (today); unlock → the **C1 partition** (so the multi-block item's leading
text — a non-DOM-leaf — is included, fixing the gap where it would otherwise be skipped). Set both
`rawLeafRef` and `hoveredRef` to the focused element.

### Outline (your #1: accept whole-`<li>` box-shadow; overlay rejected)
Keep the existing `box-shadow`-on-element outline (`outlineElement`, `useBlockEditHover.tsx:49`). For
a **multi-block** `<li>` proxy this over-covers (outlines text + sublist) while activation is the
leading text — a *bounded, documented* mismatch that only affects text-with-sublist items
(single-block `<li>` == its text, exact). An **overlay** outline is rejected: it induces manual
scroll/resize/reflow tracking (the iframe is scroll-synced), multi-rect geometry for wrapped text,
two parallel outline mechanisms (shadow vs drawn), and would have to absorb the focus ring too.

### Tests (TDD-first)
- [ ] **2.a** Integration: unlock roving visits the C1 partition (incl. a multi-block item's leading
  text); locked roving visits outer blocks (unchanged).
- [ ] **2.b** Integration: hovering a single-block item outlines exactly the item; (documented) a
  multi-block item outlines the whole `<li>` — assert the element that carries the shadow, and that
  it equals the activation target's element.

### Consequences
- Whole-`<li>` outline over-cover for multi-block items: accepted/documented.
- Locked-mode CSS: the `[data-block-pool-id]` hover style now also matches `<li>`s; in locked mode
  click still activates the `<ul>`, so the cursor style on items is cosmetic — accept, or scope the
  CSS to unlock mode (decide during 2.b).

---

## §3 — Q2 trailing-whitespace descent fix (bug fix, repro-first)

### Symptom
Nest-in gives different results from the **beginning** of a line vs **inside** it; descent should
ignore trailing whitespace consistently.

### Mechanism (established)
Descent is line-based and column-independent by design, and `surfaceLineSpan` already trims — so two
positions on the *same* line should not diverge. Candidate causes:
1. the two positions are on **different source lines** (correct caret-aware behavior — *not* a bug);
2. the caret's `Ls = lineOf(anchorR0) + bufferLine` (`PreviewRoot.tsx:1087`) **overshoots** when the
   caret sits in trailing whitespace / a trailing blank line → "nearest child" fallback;
3. the byte-space fallback `childSurfaceToward` (`nestingNav.ts:215`, used when the line path returns
   null, `PreviewRoot.tsx:1101` / `:524`) does **not** trim → disagrees with the trim-aware path.

### Fix
- [ ] **3.a** TDD: build the exact beginning-vs-inside case (jsdom + e2e) and identify which cause
  fires. **Write the failing test first.**
- [ ] **3.b** Clamp the caret-derived `Ls` to the current surface's **trimmed** span
  (`surfaceLineSpan`) so a caret in trailing whitespace maps to the last content line (kills cause 2).
- [ ] **3.c** Always use the trim-aware line path when a caret exists; align (or retire) the
  non-trimming byte fallback so it can't disagree (kills cause 3).
- [ ] **3.d** If the repro is cause 1, **don't "fix" it** — document that descent is caret-line-driven
  by design and close.

---

## §4 — Principle-A dirty-column caret consistency (refinement)

"Same line/column going in/out with the nesting cursor": the **clean** nest path maps the invariant
`(Ls,Cs)` via `cleanCaretHint`/`prefixWidth` (`PreviewRoot.tsx:1054-1064`), but the **dirty** path
projects a raw `caretBufferCol` with no `prefixWidth` (`PreviewRoot.tsx:526-527`, `:1123-1127`), so
clean vs dirty in/out can differ by a column.

- [ ] **4.a** TDD: dirty nest-in/out lands on the same column as the equivalent clean move (assert
  caret column after a dirty round-trip).
- [ ] **4.b** Route the dirty path's column through `prefixWidth` like the clean path.

---

## §5 — Q1 one-level-descent regression guard

Nest-in already descends exactly **one level** (`childSurfaceTowardLine` picks a single direct
child, `nestingNav.ts:326`) — your stated preference. No change; just lock it:

- [ ] **5.a** Add/confirm a regression test that nest-in from a 3-level structure descends exactly
  one level (not to the deepest leaf), so §0/§1 can't silently regress it.

---

## §6 — Delete a node by emptying it (no `setAst` changes needed)

### Goal
Emptying a node's text and committing **deletes the node** instead of silently cancelling. This is
the first delete affordance for block editing (we have no other way to remove an element).

### Key finding — the backend already deletes; no `null` / no Rust work
`setAst` does **not** need a literal `null`. Deletion already works mechanically through the
*existing* text channel with an empty `newText` — verified end-to-end:
1. `commitTextEdit` (`PreviewRoot.tsx:1294`) sends `{channel:'text', newText:''}`.
2. Parent routing (`ReactPreview.tsx:527`) parses it: `parse_qmd_content("")` → `{"blocks":[]}`
   (confirmed: `printf '' | pampa -t json` → zero blocks).
3. `apply_node_edit` (`apply_node_edit.rs:170` → `splice_in_blocks:198`) splices an **empty**
   `Vec<Block>` at the leaf: `current.splice(leaf_idx..=leaf_idx, vec![])` → **removes the block**;
   reconciliation + `incremental_write` delete it from the QMD source.

So the only blocker is the **frontend cancel-on-empty guard** (intentional today,
`dispatchers.tsx:204-212`: *"An empty draft would delete the block … we restore [the guard] here"*).

### Behavior (resolved with the user)
**Delete is uniform — never blocked.** Emptying a block and committing it *in any way*
(Cmd/Ctrl+Enter, arrow-away, **or blur/click-away**) deletes its contents. Cancel (no-op close) is
preserved only when there is nothing to delete:
- `!normalized && baseline` (block had content, now empty) → **delete** (commit empty text).
- `!normalized && !baseline` (block was already empty) → **cancel** (nothing to remove).
- `normalized === baseline` (unchanged) → **cancel** (today's behavior).

Deleting the leading/sole block of a **list item** leaves an empty item `[]`; deleting the sole block
of a **Div/BlockQuote/Figure** leaves an empty container. **Both are accepted** — awkward but valid
(empty items exist in Pandoc and round-trip). No special-casing, no frontend item-awareness, no
backend guard: this keeps §6 small and consistent with the rest of the new editing behavior.

### Arrow-away deletion landing (your A — neighbor is the anchor)
When the emptied block is left via an **edge arrow** (§1 cross-surface nav):
- An edge arrow only *leaves* when it isn't clamped, so **"a neighbor exists" ⟺ "the arrow leaves"** —
  the existing edge/clamp gate (`isOnLast/FirstVisualLine` + §1's clamp) already decides this. **No
  separate neighbor-finding code is needed.** At a document end the arrow **no-ops → nothing is
  deleted** (delete the terminal block via Enter/blur, or the *other* arrow). *(This makes the
  "no neighbor" case vacuous — clamping means you never left the first/last block in the first place.)*
- The reland anchors on the **neighbor, not self** (self is gone). Because the deleted block's lines
  vanish, anchor by **line in the new source index**, not raw byte offset: ↓ → the deleted block's
  start line (the former-next surface now occupies it); ↑ → start line − 1 (unchanged by the
  deletion). This is exactly §1's existing `(startLine, depth)`-in-the-new-index reland, re-pointed at
  the deletion-point line — robust to however many lines the writer actually removes.
- Caret column = `exitColumn` (Principle B), routed through **§4's `prefixWidth` path** (the delete is
  a dirty commit) → §4 must land with/before §6's arrow-delete.

### Implementation (frontend + one backend check)
- `commitIfDirty` (`dispatchers.tsx:209`) — split the combined guard: when `normalized` is empty but
  `baseline` is non-empty, fall through to `commitTextEdit(dest, '')` (delete) instead of
  `setEditTarget(null)`. Keep the cancel path for already-empty / unchanged. This handler backs both
  Cmd/Ctrl+Enter (`:290`) and blur (`:274`), so "any commit incl. blur" is covered in one place.
- Arrow-away `isDirty` (`dispatchers.tsx:319-324`) — an emptied non-empty block counts as **dirty**
  (delete-on-exit); the reland anchor switches self→deletion-point line (above). **Single-commit:**
  arrow-away both moves and blurs; today's `requestFocusRestore`/`dirtySwitchHandled` coordination
  must keep this to **one** delete (a second would be a stale-AST no-op anyway).
- Nesting-commit paths (`PreviewRoot.tsx:914` click-switch, `:950` `commitNestingEdit`) already have
  **no** empty guard and route empty text straight to a delete — add a regression test rather than code.
- **Backend (load-bearing now):** the empty-blocks splice already deletes; confirm
  `compute_reconciliation` + `incremental_write` emit **valid qmd** for the two structural results that
  uniform-delete makes reachable — an item spliced to `[]` (→ a bare `- ` item) and an **empty
  document** (`blocks: []`). These are real round-trip tests, not assumptions.

### Tests (TDD-first)
- [ ] **6.a** Rust unit (pampa): `apply_node_edit` with an empty-blocks `modified_subtree_json`
  deletes the target block from a 3-block doc (assert the middle block is gone, neighbors intact).
  **Write it failing-first** only if not already covered; this pins the backend contract §6 relies on.
- [ ] **6.b** Rust round-trip (**NEW, load-bearing**): deleting a tight single-block bullet's `Plain`
  yields valid qmd with an empty `- ` item, and the result **re-parses** cleanly.
- [ ] **6.c** Rust round-trip (**NEW**): deleting the document's only block yields a valid empty doc.
- [ ] **6.d** Integration (jsdom): empty the draft + Cmd/Ctrl+Enter → `commitTextEdit` called with
  `newText === ''` (delete), editor closes. Empty draft over an **already-empty** block → cancel
  (`setAst` NOT called).
- [ ] **6.e** Integration: empty the draft + arrow-away → delete (dirty path) **and reland on the
  deletion-point neighbor** (assert the landed surface), not cancel; assert **exactly one** commit.
- [ ] **6.f** Integration: empty the draft + **blur/click-away** → delete (the aggressive trigger).
- [ ] **6.g** e2e (`hub-client/e2e/`, real browser, **fail-on-revert**): select a paragraph, delete
  all its text, press Cmd/Ctrl+Enter → the paragraph is removed from the rendered doc and the source.
- [ ] **6.h** e2e: delete a **bullet's** text → an empty bullet remains and the list still renders
  (ties to §0's empty-item crash guard, 0.f).

### Consequences / watch-items
- **Destructive on blur** (chosen): clicking away from an emptied block deletes it. Recoverable via
  the **source editor's undo** — AST rewrites go through Monaco `executeEdits` *"(preserves undo)"*
  (`hub-client/src/hooks/useAutomergeSync.ts:59`). **Watch-item:** confirm the preview forwards undo
  (Cmd/Z) from inside the iframe; if it doesn't, recovery means switching to the source pane.
- **An emptied list/def item is not its *own* editable surface — but the content is recoverable
  in-preview** (**Amendment A6**, corrected from "non-refillable"): an empty item is `[]` with **no
  node → no `SourceInfo`** and no pool entry on the bare `-` line, so it is never its own
  landing/edit target. Navigation/roving **skips** it (a container-gap line, A2) and a click resolves
  up to the whole list — **which is the recovery path**: activating the enclosing `BulletList` opens
  its source text and round-trips through `apply_node_edit` (whole-list replace), so the bullet can be
  refilled without leaving the preview. Only addressing the empty *item slot directly* needs the
  **item-slot `setAstRange`** (out of scope below) — not a new blocker. (Empty Div/BlockQuote/Figure
  keep their own node → still selectable/editable.)
- **No `setAst` signature change, no `null` channel.** A `channel:'delete'` variant is a clean future
  addition if subtree-channel components ever need to delete without an empty-text parse — not needed here.

---

## §7 — Expand-on-edit: third editor size state

### Goal
Give the editing surface a **third size state**. Today it has two: (1) the rendered block, (2) on
activation a textarea sized to the block it replaced (`editTarget.contentHeight`,
`dispatchers.tsx:249`). Add (3) an **expanded** state that grows the textarea to fit all of its text.
Activation alone does **not** resize; the surface expands on the next *in-surface* keyboard
interaction (typing or in-surface cursoring).

### Behavior (resolved with the user)
- **Activation is unchanged** — the textarea opens at `contentHeight` (matches the replaced element).
- **Expand trigger = an in-surface keystroke that *stays* in the surface:** any printable character,
  Backspace/Delete, Home/End, or a caret arrow that moves *within* the textarea (not at an edge).
- **Leave keys never expand — they just leave.** Keys that exit the surface do their normal thing and
  do **not** expand: edge ↑/↓ cross-surface nav (`dispatchers.tsx:301-324`, the `onEdge` branch),
  nesting-cursor chords (`:281-284`), `Esc` (`:291-300`), and the commit chord Cmd/Ctrl+Enter
  (`:285-290`). These already early-return in `onKeyDown`, so the expand call is placed only on the
  fall-through (real in-textarea editing) path.
- **Keyboard activation expands immediately.** Roving-select then **Enter/Space**
  (`useBlockEditHover.tsx:280-282`) opens the surface **already expanded** — the activating keystroke
  *is* the in-surface interaction the user intends. (Pointer/click activation opens collapsed.)
- **Keyboard *hop* landings open collapsed.** Arrowing/nesting from one surface into another is
  driven by a *leave* key (which by the rule above doesn't expand), so the landed surface opens
  collapsed and its next in-surface keystroke expands it. *(Decision — flag for the user: only roving
  Enter/Space opens expanded; hop landings via `openEditTarget` open collapsed. Flip the
  `expandOnOpen` wiring at the `openEditTarget` call sites if hops should open expanded instead.)*
- **Only grows, clamped to the original.** Expanded height = `max(originalContentHeight,
  fit-to-text)`; the surface is never smaller than the element it replaced.
- **Stays text-fitted while open.** Once expanded, it auto-sizes to its text on every change (grows
  *and* shrinks as lines are added/removed) — always clamped to the `max(original, …)` floor — for
  as long as the editor stays open.

### Implementation (frontend only)
- **`EditTarget` type** (`PreviewContext.tsx`, the `contentHeight` struct ~`:51-66`): add
  `expandOnOpen?: boolean`. Set `true` at the **keyboard** `activate` site, `false`/absent everywhere
  else.
- **`activate`** (`useBlockEditHover.tsx:64`): thread an `opts?: { keyboard?: boolean }`. The
  Enter/Space path (`:280-282`) passes `{ keyboard: true }`; the pointer/touch paths
  (`:210`, `:236`) pass nothing. Carry it into the `setEditTarget({ … expandOnOpen })` call (`:104`).
- **`openEditTarget`** (`PreviewRoot.tsx:429`, `setEditTargetRaw` `:472`): leaves `expandOnOpen`
  unset (hops open collapsed — see decision above).
- **`editExpandedRef` lifecycle (root-held, mirrors `editDraftRef`):** a single ref in
  `PreviewRoot`/`PreviewContext`. **Set it at every open** — `activate` and `openEditTarget` write
  `editExpandedRef.current = expandOnOpen` (i.e. `true` only for keyboard `activate`, else `false`).
  This makes it *preserved across self-heal remounts* (same logical target → same ref value, so an
  expanded surface stays expanded) **and reset on a genuine new open** (click/hop → `false`). Without
  the reset, a hop landing right after an expanded block would read a stale `true` and open expanded,
  contradicting the "hops open collapsed" decision.
- **`EditTextarea`** (`dispatchers.tsx:124`):
  - `const [expanded, setExpanded] = useState(() => editExpandedRef?.current ?? false)` — the ref is
    freshly set per open (above), so this initializer is correct for all open paths.
  - **Height:** a `useLayoutEffect` keyed on `[draft, expanded, contentHeight]`: when `expanded`, set
    `ta.style.height = 'auto'` then `ta.style.height = Math.max(contentHeight, ta.scrollHeight) + 'px'`;
    when not expanded, height stays `contentHeight` (today's static style). *(jsdom returns
    `scrollHeight === 0`, so `max(contentHeight, 0) === contentHeight` — safe and non-shrinking under
    jsdom; pixel-fit assertions belong in e2e.)*
  - **Trigger (placement matters):** `onKeyDown` (`:276-336`) has **no real "fall-through" tail** —
    printable keys / Backspace / Delete / Home / End match *no* branch (the browser just edits), and a
    **non-edge arrow lives *inside* the ArrowDown/ArrowUp `else-if`** (`:334` "fall through — native
    caret move"), not after it. So place a single **guarded `setExpanded(true)` near the top** of
    `onKeyDown` that **excludes leave-keys** (nesting chord, Cmd/Ctrl+Enter, Esc, and an edge arrow
    when `onEdge`) and mirrors to `editExpandedRef`. This covers both the no-branch keys and the
    non-edge-arrow case; in-surface cursoring expands as required.

### Tests (TDD-first)
- [ ] **7.a** Integration (jsdom): click-activate → `expanded` is false and the height effect keeps
  `contentHeight`; type a character → `expanded` flips true and the height effect runs
  (assert the flag + that the layout effect set an explicit `style.height`; not the pixel value).
- [ ] **7.b** Integration: roving **Enter/Space** activation opens with `expanded === true`
  immediately; a pointer activation opens with `expanded === false`.
- [ ] **7.c** Integration: leave keys do **not** expand — assert `expanded` stays false after an
  edge ↓ that navigates away, after a nesting chord, after `Esc`, and after Cmd/Ctrl+Enter.
- [ ] **7.d** Integration: the floor holds — with a stubbed `scrollHeight < contentHeight`, expanded
  height clamps to `contentHeight` (never smaller than the original element).
- [ ] **7.e** e2e (`hub-client/e2e/`, real browser): activate a **multi-line** block by click →
  surface is one/short-line tall (== replaced element); type one character → it grows to show all
  lines; delete lines → it shrinks back down but not below the original. **This is the new step the
  existing e2e specs need** (see below).
- [ ] **7.f** e2e: keyboard-select (roving) + Enter on a multi-line block → opens already expanded.
- [ ] **7.g** Integration (**NEW — missing-test pass, the round-2 reset bug**): open block A **expanded**
  (keyboard activate), then **hop/click** to block B → B opens **collapsed** (`data-expanded` false),
  proving `editExpandedRef` was *reset* at B's open, not left stale-`true`.
- [ ] **7.h** Integration (**NEW — the preserve half**): open A expanded, force a self-heal **remount**
  (collaborator-shift rerender, à la `p2-3b-real`) → A stays **expanded**, proving the ref is read on
  re-mount.

### Consequences / watch-items
- **Edge detection is already expand-safe (verified — this is §7's real value).** `isOnLastVisualLine`
  / `isOnFirstVisualLine` (`caretGeometry.ts:162`) measure against a **mirror div built from the full
  `ta.value`** — text-based, *independent of the textarea's actual height*. So a *collapsed* textarea
  whose source qmd is taller than the rendered block (e.g. a 3-source-line paragraph that renders as
  one wrapped line — the textarea is sized to the **render** height, not the **source** height) does
  **not** misfire: ↓ from source-line 1 moves to line 2 (not an edge) → in-surface move → §7 expands
  and reveals the hidden lines. Revealing source clipped by render-sizing is precisely what §7 buys.
- **e2e step additions (expected, per the user):** specs that activate a surface and then *type* must
  account for the post-keystroke expansion (height changes after the first in-surface key). Specs that
  only assert the *activation* size are unaffected (state 2 is unchanged). Audit the block-editing e2e
  specs and add the expand step where they type.
- **Manual resize handle:** the textarea keeps `resize: 'vertical'`. Auto-fit on each change will
  override a manual drag once expanded; acceptable (note it), or drop the handle in expanded state.
- **Perf:** the height effect reads `scrollHeight` (a layout flush) + writes on every change — one
  reflow per keystroke on large textareas. Fine normally; gate/measure if it ever profiles (mirrors
  §0's measure-cost note).
- **IME:** the first composition keystroke (`keyCode 229`) may not reach the expand trigger;
  expansion then waits for `compositionend`/the next key. Acceptable; note it.
- **No `setAst`/Rust involvement** — purely an editor-surface UI state.

---

## Out of scope (recorded, not built): `setAstRange`

Editing a **multi-block list item as a unit** (the union of its blocks, e.g. `two` *with* its
sublist) is **not possible through `setAst`** and is deferred:
- The item is a bare `[Block]` array with **no node and no `source_info`**; `lookup_block` matches
  `block.source_info() == target` **exactly** (`crates/pampa/src/node_lookup.rs:83`), so a union
  range resolves to nothing → `apply_node_edit` returns content unchanged.
- A `setAstRange` would need `lookup_block` to resolve a union range to the **item slot**
  (`bl.content[item_idx]`, a `Vec<Block>`) and a new "replace the whole item" splice (the current
  splice replaces a single `Block`, `apply_node_edit.rs:198`). The path machinery already has
  `ContainerStep::ListItem` (`:214`), so the structure is half-present; the lookup + item-replace
  splice + reparse-to-`Vec<Block>` is the new work.
- This plan keeps **per-node** editing only: descending onto an item's leading text edits *just that
  block*. "Edit the whole item" waits for `setAstRange`.

---

## Design decisions (resolved with the user)

- **Items follow the Pandoc AST** — no synthetic item-union surfaces; surfaces are AST block nodes.
  An item's leading block is the editable/measurable/navigable surface.
- **(P) full pool-id, not measurement-only** — items become first-class clickable surfaces; the
  click-granularity change (unlock mode) is wanted.
- **Wiring B** — q2-preview list components own `<li>`/`<dd>` rendering.
- **One uniform leading-block measure** — Range-based; single-block coincides with the whole `<li>`,
  so no phase split and no deferred hole.
- **Rule B for up/down** — deepest visible surface at the adjacent line; depth-preservation rejected
  (it grabs containers). Container-start drops to the leaf at the adjacent line; whole-container
  editing is chord-only.
- **Locked = Rule B with the outer-block set** — behavior-preserving; the lock just coarsens the
  partition.
- **Clamp at ends** (both modes) — wrapping was a misfeature.
- **Roving follows the same set** as navigation (mode-switched).
- **Whole-`<li>` box-shadow outline** — overlay rejected (tracking/multi-rect/two-mechanism cost).
- **DefinitionList:** `<dd>` definitions editable; `<dt>` terms (inline-level) out.
- **§6 delete is uniform, never blocked** — emptying any block deletes its contents; an empty list
  item / div / blockquote / figure is accepted (awkward but valid). No frontend item-awareness, no
  backend guard. Empty items are non-*refillable* in-preview until item-slot editing (`setAstRange`).
- **§6 arrow-away delete anchors on the neighbor** — resolve via the existing edge/clamp gate
  (neighbor-exists ⟺ not-clamped; clamped ⇒ no-op, no delete), reland at the deletion-point line in
  the new index (self→neighbor), caret column via §4's `prefixWidth`.
- **§7 expansion** — third size state; in-surface keystroke expands (leave keys never do); keyboard
  `activate` (roving Enter/Space) opens expanded, click/hop opens collapsed; only-grows clamped to the
  original; stays text-fitted while open. `expandOnOpen` set per-open into a root `editExpandedRef`.

### Rule B example table (the container case, for the record)

```
L0  para zero    P0   d0
L1  - one        one  d1            outer list d0 (L1–L5)
L2  - two        two  d1            sub-list  d1 (L3–L4)
L3      - alpha  alpha d2
L4      - beta   beta d2
L5  - three      three d1
L6  > quote      Pq   d1            blockquote d0 (L6)
```

| Editing (depth) | Arrow | → line | Rule A (prefer ≤ depth) | **Rule B (deepest)** |
|---|---|---|---|---|
| `para zero` (d0) | ↓ | L1 | **the whole list** | `one` |
| `two` (d1) | ↓ | L3 | **the whole sub-list** | `alpha` |
| `three` (d1) | ↑ | L4 | **the whole sub-list** | `beta` |
| `beta` (d2) | ↓ | L5 | `three` | `three` |

Row 1 (paragraph ↓ → whole-list editing) is why depth-preservation is rejected.

## Risks / watch-items

- **Snapshot/measure cost on long lists** (§0 consequence) — gate/lazy if profiled.
- **Test-premise churn** — list-text click now activates the item in unlock mode (§0).
- **Multi-block outline over-cover** — accepted/documented (§2).
- **`lineSurface` refactor touches fail-on-revert-proven move/click-switch code** — mitigated by the
  characterization test (1.b) and behavior-preserving locked path.
- **Sequencing** — land the breadcrumb plan first; keep its nav tests range-based and its e2e fixture
  on a surface §0 doesn't change.
- **Empty-item crash (§0 × §6)** — uniform-delete makes empty items routine; `<li …={item[0].s}>`
  throws unless §0 guards `item.length === 0` first (test 0.f).
- **Backend round-trips load-bearing (§6)** — empty-item splice (`- `) and empty-doc must emit valid,
  re-parseable qmd (tests 6.b/6.c); the rest of §6 is frontend-only but this isn't.
- **Undo forwarding (§6)** — blur-delete is recoverable via Monaco `executeEdits` undo; confirm the
  preview iframe forwards Cmd/Z, else recovery means switching to the source pane.
- **`editExpandedRef` reset discipline (§7)** — must be set per-open (not just preserved), or a hop
  after an expanded block opens expanded against the "hops collapsed" rule.
- **Delete + §4 ordering** — §6's arrow-delete reuses §4's `prefixWidth` caret path; land §4 with/before §6.

---

## Test Seam Spec (frozen before handoff — §0.f, §6, §7)

Per `prevalidating-test-seams`: each row names the **real unit mounted**, the **seam** (mount +
events + assertion **surface**), the **mock boundary**, and the **named revert hunk → RED assertion**.
Scope = the tests authored this session (§0.f, §6.*, §7.*); the §0–§5 seams inherit the prior plan's
review and should get the same pass before their phase executes (logged below).

**Shared assertion-surface decision:** §7 adds `data-expanded={expanded ? '' : undefined}` to the
edit `<textarea>` (a deliberate production test-seam, like the existing `tabIndex`/`data-block-pool-id`).
jsdom returns `scrollHeight === 0`, so **expansion is asserted via this flag, never via pixel height**
in jsdom; pixel growth is asserted only in the e2e tier (real layout).

| Test | Tier | Real unit + seam (mount · events · **assertion surface**) | Mock boundary | Named revert → RED |
|---|---|---|---|---|
| **0.f** | jsdom | `BulletList` rendered with an AST holding one empty item `[]` + one normal item · — · **the empty item's `<li>` has no `data-block-pool-id` and render does not throw; the normal item's `<li>` *does* carry one** | supply AST/pool directly (no WASM) | remove the `item.length===0` branch (restore unconditional `item[0].s`) → render throws on the empty item → RED |
| **6.a** | Rust unit | `apply_node_edit` on a 3-block doc, dest = middle block SI, replacement `{blocks:[]}` · — · **output string = doc minus middle block, neighbors intact** | none | guard `splice_in_blocks` against empty replacement (`if !replacement.is_empty()`) → middle block remains → RED |
| **6.b** | Rust round-trip | `apply_node_edit` + `incremental_write` + re-parse on `- foo\n- bar`, dest = item-1 `Plain`, empty replacement · — · **output is valid qmd with an empty `- ` item AND re-parses to a `BulletList` whose first item is `[]`** | none | *(discovery/regression — see note)* writer's emptied-list-item-slot path; if a fix is needed it is that hunk, else this pins `incremental_write` → malformed/whole-item-removed output → RED |
| **6.c** | Rust round-trip | same chain on a single-paragraph doc, delete the only block · — · **output re-parses to `blocks:[]`** | none | writer's empty-doc path (discovery/regression) → non-empty/garbage output → RED |
| **6.d** | jsdom | `EditTextarea` in the preview harness, editTarget on a **non-empty** para · clear draft (`onChange` `''`) + Cmd/Ctrl+Enter keydown · **`setAst` spy called once, payload `{channel:'text',newText:''}`** | assert payload, not resulting qmd (parent routing out of harness) | the `!normalized&&baseline→delete` branch in `commitIfDirty` (revert to `!normalized\|\|…→cancel`) → not called → RED. *Gating sub-assert (non-discriminating, keep for shape): already-empty block → `setAst` NOT called.* |
| **6.e** | jsdom | `EditTextarea` arrow path + `requestMove`/`executeLanding`; blocks A(L0)/B(L1), editTarget A · clear A · dirty down-move · rerender with post-delete content (B at L0) · **`setAst` called exactly once with `''`; resulting `editTarget.anchorR0 === B.r0`** | mock edge detection (or call `requestMove` directly); supply post-delete pool/content on rerender | (i) arrow-away `isDirty` empty→dirty change → no commit → RED; (ii) reland anchor self→deletion-point change → lands wrong/nowhere → RED |
| **6.f** | jsdom | `EditTextarea` blur path, non-empty editTarget · clear · `onBlur` · **`setAst` called with `''`** | as 6.d | same `commitIfDirty` branch (blur entry) → not called → RED |
| **6.g** | e2e (real WASM) | full preview→parent→`apply_node_edit`→re-render; paragraph fixture · activate · select-all+Delete · Cmd/Ctrl+Enter · **target para gone from rendered DOM *and* doc source; sibling paras remain** | none | `commitIfDirty` delete branch → para stays → RED (**fail-on-revert**) |
| **6.h** | e2e | full chain + §0 list render; 2-item bullet fixture · activate item-1 text · clear+commit · **list still renders; item-1 `<li>` present but textless; item-2 intact** | none | (i) §0 empty-item guard → list render crashes → RED; (ii) §6 delete branch → item-1 keeps text → RED |
| **7.a** | jsdom | `EditTextarea` opened via **pointer** · printable keydown `'a'` · **`data-expanded` absent before, present after** | none | the `setExpanded(true)` trigger in `onKeyDown` → never set → RED. *Gating (shape): before-type height stays `contentHeight`.* |
| **7.b** | jsdom | `useBlockEditHover.activate` keyboard path + `EditTextarea` · roving Enter vs pointer-down · **`data-expanded` present (keyboard) / absent (pointer) at open** | none | the `{keyboard:true}`→`expandOnOpen` wiring on the Enter/Space activate path → roving-Enter opens collapsed → RED |
| **7.c** | jsdom | `EditTextarea` `onKeyDown`, start collapsed · fire **each** leave key (edge ↓ [edge mocked], nesting chord, Esc, Cmd/Ctrl+Enter) · **`data-expanded` stays absent after each; AND the leave action fired (e.g. `requestMove` called for edge ↓)** | mock edge detection; stub `requestMove`/`requestNestingMove`/`setEditTarget` | move `setExpanded(true)` *before* the leave-key returns (i.e. drop the exclusion) → a leave key expands → RED. **Vacuity guard:** the "leave action fired" assert proves the key was exercised in the leaving state, not a no-op state |
| **7.d** | jsdom | the height `useLayoutEffect`, `expanded=true`, `contentHeight=100`, **stub `ta.scrollHeight=40`** · trigger via draft change · **`ta.style.height === '100px'`** | stub `scrollHeight` (deterministic clamp arithmetic, not real layout) | the `Math.max(contentHeight,…)` clamp (revert to bare `scrollHeight`) → `'40px'` → RED |
| **7.e** | e2e (real layout) | `EditTextarea` in browser; **fixture where source is taller than render** (multi-source-line para rendering as fewer visual lines) · click-activate · type 1 char · delete the added lines · **height ≈ `contentHeight` on open; grows >50px after type; shrinks back but never < `contentHeight`** | none | height effect expand (revert to fixed `contentHeight`) → no growth → RED. **Fixture is load-bearing:** if collapsed==expanded height the "grows" assert is vacuous |
| **7.f** | e2e | full roving + keyboard activate; same source>render fixture · roving + Enter · **textarea opens tall (expanded), not `contentHeight`** | none | `{keyboard:true}`/`expandOnOpen` wiring → opens collapsed → RED (same fixture caveat as 7.e) |
| **7.g** | jsdom | open A expanded (keyboard) → hop/click to B · **B's `data-expanded` absent at open** | none | the per-open `editExpandedRef.current = expandOnOpen` **reset** write (revert to preserve-only) → B reads stale `true` → opens expanded → RED |
| **7.h** | jsdom | open A expanded → collaborator-shift rerender (remount) · **A's `data-expanded` still present** | supply shifted pool on rerender | reading `editExpandedRef` in the `useState` initializer (revert to `useState(false)`) → remount collapses A → RED |

### Vacuity notes (skill check 2)
- **6.d / 6.f / 7.a** each carry a *gating* sub-assertion (already-empty→no-delete; before-type height;
  collapsed-before) that does **not** discriminate the new behavior — kept for shape only; the
  discriminator is the named payload/flag flip. Do not let the gating assert masquerade as the binding one.
- **6.b**: the empty-item-present assertion is the discriminator that distinguishes "leave the item
  empty" (wanted) from "remove the whole item" (which is the *setAstRange* behavior and would be wrong
  here) — not just valid-vs-invalid qmd.
- **7.c / 7.e / 7.f** carry explicit "path actually exercised" / "fixture differs" guards against the
  sibling trap (firing in a state that no-ops, or a fixture where the two states coincide).

### Missing-test pass (skill check 3) — accepted-untested, with rationale
- **Delete-reland caret column (§6 × §4):** 6.e asserts the landed *surface* but not the caret *column*
  (jsdom can't). **Recommend** adding a caret-column assertion to a delete-reland **e2e** (extend 6.g);
  if not, accept-untested on the rationale that §4.a (clean/dirty column parity) + §1.d (nav caret
  column) + the *shared* `prefixWidth` path jointly cover it. **Decision needed at execution.**
- **Empty item excluded from C1 nav/roving set:** no dedicated test — **accepted-untested by
  construction**: C1 enumerates `[data-block-pool-id]` elements and an empty `<li>` carries none (0.f),
  so it cannot enter the set. (A one-line assertion could be folded into a §1/§2 test if cheap.)
- **§7 IME first-keystroke (keyCode 229) may not expand:** accepted-untested — documented limitation
  (expansion waits for `compositionend`/next key).
- **§0–§5 seams:** not bound in this spec (inherited from the prior plan's review/Reflections). Run the
  same three-check pass over §0.a–e, §1.a–e, §2.a–b, §3.a–d, §4.a–b, §5.a before their phase executes.
