# Breadcrumb forward-crumbs — preview the nest-in descent target

**Date:** 2026-06-15
**Branch:** feature/block-editing-improvements (worktree `.worktrees/block-editing`)
**Status:** **SKETCH / not scheduled.** Concept captured during the breadcrumb visual-design
review (2026-06-15). The *display scaffolding* lands in
`2026-06-15-breadcrumb-visual-design.md` (pivot-at-surface-left layout, empty right-group
container, `.q2-crumb-future` faded style); the *functionality* below is deferred to this plan.
**Depends on:**
- `2026-06-15-breadcrumb-visual-design.md` — provides the right-side scaffolding this plan fills.
- `2026-06-15-nesting-cursor-navigation-and-list-items.md` — provides `childSurfaceTowardLine`
  (the caret-line descent resolver) this plan reads. **Land that first.**

> **All cited files live under `ts-packages/preview-renderer/src/q2-preview/`** unless a path is
> given. Playwright acceptance specs live under `hub-client/e2e/`.

## Concept

The breadcrumb today shows the **ancestor path** — the containers the cursor is *inside*,
rendered to the **left** of the active surface (filling the indent gutter, spilling into the page
margin for non-indenting containers). This plan adds **forward-crumbs**: a faded preview, to the
**right** of the surface-left seam, of **what ▶ (nest-in) would descend into** from the current
position.

The spatial metaphor completes: **surface-left is the pivot.** Left of it = where you came from
(the indentation that already happened). Right of it = where ▶ would take you (the indentation you
would descend into). As you arrow up/down within a multi-level list, the forward-crumbs update to
reflect the descent target **at the caret's current line**.

The forward-crumbs are **faint / translucent** — they are a *hint about a potential move*, not a
record of current position.

## Why it's a separate plan (not the visual pass)

1. **Caret-driven, not activation-driven.** The ancestor path only changes when the active surface
   changes. The forward path changes on **every caret move** (arrow up/down), because the descent
   target is `childSurfaceTowardLine(currentSurface, caretLine)` — it depends on `Ls`, the caret's
   source line. The chip must subscribe to caret position, which it does not today.
2. **Builds on the navigation successor.** `childSurfaceTowardLine` /`surfaceLineSpan` /
   `depthOfSurface` (the descent resolvers) are owned/refined by
   `2026-06-15-nesting-cursor-navigation-and-list-items.md` (§1 line-anchored nav, §3
   trailing-whitespace descent fix). This plan should consume that settled resolver, not race it.

## Open questions (resolve when scheduling)

- **Depth: one crumb or the whole chain?** Show only the immediate nest-in child (a single faded
  crumb), or the full descent chain (repeated ▶ nudges) with increasing fade per level? The chain
  previews "where I end up if I keep nudging in," but costs width on the right (which competes with
  content) and more recompute.
- **Occlusion is over *content*, not whitespace.** Ancestor crumbs sit over the empty indent
  gutter; forward-crumbs sit over the **indented content area to the right** of surface-left. So
  the translucency is *required* (not decorative) — you must see through to the line/child beneath.
  And the descent target is usually spatially **below** the caret line, so projecting it rightward
  on the chip's row is an **abstraction**, not a literal pointer; the fade signals "hint, not
  location." Validate this reads correctly with users.
- **Right-side page-edge budget.** Forward-crumbs growing right compete with content width and the
  right page edge — mirror of the left-side gutter/margin/edge-clamp logic, but on the right.
- **What does clicking a forward-crumb do?** Presumably `requestNestingMove('in')` (or a
  multi-level descend to that crumb). Define the click semantics; they differ from ancestor crumbs
  (`requestNestingSelect(r0, r1)` jumps to an existing range).

## Rough shape (not a commitment)

- Compute a `forwardPath: AncestorCrumb[]` from `childSurfaceTowardLine(currentSurface, caretLine)`
  (one level) or by iterating it (the chain). Reuse `abbrevForSourceNode` / `categoryForSourceNode`
  from the visual-design plan; mark each as `future: true`.
- Recompute on caret move — wire the chip to the caret/line source the navigation code already
  tracks (`PreviewRoot` `Ls` derivation, `:1087`), debounced if needed.
- Populate the **right-group container** the visual-design plan leaves empty; apply
  `.q2-crumb-future`.
- Tests: pure unit for the forward-path derivation (mirrors the `childSurfaceTowardLine` tests);
  integration that arrowing within a multi-level list updates the forward-crumbs; e2e that the
  faded crumbs appear to the right and update on caret move.

## Out of scope

- Any change to the descent **mechanics** (owned by the navigation successor).
- The ancestor path / gutter-fill / margin-spill (owned by the visual-design plan).
