# Block editing — depth-aware editing, cross-surface cursor, AST buffers

**Date:** 2026-06-11 (substantially reworked 2026-06-12; concurrency/identity rework 2026-06-13)
**Branch:** feature/block-editing-improvements (worktree `.worktrees/block-editing`)
**Builds on:** Plans 2a/2b/2c/3 (all done). Design master:
`claude-notes/designs/2026-06-06-block-editing-design.md`.
**Status:** Design settled through six review passes. All four pre-implementation assertions
verified (A–D below). **Ready for handoff.** TDD-first; tests precede implementation in every
phase. Execute phases in order (1 → 2 → 3); each phase's TDD checklist is its unit of work.

## Overview

The live block editor in q2-preview has a real, current bug — clicking a second
time inside an open editor silently "climbs" to the parent/grandparent — and a set of
wanted improvements (reach the right surface on a click, move the cursor between
surfaces, edit nested blocks cleanly) that turn out to share one core idea: **editable
surfaces nest, and a click or an arrow selects a root-to-leaf *path*, not a single
surface.** This plan delivers them in three phases:

1. **Active-region bug fix** — stop a click inside the open editor from re-resolving
   and climbing. Small, mode-independent, ships first.
2. **Depth-aware default editing + cross-surface cursor** — resolve a click to the
   *right* surface via deepest-wins hit-testing with a principled tiebreak, and let
   arrow keys move the edit cursor between surfaces (committing as they go). This is
   the default experience for everyone; it never needs WASM serialization. **It also
   fixes a live data-integrity bug** (see Load-bearing facts): on `main`, a concurrent
   structural edit *above* an open editor silently re-targets your next commit to a
   different block.
