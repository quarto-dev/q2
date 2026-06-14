# Block editing in q2-preview — design / master spec

**Date:** 2026-06-06 (interaction model 2026-06-07; dual-node substrate 2026-06-08;
render-component API 2026-06-09). **Reworked 2026-06-13** to describe the system *as built
through Phase 2* — the resolution model, the byte-offset identity + self-heal concurrency model,
and the cross-surface cursor — which the earlier draft predated.
**Branch:** feature/block-editing-improvements (worktree `.worktrees/block-editing`).
**Status:** **Phases 1–2 implemented** (locked depth-aware editing + cross-surface cursor +
concurrency-safe identity). **Phase 3 (the nesting-cursor unlock) is designed but not yet built**, and
is owned by a separate agent. Implementation plan + execution log:
`claude-notes/plans/2026-06-11-block-editing-improvements.md`.
**Builds on:** `claude-notes/plans/2026-06-04-target-incremental-writes.md` (the `apply_node_edit`
source-slice round-trip this feature extends).

---

## Overview

A user edits a Quarto document's **markdown source, block by block, inside the live preview**. The
editor shows the *markdown source* of a block — sliced from the document `content` by the block's
byte range — not its rendered text. Activating any source-backed block turns it into a same-sized
markdown textarea; committing writes the edit back through the existing core
(`parse_qmd_content → splice into the untransformed AST → reconcile → incremental_write`). That
write-back core is unchanged; this feature is the front-end interaction and identity layer above it,
plus (in Phase 3) one new Rust serialization entry point.

Two ideas organize the whole design:

1. **The untransformed AST is a first-class dispatch input.** Every rendered node carries both its
   transformed (display) form and its untransformed `sourceNode` counterpart. Editing operates on the
   `sourceNode`, so shortcodes / cross-refs / includes inside the edited region survive as their
   *source*, not as their expansions.
2. **Editable surfaces nest, so a click or an arrow selects a root-to-leaf *path*, not a single
   surface.** Collapsing that path to one editable surface is the *resolution model* below — the
   conceptual core of Phases 1–3.

---

## The resolution model (the conceptual core)

Because editable surfaces nest in the DOM exactly as they nest in the AST, a pointer click (or an
arrow key) falls inside a whole chain of surfaces. A policy must collapse that chain to one. The
policy is **deepest-wins hit-testing with a principled tiebreak**: assign each pixel to the *deepest*
surface whose box contains it. A child's interior resolves to the child; a container's *exclusive*
pixels — its chrome (a callout title bar, a blockquote rule, a div's padding, the gaps between
siblings) — resolve to the container. Mechanically this is `closest('[data-block-pool-id]')`, and it
partitions the visible document uniquely, provided top-level blocks are always candidates.

Deepest-wins has exactly one ambiguity: **coincident extents** — a single child wrapped by a
chrome-less container whose box fits it exactly (a bare `<div>` around one paragraph, measured at 0px
on all four edges). There the child's box and the container's box coincide, and "deepest" is
undefined. The **mode** is precisely how that tie is broken:

- **Locked (the default, Phase 2): take the topmost of the coincident stack.** A text click on a
  paragraph inside a chrome-less div opens the *div*. Everything non-coincident (multi-child
  containers, anything with visible chrome) still resolves by plain deepest-wins.
- **Unlocked (Phase 3, default-off): take the leaf.** The click descends fully.

Locked mode adds one rule that **dominates** the coincidence tiebreak: **prefixing containers are
atomic.** `BlockQuote`, `BulletList`, `OrderedList`, and `DefinitionList` (`<blockquote>`/`<ul>`/`<ol>`/`<dl>`)
are never descended into in locked mode — a click anywhere inside selects the *whole outermost* such
container, edited as one buffer (`> a` / `> b`, markers and all). The locked target is therefore a
single precedence: **the outermost prefixing container if the path crosses one; otherwise the topmost
coincident ancestor of the leaf.**

That precedence is not optional, and the reason is the edit buffer. A locked buffer is a **byte-slice**
of the target's source range — cheap, synchronous, no WASM. Only the *outermost* prefixing container
has a clean slice: an inner target's slice carries the outer container's `> `/indent on every
continuation line. So locked mode, by always stopping at the outermost prefixing container, guarantees
every buffer is a clean slice and **never needs regeneration**. That guarantee is what lets the
expensive AST-regeneration path stay entirely behind the Phase-3 flag (below).

