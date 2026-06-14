# Nesting-cursor UI enhancements — geometry snapshot, caret-aware nest-in, mode-aware highlight

**Date:** 2026-06-14
**Branch:** feature/block-editing-improvements (worktree `.worktrees/block-editing`)
**Builds on:** `2026-06-11-block-editing-improvements.md` (the nesting-cursor / outer-block
editor, Phases 1–3.5, all done). Same machinery; this plan is a follow-on set of UI
enhancements + one mechanical rename, all q2-preview-only.
**Status:** Design settled through two reflection passes, a source-grounded design review, a
fourth internal-consistency pass on the landing-core refactor (2026-06-14, recorded in
*Reflections* #14–15), **and a fifth consequence-chasing pass (2026-06-14) that verified every
cited source line against the worktree, parse-verified the acceptance fixture with the real binary,
and resolved the design questions in *Reflections* #16–22**. All cross-feature ambiguities resolved
with the user. TDD-first; checklist below is ready to execute.

> **All cited production files live under `ts-packages/preview-renderer/src/q2-preview/`** unless
> a path is given (e.g. `../utils/byteLineMap.ts`). The Playwright acceptance specs live under
> `hub-client/e2e/` — that SPA bundles `preview-renderer` from source and runs the real WASM, so
> the iframe-side TS is exercised through `hub-client`'s e2e tier even though it does not live there.

## Overview

Three UI enhancements to the (already shipped) nesting cursor, a vocabulary rename sequenced
first, and a **landing-core refactor** (Phase 0 of §2) that makes the editor's open/reland paths
reusable so caret-aware nest-in slots in as a parameter rather than a parallel code path:

0. **Rename "locked tile" → "outer block"** — a pure mechanical rename, committed before any
   feature work, so the features are written in the final vocabulary.
1. **Geometry snapshot** — fix wrong editing-surface sizes when moving the nesting cursor
   in/out. Capture the original rendered geometry of a multilevel block's whole subtree at
   activation (before the textarea swap) and size nest-move destinations from that snapshot
   instead of the edit-distorted live DOM.
2. **Caret-aware nest-in** — nest-in descends toward the surface the **caret** is in, not a
   frozen "first clicked leaf". Built on a refactored **landing core** (resolver + opener) that
   also unifies the existing arrow-move / click-switch open paths and adds commit-if-dirty to
   every nesting move (chord, ◀/▶ buttons, and breadcrumb crumb-jumps).
3. **Mode-aware activation indicators** — in locked mode the hover/focus highlight shows the
   **outer block** that will actually activate (not the deepest leaf); roving-tabindex becomes
   mode-aware to match. Fixes a latent stale-context bug in the hover handlers along the way.

All three are gated behind the existing `unlockNestingCursor` preference except Feature 3's
locked-mode highlight change (which improves the *default* locked experience).

## Background / where the machinery lives

Citations are post-rename names; the rename mapping is in §0. Current tree (pre-rename) uses
`lockedTiles.ts` / `resolveLockedTile` / etc.

- **Two pools, one join key.** `poolRef` is built from the **transformed** `astJson`
  (`PreviewRoot.tsx:859`); `sourceIndex` is built from the **untransformed** AST
  (`PreviewRoot.tsx:889`) and is keyed by `"t:r0-r1:d"` (`sourceIndex.ts:35,57-60`). Nesting
  navigation runs over the untransformed *surfaces*; the geometry snapshot measures *transformed*
  DOM elements (`pool[id].r`). **The only thing that joins a transformed DOM element to a nesting
  surface is the full `(r0,r1)` source range.** Everything that identifies a surface — the source
  index key, `buildNestingSurfaces`' dedupe, `parentSurface`/`childSurfaceToward`, and
  `applyNestingRetarget`'s `next` — uses `(r0,r1)`. This drives the §1 key choice and the §2
  resolver design.
- **Activation** — `useBlockEditHover.tsx`: `activate(el)` (`:59`) resolves the click mode-aware
  (leaf in unlocked, `resolveOuterBlock` in locked), captures the identity triple
  (`captureEditTarget`), measures the box (`measureBlockBox`), seeds the draft, and calls
  `setEditTarget`. Hover outline is `box-shadow` set in `outlineElement` (`:49`) off
  `findEditTarget` = `closest('[data-block-pool-id]')` (the deepest leaf — **the Feature-3 bug**).
  Roving-tabindex `onKeyDown` (`:219`, arrow loop `:227-238`) focuses `enumerateOuterBlocks`
  **unconditionally** (**the Feature-3 follow-on**). **Note:** `onPointerMove`/`onPointerLeave`
  are `useCallback(…, [])` (`:132,:217`) and close over a **stale `ctx`** (the provider value is a
  fresh object literal every render, `PreviewRoot.tsx:948-975`) — see §3 and Reflection #11.
- **Edit substitution** — `dispatchers.tsx`: `renderMeasuredEdit` (`:47`) replaces the *entire*
  matched subtree with a synthetic `<div id="q2-active-edit-region">` (ref =
  `activeEditRegionRef`). The wrapper div is sized by `editTarget.boxStyle`; `editTarget.contentHeight`
  sizes the inner `<textarea>` (`dispatchers.tsx:249`), not the div. The nesting chord is
  handled in `EditTextarea`'s `onKeyDown` (`:281`) via `classifyNestingKey` → `requestNestingMove`.
  The pending-caret hint is applied once on mount via `placeCaretAtColumn` (`:152-162`).
  `LEFT_INSET_STRIPPED_TYPES` (`:27`, applied `:60`) zeroes left margin/pad/border for the three
  **list** types (`BulletList`/`OrderedList`/`DefinitionList`) — **not** `BlockQuote`, so a
  blockquote editing surface keeps its left gutter (relevant to §2's `prefixWidth`).
- **Open / reland machinery** — `PreviewRoot.tsx`: `PendingLanding` (`:42-64`) has two variants
  today (`intent:'activate'` opens an outer block by line; `intent:'focus'` just focuses a tile).
  `executeLanding` (`:323`) resolves + opens; the reland layout effect (`:401-413`) and a 250 ms
  fallback timer drive it post-commit. `requestMove` (`:419`) is the arrow-nav producer (clean →
  sync hop `:469-483`; dirty → stash + commit + arm timer). `handleClickSwitchBlur` (`:612`) is
  the click-switch producer. **The find-destination-by-line block is duplicated** between
  `executeLanding` (`:352-373`) and `requestMove` (`:446-467`); the edit-target assembly is
  duplicated across `activate`, `executeLanding` (`:388`), `requestMove` (`:479`), and
  `applyNestingRetarget` (`:763`). Phase 0 of §2 collapses both.
- **Nest moves** — `PreviewRoot.tsx`: `requestNestingMove(dir)` (`:780`) → `applyNestingRetarget`
  (`:742`); the latter reseeds the draft from the destination buffer and re-anchors `editTarget`,
  measuring the destination box via `outerBlockForAnchorR0(... {exactOnly:true})` →
  `measureBlockBox`, **else keeping the current box** (`:752-761`, the stale fallback — **the
  Feature-1 bug**; and it reseeds **without committing** — **the §2 data-loss bug**).
  `requestNestingSelect(r0,r1)` (`:794`) is the breadcrumb crumb-jump; both share
  `applyNestingRetarget`. `pendingCaretRef` is set `null` on nest moves (`:762`).
- **Nesting surfaces** — `nestingNav.ts`: `buildNestingSurfaces(sourceIndex)` (`:62`) → sorted
  `{r0,r1}[]` (sort comment at `:80-84` anticipates *multiple surfaces at the same `r0`*);
  `parentSurface` (`:103`) / `childSurfaceToward` (`:147`) navigate by **byte containment**
  (`childSurfaceToward` uses an **exclusive** end, `:165`). `buildAncestorPath` (`:307`) builds the
  breadcrumb path from the **live cursor** (`anchorR0/R1`). No geometry anywhere — surfaces are
  pure source coordinates.
- **Line map** — `../utils/byteLineMap.ts`: `buildByteLineMap(content)` → `{lineOf, lineStart,
  lineCount}`, 0-based, CRLF-safe (`:69-115`). `lineStart` gives line→offset (needed for caret
  placement); `lineOf` gives offset→line.
- **Breadcrumb** — `BreadcrumbChip.tsx`: keys off `et.anchorR0/R1` (`:44`), repositions on
  `anchorR0/R1` change (`:40`). Buttons `preventDefault` on pointerdown (`:81,:91,:101`) so the
  textarea is **not** blurred (no blur-commit), and call `requestNestingMove('out'|'in')` (`:82,:102`)
  / `requestNestingSelect(r0,r1)` (`:92`). Because the textarea keeps focus, **`selectionStart`
  survives a button press** — the centralized caret read in §2 works for both chord and button.
- **Cross-surface move / reland** — see "Open / reland machinery" above. Nest moves (§2,
  commit-if-dirty) **reuse this infrastructure** via the Phase-0 landing core with a nest-specific
  resolver kind.
- **Self-heal** — `PreviewRoot.tsx:244-285`: on an external re-render, `findReanchorCandidate`
  re-anchors `anchorR0/R1` by content-verification (KEEP) or closes the editor (DROP). Keeps the
  box on KEEP.

---

## §0 — Rename: "locked tile" → "outer block"

The concept "which near-top block activates in locked mode" is currently called a **locked
tile**. Rename to **outer block** (reads against the mode pair: *locked mode activates outer
blocks; nesting mode descends to inner/leaf surfaces*). **One mechanical commit, first**, mirroring
the prior depth→nesting rename (`16382ff2`). No behavior change.

| Current | New |
| --- | --- |
| `lockedTiles.ts` (+ `lockedTiles.integration.test.ts`, `lockedTiles-p2-3b.integration.test.ts`) | `outerBlocks.ts` (+ matching test files) |
| `resolveLockedTile` | `resolveOuterBlock` |
| `enumerateLockedTiles` | `enumerateOuterBlocks` |
| `tileForAnchorR0` | `outerBlockForAnchorR0` |
| `measureTileBox` *(mode-neutral)* | `measureBlockBox` |
| `isVisibleTile` *(mode-neutral)* | `isVisibleBlock` |
| "locked tile" in comments/docs, test vars (`tileA`, `destTile`, `mockTileRects`, …) | "outer block" / `outerBlockA`, `destBlock`, `mockBlockRects`, … |

`measureBlockBox` / `isVisibleBlock` drop "tile" rather than gain "outer" because they operate on
*any* surface (nesting-mode leaves included). New symbols added by Features 1–2
(`snapshotOuterBlockGeometry`, `childSurfaceTowardLine`, `enumerateNestingLeaves`, the landing-core
`openEditTarget` / `resolveLanding`) are authored with final names in the renamed files.

**Identifier-scoped only.** The sweep renames *identifiers*, not string literals. Before running it,
grep for `tile`/`Tile` in CSS class names, `data-*` attributes, ARIA, and Playwright/test selectors
— a "no-op" rename that hits a selector string would change behavior. Also update the 3 in-file
callers of `isVisibleTile` (in `resolveLockedTile` / `enumerateLockedTiles`).

---

## §1 — Geometry snapshot for nesting-cursor moves

### The bug
The nest-move destination box is measured from the **live DOM at move time**, but during an
open editing session the active subtree is a synthetic textarea, so the destination surface is
distorted (nest-out: parent contains a textarea, not the real child) or absent (nest-in: the
child isn't rendered at all → `applyNestingRetarget` keeps the stale parent box). `PreviewRoot.tsx:751`
even documents the gap ("real box fidelity for 'in'/jumps is P3.5").

### The fix
Capture the original rendered geometry of the whole top-level block's subtree **at activation,
before the swap**, keyed by source identity, and have nest moves size from that snapshot.

**Capture (`snapshotOuterBlockGeometry`)** — a new pure helper in `outerBlocks.ts`:
1. From the opened element, climb to the outermost `[data-block-pool-id]` (the top-level block),
   reusing the chain-walk in `resolveOuterBlock`.
2. If the subtree has **≤ 1** visible `[data-block-pool-id]` (a flat block) → return empty (no
   nesting possible; the single `measureBlockBox` at open already sizes it).
3. Otherwise `querySelectorAll('[data-block-pool-id]')` within it, filter to visible, and
   `measureBlockBox` each.

**Key by the full block-relative range `(r0 − topBlockR0, r1 − topBlockR0)`** — *not* `r0` alone.
Rationale: the snapshot is a *join* from a transformed DOM element (`pool[id].r`) to an
untransformed nesting *surface* (the `(r0,r1)` navigation picks), and that join key is the full
range — it is the one place a surface must be identified by its whole range, exactly as the source
index, `buildNestingSurfaces`, and `parentSurface`/`childSurfaceToward` already do. (`r0`-only
collides for any container whose first child starts at the container's `r0`, and for coincident
wrappers; the prefix-marker model usually offsets `r0`, e.g. `BlockQuote r=[0,23]` vs child
`Para r=[2,22]`, which is why `r0`-only happens to pass today — but it is the wrong key for the
join regardless of collision frequency.) Value = `{contentHeight, boxStyle}`.

**Duplicate-key rule.** Two visible `[data-block-pool-id]` elements *can* share a source range
(a filter attributing a wrapper and its content to the same range). When they do they are normally
rect-**coincident** (same box → harmless). Keep DOM-pre-order-first (outermost), matching
`enumerateOuterBlocks`' dedupe convention (`outerBlocks.ts:188-191`). The non-coincident
same-range case is not produced by the current renderer; the key-uniqueness regression test
(below) will fail loudly if that ever changes.

**`topBlockR0` is derived, not stored** — the outermost `NestingSurface` containing the active
range. New pure helper in `nestingNav.ts`:

```ts
// outermost surface r0 containing [r0,r1]; the cursor's own r0 if already outermost.
export function topBlockR0(surfaces: NestingSurface[], r0: number, r1: number): number {
  let cur = { r0, r1 };
  for (;;) {
    const p = parentSurface(surfaces, cur.r0, cur.r1);
    if (!p) return cur.r0;       // NOT null — the last non-null (or the cursor's own r0 at top)
    cur = p;
  }
}
```

Computed from the *same* source index at both capture and lookup, so block-relative keys are
**shift-invariant under an insert-above** with no re-keying and no `editTarget` field. (Off-path-
internal edits remain best-effort — documented.) **Coupling to record:** capture keys subtract a
*source-surface*-derived `topBlockR0` from *transformed-pool* `r0` values; the two share a byte
space only because the transformed pool's source pointers equal the untransformed source ranges —
i.e. the §10 alignment assumption is load-bearing for the key *arithmetic*, not just set membership.

**Where capture runs** — at **three** fresh-open sites that seed a new draft, synchronously
*before* `setEditTargetRaw` (a post-open effect is too late; the children are already swapped out):
- `activate()` (`useBlockEditHover.tsx:96`),
- `executeLanding`'s open (`PreviewRoot.tsx:388`) — this is the reland for both arrow-move **and**
  click-switch (click-switch stashes `intent:'activate'` and lands *through* `executeLanding`; the
  earlier "`:387` click-switch" was a miscount — it is the same site),
- `requestMove`'s sync-hop open (`PreviewRoot.tsx:479`).
The §2 nest-reland adds a fourth open site; it captures a fresh snapshot on arrival.
Only the nest consumer (`applyNestingRetarget` via the landing core) **consumes** the snapshot; the
above opens still measure their own (rendered) outer-block destination live —
`snapshot[thatBlock]` equals that live measure by construction.

**Exposing the map.** The snapshot `Map` lives on `PreviewRoot` (`editGeometryRef`), but `activate()`
lives in `useBlockEditHover`. Expose a `captureGeometry(openedEl)` callback (or the ref itself)
through `PreviewContext` so the activation site can write it. (This is a new context surface —
mirrors how `requestMove`/`commitNestingEdit` are already threaded.)

**Consume** — the landing core's box resolution (`box: 'snapshot'`) replaces the live-DOM measure
in `applyNestingRetarget` (`:752-761`) with a snapshot lookup keyed by
`(next.r0 − topBlockR0(current), next.r1 − topBlockR0(current))`. Fallback to today's best-effort
(live measure if rendered, else keep) when the key is missing (jsdom/no-layout, or an unrendered
surface).

**Lifecycle** — `editGeometryRef: Map<string, {contentHeight, boxStyle}>` on `PreviewRoot` (string
key = `"${dr0}:${dr1}"`). **Each capture REPLACES the map** (assigns a fresh `Map`), so stale keys
from a prior subtree never accumulate. **Close / invalidate paths (three, plus self-heal):**
- *Plain close* (Esc / blur / Cmd-Enter cancel) goes through the `setEditTarget(null)` **wrapper**
  (`:191`) → **clears** the map.
- *Commit-and-reland* (dirty arrow-move, click-switch, **dirty nest move**) closes via
  `setEditTargetRaw(null)` **directly** (`:504,:515,:685,:700`, and the new nest path) — it does
  **not** clear the map; the reland re-captures a fresh snapshot on arrival, overwriting it (the
  stale boxes are never read across the gap).
- *Self-heal* (`:244-285`, **both KEEP and DROP**) → **clears** the map. DROP already closes via
  `setEditTargetRaw(null)` directly at `:274` (a *third* close path the earlier "two close paths"
  framing missed); KEEP keeps the editor open but must still invalidate.

So the rule is: *the map is valid only between a fresh capture and the next close / commit / self-heal;
otherwise consume falls back.* This is intentionally **not** "kept across self-heal KEEP."

**Why self-heal must clear (Reflection #16).** `findReanchorCandidate` (`lockedTiles.ts:409`) is
**content-based**, not a guaranteed uniform δ: it picks `exact ?? nearest` off the *old* `anchorR0`
and gates on bare text equality (`:417-429`). A deletion-above plus a content-identical block can
re-anchor the cursor into a **different subtree**, so block-relative keys would describe the wrong
top block; and KEEP spreads the old box forward without re-measuring (`:259`), so heights are stale
after *any* height-changing re-render (even a benign insert-above that merely reflows). The snapshot
is an optimization with a documented fallback, so clearing on self-heal and re-capturing at the next
fresh open is the cheap, correct choice.

**Gating** — only captured when `unlockNestingCursor` is on AND the block is multilevel. Locked
default and flat blocks: zero cost. (The arrow-move/click-switch open sites coexist with nesting
under `unlockNestingCursor` — the bare-arrow cross-surface move in `dispatchers.tsx:301-335` is
*not* gated on the preference, so a snapshot captured there is genuinely consumable by a subsequent
nest-in; it is not dead code under the gate.)

**Validation insight:** `snapshot[surface]` is *by construction* identical to the box a direct
click on that surface would produce (both measure the same element in the same pre-swap DOM). So
"nest-in to item 3" reproduces "click item 3", which is already correct.

---

## §2 — Caret-aware nest-in (built on a reusable landing core)

### Phase 0 — refactor the open/reland paths to a landing core (behavior-preserving)

Today four sites independently assemble an edit target, and the find-destination-by-line resolver
is copy-pasted between `executeLanding` (`:352-373`) and `requestMove` (`:446-467`). Factor two
reusable pieces so caret-aware nest-in is a *parameter*, not a parallel path:

```ts
// (2) ONE opener — collapses the THREE PreviewRoot-internal sites (executeLanding,
//     requestMove sync-hop, applyNestingRetarget). `activate` (in useBlockEditHover) does NOT
//     route through it — it shares only the pure seeding helper (below), keeping its pre-open
//     dedup local and avoiding a second new context method. The box source is decided by the
//     resolver (it returns it); caret uses the EXTENDED hint shape.
function openEditTarget(
  range: { r0: number; r1: number },
  opts: {
    box: 'snapshot' | { measure: Element } | 'keep';
    caret?: CaretHint;          // null/undefined → caret at top (default)
    leafAnchorR0?: number;      // preserved across nest moves; defaults to range.r0
  },
): void;
//   {anchorSlice, seededDraft} = seedForRange(range, content, nestedEditBuffers)  ← the pure helper
//   writes editDraftRef; resolves box (snapshot[key(range)] / measure(el) / keep);
//   sets pendingCaretRef = caret ?? null; setEditTargetRaw(...).

// Pure seeding helper, shared by `activate` AND `openEditTarget` (de-dups activate:88-89 /
// applyNestingRetarget:748). Lives in outerBlocks.ts (it is the captureEditTarget sibling).
function seedForRange(range, content, nestedEditBuffers): { anchorSlice: string; seededDraft: string };

// (1) ONE resolver — a PreviewRoot DISPATCHER (not pure: `outerByLine` needs the live DOM via
//     enumerateOuterBlocks; `nest`/`crumb` are pure-surface and delegate to nestingNav helpers).
//     It returns the box source because that is coupled to the kind (outer → measure the live
//     tile it found; nest/crumb → the snapshot, since their surfaces have no clean live element
//     while a textarea occupies the subtree).
type ResolverSpec =
  | { kind: 'outerByLine'; direction: 'down' | 'up'; destLine: number }        // today's 'activate'
  // nest: relocate the committed node by its (stable) start line, place by buffer-relative line:
  | { kind: 'nest';  direction: 'in' | 'out'; fromStartLine: number; caretBufferLine: number }
  // crumb: relocate the chosen ancestor by start line AND nesting depth (depth is commit-stable;
  // start line alone is ambiguous if an ancestor and its first child share a start line):
  | { kind: 'crumb'; targetStartLine: number; targetDepth: number };
function resolveLanding(
  spec: ResolverSpec,
): { range: { r0: number; r1: number }; caret?: CaretHint;
     box: 'snapshot' | { measure: Element } } | null;
//   reads poolRef / renderedContentRef / sourceIndexRef / previewHostRef internally.
```

Producers then reduce to *build a `(spec, caret)` and call one `move`*:

```ts
function move(spec, caret) {
  if (!isDirty) {                                        // synchronous — resolve against current render
    const r = resolveLanding(spec);
    if (r) openEditTarget(r.range, { box: r.box, caret: r.caret ?? caret });
  } else {                                               // commit → reland against the NEW render
    stash({ intent: 'open', spec, caret, fromFile }); commit(); close(); armTimer();
  }
}
```

`PendingLanding` generalizes to `{intent:'focus'} | {intent:'open', spec, caret, fromFile}`; the
reland effect/timer become: *(for a nest reland)* `captureGeometry(destTopBlock)` **then**
`const r = resolveLanding(pl.spec); if (r) openEditTarget(r.range, { box: r.box, caret: pl.caret ?? r.caret })`.
The capture-before-open ordering matters (the destination subtree is clean and rendered at reland;
once we open, it is swapped to a textarea).

**Layering:** the pure surface helpers (`childSurfaceTowardLine`, `parentSurface`, the crumb pick)
live in `nestingNav.ts` (no DOM/React, jsdom-testable). `resolveLanding`, `openEditTarget`, and
`seedForRange`'s *caller* glue live in `PreviewRoot` because `outerByLine` and the box/snapshot
lookup touch the DOM and refs. The earlier "resolveLanding is pure / lives in nestingNav" framing
was wrong — `outerByLine` is inherently DOM-based (`enumerateOuterBlocks(previewHostRef.current)`).

**Rollout — incremental, to protect the fail-on-revert-proven move/click-switch code:**
0. **Before extracting:** add a *characterization* test pinning the up/down find-by-line behavior —
   `executeLanding` compares the destination against `pl.destLine` for both directions, but
   `requestMove` projects `destLine = L0 + draftLineCount` for **down** and uses **`L0`** for **up**
   (`PreviewRoot.tsx:446-467`). The two "duplicated" blocks are NOT identical; the resolver must
   preserve the per-direction comparison line. No existing spec pins the `up`/`L0` case → it could
   be silently flattened by a "behavior-preserving" extraction. (Reflection #21.)
1. Extract `seedForRange` + `openEditTarget`; retarget the three internal assembly sites (and point
   `activate` at `seedForRange`). Pure dedup → existing specs stay green (the extraction is a
   behavioral no-op; that *is* the fail-on-revert lever for Phase 0).
2. Extract `resolveLanding` for `kind:'outerByLine'`; point `executeLanding` + `requestMove`
   sync-hop at it, **and migrate `handleClickSwitchBlur` (`:612,:653-659`)** — a *third* landing
   producer that today stashes `intent:'activate'`; when `PendingLanding` generalizes to
   `{intent:'open', spec, caret}` (dropping `'activate'`), this producer must move to the new shape
   too, or its dirty click-switch reland breaks. Removes the copy-paste; still no behavior change.
3. *Then* add `kind:'nest'` / `kind:'crumb'` + the extended caret — net-new behavior, own specs.

### Behavior
Nest-in descends toward the surface the **live caret** is in (not the frozen `leafAnchorR0`). The
live cursor + caret are the **single source of truth**; the breadcrumb re-derives from the cursor
each render and stays consistent.

### Mechanism — line space end to end
- **Caret → source line:** read the caret off the live textarea → buffer line `Lb` (count
  `\n` before caret) → `sourceLine Ls = lineOf(cursorR0) + Lb`. This holds for **both** draft kinds:
  a verbatim slice trivially preserves lines, and a regenerated clean buffer (below) preserves line
  count and order *except under writer reflow/normalization* — guarded by warn-and-proceed.
  **The clean buffer is a re-serialization, not a textual strip:** `regenerate_nested_buffers`
  (`crates/pampa/src/regenerate_nested_buffers.rs:160-202`) runs `write_single_block` on the block's
  AST and `trim_end()`s it, so the ancestor `> `/indent is *absent* (the block is rendered as if
  top-level) rather than line-by-line removed. Line-count/order correspondence is therefore a
  property of the **writer**, to verify per block type (paragraphs with soft line breaks are the
  risk); `nesting_cursor_roundtrip_tests.rs` covers round-trip *content* equivalence — add a
  line-index-correspondence assertion. (Reflection #19.)
- **Source line → child (synchronous / clean move):** new pure
  `childSurfaceTowardLine(surfaces, cursorR0, cursorR1, Ls, map, content)` picks the direct child
  whose **effective line span** contains `Ls`. The span MUST be computed from the surface's
  **trimmed** content range, not its raw `(r0,r1)`: node ranges absorb trailing whitespace / the
  next line's indentation, so raw line spans of siblings **overlap** — verified on the acceptance
  fixture, where the caret on the `nother` line is contained by *both* the sub-sub-item list
  `[39,60]` (whose raw end byte 60 reaches into the `nother` line) *and* `nother` itself. Use a
  shared helper `surfaceLineSpan(surface, content, map)` →
  `[lineOf(r0 + leadingWs), lineOf(r0 + sliceBytes(content,r0,r1).trimEnd().length − 1)]`, mirroring
  the `…trimEnd()` convention used by `anchorSlice` / the dirty guard / `findReanchorCandidate`. The
  end-trim resolves the demonstrated collision (the sub-sub-item list's effective span becomes
  `[3,3]`); the start-trim is symmetric insurance against a leading blank line. **Tiebreak** when
  more than one direct child still contains `Ls`: prefer the child whose **start line == `Ls`**;
  else the **narrowest** span; else nearest direct child by line distance. Line space also sidesteps
  the `> `/indent prefix (which only shifts *columns*) and `childSurfaceToward`'s exclusive-end byte
  containment. **`childSurfaceToward` (byte space) is retained**, not replaced — it is the
  no-readable-caret / programmatic fallback (see `leafAnchorR0` below). (Reflection #17.)
- **Dirty move — line-based parent re-resolution (the `kind:'nest'` resolver).** After a dirty
  commit you cannot content-verify the committed node (its bytes now hold the *edited* text), so —
  exactly like the existing arrow-move's line projection (`destLine = L0 + draftLineCount`,
  `:448`) — resolve by **line**: the committed container's **start line is stable** (an in-place
  list edit/renumber does not move its first line). So post-render, `resolveLanding(kind:'nest')`
  finds the surface at `fromStartLine` in the *new* source index, then `childSurfaceTowardLine`
  (for `'in'`) / `parentSurface` (for `'out'`) toward the projected caret line. No new resolution
  primitive — `buildByteLineMap` + the `nestingNav` helpers.
- **Line-count mismatch → warn-and-proceed:** if `draft` line count ≠ the cursor node's source
  line count, `console.warn(siKey, draftLines, sourceLines)` and **still** use line-anchoring
  (better than collapsing to line 1; a real reflow regression stays visible).

### Caret reading — centralized
`requestNestingMove('in')` reads the live textarea itself via
`activeEditRegionRef.current?.querySelector('textarea')` (its selection survives the
breadcrumb button's `preventDefault`-on-pointerdown), so the keyboard chord (`dispatchers.tsx`) and
the `▶` button (`BreadcrumbChip.tsx`) share one source of truth — no signature change. **The descent
line is read from the caret** (collapsed: `selectionStart === selectionEnd`); on a **non-collapsed
selection** use `selectionEnd` (the active end). **Ordering: read the caret synchronously, before
any commit/close unmounts the textarea**, and stash the resulting `caretLine` in the `kind:'nest'`
spec for the dirty path. (Reflection #18.)

### Round-trip caret placement (consistency) — one rule for nest-in *and* nest-out
The caret's **source position `(Ls, Cs)` is invariant under a clean nesting move** (the document is
unchanged), so both directions reduce to rendering that same source position into the destination
surface's local coordinates — there is **no nest-out special case** (Reflection #20):

```
destBufferLine = Ls − lineOf(destR0)
destBufferCol  = Cs − prefixWidth(destSurface, Ls)        // 0 for a verbatim parent
```

where `Cs = currentBufferCol + prefixWidth(currentSurface, Ls)` and
`Ls = lineOf(currentR0) + currentBufferLine` are read off the live textarea before the move. This
preserves the caret's actual **row** (not "the exited child's first line") and makes in→out→in land
where it started.

This needs the caret primitive *extended*, not reused as-is — `placeCaretAtColumn` lands only on the
**first/last** logical line today (`caretGeometry.ts:222`; hint `{ edge: 'first' | 'last'; column }`
at `PreviewRoot.tsx:302` / `PreviewContext.tsx:197`):
- Widen `CaretHint` to `{ line: number; column: number } | { edge: 'first' | 'last'; column }`.
- Generalize `placeCaretAtColumn` to accept a `line` (convert via `lineStart`-style summation — the
  `edge:'last'` branch at `caretGeometry.ts:237-244` already does this sum). The single mount-time
  consumer (`dispatchers.tsx:152-162`) needs only the widened call.

**`prefixWidth(surface, line)` is a bidirectional primitive** (added on read, subtracted on place).
Because the buffer is a *re-serialization*, not a strip (see Mechanism), an exact integer offset
exists **iff** the clean line is a suffix of the full source line (no inline normalization on that
line — true for the common case and all acceptance fixtures). Compute it in TS from data already in
hand, comparing the **full source line** to the clean-buffer line — **not** the `r0`-sliced partial
(`r0` is not at a line boundary; the fixture confirms it starts mid-indent):

```
fullSourceLine = content.slice(lineStart(L), lineStart(L+1))   // via byteLineMap
prefixWidth    = fullSourceLine.trimEnd().length − cleanLine.trimEnd().length,
                 guarded by fullSourceLine.trimEnd().endsWith(cleanLine.trimEnd())
```

When the guard fails (a normalized line), **clamp the column to the clean line length and warn** —
the warn-and-proceed posture the plan already takes for line-count mismatch; do not fake an exact
column when serialization changed the line. For a verbatim parent (nest-out, no clean buffer) the
clean line equals the source line, so `prefixWidth` falls out to 0. (Reflections #19, #20.)

(This is the **clean / sync** path. The **dirty** path still commits, relocates the node by its
commit-stable `fromStartLine`, and projects `caretBufferLine` against the relocated node — there is
no invariant source line to preserve once the source changed. Both paths end at a `destBufferLine`,
so the placement primitive is shared.)

### `leafAnchorR0`
Demoted to a pure fallback (no readable caret / programmatic), driving the retained byte-space
`childSurfaceToward`. The P3.4 "crumb-jump preserves the original leaf" test (test 6) is **updated**
to the caret model: after a crumb jump, the next nest-in follows the caret.

### Commit-if-dirty, then nest — in the shared core, covering buttons + crumbs (#6)
A nest move while the buffer is dirty must **not** discard edits (today `applyNestingRetarget`
reseeds with no commit — a data-loss footgun, worsened by caret-aware nest-in, and it bites the
◀/▶ buttons and crumb-jumps too, since their `preventDefault` keeps the textarea from blur-
committing). Land commit-if-dirty in the **shared core** so *all* entry points inherit it:
`requestNestingMove` (chord + ◀/▶) **and** `requestNestingSelect` (crumb-jump) both route through
`applyNestingRetarget`, which becomes the `move(spec, caret, box)` chokepoint:
- **Unmodified** → synchronous hop: `resolveLanding(kind:'nest')` (pure-surface, against the
  current source index), `openEditTarget` with `box:'snapshot'`, no commit, no re-render. **Nest
  sync must use the snapshot, never a live measure** — at sync time the active textarea occupies
  the subtree, so even nest-*out*'s parent is present-but-distorted (it contains the textarea).
- **Modified** → commit (`SET_AST`), close, stash an `intent:'open'` landing with the `kind:'nest'`
  (or `kind:'crumb'`) spec carrying `{fromStartLine, caretBufferLine}` (a buffer-relative line, not
  an absolute one). Post-render the destination subtree is clean (textarea gone), so the reland
  **captures a fresh snapshot of it and opens `box:'snapshot'`** — that fresh capture *is* the live
  measure of the clean subtree, so no separate raw-element measure is needed, and `box:'snapshot'`
  stays uniform for every nest open. `resolveLanding(kind:'nest')` relocates the committed node by
  `fromStartLine` (its start line is commit-stable) and projects the caret to
  `lineOf(relocatedNodeR0) + caretBufferLine`, then `childSurfaceTowardLine` / `parentSurface`.
- **Re-entrancy guard (Reflection #22):** the Modified path opens an async commit→close→reland
  window (the reland layout effect / 250 ms fallback). A second nest request arriving before the
  reland lands would hit an unmounted textarea or clobber `pendingLandingRef`. With commit-if-dirty
  this is newly reachable via rapid chording / ◀▶ clicks mid-edit, so **ignore nest requests while a
  landing is pending** (`pendingLandingRef.current != null`) — one in-flight nest at a time.
- Caveat (carried over): committing at the list level reformats/renumbers the whole list. The commit
  is an AST reconcile + `incremental_write` **re-serialization of the whole top-level list**, NOT a
  byte splice (confirmed: `apply_node_edit.rs` → `compute_reconciliation` → `incremental.rs`; the
  per-item keep/reconcile plan is computed but ignored by the incremental writer), and it can
  normalize loose↔tight spacing. Commit fires **only on a genuinely dirty buffer** — navigation
  never commits — once per *dirty* traversal step (see *Rationale* for the rejected draft-stack
  alternative). The committed node's own **start line is stable** (everything before the rewritten
  list is emitted verbatim), which is what makes the dirty path's `fromStartLine` relocation safe;
  only *later* siblings can shift lines under normalization.

---

## §3 — Mode-aware activation indicators

### Fix the stale-context hazard first (prerequisite)
`onPointerMove`/`onPointerLeave` are `useCallback(…, [])` and capture render-0's `ctx`; the provider
value is a fresh literal each render (`PreviewRoot.tsx:948-975`), so any `ctx.unlockNestingCursor`
read inside them would be frozen at mount (and the existing `ctx?.editTarget` guard at `:113` is
*already* stale — hover outlines likely persist during edits). Use the **latest-ref pattern** the
file already standardizes on (`editTargetRef`, `sourceIndexRef`, `nestedEditBuffersRef` updated in
the render body): read `ctx.editTargetRef.current` (stable ref object, live value) and add an
`unlockNestingCursorRef` for the one scalar that lacks a ref. Keep `[]` deps. *Not* `[ctx]` deps
(churns the handler against a per-render-new provider value); *not* `experimental_useEffectEvent`
(unstable API). See Reflection #11 and *Rationale → deeper context split* for the larger structural
option deferred to backlog.

### Hover / touch outline (the reported bug)
The outline highlights the deepest leaf, but locked-mode click activates the **outer block** —
the highlight promises a granularity the click doesn't deliver. Make the outline use the same mode
branch `activate()` and the click-switch already use. **Split `hoveredRef`'s overloaded role**
(it is both the dedupe key *and* the Enter/Space activation target, `:129,:237,:246`):
- `rawLeafRef` — the raw `closest('[data-block-pool-id]')`, used **only** for cheap dedupe.
- `hoveredRef` — the **resolved** element that carries the box-shadow and is the activation target.

```ts
const onPointerMove = (e) => {
  if (ctx?.editTargetRef?.current != null) return;          // latest-ref guard (see above)
  const rawLeaf = findEditTarget(e);
  if (rawLeaf === rawLeafRef.current) return;               // dedupe on raw leaf → pointermove stays cheap
  rawLeafRef.current = rawLeaf;
  const resolved = !rawLeaf ? null
    : unlockRef.current ? rawLeaf : resolveOuterBlock(rawLeaf);
  outlineElement(resolved);                                 // box-shadow on the surface that will activate
  hoveredRef.current = resolved;
};
```

`activate(hoveredRef.current)` already re-resolves mode-aware internally (`:62-64`), so Enter works
whether `hoveredRef` holds a leaf (unlock) or an outer block (locked) — idempotent. `outlineElement`
already removes the shadow from the prior `hoveredRef.current` (`:50-51`). Two call sites:
`onPointerMove` mouse hover (`:128`) and the touch branch of `onPointerDown` (`:177`); both set
`rawLeafRef` (raw) and `hoveredRef` (resolved), and the hold timer's `activate(rawLeaf)` (`:183`) is
unchanged — `activate` re-resolves the raw leaf itself, so the resolved/raw split is hover-only.

### Roving-tabindex (follow-on for consistency)
`onKeyDown` (`:227`) focuses `enumerateOuterBlocks` unconditionally, so after the hover fix mouse
and keyboard would disagree in nesting mode. Make it mode-aware:
- **Locked:** `enumerateOuterBlocks` (today).
- **Nesting:** new pure `enumerateNestingLeaves(host)` (visible `[data-block-pool-id]` with no
  pool-id descendant), so arrows focus/activate the same leaves mouse does. Set both `rawLeafRef`
  and `hoveredRef` to the focused element so a following hover doesn't redundantly re-resolve.

---

## Reflections (resolved)

1. **Snapshot must capture at all fresh-open sites, synchronously pre-swap** — there are **three**
   (`activate`, `executeLanding` open, `requestMove` sync-hop); click-switch lands *through*
   `executeLanding`, not a separate site. (§1.)
2. **`topBlockR0` derived, not stored** — simpler than store+re-anchor; block-relative keys are
   shift-invariant under a uniform insert-above. **But self-heal does NOT preserve the map** — the
   re-anchor is content-based, not a guaranteed uniform δ (Reflection #16), so the map is *cleared*
   on every self-heal fire rather than relied on across KEEP. (§1.)
3. **Caret→child in line space via a new helper** — `childSurfaceToward`'s exclusive-end byte
   containment misfires on the prefix; the byte-space helper is retained as the fallback. (§2.)
4. **Caret reading centralized in `requestNestingMove`, read pre-commit.** (§2.)
5. **Caret-driven nest-in vs. breadcrumb invariant** → caret wins; breadcrumb re-derives from the
   live cursor (already structural); P3.4 test updated. (§2.)
6. **Roving-tabindex made mode-aware** to match Feature 3. (§3.)
7. **Round-trip caret placement** is the concrete mechanism that makes in/out consistent — and it
   **extends** the caret primitive (interior line + clean-buffer column rule), it does not reuse it
   as-is. (§2.)
8. **Nest-move-while-dirty discarded edits** → commit-if-dirty in the shared core, covering chord,
   buttons, and crumb-jumps, unifying with the reland path. (§2.)
9. **Snapshot == direct-click box by construction** — de-risks Feature 1. (§1.)
10. **Snapshot ↔ nesting-surface alignment** (DOM pool-id set == source-index non-Opaque set) is an
    assumption that is *also load-bearing for the key arithmetic* — add a regression test (a nested
    list/blockquote) **and** a key-uniqueness assertion. (§1.)
11. **The open/reland paths share one shape (resolver + opener)** — Phase-0 refactor collapses the
    quadruplicated edit-target assembly and the duplicated find-by-line resolver, so nest/crumb are
    new *resolver kinds*, not new code paths. (§2.)
12. **The hover handlers read a stale `ctx`** — fix with the latest-ref pattern (idiomatic for this
    file) before adding the mode branch; this also repairs a pre-existing hover-during-edit leak.
    (§3.)
13. **Snapshot keyed by full `(r0,r1)`** — the cross-pool join key; `r0`-only is the wrong key
    regardless of collision frequency. (§1.)
14. **`resolveLanding` is a dispatcher, not a pure function** — `outerByLine` is inherently
    DOM-based (`enumerateOuterBlocks`); only the `nest`/`crumb` *surface* picks are pure (in
    `nestingNav`). It returns the box source because the box is coupled to the kind. (§2 Phase 0.)
15. **Every nest open uses `box:'snapshot'`** — sync reuses the snapshot from the original open;
    the dirty reland captures a *fresh* snapshot of the clean re-rendered subtree (which *is* the
    live measure) before opening. Nest never live-measures, because while editing the active
    textarea distorts the subtree (even nest-out's parent). The map is *replaced* per capture, not
    merged. (§1, §2.)
16. **Self-heal must CLEAR the geometry map, not preserve it across KEEP** — `findReanchorCandidate`
    is content-based (`exact ?? nearest` off the old `anchorR0` + text equality), not a uniform δ, so
    KEEP can re-anchor into a *different* subtree (deletion-above + duplicate content) and never
    re-measures heights; clear on every self-heal fire (KEEP and DROP) and fall back. Supersedes the
    earlier "kept across KEEP" claim. (§1.)
17. **`childSurfaceTowardLine` uses trimmed line-spans + a tiebreak** — raw node ranges absorb
    trailing whitespace, so sibling line-spans overlap (shown on the real fixture: caret on `nother`
    is in both the sub-sub-item list and `nother`); trim both ends (the `…trimEnd()` convention) and
    prefer start-line-==-`Ls` / narrowest / nearest. (§2.)
18. **Descent line read from the caret; a non-collapsed selection uses `selectionEnd`.** (§2.)
19. **The clean nesting buffer is a re-serialization (`write_single_block`), not a per-line strip** —
    so `prefixWidth` is an exact integer only when the clean line is a suffix of the full source
    line; recover it in TS by comparing the *full* source line (not the `r0`-sliced partial) with an
    `endsWith` guard and a clamp-and-warn fallback. Line-count correspondence is a writer property to
    verify per block type. (§2.)
20. **One caret-placement rule for nest-in and nest-out** — the source position is invariant on a
    clean move, so map `(Ls,Cs)` into dest coords (`destLine = Ls − lineOf(destR0)`,
    `destCol = Cs − prefixWidth(dest,Ls)`); `prefixWidth` is bidirectional. Deletes the nest-out
    special case. (§2.)
21. **The two find-by-line blocks are not identical** (`down: L0+draftLineCount` vs `up: L0`) — add a
    characterization test before the Phase-0 extraction so the asymmetry isn't flattened. (§2 Phase 0.)
    **Implementation note (2026-06-14, commit bfb09706):** the up/L0 asymmetry is only *observable* at
    reland when the **just-edited block is enumerable** (visible) at reland time. In the existing jsdom
    harness it is NOT: during the edit the edited block's `<p data-block-pool-id>` is swapped for the
    textarea wrapper, so post-commit it is a *fresh* node, and the reland `useLayoutEffect` fires DURING
    the re-render — *before* a per-element `mockTileRects` re-mock can run — leaving the fresh node with a
    zero rect → `isVisibleBlock` false → excluded. With it excluded, `destLine=L0` and
    `destLine=L0+draftLineCount` pick the *same* block (the asymmetry is masked). The characterization
    test therefore installs a **prototype-level** `getBoundingClientRect` spy so the freshly-rerendered
    edited block is visible at reland — faithfully matching the **real browser**, where the edited block
    IS laid out. Only then does flattening `up` re-enter the just-edited block (RED). The Phase-0
    extraction must preserve this: `resolveLanding(kind:'nest'|'outerByLine')` reads the live DOM, and the
    edited block's reland visibility is real-browser behavior the jsdom test reproduces via the rect spy.
22. **Nest requests are queued while a landing is pending** — commit-if-dirty makes the async
    commit→reland window newly reachable; allow one in-flight nest at a time. (§2.)

## Rationale / alternatives considered

**Alternative considered — in-memory draft stack (rejected; recorded because it is tempting).**
Instead of committing on a dirty nest move, the editor could keep an *uncommitted* stack of drafts
and never round-trip through `SET_AST` until an explicit close. Nest-in would slice the child's
region out of the *dirty parent draft* by relative offset, push `{parentDraft, childOffset}`, and
seed the child editor with that slice; nest-out would pop and splice the (possibly edited) child
text back into the parent draft. Traversal would be instant — zero wholesale-list rewrites, no
async reland, no re-render between levels.

It is rejected because the invariant it rests on — "the child draft is a literal substring of the
parent draft" — is **false** the moment a clean buffer is involved: `nestedEditBuffers` strips the
`> `/indent prefix per line, so the child editor shows a *stripped* view, and reconciling a stripped
child edit back into a prefixed parent means re-applying the prefix per line and re-deriving splice
offsets that shift on every keystroke. It would fork the persistence model away from the single
commit→reland path that arrow-move, click-switch, and self-heal already share and test, in exchange
for an optimization (avoiding repeated list reformats during traversal) whose cost is only cosmetic
noise in the document history. Commit-if-dirty reuses one battle-tested path; the draft stack is an
elegant parallel universe of state we would have to verify from scratch.

**Deferred (backlog) — split `PreviewContext` into stable + volatile.** The deeper fix for the
stale-`ctx` hazard (§3) is to `useMemo` a stable context of refs+callbacks and put the volatile
`editTarget` in a separate provider; consumers reading only callbacks would then stop re-rendering
on every edit-state change, and `[ctx]` deps would be cheap. Out of scope here — the latest-ref
pattern is the proportionate fix that matches the surrounding code. File as a follow-up if the
context churn ever shows up in a profile.

## Risks / watch-items
- **Snapshot capture cost** — `getComputedStyle` + `getBoundingClientRect` per surface, once per
  activation, gated to unlock+multilevel. Fine now; watch on pathologically large blocks.
- **Hover re-resolution cost** — `resolveOuterBlock` per `pointermove` (coincidence rect-climb);
  deduped by raw-leaf change.
- **Landing-core refactor regressions** — Phase 0 touches fail-on-revert-proven move/click-switch
  code; mitigated by the behavior-preserving 3-step rollout (each step green before the next).
- **Wholesale container rewrite per dirty traversal step** — commit-if-dirty fires a list-level
  reformat on every dirty nest move; accepted (see *Rationale*), snapshot never byte-asserts siblings.
- **Concurrent (self-heal) edit** — any external re-render **clears** the geometry map (Reflection
  #16); the next nest move falls back until a fresh open re-captures. Block-relative keys are not
  trusted across a self-heal (the re-anchor is content-based, not a uniform δ), so there is no
  stale-key / wrong-block hazard — only a transient fallback. Bounded; documented.
- **Line-count mismatch (reflow)** — warn-and-proceed; the warning surfaces a real regression.
- **Clean buffer is re-serialized, not stripped** — `regenerate_nested_buffers` runs
  `write_single_block`, so inline normalization can break the source-line↔buffer-line suffix
  relation `prefixWidth` relies on; guarded by the `endsWith` check + clamp-and-warn (Reflection
  #19). Verify line-index correspondence per block type (paragraphs with soft breaks).
- **Hover dedupe keyed on raw leaf** — toggling `unlockNestingCursor` mid-hover won't refresh the
  outline granularity until the next pointer move; acceptable (optionally include the mode in the
  dedupe key). (§3.)
- **Dirty crumb-jump relocation ambiguity** — post-commit, a crumb is relocated by start line; if
  an ancestor and its first child share a start line, start line alone is ambiguous, so the spec
  also carries a commit-stable `targetDepth` (containment count). Bounded; a jsdom test should cover
  the shared-start-line case.
- **New context surfaces (two, not one)** — the landing core adds a `captureGeometry(openedEl)`
  **method** (for the `activate()` site to write the snapshot map; opener + resolver stay
  PreviewRoot-internal), **and** §3 adds an `unlockNestingCursorRef` **ref** — created/updated in
  `PreviewRoot`'s render body and exposed via context, since `useBlockEditHover`'s empty-deps
  handlers can't see the render scalar. Keep the context's stable/volatile split in mind (see
  *Rationale → deeper context split*).

## Verification

### Acceptance criteria — named Playwright specs (the real bar)

The headline correctness is **pixel geometry**, which jsdom cannot measure — so the bug-fix
acceptance lives entirely in real-browser specs. These four specs **are** the definition of done;
each derives from a concrete reported case, asserts a real `boundingBox()`, and ships with an
explicit **fail-on-revert lever** (verified cold: revert the named production line → spec RED →
restore → GREEN). Tier: `hub-client/e2e` (vite-built SPA + real WASM via `window.__quartoTest`),
per the Phase-3.5 precedent (`hub-client/e2e/q2-preview-breadcrumb-geometry.spec.ts`). Tolerance
≤ 2px (the existing geometry-spec convention). All four use a fixture with a **genuinely 3-level**
nested bullet list (so the snapshot's multilevel path is exercised, not a 2-level fixture
mislabeled "sub-sub"):

```
* another
* hello
    * sub-item
        * sub-sub-item
    * nother
```

> **Fixture verified (2026-06-14).** Parsed with the real binary
> (`cargo run --bin pampa -- -t json`): it is **three genuine levels** — BulletLists at byte ranges
> `[0,69]`, `[20,69]`, `[39,60]`; line breaks at `[9,17,32,55,68]`; `nother` is a depth-1 sibling of
> `sub-item`, **not** top-level. siKeys: `0:0-69:0`, `0:20-69:0`, `0:39-60:0`. **Critical offset
> caveats** (load-bearing for the §2 line/column math): `(r0,r1)` are **byte** offsets; a surface's
> `r0` is **not** at the list marker or at start-of-visible-text — it absorbs the prior line's
> trailing whitespace (the level-1 list starts at byte 20, two bytes before its `* ` marker at 22),
> and a node range absorbs the **next** line's leading indent at `r1` (exactly the overlap that
> forces the trimmed line-span in §2 / Reflection #17). Derive marker columns from the CST or a line
> rescan, never from `r0`.

1. **`q2-preview-nesting-size-out.spec.ts`** (Feature 1, the reported nest-out bug). Click into the
   **inner** list → nest-out to the parent list → assert the active edit region's
   `boundingBox().height` ≈ the parent list's **original rendered height** (covers parent + child),
   **not** the child-only height. *Fail-on-revert:* restore the live-DOM measure in the landing
   core's box resolution → the box collapses to ~child height → RED.
   **── EXECUTION FINDING (2026-06-14): NOT REPRODUCIBLE as a fail-on-revert lever; spec dropped. ──**
   A clean nest-**out** destination (the parent container) is **always rendered** — it wraps the
   child's textarea, and the textarea is fixed-height (== the child's captured box), so the parent's
   *live* height ≈ its *original* height. Empirically (chromium, 3-level list): whole-list H0=127.5;
   nest-out edit height = 127.5 **both with and without** `box:'snapshot'` (reverting to
   measure-or-keep still measures the rendered parent at ~127.5). So `box:'snapshot'` and the live
   measure are indistinguishable for nest-out → no fail-on-revert assertion exists at this tier; a
   green spec would be **test theater**. The "collapses to child height" premise only holds when the
   parent is NOT found by `outerBlockForAnchorR0` (forcing the `keep` branch), which does not occur
   for a found, rendered parent. The snapshot's real value is the **absent-destination** case
   (nest-**in** / descend), which IS discriminating and IS covered below. nest-out clean is correct
   **by construction** (the original P3.5 gap was explicitly *'in'/jumps*, not *out*). Verified, not faked.
2. **`q2-preview-nesting-size-in.spec.ts`** (Feature 1, the reported nest-in bug). Click the parent
   list directly (full-height editor) → nest-in → assert the first item's editor height ≈ **one
   item's** rendered height (the child's snapshot box), **not** the parent's full height.
   *Fail-on-revert:* restore the stale-box fallback → the box stays at parent height → RED.
   **── DONE & VERIFIED (2026-06-14). ──** Implemented with a robust interaction: activate the
   **sub-list** (`nother`) → nest-**out** to the whole list (≈ H0=127.5) → nest-**in** back to the
   sub-list (`leafAnchorR0` preserved → descends to a *pool-id* surface that has captured geometry,
   not a tight-list item which has no pool-id element to measure — caret-aware descent into items is
   §2). Asserts the nest-in editor ≈ H1=76.5 (the sub-list's captured box), not H0=127.5.
   **Fail-on-revert verified COLD:** revert `applyNestingRetarget` to measure-or-keep, rebuild,
   re-run → nest-in editor = 127.5 (stale parent box) → RED; restore → 76.5 → GREEN. Tolerance ≤2px.
3. **`q2-preview-nesting-caret-in.spec.ts`** (Feature 2). Edit the whole list, move the caret onto
   the **third** item's line, nest-in → assert the opened editor targets the **third** item (its
   `anchorR0` / buffer), not the first. *Fail-on-revert:* disable the centralized caret read (fall
   back to `leafAnchorR0`) → opens the first item → RED.
4. **`q2-preview-locked-hover.spec.ts`** (Feature 3). Nesting **off**; hover deep inside the list →
   assert the outline (box-shadow) lands on the whole `<ul>` (outer block), matching what a click
   would activate, **not** the leaf `<li>`/`<p>`. *Fail-on-revert:* restore the leaf-only outline →
   highlight lands on the leaf → RED.

> **No test theater (this codebase has a documented history of it — the self-heal `SelfHealHarness`
> that reimplemented the effect, parent plan §Self-heal design bug).** Every spec above drives the
> **real** render path; none reimplements geometry or stubs the box. A spec that can't be made
> fail-on-revert at this tier (e.g. the deferred collapsed-region drop, blocked on `bd-k1evg0g1`) is
> marked deferred, not faked green.

### Supporting tiers
- **jsdom/RTL (wiring, not pixels):** proves *which surface* is selected, not its size —
  `openEditTarget` + `resolveLanding` (all three `kind`s, incl. the nest line-resolution and the
  commit-if-dirty reland); snapshot capture + consume with the `(r0,r1)` key (mock
  `getBoundingClientRect` per surface, the existing `mockBlockRects` pattern); **key-uniqueness
  assertion** over the fixture; `childSurfaceTowardLine`; the caret-primitive extension
  (interior-line + clean-buffer column); mode-aware outline + roving with the latest-ref guard;
  surface/pool alignment (Reflection #10). Fail-on-revert on each behavior lever.
- **Rust/WASM:** none — all features are iframe-side TS. (The regenerated-buffer line-count
  property relies on the existing per-line `> `/indent strip; no writer change.)

## TDD checklist (execute one item at a time)

### §0 Rename (first commit)
- [x] `outerBlocks.ts` + symbol/test-var sweep; green build + suites; no behavior delta. (commit 2d26caaf; tsc clean, 366+376 green)

### §2 Phase 0 — landing core (behavior-preserving, before any feature)
- [x] **Characterization test first:** pin the up/down find-by-line asymmetry (`down: L0+draftLineCount`
  vs `up: L0`) so the extraction can't flatten it (Reflection #21). (commit bfb09706; real
  PreviewRoot mount, fail-on-revert verified cold: flatten up→RED, restore→GREEN; integ 377)
- [ ] Extract `seedForRange` + `openEditTarget`; retarget the three internal sites (`executeLanding`,
  `requestMove` sync-hop, `applyNestingRetarget`) to `openEditTarget`, and point `activate` at
  `seedForRange`. Existing specs green (no-op extraction = the fail-on-revert lever).
- [x] Extract `seedForRange` + `openEditTarget`; retarget the three internal sites + point `activate`
  at `seedForRange`. (commit 9e8fe88d; tsc clean, 366+377 green — no-op verified)
- [ ] Extract `resolveLanding` (`kind:'outerByLine'`, DOM-based dispatcher returning `{range, caret?,
  box}`); point `executeLanding` + `requestMove` sync-hop **and `handleClickSwitchBlur`** at the new
  `{intent:'open', spec}` landing shape. Remove the copy-pasted find-by-line. Existing specs green.
  **RE-SEQUENCED (2026-06-14):** deferred to the start of §2 — §1's snapshot capture/consume depends
  only on `openEditTarget` (step 1, done), not on `resolveLanding`, which is §2 (nest/crumb) infra.
  §1 is independent and is being done first (it is the user's review milestone).

### §1 Geometry snapshot
- [x] `topBlockR0` pure helper (commit fd990709; 5 unit tests, nestingNav 62 green).
- [x] `snapshotOuterBlockGeometry` capture helper (commit f98ec2eb; 5 jsdom tests; gating, block-relative
  full `(r0,r1)` key, duplicate-key rule, key-uniqueness fail-on-revert verified cold). 371 unit + 382 integ.
- [x] **DONE (commit dbb51bab + e2e spec):** `editGeometryRef`
  lifecycle (three close paths **+ clear on every self-heal fire**, Reflection #16) + `captureGeometry`
  exposed via `PreviewContext`; capture at all three open sites; consume via the landing core's
  `box:'snapshot'`; **self-heal clears the map** (test: KEEP after an insert-above falls back, not a
  stale / wrong-block box); alignment + **key-uniqueness** tests. **Acceptance:**
  `q2-preview-nesting-size-out.spec.ts` + `q2-preview-nesting-size-in.spec.ts` (both fail-on-revert).

  **── EXECUTION HANDOFF (2026-06-14, paused at ~40% context per user's 50% gauge) ──**
  Pre-validated test seams for the next session (decided in the parent context, not to be re-derived):
  - **`snapshotOuterBlockGeometry(openedEl, pool, topBlockR0Num)` → `Map<string,{contentHeight,boxStyle}>`**
    lives in `outerBlocks.ts`. Climb to outermost `[data-block-pool-id]` (reuse `resolveOuterBlock`'s
    chain-walk); if ≤1 visible pool-id descendant → return empty `Map`; else `querySelectorAll`, filter
    `isVisibleBlock`, `measureBlockBox` each; key = `` `${pool[pid].r[0]-topBlockR0Num}:${pool[pid].r[1]-topBlockR0Num}` ``,
    DOM-pre-order-first on duplicate key (matches `enumerateOuterBlocks` dedupe). **Tier: jsdom unit**
    (in `outerBlocks.integration.test.ts` or a new `*.test.ts`) — mount a real nested-list DOM subtree,
    mock per-element `getBoundingClientRect` (the existing `mockBlockRects` pattern but **prototype-level**
    if any consume test relands — see Reflection #21 note above for why fresh nodes need the prototype spy),
    assert the returned Map's keys (block-relative) and that each value's `contentHeight` matches the mocked
    rect. **Key-uniqueness assertion:** over the 3-level fixture, assert no two visible pool-id elements
    produce the same block-relative key (Reflection #10/#13). Fail-on-revert: switch the key to `r0`-only →
    a collision appears on the nested-list fixture.
  - **`editGeometryRef: Map<string,{contentHeight,boxStyle}>` on PreviewRoot.** Capture REPLACES (assigns a
    fresh Map). Clear paths: the `setEditTarget(null)` **wrapper** (PreviewRoot ~:191, plain close) clears;
    self-heal (both KEEP and DROP, ~:244-285) clears; commit-and-reland's direct `setEditTargetRaw(null)`
    does NOT clear (reland re-captures). **jsdom test for self-heal-clear:** open editor on a multilevel
    block (capture populates the map) → trigger an external re-render that self-heals KEEP → nest-move →
    assert the consume FELL BACK (used live/keep box), not a stale snapshot box. Fail-on-revert: remove the
    self-heal clear → the move reads a stale/wrong-block box.
  - **`captureGeometry(openedEl)` context method** on `PreviewContext` (mirrors how `requestMove`/
    `commitNestingEdit` are threaded) so `activate()` (useBlockEditHover) can write `editGeometryRef`
    synchronously **before** `setEditTargetRaw` (the children are swapped to a textarea after). Capture at
    the 3 open sites: `activate` (via context), `executeLanding` open, `requestMove` sync-hop — all gated on
    `unlockNestingCursor && multilevel`.
  - **Consume:** add `'snapshot'` to `openEditTarget`'s `box` union; in `applyNestingRetarget` pass
    `box:'snapshot'` and resolve in `openEditTarget` by `editGeometryRef.get(``${next.r0-topBlockR0}:${next.r1-topBlockR0}``)`,
    falling back to today's measure-or-keep when the key is missing (jsdom/no-layout or unrendered surface).
    `topBlockR0` for the lookup = `topBlockR0(buildNestingSurfaces(sourceIndexRef.current), et.anchorR0, et.anchorR1)`.
  - **Acceptance Playwright specs** (`hub-client/e2e`, real WASM via `window.__quartoTest`, ≤2px tolerance,
    precedent `q2-preview-breadcrumb-geometry.spec.ts`): the 3-level bullet fixture in the §Verification
    block. **size-out:** click inner list → nest-out → assert active-edit-region height ≈ parent list's
    ORIGINAL rendered height (fail-on-revert: restore live-DOM measure → collapses to child height).
    **size-in:** click parent list → nest-in → assert first item editor height ≈ one item's rendered height
    (fail-on-revert: restore stale-box fallback → stays at parent height). **Open question for next
    session:** confirm `hub-client/e2e` runs headless in this environment; if not, write the specs and mark
    "authored, not executed in-session" (do NOT fake green) — per the plan's no-test-theater rule.
  - **byte-vs-UTF16 watch (carry into §2):** the §2 `prefixWidth`/line math must use byte-correct slicing
    (`utils/utf8Slice.ts` / `sliceBytes`), NOT `String.prototype.slice`, on `byteLineMap` byte offsets.
    Harmless for the all-ASCII acceptance fixtures; wrong for non-ASCII source lines.

### §2 Caret-aware nest-in
- [ ] `childSurfaceTowardLine` with **trimmed `surfaceLineSpan` + tiebreak** (Reflection #17; test on
  the verified fixture's `nother` / sub-sub-item overlap) (+ retain `childSurfaceToward` as fallback);
  caret→source-line read **from the caret / `selectionEnd` on a selection**, pre-commit, centralized
  in `requestNestingMove`; widen `CaretHint` + generalize `placeCaretAtColumn` to interior line;
  **bidirectional `prefixWidth` via full-source-line `endsWith` guard + clamp-and-warn** (Reflection
  #19); **unified in/out caret-placement rule** (Reflection #20); warn-and-proceed; `leafAnchorR0`
  demotion + P3.4 test update; `resolveLanding` `kind:'nest'` + `kind:'crumb'`; commit-if-dirty in
  the shared core covering chord, ◀/▶ buttons, and crumb-jumps, **with the pending-landing
  re-entrancy guard** (Reflection #22). **Acceptance:** `q2-preview-nesting-caret-in.spec.ts`
  (fail-on-revert).

### §3 Mode-aware indicators
- [ ] Latest-ref fix for `onPointerMove`/`onPointerLeave` (+ `unlockNestingCursorRef`); split
  `hoveredRef`/`rawLeafRef`; mode-aware outline (mouse + touch); `enumerateNestingLeaves` +
  mode-aware roving; dedupe. **Acceptance:** `q2-preview-locked-hover.spec.ts` (fail-on-revert).