3. **"Depth cursor (nested blocks)" — a power-user unlock** (a default-off user
   setting). When on: clicks resolve to the leaf, two keys move in/out through nesting
   depth, a breadcrumb chip shows/operates the depth, and — only here — editing a
   block nested inside a marker-prefixed container needs an **AST-regenerated buffer**
   (so it isn't polluted with `> `/indent). Because regeneration is unreachable in the
   locked default, the whole expensive serialization path is gated behind this flag.

All of this is q2-preview-only (q2-debug / q2-slides don't consult `sourceIndex`).

## Pre-implementation verification — DONE (the design rests on these)

The plan is reasoned a priori; these four assertions are load-bearing. All are verified.

- [x] **A. Rect coincidence** — **VERIFIED 2026-06-13** (Playwright against the real `q2
  preview` binary, real Bootstrap + theme CSS, chromium 1000×1400). A chrome-less single-child
  `Div` box **coincides exactly** with its child; a `BlockQuote` / `Callout` / list / multi-child
  div **differs**. Measured container-vs-child deltas (L/T/R/B px):

  | Case | Δ L / T / R / B | Verdict | Decoration |
  | --- | --- | --- | --- |
  | 1. `Div` / 1 `p` | `0 / 0 / 0 / 0` | **coincide (exact)** | margin collapse; zero border-box gap |
  | 2. `BlockQuote` / `p` | `+25.25 / +10.62 / −21.25 / −10.63` | differ | 4px left rule + padding |
  | 3. `Div` / 3 `p` | up to `±85` top/bottom | differ | container spans all three |
  | 4. `Callout` / body `p` | `+12.64 / +25.45 / −8.64 / −1` | differ | ~25px title bar + 5px left border |
  | 5. `ul` / `li` | `+34 / 0 / 0 / 0` | differ | 34px left marker gutter |

  The prediction that mattered most holds: **#1 coincides at 0px** (Bootstrap reboot adds no
  stray div/p margins). **Two consequences pinned into §2a:** (i) coincidence must compare **all
  four edges** — the list case (#5) differs *only* on the left gutter, so a vertical-only check
  would misclassify it; (ii) true coincidence is **exactly 0px** and the nearest deciding edge in
  any "differ" case is ≥~12px, so the epsilon is **tight (~1px)**, not the old loose "~1–2px."
- [x] **B. `r[0]` uniqueness** — in real Q2 pool output, a container and its first child
  have **distinct** `r[0]`, and all Original blocks' `r[0]` are unique. *Underpins the
  `anchorR0` identity (§2b).* **VERIFIED 2026-06-12** via `cargo run --bin pampa -- f.qmd
  -t json` (pool at `.astContext.p`): Div `0` vs Para `7`; BlockQuote `0` vs Para `2`;
  list `0` vs items `2/8/14` — all distinct, container range opens at its marker before
  the child. **Two caveats folded into §2b:** (1) uniqueness holds only among **Original**
  entries (`t==0,d==0`) — filter to those (`resolveSource` already does); (2) `r[0]` is
  exact only when bytes before the block are unchanged — a collaborator editing *above*
  your active block shifts it, so the self-match needs a nearest-at-or-after fallback.
- [x] **C. `apply_node_edit` re-serializes the enclosing container** (re-adds `> `/indent
  via the qmd writer) rather than byte-splicing the replacement into the node's source
  range. *Underpins Phase-3 regeneration commit (a clean buffer must round-trip into a
  `> `-prefixed quote) and the locked whole-list commit.* **VERIFIED 2026-06-12** —
  AST-splice (`apply_node_edit.rs:169`) → `incremental_write` (`apply_node_edit.rs:174`) →
  `BlockQuoteContext` re-adds `> ` per line (`qmd.rs:128`); a native test confirmed a clean
  2-line buffer → `> Line one.\n> Line two.`. (Caveat: the whole container is re-serialized —
  exactly the wholesale-rewrite that drives the §2a concurrency-clobber note; not new.)
- [x] **D. Pool regenerates per render; `r[0]` is byte-stable across a block's own edit.**
  Pool indices are not stable across renders (so identity must key off source position);
  a block's start offset is unchanged by an edit to its own content. *Underpins the whole
  identity-churn premise and the `anchorR0` self-heal.* **VERIFIED 2026-06-12** — fresh
  pool per `write_with_config` (`json.rs:1859`; the fresh `pool: Vec::new()` is at
  `json.rs:289`); offsets are absolute byte positions assigned at parse, no renumbering
  (`quarto-source-map/src/source_info.rs`). **Caveat:** "absolute" holds for **`Original`**
  entries only; `Substring` offsets are *relative to parent*. The `anchorR0` identity stays
  in the `Original` domain (`t==0,d==0`) — which is exactly why B's filter is load-bearing.

## Background / where the machinery lives

Verified against the tree (citations current as of the 2026-06-13 rework).

- **Surfaces & the editability gate** — `ts-packages/preview-renderer/src/q2-preview/sourceIndex.ts`
  builds a SourceInfo-value index from the untransformed AST, tagging each block
  `TopLevel` / `Descendable` / `Opaque`. `resolveSource(node)` (`entry.tsx:539`)
  returns `{ sourceNode, reachabilityClass, sourceEntry }`. `serializeSourceEntry`
  (`sourceIndex.ts:30`) yields `"t:r0-r1:d"`. 16 leaf components (`blocks/*`, `custom/*`)
  emit `data-block-pool-id` when editable; they nest in the DOM **exactly as the AST
  nests**, which is what makes hit-testing depth-aware.
- **Click/touch activation** — `useBlockEditHover.tsx`: a delegated host handler.
  `findEditTarget` is `e.target.closest('[data-block-pool-id]')` (`:88`) — i.e.
  **deepest-wins** (the innermost surface under the pointer). `activate()` measures
  the box and sets `editTarget` (`:57`); today it reads only the DOM pool-id attribute.
  Roving-tabindex arrow nav between blocks (when *not* editing): the **host is the sole Tab
  stop** (`tabIndex:0`, `:208`), blocks are `tabIndex={-1}`, arrows `.focus()` among them
  (`:162`) and Enter/Space calls `activate()` (`:183`).
- **Edit substitution** — `dispatchers.tsx`: when `editTarget` matches a block (`isBlockEditTarget`,
  `:84`, used at the two `Block`/`CustomBlock` sites `:186,:248`), it's replaced by a
  measure-and-set textarea wrapper (`renderMeasuredEdit` + `renderBlockTextarea`, `:41,:100`).
  The wrapper carries **no** `data-block-pool-id` (central to the Phase-1 bug). **Today the
  textarea is uncontrolled** (`defaultValue = sliceBytes(content,r0,r1).trimEnd()`, `:124,:105`);
  on-blur commit is unconditional except empty (`:111`). *Phase 2 makes it controlled — see §2b.*
- **Block-children keying** — `utils.tsx:167,180` renders block/inline child lists with
  **`key={i}` (the array index)**. This is load-bearing for §2b: an edit *above* the active
  block shifts indices and **remounts** the textarea, which is why the draft must survive
  outside the DOM node (controlled value).
- **Commit** — `commit` → `commitTextEdit(JSON.stringify(resolved.sourceEntry), text)`
  → `props.setAst` → iframe `postMessage({type:'SET_AST'})` (`entry.tsx:689`) → parent
  `handleSetAst` → `parseQmdContentSync` + `applyNodeEdit` → `onContentRewrite` →
  full re-render → fresh `UPDATE_AST`. **The commit destination is the byte range
  (`sourceEntry.r`), not the pool integer** — `apply_node_edit` locates the node by SourceInfo.
- **Writer** — `crates/pampa/src/writers/qmd.rs::write_single_block` (`:2392`, public)
  re-serializes a block with a **fresh** context (no `> `/indent prefixer), so a
  nested block in isolation comes out clean. **Not yet exposed to WASM.**
- **WASM boundary** — reachable only via `@quarto/preview-runtime`
  (`wasmRenderer.ts` — `applyNodeEdit`, `parseQmdContentSync`). The Rust entry points
  live in `crates/wasm-quarto-hub-client/src/lib.rs` (`apply_node_edit`, `:2938`). The
  **iframe** (`entry.tsx`, `dispatchers.tsx`) has zero WASM *and* zero Automerge
  access — it receives `renderedContent` + `untransformedAstJson` over postMessage and
  can resolve everything else (DOM rects, byte→line) **locally**. (`content` in
  `PreviewContext` *is* `props.renderedContent` — the raw parse-input source the offsets
  index, `entry.tsx:610`, `ReactPreview.tsx:382`.)
- **Two hosts, one iframe** — `Q2PreviewIframe` is driven by **(A)** hub-client
  (`ReactPreview.tsx` → `ReactRenderer.tsx` → `Q2PreviewIframe`) and **(B)** the
  standalone SPA (`q2-preview-spa/src/PreviewApp.tsx` → `Q2PreviewIframe` directly,
  `:1135`). Both already call WASM and compute `untransformedAstJson` +
  `renderedContent`. Any flag/payload added for the iframe must be produced by both.
- **Precedent for a host→iframe boolean — read carefully.** `editingDisabled` rides a
  prop → `UPDATE_AST` payload (`Q2PreviewIframe.tsx:233`) → `entry.tsx` → `PreviewContext`
  (`:71`) → `useBlockEditHover.tsx:190`. **That iframe-side leg is a real, proven precedent.**
  But on the *host* side it is wired **only in the SPA** (`PreviewApp.tsx:1157`); **hub-client
  threads `editingDisabled` nowhere** — `ReactPreview`/`ReactRenderer` never mention it, and
  `ReactRenderer` forwards no such flag. So the Phase-3 setting reuses the proven iframe wire
  but needs a **new `ReactRenderer` pass-through** on the hub-client host leg (see §3a).

### Load-bearing facts (do not re-litigate)

- **The pool integer `s` is a positional ordinal, reassigned every render.** `intern`
  assigns `id = pool.len()` in document/traversal order over **all** source-tracked nodes
  (blocks *and* inlines, `json.rs:400`), fresh each render. So `data-block-pool-id` is **not**
  a stable identity. A block's **start offset `r[0]` IS stable** across its own edit (bytes
  before it are byte-verbatim; only `r[1]` and later offsets shift). **Identity keys off source
  position (`r[0]`), never the pool id.**
- **On `main`, a concurrent structural edit *above* an open editor silently re-targets the
  commit — this is a live data-integrity bug, and Phase 2 fixes it.** Matching today is
  `editTarget.poolId === node.s` (`dispatchers.tsx:95`); `editTarget` is *not* cleared by an
  incoming `UPDATE_AST`. When a collaborator adds/removes nodes above you, every ordinal shifts,
  so your stale `poolId` now selects a **different real block** — displaced by the size of their
  edit, *directed toward it* (not random). Your next blur then commits your text to **that
  block's byte range**. Bounded but real; with no dirty guard, even an unmodified blur re-writes
  the wrong block. The fix is the **byte-offset `anchorR0` identity + content guard** (§2b), which
  moves matching into the *same coordinate system the commit targets by*.
- **The existing focus restoration is silently broken on commit** (`entry.tsx:385–403`):
  it `setTimeout`-focuses `[data-block-pool-id="<id>"]`, but on a re-render that id maps to a
  different block. Phase 2 replaces it with position-anchored re-identification (§2b).
- **postMessage is one-way** (parent→iframe `UPDATE_AST`/`UPDATE_THEME`/`LOAD_CUSTOM_COMPONENTS`;
  iframe→parent `SET_AST`/`NAVIGATE_TO_DOCUMENT`/`IFRAME_READY`); **no request/response channel.**
  The iframe holds `renderedContent` + the pool, so it resolves positions and builds a byte→line
  map locally. *Consequence:* `pendingLanding` cannot tell *your* commit's re-render from a
  *collaborator's* — see the documented out-of-scope race in Risks.
- **The commit key and any buffer-lookup key are different formats.** Commit passes
  `JSON.stringify(resolved.sourceEntry)`; the Phase-3 buffer table is keyed by
  `serializeSourceEntry` (`"t:r0-r1:d"`). The iframe computes the latter only for the
  buffer lookup.

---

## The resolution model (shared by all phases)

This is the conceptual core; the phases are implementations of it.

Because surfaces nest, a point (click) or a source position (arrow) identifies a
**path** — the root→leaf chain whose extents contain it — not one surface. Collapsing a
path to a surface needs a policy. The policy is **deepest-wins hit-testing**: assign
each pixel to the *deepest* surface whose box contains it. A child's interior → the
child; a container's *exclusive* pixels (its chrome — a callout title bar, a blockquote
left rule, a div's border/padding, gaps between siblings) → the container. This is
`closest('[data-block-pool-id]')`, and it yields a **full, unique partition of the
visible document for free** — provided top-level blocks are always candidates.

Deepest-wins has exactly **one** ambiguity: **coincident extents** — a single child
wrapped by a container with *no chrome* (a bare `<div>` that fits its lone child
exactly, measured at exactly 0px in assumption A), where the two boxes coincide. The
**mode** is precisely that tiebreak:

- **Locked (default): take the topmost of the coincident stack** ("sensible
  almost-top" — you reach the wrapper/fence).
- **Unlocked: take the leaf.**

Everything non-coincident (multi-child containers, and any container with visible
chrome) resolves by plain deepest-wins, identically in both modes — so a single-child
*blockquote* (which has a left rule, hence distinct boxes) is reached by clicking its
rule, while its text-click goes to the child. **Chrome-as-handle falls out for free.**

Coincidence is a **screen-extent** property — compare bounding rects on **all four edges**
within a **~1px** epsilon (assumption A: true coincidence is exactly 0px; the list case
proves single-edge chrome must count) — not an AST child count. This dissolves the old
"what is an editable child" question entirely.

**Locked mode adds one rule, and it *dominates* the coincidence tiebreak:** **prefixing
containers are atomic.** `BlockQuote`, `BulletList`, `OrderedList`, `DefinitionList` are
never descended into in locked mode — a click anywhere inside selects the *whole*
container, edited as one buffer (`> a\n> b`, markers and all, a clean slice of the
container's own range). Locked mode descends only through fenced / column-0 structure
(Div, Figure, Callout/Theorem/Proof), whose children slice clean. (Table cells and
LineBlock lines never expose block descendants — `Table.tsx:22` NOOPs cell content,
LineBlock holds inlines — so the prefixing-atomic set is complete *given* those facts;
if either ever exposes block children, the set must be revisited.)

So the locked target is, as a single precedence: **the outermost prefixing container if
the path crosses one; otherwise the topmost coincident ancestor of the leaf.** The
ordering is not optional — prefixing-atomic *must* win, or the invariant below fails (a
chrome-less div nested inside a blockquote would coincidence-climb to the div, whose
slice still carries the quote's `> ` on continuation lines).

**The key consequence** (verified by construction, dependent on that precedence):
regeneration is needed *iff* a block has a prefixing ancestor and is multi-line; locked
mode never lands on anything with a prefixing ancestor (it stops at the outermost one);
therefore **locked mode never needs AST regeneration.** All locked tiles are cleanly
sliceable. The regeneration set and the "prefixing container" set are defined by the same
predicate, so they cannot drift. This is what lets Phase 3's expensive serialization be
flag-gated.

A partition also means **navigation has no redundancy**: arrow nav over the locked tiles
visits each once, never "a container then its own lone child."

---

## Phase 1 — Active-region bug fix

### The bug
Clicking a second time inside an open editor climbs to the parent. Mechanism: (1) the
clicked leaf's `data-block-pool-id` *disappears* when it becomes the textarea wrapper
(`dispatchers.tsx:41–63`); (2) `onPointerUp` has no "already editing" guard
(`useBlockEditHover.tsx:137`, unlike `onPointerDown` at `:120`); so a click bubbles to
the host, `closest()` walks up past the affordance-less wrapper to the **parent's**
pool-id, and `activate(parent)` fires (the `:61` dedup only catches the *same* id).
Even a click meant to reposition the caret jumps you to the parent.

### Fix
Mark the active edit region — prefer a **tracked ref** on the inner measure-and-set
wrapper over a `data-active-edit` attribute, since the wrapper is an `AttributionWrap`
that may not forward arbitrary `data-*` attrs. **A click whose target is inside the active
region must *not* activate any other surface — but it is *not* a blanket no-op:** it
should register as an ordinary caret-move, letting the textarea own its caret. Only a
click *outside* the region resolves-and-switches. The caret-move falls out for free once
the spurious activation is suppressed (the textarea keeps focus). Mode-independent;
preserves today's deepest-wins (`closest`) resolution otherwise. Ships first, on its own.
(Phase 1 ships against today's uncontrolled textarea; Phase 2 converts it to controlled —
the active-region ref is independent of that.)

### TDD work items
- [x] RTL: with an editor open, a click inside the textarea/wrapper does **not** change
  `editTarget` (no climb to parent) **but does not tear down the editor either** (caret-move
  allowed); a click on a *different* surface switches to it.
- [x] RTL: regression — single click still activates the deepest surface as today.
- [x] Implement the active-region marker + `onPointerUp`/activation guard (inside → suppress
  cross-surface activation only; outside → resolve-and-switch).

---

## Phase 2 — Depth-aware default editing + cross-surface cursor (locked)

Everyone gets this; no WASM, no setting. Three parts: **locked resolution**,
**cross-surface navigation + concurrency-safe identity**, and the **dirty guard**.

### 2a. Locked resolution
Resolve a click via deepest-wins, then collapse it to the locked target by a **single
precedence** (prefixing-atomic dominates — see the resolution model):
1. **Prefixing-atomic (wins):** if the path crosses a `BlockQuote`/`BulletList`/
   `OrderedList`/`DefinitionList`, the target is the **outermost** such container (never
   its children) — full stop, skip step 2.
2. **Coincidence climb (else):** from the deepest pool-id element, climb while the parent
   surface's bounding rect coincides with the child's on **all four edges within ~1px**;
   the topmost coincident ancestor is the target. (Chrome-less single-child wrapper → the
   wrapper; otherwise → the leaf.) Epsilon is tight because assumption A measured true
   coincidence at exactly 0px; a hairline-border fixture pins the border→leaf boundary
   (a 1px border is chrome, so it resolves to the leaf).

**Skip non-visible surfaces.** Both the climb and the tile enumeration must ignore
surfaces that aren't laid out — `display:none`, zero client rect, or inside a collapsed
region (`offsetParent === null`). q2-preview has collapsible callouts whose collapsed
body keeps `data-block-pool-id` but renders inside a `.callout-collapse.collapse`
(`display:none`, `Callout.tsx:207`) — a zero-rect would corrupt the coincidence
comparison and nav could open an editor on something the user can't see. Filter to
visible surfaces before resolving or scanning.

**Tile enumeration is a shared primitive.** §2a's collapse-to-locked-tile, §2b's
cross-surface scan, and the roving-tabindex (below) all consume **one helper** that
returns the ordered, deduped, *visible* locked tiles (and "the tile for a given element /
`anchorR0`, exact-or-nearest"). Build it once.

**Roving-tabindex aligns to the locked tiles (cheap).** The not-editing arrow nav
(`useBlockEditHover.tsx:162`) currently `.focus()`es raw `data-block-pool-id` leaves, so
the focus ring lands on a leaf while Enter (→ `activate()` → locked resolution) opens the
*tile* — and it can land on a hidden block. Point its existing next/prev/wrap at the shared
visible-locked-tile helper and focus each tile's representative element. No new tabindex
bookkeeping (host stays the sole Tab stop; leaves stay `-1`); the focus ring then matches
the edit target and skips hidden blocks for free.

All locked targets are cleanly sliceable; buffers are `sliceBytes(...).trimEnd()` (with
line-ending normalization — see §2b/§2c) — including the whole-list/quote-with-markers
case. **No regeneration.**

> **Deliberate coarseness — and its collaboration cost:** in locked mode you edit a whole
> list/blockquote as one buffer (clicking item 6 of a 10-item list opens all ten). This is
> the safe, regeneration-free default; per-item editing is the Phase-3 unlock.
> **Known risk (documented, out of scope here):** the locked unit is also the
> *concurrency-clobber* unit. Two users editing different items of the same list both hold
> the whole-list buffer; whoever commits last rewrites the entire list — last-writer-wins,
> because the CRDT can't merge two full-container rewrites. Per-item (unlocked) editing
> splices each item at its own `destSI` and doesn't collide. Editing a list also always
> reformats/renumbers the whole list on commit (the existing wholesale rewrite, now reached
> on every list click). Not addressed in this plan.

This **changes the default click target** vs. today (leaf-first): a paragraph inside a
chrome-less single-child div now opens the **div**. Intended.

### 2b. Cross-surface cursor + concurrency-safe identity

While editing, **ArrowDown on the last visual line** → next tile; **ArrowUp on the
first visual line** → previous; wraps. Moving **commits** (empty = cancel; unmodified =
no commit, per the dirty guard). Tiles are the locked partition, ordered by source
position.

#### Data model — `editTarget`

Re-key `editTarget` from `poolId` to a **byte-offset identity plus a root-held draft**:

```
editTarget = {
  anchorR0,        // byte offset of the edited node's source start (NOT a line number)
  anchorR1,        // byte offset of its end
  anchorSlice,     // normalize(sliceBytes(content, anchorR0, anchorR1)).trimEnd() — frozen at open
  draft,           // the LIVE editable text (controlled value), held at the iframe root
  contentHeight, boxStyle,   // the existing measure-and-set box
}
```

- **`anchorR0` is a byte offset, not a line.** Identity wants exactness: the dispatcher
  match is the direct integer compare `resolved.sourceEntry.r[0] === editTarget.anchorR0`
  (everything it needs is already in `resolved`; no new plumbing at the match site). A line
  number would be coarser and, for a nested child whose `r[0]` sits *after* the `> ` marker
  (assumption B), would not even be a line start. The line number `anchorLine = lineOf(anchorR0)`
  is *derived* (byte→line map) and used **only** for the arrow-distance / `destLine` math below.
- **Selection captures the identity; matching is rect-free.** *Selection* (which tile)
  runs **once, at event time** (click/arrow keydown) when the DOM is laid out — the locked
  rules need rects. `activate()` (the selection site) gains pool access — which §2a's locked
  resolution needs anyway — and captures `{anchorR0, anchorR1, anchorSlice}` off the resolved
  tile's pool entry plus `ctx.content`. *Matching* (which block renders the textarea) is then
  **rect-free and render-safe** in `isBlockEditTarget` (the 2 `Block`/`CustomBlock` sites, not
  the 16 leaf emit-gates): `resolved.sourceEntry.r[0] === editTarget.anchorR0`. This works
  because a container and its child have **distinct `r[0]`** (assumption B; among **Original**
  entries, so the match filters to `t==0,d==0`, which `resolveSource` already does).
- **The textarea is controlled, with the draft at the root.** Today it's uncontrolled
  (`defaultValue`), so any remount discards typed text. But block children are index-keyed
  (`utils.tsx:167`), so a collaborator edit *above* you remounts the textarea — the exact
  re-render self-heal targets. So the draft must live **outside the DOM node, at the iframe
  root** (alongside `editTarget`, which survives every block re-render): `value={draft}` +
  `onChange`. On remount React re-renders it from the surviving `draft` → text preserved.
  Keep the per-keystroke `setState` isolated so typing doesn't re-render the whole document
  (a dedicated small state/context consumed only by the textarea). **Handle IME composition**
  (`compositionstart`/`compositionend`) so a mid-composition `setState` doesn't prematurely
  commit/duplicate CJK/dead-key input. **Seeding moves to `activate`** (selection time): the
  draft (and, in Phase 3, the regenerated-buffer choice) is read once at open; `renderBlockTextarea`
  becomes a pure `value={draft}` renderer. The dirty guard is then just `draft !== anchorSlice`.

#### Self-heal across an external re-render

The active edit **self-heals** across a collaborator's re-render (the editor stays *open* on
the right block) — distinct from focus-restoration (a *plain commit*). On an external
re-render with the editor open:

```
const orig = pool.filter(e => e.t === 0 && e.d === 0);
let cand = orig.find(e => e.r[0] === anchorR0)                 // exact preferred
        ?? minBy(orig.filter(e => e.r[0] >= anchorR0), e => e.r[0]); // nearest-at/after fallback
if (cand && normalize(sliceBytes(content, cand.r[0], cand.r[1])).trimEnd() === anchorSlice) {
    anchorR0 = cand.r[0]; anchorR1 = cand.r[1];   // re-anchor; anchorSlice + draft unchanged
    // keep editor open; dispatcher re-matches by the new anchorR0
} else {
    setEditTarget(null);                          // drop: discard draft; focus per "drop focus"
}
```

- **Exact is preferred; nearest is only a fallback** (the `??`). The `anchorSlice`
  byte-equality is the *actual arbiter* — applied to **either** candidate, because a
  collaborator's byte shift can place a *different* block exactly at `anchorR0`. Preferring
  exact shrinks the duplicate-content false-positive window (below) to the astronomically-rare
  exact-collision.
- **`anchorR0` is exact only when bytes before the block are unchanged** (your own edit;
  a commit re-render). A collaborator editing *above* shifts `r[0]`; the nearest-at/after
  candidate is then content-verified. **Re-anchor** (write the new offset back) or the next
  re-render's exact match misses again.
- **Drop on mismatch is the policy** (your node was edited under you): discard the draft.
  At this granularity a merge is unlikely, so dropping is the safe, intended outcome.
- **Duplicate-tile false-positive (rare, controlled-value consequence):** on the *nearest*
  path, a byte-identical neighbour can pass the content check. Without controlled value this
  merely lost edits (remount reset); **with controlled value the draft is preserved and a
  commit can land on the identical neighbour.** Still bounded (identical content, adjacent)
  and rare; the exact-preferred rule above is the mitigation. Documented, not engineered away.
- **Also drop if the active surface becomes invisible.** A re-render can move your block into
  a collapsed region; the dispatcher still matches by `anchorR0` and would render a focused
  textarea inside `display:none`. In the post-render layout effect, if the active element has
  no box (`offsetParent === null` / zero rect — the §2a predicate), **drop**.

#### Three post-commit / re-render outcomes — don't conflate them

A commit carries `pendingLanding { intent, ... }`:
- *Plain commit* (Esc / Cmd-Enter / blur, **no** nav) → editor **closes**;
  `intent:'focus'` → after the settled render, **focus the edited tile** by its own
  `anchorR0` (the real focus-restoration fix; the old `setTimeout` pool-id version is
  `entry.tsx:385–403`). This lets roving-tabindex resume there.
- *Move* (arrow or click-switch) → editor closes; `intent:'activate'` + `destLine` →
  open the destination editor (below).
- *External re-render* (collaborator) while editing → **no** commit; self-heal (above).

#### Focus on a drop

A drop (content-mismatch, invisibility, or no-candidate) is **not** a commit, so the
`intent:'focus'` path doesn't apply. **Focus the nearest *visible* tile at/after the
(re-anchored) `anchorR0`, best-effort, never reopening an editor.** Focus is low-stakes
(it positions roving-tabindex; it writes nothing), so an approximate landing is acceptable
even though an approximate *edit target* was not — which is exactly why we drop the edit
but keep the position. Reuse the existing "don't steal focus if a new edit started" guard
(`entry.tsx:393`, re-keyed to `anchorR0`). If no visible candidate, don't force focus.
*Optional UX:* a brief "edit discarded — block changed remotely" signal so the loss is legible.

> **One shared primitive.** "Find the visible DOM element / tile for an `anchorR0`
> (exact, else nearest-at/after)" backs self-heal candidate-finding, focus-restoration
> (now a pool scan, not a `querySelector` by pool-id — the attribute is the index `s`, not
> `r0`, so `entry.tsx` root gains pool access), drop-focus, **and** the roving-tabindex
> alignment. Define it once.

#### Navigation mechanics

- **Ordered tiles, scanned not sorted.** Derive candidates from the live, **visible**
  `[data-block-pool-id]` at **event time** (committed DOM, rects available), collapse each
  to its locked tile and dedupe, and **linear-scan** for next/prev (track nearest past the
  edge plus the global extreme for wrap) — via the shared helper. A single-tile document is
  a no-op.
- **Destination, unified formula.** Let `L0` = the cell's start line (stable across its
  own edit) and `n` = the textarea's current line count. Down → first tile at/after `L0 + n`;
  up → the tile before `L0`; wrap at the ends. For an unchanged cell `n` is the original, so
  it reduces to "the next tile."
- **Self is exact; the destination is a *projection*.** The active edit re-finds itself by
  exact `anchorR0`. But the *destination* shifts when a modified commit grows/shrinks the
  current cell, so it's stored as a **projected line `destLine`**, not a fixed offset:
  delta-adjusted (`L0 + n`) for a destination that *follows* the edit, unadjusted for one
  that *precedes* it. Post-render it resolves to the nearest locked tile at/after (down) or
  before (up) `destLine`. A fixed `destR0` would silently miss.
- **A "move" is arrow-nav *or* a click onto a different tile** — both commit-and-reland.
  Wire the logic into the textarea `onKeyDown` *and* the activation path.
- **Resolution timing — one branch on the dirty flag.**
  - *Unmodified* → no commit, resolve **synchronously**, hop. No editability gap.
  - *Modified* → commit (SET_AST), `setEditTarget(null)`, stash `pendingLanding {intent,
    destLine, direction, desiredColumn, fromFile, fromContent}`, resolve on the next render.
    Primary trigger: `renderedContent` differs; **fallback: next render regardless** (a
    dirty-but-byte-identical commit can't strand the cursor). **Dirty = `draft !== anchorSlice`**
    (both normalized — see §2c; kills type-then-undo).
  - *Reland needs a post-layout measure.* Resolving `destLine → tile` is source-based, but
    *opening* the destination needs its measure-and-set box, so the reland is a layout-effect
    (lay out, measure, swap in the textarea — two renders, like `activate()`). The "blink off"
    is inherent.
  - **`pendingLanding` is cancelled** on file switch / nav-epoch bump / Esc; `fromFile` is
    file-scoped (a new file's content differs but must *cancel*, not resolve).
- **Only bare arrows move.** `Shift`/`Ctrl`/`Alt`/`Meta` + Arrow keep native textarea
  behavior and must not leave the surface.
- **Caret on arrival + "last visual line" detection (v1 capture-and-clamp).**
  - *Visual vs logical line.* A textarea's ArrowDown moves by **visual** (soft-wrapped) rows,
    not `\n`-delimited logical lines. So "am I on the last line?" (the jump trigger) is a
    **visual/geometry** question, *distinct from* the **logical** source line used as the
    `destLine` coordinate. Don't conflate them: trigger from geometry, compute the destination
    in source-line arithmetic.
  - *Detection.* Textareas don't expose a caret rect. Use a **mirror div** (a hidden element
    styled identically — font, line-height, padding, `clientWidth` so a scrollbar doesn't
    desync wrapping, `white-space:pre-wrap`): copy the value up to `selectionStart`, append a
    marker span, read its `offsetTop`. First visual row ⇒ top within ~one line-height of 0;
    last ⇒ within one line-height of the mirror's `scrollHeight`. Build it as a small tested
    utility. **Geometry only exists in a real browser — Playwright, not jsdom.**
  - *Caret placement.* Capture the exit logical column (`selectionStart − lineStart`); on
    arrival place the caret on the first (↓) / last (↑) line at `min(column, lineLen)`.
    *Known limit (deferred):* `setSelectionRange` resets the native goal column, so a desired
    column isn't carried past a *short* arrival line without owning goal-column ourselves.
- **Intermediate state.** A *modified* move blinks editability off (unavoidable — blur *is*
  the commit). Unmodified hops are seamless.
- **Accepted imprecision.** The writer's reflow, not the textarea line count, sets the true
  footprint, so the projected destination is an estimate; nearest-tile absorbs it. Worst case:
  off by one tile — the editor opens on a neighbor, never on nothing. (Exact-through-concurrency
  would be Automerge cursors — see *Deferred*.)

#### CRLF / line-ending normalization (load-bearing for the comparisons)

`content` *is* the raw parse-input source, and **pampa parses CRLF natively** (offsets
against raw bytes including `\r`; `treesitter.rs:1311`, no normalization in the render path).
The textarea **LF-normalizes** its value (HTML spec). So `sliceBytes` of a CRLF document
yields `\r\n` while `draft` is `\n` — making `draft !== anchorSlice` (dirty guard) and the
self-heal content check both **misfire** on CRLF docs (false-dirty, false-drop). **Fix:
normalize `\r\n`→`\n` on every *sliced string* — `anchorSlice`, the seed for `draft`, and any
candidate slice — never on the `content` buffer itself** (that would desync every offset, which
are CRLF-domain). The commit already LF-ifies the edited region (the qmd writer emits `\n`), so
broader CRLF *fidelity* is pre-existing and out of scope.

### 2c. Dirty guard (lands here; needed by 2b and by Phase 3)
With the controlled value, the guard is **`draft !== anchorSlice`** (both line-ending
normalized) — skip the on-blur commit entirely when unmodified (treat as cancel). This
(a) gives 2b its "unmodified move → no commit" path, (b) fixes the pre-existing bug where
a top-level list re-parsed on blur renumbers with no edit, and (c) removes the *unmodified*
half of the §Load-bearing data-integrity bug (an untouched silently-retargeted editor no
longer re-writes the wrong block on blur).

### TDD work items
- [x] **byte→line map** (unit): line-start table + `lineOf`; CRLF + trailing-newline.
- [x] **Locked resolution** (RTL done in P2.2; real-browser epsilon tuning → P2.5 Playwright): chrome-less single-child div →
  the **div**; multi-child div → the clicked **child**; a blockquote (single- or
  multi-child, click anywhere — text *or* rule) → the **whole blockquote**
  (prefixing-atomic); a list (any size) → the **whole list** (prefixing-atomic); nested
  prefixing containers (list-in-blockquote, blockquote-in-list-item) → the **outermost**
  prefixing container. Coincidence epsilon (~1px, all four edges) pinned with a
  hairline-border fixture (→ leaf). *(Confirmed 2026-06-13: blockquote is atomic in
  locked mode like lists; the earlier "blockquote text → child" line was stale — full
  per-layer descent is the Phase 3 unlock. Reason: only the outermost prefixing
  container has a clean byte-slice; inner targets' slices carry the outer `> `/indent.)*
- [x] **Ordered tiles** (RTL; enumerate P2.2, next/prev scan P2.4b): next/prev derived from live `[data-block-pool-id]` at
  event time, locked-resolved, linear-scanned; a `HorizontalRule` between paras is
  skipped; partition has no container-then-child redundancy.
- [x] **Visibility filter** (RTL done in P2.2; real collapsed-callout → P2.5 Playwright): a **collapsed callout's** body is skipped
  by the climb, the tile scan, **and** the roving-tabindex — no editor opens on a hidden
  block; the zero-rect doesn't corrupt the coincidence comparison.
- [x] **Roving-tabindex alignment** (RTL, P2.2): not-editing arrows focus locked-tile
  representatives (not raw leaves), skip hidden tiles, and Enter opens the same tile the
  focus ring is on.
- [x] **Cross-surface nav** (RTL, P2.4b; geometry mocked → real soft-wrap in P2.5 Playwright): ArrowDown last line → next tile; ArrowUp first → prev;
  wrap both ways; arrow *within* a multi-line buffer does not leave; modifier+Arrow at an
  edge does not leave.
- [x] **Dirty guard** (RTL, P2.3a): blur without typing → no commit, document unchanged (write
  first; fails before the guard). Type-then-undo → unmodified. **CRLF**: a CRLF-source
  block, blur untyped → no commit (normalization; fails before the fix). Also empty→cancel + IME.
- [x] **Controlled value survives remount** (RTL, P2.3a + P2.3b): type into a block, simulate an external
  re-render that **shifts indices** (insert a block above), assert the draft is **preserved**
  (not reset to original) and the editor stays on the right block. *(P2.3a: controlled-value/ref
  survives a remount; P2.3b self-heal re-anchors so it stays on the right block.)*
- [x] **Trigger robustness** (RTL, P2.4b): unmodified move → synchronous hop, no commit, no gap;
  modified move → commit + `setEditTarget(null)` + `pendingLanding`; a dirty byte-identical
  commit still resolves (fallback); a file switch cancels a pending land.
- [x] **`r[0]` uniqueness** (unit): every block's `pool[s].r[0]` is distinct in a doc with
  nested containers (the assumption the `anchorR0` match relies on).
- [x] **Self-heal — keep** (RTL via real `PreviewRoot`): the active editor survives an *external*
  re-render that edits *elsewhere* — stays open, re-anchored, draft preserved.
  **Was FOUND BROKEN 2026-06-13** — P2.3b's `SelfHealHarness` test was theater (it reimplemented the
  effect), masking that KEEP was unreachable in production; the real `p2-3b-real.integration.test.tsx`
  exposed it. **FIXED** (see *Self-heal design bug* below): removed the broken tile-based visibility
  check; KEEP works (dirty + unmodified, offset-unchanged + offset-shifted), fail-on-revert verified.
- [x] **Self-heal — drop** (RTL via real `PreviewRoot`, `p2-3b-real`): an external edit *to the active
  block* (content mismatch) **closes** the editor and discards the draft; drop-focus best-effort.
  *(DROP + no-spurious are genuinely covered + fail-on-revert verified. KEEP and the commit-on-drop
  corruption are the fix below.)*
- [~] **Active editor goes hidden** (collapsed region → drop): **DEFERRED 2026-06-13.** The original
  tile-based visibility check was *exactly* what broke KEEP — while editing, the active block is a
  textarea wrapper with **no `data-block-pool-id`**, so `tileForAnchorR0` can never find it → always
  reads "hidden" → spurious drop. The fix below **removed** that broken check (restoring KEEP). The
  correct collapsed-region drop — measuring the editor's own wrapper (`activeEditRegionRef`) box
  *after* the re-anchor remount — is **deferred**: jsdom returns zero rects for everything, so it
  needs Playwright (real layout) to test, and the case is rare (a collaborator re-render moving your
  *unchanged* edited block into a `display:none` region → an invisible focused textarea; not data
  corruption). Tracked here + in a `PreviewRoot.tsx` code comment. **→ P2.5/Phase-3 Playwright.**
- [x] **Focus-restoration** (RTL, P2.4c): a *plain* commit (Esc / Cmd-Enter / blur) closes the
  editor and returns focus to the edited tile by `anchorR0` (not a stale pool id), so
  roving-tabindex resumes there.
- [x] **Update existing editing tests** — done incrementally across P2.2–P2.4 as each task changed
  behavior; **verified by the P2.5 audit**: no stale `editTarget.poolId` or uncontrolled `defaultValue`
  remains anywhere; the corpus is on the new locked-tile/controlled-value model. *(The audit also found
  the P2.4b/c + P2.3b tests were harness "theater" — replaced with real-`PreviewRoot` coverage in
  P2.5a + the self-heal fix below.)*
- [x] **Click-switch** (RTL): clicking from a *dirty* tile A onto tile B commits A and
  lands the cursor in B (same reland path as an arrow move), via B's `anchorR0` after the
  re-render.
- [x] **Caret on arrival** (RTL done P2.4a/P2.4b; mirror-div real-browser verification → P2.5 Playwright): ↓ lands on the destination's first line, ↑
  on its last; exit column preserved (clamped on a short line); mirror-div last/first-visual-row
  detection verified in a real browser.
- [x] Playwright (`q2-preview-block-nav-p2-5b.spec.ts`): 13 tests covering arrow-nav, click-switch,
  locked resolution, caret on arrival, soft-wrap. **Two bugs found and fixed:** (1) `isOnLastVisualLine`
  false-negative in Chromium (scrollHeight integer rounding — fixed with 2px LAST_LINE_TOLERANCE in
  `caretGeometry.ts`); (2) `requestMove` bailed out on 2-tile docs (active tile's pool-id absent from
  DOM during edit — fixed guard from `<= 1` to `=== 0` in `PreviewRoot.tsx`). All 13 pass. Commit: 05500132.
- [x] Implement 2a (locked resolution + roving-tabindex alignment) + 2b (nav, byte-offset
  identity, controlled value, self-heal, focus) + 2c (dirty guard). *(All implemented; the
  self-heal KEEP path + commit-on-drop are re-opened as the design-bug fix below.)*

### Self-heal design bug (found 2026-06-13 by real-`PreviewRoot` testing) — fix

Retiring the `SelfHealHarness` test theater (it reimplemented the effect and so passed against a
fiction) and writing the test against the **real `PreviewRoot`** exposed that the headline
data-integrity feature is broken in production. Two bugs:

- **Bug 1 — KEEP is unreachable; self-heal drops on (essentially) every external re-render.** The
  self-heal effect (`PreviewRoot.tsx` ~:214–253) re-anchors correctly (Step 1, pure pool/content),
  then in Step 2 checks visibility via `tileForAnchorR0(host, pool, cand.r0, {exactOnly:true})`. But
  while editing, the active block is a **textarea wrapper with no `data-block-pool-id`**, so that
  tile lookup can never find it → returns `null` → read as "hidden" → drop. The re-anchor is
  immediately overridden by a false hidden-drop. **Root cause: the visibility check asks the
  *tile* oracle about something that, by construction, is never a tile while edited.**
- **Bug 2 — commit-on-drop corrupts.** When self-heal (or the transient re-render where the old
  `anchorR0` stops matching) unmounts the textarea, its `onBlur` fires → `commitIfDirty` writes the
  **stale draft** onto whatever block now occupies that byte range. Worse than the original bug.

**Fix:**
- Separate the two questions that were conflated: *"did my block survive?"* (logic — pool/content,
  already correct) vs *"is my block currently visible?"* (DOM). Step 2's visibility check must use
  the active editor's **own wrapper** via `activeEditRegionRef` (set by the Phase-1 fix), **not**
  `tileForAnchorR0`. And it must run **after** the re-anchor (re)mounts the textarea — i.e. a
  **follow-up layout effect** keyed on the open editor, checking `activeEditRegionRef.current`'s box
  (`offsetParent`/zero-rect = collapsed region → drop). The self-heal effect itself just re-anchors
  (KEEP) or drops on content-mismatch/no-candidate — no DOM visibility check inline.
  *(Fallback if the follow-up timing proves fiddly: ship KEEP-correctness first by removing the
  broken tile-based check; re-add the collapsed-region drop as a small follow-on. Prefer the
  principled two-effect version.)*
- Guard `commitIfDirty` (`dispatchers.tsx`) to **no-op unless this textarea is still the active edit
  target** (`editTarget` non-null and `anchorR0` matches). On a drop, `editTarget` is null/re-anchored
  away, so the unmount-blur discards the draft (intended DROP semantics) instead of committing it.

**TDD work items (against the real `PreviewRoot` — `p2-3b-real.integration.test.tsx`; NO harness
reimplementation; fail-on-revert mandatory):**
- [x] **KEEP** (was impossible to write as passing; now passes): open an editor, simulate a
  collaborator edit *elsewhere* → editor **stays open** on the same block (re-anchored `anchorR0`),
  **draft preserved**. Covers dirty + unmodified, offset-unchanged + offset-shifted. Fail-on-revert:
  a surgical revert of the broken tile-based check fails the KEEP tests. (Commit `86738bff`.)
- [x] **Commit-on-drop guard**: open an editor, type (dirty), simulate a collaborator edit *to the
  active block* → editor drops **and `commitTextEdit` is NOT called** with the stale draft.
  Fail-on-revert verified (reverting the guard → stale draft committed). Plus a follow-up hoisted the
  guard to the top of `commitIfDirty` so the *cancel* branch can't fire on a stale textarea either
  (commit `2e6e1133`; hardening — the race is not jsdom-reproducible, test kept as a regression guard).
- [~] **Collapsed-region drop (reworked)**: **DEFERRED → P2.5/Phase-3 Playwright** (jsdom has no
  layout; the `activeEditRegionRef`-box check can't be tested without real rects). The broken
  tile-based check was removed; this correct version is the remaining piece. Documented in code + ↑.
- [x] Existing **DROP (content mismatch)** and **no-spurious-on-fresh-open** stay green; fail-on-revert
  on the self-heal effect still breaks the DROP + KEEP tests. (Real `PreviewRoot`, no harness.)

### Self-heal on write (architectural follow-up — added 2026-06-13; not yet implemented)

The stale-commit failure modes we keep hitting (the commit-on-drop corruption, the unmodified-KEEP
cancel race, the dirty-edit-in-a-collapsing-callout race) are **one root cause**, not separate bugs:
**identity was centralized for *reads* but not for *writes*.** We added the byte-offset identity
(`editTarget`/`editTargetRef`) + self-heal so that *matching* (which block renders the textarea) and
*keeping/dropping* always track the live, re-anchored location. But the **commit** still takes its
destination from a **per-render closure** — `resolved.sourceEntry`, frozen at the render that mounted
the textarea — and fires from a **DOM-lifecycle event** (`onBlur`). So when an external AST change
shifts a block, React unmounts the old index-keyed textarea, and *its* `onBlur` fires **during the
swap** carrying the *old* closure → a commit aimed at a stale byte range. The components do refresh;
the stale write is a **parting shot from the outgoing instance**, using the snapshot it was born with,
while self-heal re-anchors the live identity elsewhere. Two identities for one block (live + frozen),
and the writer uses the un-healed one.

**Principle — "self-heal on write": there is ONE self-healed identity; both match and commit read it.**
- **Commit destination = the live `editTargetRef.current` range**, not the render closure. There is
  exactly one open editor; `editTargetRef.current` *is* the block being edited, already re-anchored by
  self-heal. Build the `sourceEntry` from `{t:0, r:[editTargetRef.current.anchorR0, anchorR1], d:0}`;
  if `current === null` (dropped), the commit **no-ops**. With no closure-captured destination, the
  whole "stale snapshot" class disappears, and the commit guard's job collapses from "do these two
  snapshots agree?" to the trivial "is there still an active target?"
- **Commits are intentional, not teardown side-effects.** Distinguish a real commit (Cmd-Enter, a
  genuine focus-leaves-to-elsewhere, an explicit move/click-switch) from a React-unmount `blur`; the
  latter must never write.

**Scope:** small + localized — the commit call sites (`EditTextarea`'s commit, `commitSubtreeEdit`)
+ the lifecycle gating. Not a rearchitecture; it **completes** the identity migration that reads
already finished. **Phase 3 should adopt this from the start:** the regenerated-buffer commit (§3c)
is a *new* write path — wire it to the live identity, don't inherit the closure pattern.

**Residual (separate, already Deferred — not solved by this):** byte offsets are **version-relative**,
so even the live re-anchored offset can be stale *at the parent* across a truly concurrent edit
(postMessage has no version handshake). The exact fix is **Automerge cursors** (version-independent
positions) — see *Deferred*. This follow-up closes the **within-iframe** window (the recurring one);
cursors close the **cross-actor** window (the rare, hard one).

---

## Phase 3 — "Depth cursor (nested blocks)" unlock (flagged)

A default-off user setting that turns the locked default into a power-user, depth-aware
editor. **Only here is AST regeneration reachable, so only here is its cost paid.**

### 3a. The setting
- **`unlockDepthCursor: boolean`**, default `false`, in hub-client's preferences
  (`hub-client/src/services/preferences/schema.ts` — add to `UserPreferencesSchema` +
  `DEFAULT_PREFERENCES`), a checkbox **"Depth cursor (nested blocks)"** in
  `SettingsTab.tsx` (mirror the `errorOverlayCollapsed` block), read via `usePreference`.
- **Threading — reuse the proven iframe wire; add the missing hub-client host leg.** The
  iframe-side path (`UPDATE_AST` payload → `entry.tsx` → `PreviewContext` → consumer) is the
  same one the SPA already drives for `editingDisabled` — reuse it. **Source the flag from
  `usePreference`** (per-device localStorage, like `errorOverlayCollapsed`), *not* a server
  flag. The new work is the **`ReactPreview` → `ReactRenderer` → `Q2PreviewIframe`** pass-through:
  hub-client threads no such flag today (only the SPA does, and it bypasses `ReactRenderer`),
  so `ReactRenderer`'s props interface gains its first preference-driven row. `usePreference`
  is reactive (`ReactPreview.tsx:266` precedent), so toggling mid-session re-renders and posts
  a fresh `UPDATE_AST`.
- **Both hosts opt in — hub-client via the setting, the SPA via a query param** *(revised
  2026-06-13: the SPA depth cursor is now in scope, not deferred).* The flag `unlockDepthCursor` has
  two sources feeding the **same** host-agnostic iframe behavior:
  - **hub-client:** `usePreference` (above) — reactive, so a mid-session toggle re-renders + posts a
    fresh `UPDATE_AST`.
  - **SPA (`PreviewApp.tsx`):** a **`?depthCursor=1` URL query param**, read once at load
    (`new URLSearchParams(location.search)`), passed as `unlockDepthCursor={true}` to
    `Q2PreviewIframe`. The SPA drives `Q2PreviewIframe` **directly** (no `ReactRenderer` hop), so this
    is just sourcing the flag + computing the buffer table (§3c) in `PreviewApp`. Read-at-load = **no
    live toggle** in the SPA (set the param, (re)load) — fine for a power-user/dev affordance; a live
    control is a later refinement, not needed. Still additive on top of the SPA's `--allow-edit`
    server gate (no `--allow-edit` → no editing at all; with it → Phase-2 locked, or unlocked when
    `?depthCursor=1`).
  - **The props stay optional** (mirror `editingDisabled`): a host with its opt-in off omits both
    `unlockDepthCursor` and `nestedEditBuffers`, and the iframe reads that as locked. So locked
    remains zero-touch on either host; unlocked is each host supplying the two fields.

### 3b. Behavior when on — depth identity is data-driven and unified with Phase 2
- **Click resolves to the leaf** (deepest-wins, no coincidence-climb-to-top, no
  prefixing-atomic — descend fully).
- **Identity is the same `anchorR0` as Phase 2 — it just tracks a deeper node.**
  `editTarget.anchorR0` is **always the node being edited** (the locked *tile* in Phase 2; the
  *cursor node* in Phase 3). Phase 3 stores one extra scalar, **`leafAnchorR0`** (the clicked
  path-bottom), so the **in** direction knows which child to descend toward. **No stored depth
  integer, no stored path:** the breadcrumb path is **derived from the AST each render** (the
  cursor node's ancestors via `resolveSource`), so re-validation is automatic — an ancestor-only
  change re-derives the path with the cursor unchanged. Self-heal/match/drop is **identical to
  Phase 2** (content-verify the edited node's `anchorSlice`, drop on mismatch, re-anchor).
- **Depth keys** mutate `anchorR0` along the path: **left = out** (cursor → its AST parent),
  **right = in** (cursor → the child whose range contains `leafAnchorR0`); **clamp at the ends
  at key-press time** (out stops at the outermost tile, in stops at the leaf) — *not* on
  re-render. A click within the active subtree is caret-only (the Phase-1 guard); a click
  outside resets to that area's leaf. If a structural change deleted the remembered leaf, **in**
  re-establishes one by descending first-child.
  - **Bindings (per-platform — `←`/`→`, no native conflict):**
    **macOS `Cmd+Ctrl+←/→`** (preserves `Option+Shift` word-select and `Cmd+Shift`
    line-select); **Windows/Linux `Alt+Shift+←/→`** (word-nav there is `Ctrl`-based).
    Mnemonic: breadcrumb — root on the left. **Modifier conflicts can only be verified by
    trying them — cross-platform Playwright** (assert the depth move *and* that native
    word/line-select still work on each platform).
- **Descending into prefixing containers** reaches their (possibly multi-line) children →
  **these need regeneration** (3c). Because identity is data-driven, **the controlled-value
  draft survives even when the active block's `siKey` shifts** (the clean buffer is read once
  at selection and lives in `draft`, never re-derived — see 3c).

### 3c. AST-regenerated buffers (flag-gated)
Editing a multi-line block inside a prefixing container by slicing pulls in `> `/indent.
Regenerate a clean buffer from the AST instead (reformatting accepted).

- **Restriction (computed in one WASM walk):** a block enters the table iff it (1) has a
  prefixing ancestor (a boolean flag during descent; fenced `:::` containers excluded)
  and (2) is multi-line in source (`content[r0..r1]` contains `\n`). **No reachability/
  `Opaque` filter in Rust** — over-inclusion is harmless, and replicating the TS
  classification would be a second source of truth. Typically a handful, often zero.
- **Rust:** `pampa::regenerate_nested_buffers(content, untransformed_ast_json) -> String`
  (JSON `{ siKey: cleanQmd }`, native-testable) using `qmd::write_single_block` →
  `trim_end`; `siKey` = `serializeSourceEntry` format. WASM export in
  `crates/wasm-quarto-hub-client/src/lib.rs` mirroring `apply_node_edit`. JS wrapper
  `regenerateNestedBuffers` in `ts-packages/preview-runtime/src/wasmRenderer.ts`.
- **Gating + reactivity (the perf win, done right):** in `ReactPreview`, compute the
  table in a `useMemo` keyed on **`[unlockDepthCursor, rendered.renderedContent,
  rendered.untransformedAstJson]`** — the table when the flag is on (and both inputs
  present), else a **module-level shared empty object** (referential stability, so an off
  render never churns the iframe effect). The flag dep makes a *mid-session toggle*
  recompute (`usePreference` is reactive). Wrap the WASM call in try/catch returning the
  empty object. When off: no WASM pass, no payload, no cost.
- **Seeding reads the table at selection time (which fixes the `siKey`-shift problem).**
  `serializeSourceEntry`/`siKey` is offset-based, so a collaborator edit *above* the active
  Phase-3 leaf shifts its `siKey` and would miss a *re-derived* lookup. But with the controlled
  value, `activate` seeds `draft = ctx.nestedEditBuffers?.[siKey] ?? normalize(sliceBytes(...))`
  **once at click time** (current render → correct `siKey`); the draft then lives in state and
  is never re-derived from the shifting table. So the shift is a non-issue for the active editor.
- **Plumbing:** `nestedEditBuffers?: Record<string,string>` (and `unlockDepthCursor?: boolean`) are
  **optional** fields onto `Q2PreviewIframe`'s UPDATE_AST payload + deps, through
  `entry.tsx`/`PreviewContext`. **Each host computes/passes them when ITS opt-in is on, else omits
  them** (omitted optional field → iframe reads as locked → zero-touch). Compute via the same gated
  `useMemo` on both hosts: `unlockDepthCursor && rendered ? regenerateNestedBuffers(content,
  untransformedAstJson) : EMPTY` (module-level shared `EMPTY` for referential stability).
  - **hub-client (`ReactPreview`)** — flag from `usePreference`; passes through the new
    `ReactRenderer → Q2PreviewIframe` hop (§3a).
  - **SPA (`PreviewApp.tsx`)** — flag from the `?depthCursor=1` query param (§3a); computes the table
    in the same `useMemo` and passes it **directly** to `Q2PreviewIframe` (no `ReactRenderer`). The
    SPA already has WASM + `untransformedAstJson`/`renderedContent`, so this reuses the existing
    `regenerateNestedBuffers` call — no new infra.
  *(History 2026-06-13: §3a originally scoped Phase 3 hub-client-only and a mid-draft "both hosts
  compute it" line conflicted with that; the SPA depth cursor was then brought into scope via the
  query param, so "both hosts compute it (gated by each host's opt-in)" is now correct. The
  optionality still guarantees locked is zero-touch on a host whose opt-in is off. Keep the props
  optional, mirroring `editingDisabled`.)*
- **`siKey` contract:** Rust keys from the untransformed pool; the iframe looks up from the
  transformed pool — they match because `resolveSource` is value-keyed. Rust test asserts the
  exact `"0:<r0>-<r1>:0"` string.

### 3d. Breadcrumb floating toolbar
- A **floating toolbar that fully intercepts its own mouse events** — it `stopPropagation`s
  on its pointer handlers so the host's delegated `onPointerUp`/`onPointerDown` never see
  them (no switch / leaf-reset). Works whether the chip is a normal child or a React portal
  (React event propagation follows the React tree). Sits on top (`z-index`).
- **Absolutely positioned, anchored to the active surface's top-left but sitting *above* it**
  (chip's bottom edge aligned to the surface's top, negative `top`), so it never occludes the
  first line of edit text. Never in document flow → zero reflow. At the very top of the
  document the chip sits in the page margin — no flip-below fallback needed.
- Shows the ancestor path (`Section › Div › Paragraph`, current level highlighted), **derived
  from the AST each render** (the cursor node's ancestors via `resolveSource`). Labels are each
  node's type `t` plus any id/class from the AST attr.
- Flanked by **`◀` (out) / `▶` (in) buttons**, with the platform shortcut as each button's
  tooltip — the **discoverability** answer and the **primary touch affordance** (touch has no
  modifier keys). Clicking a crumb jumps to that depth.
- **Shown only when `unlockDepthCursor` is on** — the 95% see nothing new.

### TDD work items
- [x] **Setting + threading — hub-client** (RTL): checkbox toggles `unlockDepthCursor`; the value
  reaches the iframe (new `ReactRenderer` pass-through → `PreviewContext`); off ⇒ **no
  `regenerateNestedBuffers` call**, on ⇒ buffers computed + passed. *(P3.2: `7f14e5ed`. Gate is the pure
  `computeNestedEditBuffers` helper shared by both hosts; gating fail-on-revert independently verified
  `c96f06a0`. Mid-session reactivity is structural — `usePreference` + the memo dep on the flag — and
  has no dedicated test; add one if desired.)*
- [x] **SPA query-param opt-in** (RTL): `PreviewApp.tsx` reads `?depthCursor=1` → passes
  `unlockDepthCursor` + computes/passes `nestedEditBuffers` **directly** to `Q2PreviewIframe` (no
  `ReactRenderer`). **No param ⇒ omitted ⇒ locked**, no `regenerateNestedBuffers` call (type-safe via
  the optional props). With `--allow-edit` + `?depthCursor=1` the SPA gets the unlock; `--allow-edit`
  alone stays Phase-2 locked. (Read-at-load: no live toggle — acceptable.) *(P3.2; SPA strict mocks
  fixed `b66f898a`.)*
- [ ] **off ⇒ no keys, no chip, leaf-click disabled** (RTL/Playwright): verify the unlock behaviors are
  inert when the flag is off. *(Deferred — can only be tested once the behaviors exist: keys + leaf-click
  in P3.3, chip in P3.4.)*
- [ ] **SPA depth-cursor e2e** (`q2-preview-spa/e2e`, real `q2 preview` binary): with `?depthCursor=1`,
  load a fixture → confirm leaf-click + a clean nested-blockquote-child edit; load without it → confirm
  locked (whole-quote). *(Needs P3.3 behavior; P3.5 e2e tier.)*
- [ ] Unlocked click → leaf; depth keys out/in along the AST path; clamp at the ends at
  key-press; click-in-subtree = caret-only; click-outside resets. Path derived from the AST
  (ancestor-only change re-derives with cursor unchanged).
- [x] Rust: `write_single_block` on a blockquote/list child → clean (no `>`/indent);
  `regenerate_nested_buffers` includes multi-line prefixed children (single- and
  multi-child), excludes single-line items and fenced-div children, keyed by `siKey`.
  *(P3.1, `f5cb3132` + `8a51bb92`. Restriction predicate: prefixing ancestor ∈
  {BlockQuote,BulletList,OrderedList,DefinitionList} ∧ multi-line. + WASM export
  + JS wrapper `regenerateNestedBuffers`. 14 integration tests; review APPROVED.)*
- [x] Rust **`siKey` contract**: exact `"0:<r0>-<r1>:0"`. *(P3.1)*
- [x] Rust **source fidelity**: a blockquote child with shortcode + inline math + raw
  span → buffer is source form, not expanded. *(P3.1)*
- [x] Rust **code-block fidelity**: a multi-line code block in a blockquote → content
  byte-exact (modulo `>`-removal). *(P3.1)*
- [x] Rust **offset-domain assertion**: untransformed pool `r[0]/r[1]` index the same
  string as `content` (so the multi-line check is correct). *(P3.1)*
- [ ] RTL: activating an unlocked multi-line blockquote child seeds `draft` from
  `nestedEditBuffers` (clean) at selection time; a single-line child slices. A re-render
  that **shifts the `siKey`** keeps the clean draft (controlled value, not re-derived).
- [ ] WASM round-trip: edit an unlocked multi-line blockquote child → commit → blocks
  outside the quote byte-verbatim, quote re-wrapped (snapshot, Tier-2).
- [ ] Breadcrumb (RTL/Playwright): chip renders at the active surface's top-left, shows
  the AST-derived path, `◀`/`▶` move depth and carry shortcut tooltips, crumb-click jumps;
  hidden when the setting is off. Bindings verified in cross-platform Playwright.
- [ ] Implement 3a–3d. *(3a setting + threading ✓ P3.2; 3b behavior + 3c regenerated-buffer commit → P3.3; 3d breadcrumb → P3.4.)*

---

## Implementation order & process
1. **Phase 1** — bug fix. Independent, no behavior change beyond stopping the climb.
2. **Phase 2** — locked resolution + roving-tabindex alignment + cross-surface cursor +
   byte-offset identity + controlled value + self-heal + dirty guard. No WASM. This is where
   the default click target changes (leaf → locked tile) and the live mis-target bug is
   fixed — call both out in the commit.
3. **Phase 3** — the unlock: setting + keys + leaf-click + descend + **flag-gated
   regeneration** + breadcrumb.

**Process:** Phases touch hub-client (`ReactPreview.tsx`, `ReactRenderer.tsx`) and the
SPA (`PreviewApp.tsx`) → each such commit needs a `hub-client/changelog.md` entry
(two-commit workflow). Phase 3 touches the WASM leg → full `cargo xtask verify` (not
`--skip-hub-build`) and the `build:wasm → build-q2-preview-spa → build --bin q2` chain
before any live `q2 preview` check. Phases 1–2 are Rust-free → `--skip-hub-build` verify
plus the hub-client JS suites suffice.

## Risks / watch-items
- **Tile enumeration cost on large docs** — each arrow press collapses every visible
  `[data-block-pool-id]` to its locked tile (a rect-climb per element), and `wrap` needs
  the global first/last. Cheap at normal sizes (cached layout); a watch-item for very large
  documents; the visible-only filter trims it.
- **Coincidence epsilon** — pinned at ~1px on all four edges (assumption A: true coincidence
  is exactly 0px, nearest deciding "differ" edge ≥~12px). The only tuning is the hairline-border
  fixture (1px border → leaf). Tune in Playwright, not jsdom.
- **`pendingLanding` vs a concurrent collaborator re-render (documented, out of scope).**
  postMessage has no request/response channel, so the iframe cannot tell *your* commit's
  re-render from a *collaborator's* (both change `renderedContent`). A concurrent edit landing
  first can resolve `destLine` against a doubly-changed document — worst case off by more than
  one tile. The self-heal content guard protects the *active* editor (the "self"); there is **no
  equivalent guard on the destination projection.** We looked before and have no way to wait for
  a specific commit's render. Accepted; exact-through-concurrency is the Automerge-cursor path
  (*Deferred*).
- **Controlled-value cost + IME** — per-keystroke `setState`; isolate the draft state so typing
  doesn't re-render the whole document, and handle composition events. Single textarea, so cheap.
- **Per-render second WASM pass + buffer payload (Phase 3 only)** — opt-in, so it doesn't
  affect the default path; on a pathological unlocked doc it re-walks the AST per render.
  Acceptable for the demo; production fix (fold into the render pass / lazy / cache) is later.
- **Two-host divergence (Phase 3)** — if only one host wires `nestedEditBuffers`, unlocked
  nested editing silently falls back to the polluted slice; the `siKey` mismatch is silent.
  One checklist item for both.
- **Wholesale container rewrite** (Plan 3, unchanged) — commits still reformat the enclosing
  container; snapshot, never byte-assert siblings.

## End-to-end verification (per phase)
- `cargo nextest run -p pampa` (+ new integration tests, Phase 3); full `cargo xtask
  verify` for Phase 3 (WASM); `--skip-hub-build` suffices for Phases 1–2.
- `npm run test:integration` (preview-renderer) + `npm run test:ci` (hub-client) +
  `npm run build:all`.
- Live `q2 preview`: (P1) clicking inside an open editor doesn't climb, repositions caret;
  (P2) clicking a single-child div edits the div, a list edits whole, arrow between tiles with
  wrap, blur-without-typing doesn't mutate, keyboard-arrow focus ring matches the edit target;
  (P3) toggle the setting on → leaf-click, depth keys + chip, an unlocked multi-line blockquote
  child edits clean (no `>`).

## Design history / deferred (no strands filed — this is the record)
- **Self-heal / concurrency is new in this plan** — nothing like it existed on `main` (the
  apparent "survival" today is accidental, from positional `s` stability, and silently
  mis-targets on a structural edit above; the only deliberate cross-render mechanism was the
  broken pool-id focus-restoration). Phase 2's byte-offset identity + content guard is the fix.
- **Controlled value over stable keys** — there is *no* render-stable per-block identity to key
  React on (offsets and `s` both shift), so the typed-text-survives-remount requirement is met
  by holding the draft at the root (controlled value), not by re-keying the child lists. The
  heavier portal-editor alternative was considered and not needed.
- **Single-child *affordance suppression* (dropped 2026-06-12).** An earlier plan removed a
  lone child's `data-block-pool-id` to expose its container. Wrong mechanism — it deleted
  surfaces from a tree (breaking the render-component path, forcing a 16-site gate refactor,
  and hitting an unresolvable list-item child-count question). The *instinct* survives — as the
  locked-mode **resolution policy** over a full surface set (deepest-wins + coincidence tiebreak).
- **Full desired-column carry across the seam** — needs owning goal-column over a wrapping
  textarea; Phase 2 ships capture-and-clamp; the carry is a refinement.
- **Automerge native cursors for exact position tracking through concurrent edits**
  (`A.getCursor`/`getCursorPosition`, used by `presenceService.ts`). The correct tool if preview
  ever tracks positions as cursors, but not wired into q2-preview today and not worth the
  parent-side plumbing + UTF-8↔UTF-16 bridge for the narrow benefit. This is also the only exact
  fix for the `pendingLanding` race and the self-heal nearest-tile imprecision. Phase 2's line
  resolver would swap to cursor resolution then.
- **SPA / hub-client edit-flag asymmetry (deliberate divergence).** The SPA gates on a server
  `allowEdit` flag; hub-client uses localStorage `usePreference` and threads no `editingDisabled`.
  Left as-is; may become useful to unify if hub-client ever needs server-side edit permissions.
- **Attr/class/id/style parity** between preview components and the HTML writer — separate
  audit plan `2026-06-11-react-html-attr-parity-audit.md`.

## References
- Plans 2a/2b/2c/3; design `2026-06-06-block-editing-design.md`.
- `ts-packages/preview-renderer/src/q2-preview/{sourceIndex.ts,dispatchers.tsx,useBlockEditHover.tsx,PreviewContext.tsx,entry.tsx,utils.tsx}`,
  `blocks/*`, `custom/*`; `iframe/Q2PreviewIframe.tsx`.
- `ts-packages/preview-runtime/src/wasmRenderer.ts`.
- `crates/pampa/src/writers/qmd.rs` (`write_single_block:2392`, `BlockQuoteContext` `> ` at `:128`);
  `crates/pampa/src/apply_node_edit.rs` (splice `:169`, `incremental_write` `:174`);
  `crates/pampa/src/writers/json.rs` (`write_with_config:1859`, fresh pool `:289`, intern ordinal `:400`);
  `crates/wasm-quarto-hub-client/src/lib.rs` (`apply_node_edit:2938`).
- `hub-client/src/components/render/{ReactPreview,ReactRenderer}.tsx`;
  `hub-client/src/services/preferences/{schema.ts,index.ts}`,
  `hub-client/src/hooks/usePreference.ts`, `hub-client/src/components/tabs/SettingsTab.tsx`;
  `q2-preview-spa/src/PreviewApp.tsx`.
- Automerge cursor reference: `hub-client/src/services/presenceService.ts`,
  `automergeCursor.probe.test.ts`.