**Coincidence is a screen-extent property, not an AST child count.** It compares bounding rects on
**all four edges** within a tight epsilon. (Measured against real Bootstrap + theme CSS: a chrome-less
single-child div coincides with its child at *exactly* 0px, while the nearest deciding edge of any
chrome-bearing container is ≥~12px. A 1px border counts as chrome and resolves to the leaf.) The
comparison and the tile enumeration both **skip non-laid-out surfaces** (`offsetParent === null` /
zero rect — e.g. a collapsed callout body), so a zero-rect never corrupts a comparison and an editor
never opens on a block the user cannot see.

The partition this yields — the ordered, deduped, visible **locked tiles** — is a shared primitive.
Activation, cross-surface navigation, and the keyboard roving-tabindex all consume the same tile
helper, so the focus ring, the arrow destinations, and the click target can never disagree.

---

## Identity and concurrency (the data-integrity core)

The preview is a collaborative surface: a second user (or any pipeline re-render) can rewrite the
document while you have an editor open. The identity of "the block I am editing" must survive that.

**The pool index is not an identity.** Every source-tracked node carries a pool index `s`
(`data-block-pool-id`), but `s` is a *positional ordinal reassigned on every render*. A block's
**start byte offset `r[0]` is stable** across the block's own edit, and distinct per block. So the
editor keys identity off the byte offset, never the ordinal:

```
editTarget = { anchorR0, anchorR1, anchorSlice, contentHeight, boxStyle }
```

`anchorR0`/`anchorR1` are the edited block's source byte range; `anchorSlice` is its
normalized, trimmed source text, frozen when the editor opens. The dispatcher renders the textarea
for the block whose `resolved.sourceEntry.r[0]` equals `editTarget.anchorR0` — a rect-free,
render-safe integer compare in the byte-offset coordinate system the commit *also* targets by.

**The bug this fixes is real and was live on `main`.** When matching keyed off the pool ordinal, a
collaborator inserting or removing a block *above* your open editor shifted every ordinal, so your
stale ordinal silently selected a *different* block — and your next commit wrote your text into that
block's range. Byte-offset identity moves matching into the commit's own coordinate system, and a
content guard (below) closes the rest of the window.

**Self-heal carries the open editor across an external re-render.** When new content arrives while an
editor is open, a layout effect re-finds the block by content: it locates the candidate at-or-after
`anchorR0` and verifies its slice still equals `anchorSlice`. On a match it **re-anchors** (updates
`anchorR0`/`anchorR1`, preserving the draft) and the editor stays open on the same block; on a
mismatch (your block was edited under you) it **drops** the editor, discards the draft, and moves
focus to the nearest visible tile. Dropping is the safe outcome at this granularity, where a merge is
not attainable.

**The draft is controlled, and lives at the iframe root.** Block child lists are index-keyed, so a
collaborator edit above you remounts the textarea — which would discard an uncontrolled
`defaultValue`. The draft therefore lives in a root ref (`editDraftRef`), seeded once at open and
mirrored into the textarea's local state for the controlled value; a remount re-seeds from the
surviving ref, so typed text survives. The per-keystroke `setState` stays local to the textarea, so
typing never re-renders the document. IME composition is handled so a mid-composition update cannot
prematurely commit.

Three invariants fell out of building (and breaking, and fixing) this layer; future work must respect
them:

- **An actively-edited block is a textarea *wrapper* with no `data-block-pool-id`.** The dispatcher
  replaces the block element with a measure-and-set wrapper, and that wrapper deliberately omits the
  pool-id (so a click inside the editor cannot "climb" to a parent — Phase 1). Consequently the
  active editor is **not a tile**: any tile-set query (`tileForAnchorR0`, `enumerateLockedTiles`) will
  never find it. The wrapper is reachable only through a ref (`activeEditRegionRef`).
- **The active editor's visibility must be judged from its wrapper, never the tile set.** "Did my
  block survive the edit?" is a *logic* question (pool + content); "is my block still visible?" is a
  *DOM* question answered by `activeEditRegionRef`'s box, after the re-anchor remount. Conflating the
  two — asking the tile set whether the active editor is visible — makes the answer permanently
  "hidden" (it is never a tile) and silently drops every keep.
- **A textarea must not commit unless it is still the active target.** Because `onBlur` fires during
  unmount, a dropped or re-anchored-away editor would otherwise commit its stale draft onto whatever
  block now occupies that range. The commit checks the *current* `editTarget` (via a ref) and no-ops
  when it no longer matches — so a drop discards, as intended, rather than corrupts.

---

## Editability gate — the structural (dual-node) model

A node is editable iff the commit path can reach its untransformed counterpart. The iframe receives
the untransformed AST and builds a SourceInfo-**value** index (`sourceIndex`) in `PreviewContext`; the
two trees carry separate pools but shared values, so correspondence is by value and survives
transforms that *move* blocks (appendix structure, footnotes) or *insert* structure (sectionize → a
`Generated` Div with no source counterpart).

```
editable(node) = ctx.resolveSource(node)?.reachabilityClass ∈ allowed(mode) && editableType(node)
```

- Presence in `sourceIndex` subsumes the old `t==0` (Original) **and** `d==0` (active-file)
  conjuncts: a Generated / Substring / included-file node's `s` matches nothing in the active-file
  index.
- `reachabilityClass` is assigned once at index-build from the untransformed ancestry: **`TopLevel`**
  (top-level block); **`Descendable`** (nested only in `Div`(non-section) / `BlockQuote` / lists /
  `DefinitionList` / `Figure`-body); **`Opaque`** (reached by crossing a `Table` cell/caption or
  `Figure` caption — absorbing, never editable).
- `allowed`: locked mode admits `{TopLevel, Descendable}` through the gate, then the *resolution
  model* collapses to the locked tile; the unlocked (Phase 3) mode descends to the leaf. `Opaque` is
  never editable in this series.
- Key format: `serializeSourceEntry(entry)` = `"${t}:${r[0]}-${r[1]}:${d}"`.
- `NodeArgs` (`framework/types.ts`) is unchanged; the index lives entirely in `PreviewContext`, so
  q2-debug and q2-slides are unaffected.

This **replaces** the earlier opt-out `InsideContainerContext` design entirely: no context push, no
default, no closed-container audit, no drift hazard.

---

## Interaction model

The block *is* its own affordance — no pencil, no buttons. Hovering (mouse), pressing (touch), or
focusing (keyboard) a surface outlines it; activating turns it into the markdown editor. The outline
is a `box-shadow` on the element itself, so there is no overlay to position and no scroll to track.

- **Mouse:** hover outlines the resolved locked tile; click activates it. A click *inside* an open
  editor is a caret move, not a re-resolution — the active-region ref suppresses the climb that would
  otherwise carry the click up to a parent tile (the Phase-1 fix).
- **Touch (Pointer Events):** one progressive press — `pointerdown` outlines (reveal); holding past
  `HOLD_MS` activates; early release or move-beyond-threshold cancels. OS gestures are suppressed
  (`touch-action: pan-y`, `contextmenu` preventDefault for touch, `-webkit-touch-callout: none`).
- **Keyboard (roving tabindex):** the edit layer is a single Tab stop; arrows move a programmatic
  focus across the **locked tiles** (so the focus ring lands exactly where Enter will open an editor,
  and hidden tiles are skipped); Enter/Space activates; Esc exits; `:focus-visible` reuses the
  outline; ARIA role + name per region.

**Cross-surface cursor.** While editing, the caret can leave the surface: **ArrowDown on the last
visual line** opens the next tile, **ArrowUp on the first visual line** opens the previous, and the
ends wrap. "Last/first *visual* line" is a geometry question (soft-wrapped rows, not `\n`-delimited
lines), answered by a hidden mirror-div that mirrors the textarea's wrapping; the *destination* is a
*logical* source-line projection (down → the first tile at/after `L0 + n`; up → the tile before `L0`),
resolved against the post-commit document. Only bare arrows move; modifier+arrow keeps native textarea
behavior.

Three post-interaction outcomes route through one landing mechanism and must not be conflated:

- **A move** (arrow, or a click onto a *different* tile) commits the current edit and opens the
  destination. A modified move commits, closes, and *relands* after the commit's re-render (a render
  arrives via the one-way postMessage channel; a short timeout is the fallback for a byte-identical
  commit that changes nothing to key off). An unmodified move hops synchronously, with no commit and
  no editability gap. The caret lands on the destination's first (↓) or last (↑) line at the captured
  column, clamped.
- **A plain commit** (Esc / ⌘-Enter / blur with no move) closes the editor and returns focus to the
  edited tile **by `anchorR0`** — so roving-tabindex resumes there, even though the pool ordinal has
  changed.
- **An external re-render** (collaborator) while editing triggers **self-heal**, not a commit (above).

---

## Edit buffer and commit

The locked buffer is a byte-slice of the target's source range, line-ending-normalized and trimmed.
Two normalizations are load-bearing because the comparisons depend on them:

- **CRLF.** pampa parses CRLF natively (byte offsets include `\r`), but a textarea LF-normalizes its
  value. So a sliced CRLF block reads `\r\n` while the draft reads `\n`. The fix normalizes every
  *sliced string* (`anchorSlice`, the draft seed, any self-heal candidate slice), **never** the
  `content` buffer itself — whose offsets live in the CRLF domain.
- **Dirty guard.** A commit fires only when `normalize(draft).trimEnd() !== anchorSlice`. An untouched
  blur is a no-op (so a list no longer renumbers on an empty blur, and a silently re-targeted editor
  no longer rewrites a wrong block); an emptied buffer cancels rather than deleting.

Commit then runs the existing write-back core unchanged: `commitTextEdit(JSON.stringify(sourceEntry),
text)` → parent `parse_qmd_content` → `apply_node_edit` (locates the node by SourceInfo, splices,
re-serializes the enclosing container wholesale) → `onContentRewrite` → re-render → fresh
`UPDATE_AST`. The kept guarantee is that blocks *outside* the edit stay byte-verbatim; the edited
container is reformatted (Tier-2), so commits on lists/quotes are snapshot-tested, never byte-asserted.

---

## Two edit modalities, two channels

- **Built-in editing → text channel.** The textarea shows source markdown; commit sends `newText`;
  the parent parses and splices. (The iframe cannot run the writer, so text is the right
  representation for a textarea.)
- **Render-component editing → subtree channel.** A component calls
  `commitSubtreeEdit(destinationSourceInfoJson, modifiedBlock)` with a clone of `resolved.sourceNode`;
  `PreviewNodeEditPayload` is a `channel`-discriminated union (`'text'` | `'subtree'`); the parent
  passes the subtree variant straight to `apply_node_edit` (no parse). Editing the `sourceNode`
  (not the transformed node) keeps shortcodes/refs/includes inside the region from being baked in.

Render-component authors reach `resolveSource` + `commitSubtreeEdit` via **`usePreviewEdit()`** (from
`window.__Q2_PREVIEW_RENDERER__`), which wraps `useContext(PreviewContext)` and returns nullish
functions when the context is absent (q2-debug, q2-slides). `NodeArgs`/`PreviewContext` are not
exposed for this purpose; `usePreviewEdit` is the public surface.

---

## Format routing (by design)

**q2-preview** ships `untransformedAstJson`, builds `sourceIndex`, uses the structural gate, and
commits targeted subtrees (SourceInfo → `apply_node_edit`) — no copy-on-write bubble, since the Rust
reconcile rebuilds to root.

**q2-debug / q2-slides** drive the display directly from the AST they receive, with no pipeline,
transforms, `untransformedAstJson`, or `sourceIndex`. The shared framework (`Node`, `renderChildren`,
`NodeArgs`) is unchanged. The two routings differ because only q2-preview has a transform gap between
the displayed AST and the source AST.

---

## Plan 5 dissolved (CustomNode writing/editability)

A callout is a plain `Div.callout-note` in the *untransformed* AST; the
Callout/Theorem/Proof/FloatRefTarget CustomNodes are transform products that do not exist
pre-pipeline. Since edits operate on the `sourceNode`, the writer is never asked to serialize a
CustomNode on the edit path, so the old "empty `Block::Custom` writer arm" worry is moot. Callout /
theorem **editability** is just whole-`Div` editing once the `custom/` components carry the affordance;
their **bodies** ride Phase 3 (descent into the untransformed `Div`).

---

## Component / module map

The edit-state machine lives in **`PreviewRoot.tsx`** (extracted from `entry.tsx` so it is mountable
in tests): it owns `editTarget` state, `editDraftRef`, `editTargetRef`, `activeEditRegionRef`, the
**self-heal** effect, the **landing** effect (`pendingLandingRef`/`pendingCaretRef` + a render-keyed
layout effect with a timeout fallback) serving `intent:'activate'` (move/click-switch) and
`intent:'focus'` (plain commit), the `requestMove`/`requestFocusRestore`/`requestClickSwitch`/
`cancelPendingLand` callbacks, and the `PreviewContext` provider. (`entry.tsx` is now thin: module-top
side effects + the `setAst` wiring.)

Under `ts-packages/preview-renderer/src/q2-preview/`:

- `lockedTiles.ts` — the resolution primitives: `resolveLockedTile`, `enumerateLockedTiles`,
  `isVisibleTile`, `rectsCoincide`, `tileForAnchorR0`, `findReanchorCandidate`, `captureEditTarget`,
  `measureTileBox`.
- `byteLineMap.ts` — UTF-8 byte ↔ 0-based line (`lineOf`/`lineStart`/`lineCount`).
- `caretGeometry.ts` — `isOnFirst/LastVisualLine` (mirror-div geometry), `getLogicalColumn`,
  `placeCaretAtColumn`.
- `useBlockEditHover.tsx` — the delegated host handler (`activate` resolves the locked tile + captures
  identity + measures the box + seeds the draft; pointer classification for caret-move vs switch).
- `dispatchers.tsx` — `Block`/`CustomBlock`, `isBlockEditTarget` (matches `anchorR0`), `EditTextarea`
  (controlled value, dirty + commit guard, IME, move trigger, caret-on-arrival).
- `PreviewContext.tsx`, `sourceIndex.ts` — context surface and the value index.

---

## Phase breakdown

- **Phase 1 — active-region fix (done).** A click inside an open editor no longer climbs to the
  parent; the active editor's wrapper is marked by `activeEditRegionRef`, and a click inside it is a
  caret move. Mode-independent; ships first.
- **Phase 2 — locked depth-aware editing + cross-surface cursor + concurrency-safe identity (done).**
  Everyone gets this; no WASM, no setting. Resolution collapses a click to the locked tile
  (deepest-wins + coincidence + prefixing-atomic); the roving-tabindex aligns to the locked tiles;
  the cross-surface cursor moves between tiles (arrows + click-switch, committing as it goes); the
  byte-offset identity + controlled value + self-heal + dirty/commit guards make editing
  concurrency-safe and fix the live re-target data-integrity bug. The default click target therefore
  changes from leaf-first (old) to locked-tile (a chrome-less single-child div now opens the div; a
  list/quote opens whole) — intended.
- **Phase 3 — "Nesting cursor" unlock (designed, not yet built; separate agent).** A
  default-off `unlockNestingCursor` flag turns the locked default into a power-user nesting editor: clicks
  resolve to the leaf; two nesting keys move in/out through nesting (`Cmd+Ctrl+←/→` on macOS,
  `Alt+Shift+←/→` elsewhere); a breadcrumb chip shows the AST-derived path. **Both hosts can opt in,
  feeding the same host-agnostic iframe behavior:** hub-client via a reactive `usePreference` setting;
  the SPA via a `?nestingCursor=1` URL query param (read at load) in `PreviewApp.tsx`. The flag +
  regenerated buffers are optional payload fields, so a host with its opt-in off omits them and the
  iframe stays locked (zero-touch).
  Editing a multi-line block *inside* a prefixing container would slice in `> `/indent, so — and only
  here — the buffer is **regenerated clean from the AST** (`pampa::write_single_block` via a new
  `regenerate_nested_buffers` WASM entry point), gated so the regeneration pass is unreachable in the
  locked default. The identity, self-heal, and commit machinery are reused unchanged; the cursor
  simply tracks a deeper `anchorR0`. (This **supersedes** the earlier "Plan 3 = recursive
  `lookup_block` path-splice in Rust" mechanism: descent rides AST-regenerated buffers + the existing
  splice, not a new recursive lookup.) See the plan's Phase 3 section.

---

## Key facts (pool / write-back, with refs)

- **Pool entry shape** (`types/sourceInfo.ts`): `Original {t:0, r:[s,e], d:file}`; `r` are UTF-8 byte
  offsets (slice via `TextEncoder`/`TextDecoder`). A block and its first *inline* child can share
  `r[0]`; the dispatcher matches block nodes only, and the `anchorSlice` content check is the arbiter,
  so the sharing is harmless.
- **`anchorR0` lives in the pool `r` space, which equals the untransformed `sourceEntry.r`** by
  value-keyed correspondence (`resolveSource` returns `sourceEntry = pool[node.s]`). Capture at click
  time from `pool[s].r`; match at render time against `resolved.sourceEntry.r[0]`.
- **The writer re-serializes containers wholesale** (Tier-2, `incremental.rs`), preserving outside
  bytes. Container shapes (`block.rs`): `Div/BlockQuote/Figure` hold `Blocks`; lists hold
  `Vec<Blocks>`; `DefinitionList` holds `Vec<(Inlines, Vec<Blocks>)>`.
- **postMessage is one-way** (parent→iframe `UPDATE_AST`; iframe→parent `SET_AST`); there is no
  request/response channel, which is why a modified move's reland keys off the next render (+ timeout
  fallback) rather than awaiting a specific commit.

---

## Known limitations (v1) and risks

- **LineBlock** not editable (`| line` parses as `Para`); **inline-span editing** out of scope (only
  `Cite`/`Note` carry `s`); **table cells/captions, figure captions** not block-editable (`Opaque`);
  **generated-container regions** and **cross-file edits** out of scope.
- **Commit is not a no-op.** Only *cancel* is a guaranteed no-op; a commit reformats the edited
  container (Tier-2). The kept guarantee is byte-verbatim *outside* the edit.
- **Locked coarseness = concurrency-clobber unit.** Two users editing different items of one list both
  hold the whole-list buffer; last-writer-wins, since the CRDT cannot merge two full-container
  rewrites. Per-item (unlocked, Phase 3) editing splices each item separately and does not collide.
- **The destination projection has no concurrency guard.** Self-heal protects the *active* editor; a
  concurrent collaborator re-render landing first can resolve a move's `destLine` against a
  doubly-changed document (worst case: off by a tile, never onto nothing). The exact fix is Automerge
  native cursors — deferred.
- **Geometry is browser-only.** Coincidence epsilon, collapsed-callout visibility, soft-wrap
  last-visual-line, and caret-on-arrival are verified in Playwright; jsdom (no layout) cannot test
  them. Two real bugs (an integer-`scrollHeight` rounding miss in last-line detection; a 2-tile
  navigation no-op) were caught only by the real browser — a standing argument that environment-
  dependent behavior must be tested in Playwright, not mocked geometry.

---

## References

- Round-trip core: `claude-notes/plans/2026-06-04-target-incremental-writes.md`;
  `apply_node_edit.rs`, `writers/{incremental,qmd,json}.rs`, `quarto-source-map/src/source_info.rs`.
- Implementation plan + execution log: `claude-notes/plans/2026-06-11-block-editing-improvements.md`;
  hand-off companion: `claude-notes/plans/2026-06-13-block-editing-handoff.md`.
- Front-end: `q2-preview/{PreviewRoot,entry,dispatchers,useBlockEditHover,PreviewContext,sourceIndex,
  lockedTiles,byteLineMap,caretGeometry}.tsx`, `q2-preview/{blocks,custom}/*`,
  `iframe/Q2PreviewIframe.tsx`; `hub-client/src/components/render/{ReactPreview,ReactRenderer}.tsx`.
- Phase 3 Rust/WASM entry points: `crates/pampa/src/writers/qmd.rs::write_single_block`,
  `crates/wasm-quarto-hub-client/src/lib.rs::apply_node_edit`,
  `ts-packages/preview-runtime/src/wasmRenderer.ts`.
