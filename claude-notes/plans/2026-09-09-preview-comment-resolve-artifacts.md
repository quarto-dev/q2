# q2-preview comments: resolving the last comment leaves an empty pill and a stuck glow

**Strand:** bd-bpt089zw
**Branch / worktree:** `braid/bd-bpt089zw-q2-preview-comments-resolving` at
`.worktrees/bd-bpt089zw-q2-preview-comments-resolving/`
**Status:** diagnosed; fix plan awaiting review (no code changes yet)

## Overview

In hub-client, with a `format: q2-preview` document, hovering the right half of
a block shows a `+` affordance; clicking it opens a comment bubble with an
inline input; Enter commits a `[>> ...]` span into the source. Clicking the
`✓` (Resolve) button in the expanded bubble removes the span. After resolving
the **last** comment on a block, two visual artifacts remain:

1. a small empty pill (~14×6 px) where the bubble was, and
2. the block's light-blue "glow" (the bubble-hover glow on the block wrapper)
   never clears — not even after moving the mouse away or clicking elsewhere.

Both live entirely in `CommentWrapper` in
`ts-packages/preview-renderer/src/q2-preview/custom/CommentBlock.tsx`. Both
are state-lifecycle bugs: React state inside the bubble component survives the
resolve commit round-trip (blocks are keyed by index in
`framework/dispatch.tsx`, so the component instance is preserved when the new
AST arrives) and nothing resets it. Both fields date from Comments v1
(`a6fc44b8e`, 2026-07-30); this is not a regression from the rich-bubble work
(bd-y66gbfs4).

## Reproduction (done, 2026-09-09)

Environment: fresh worktree, `npm install`, `npm run build:wasm && npm run
build:sandboxed` in `hub-client/`, then `npx vite --port 5199` (default
`wss://sync.automerge.org` sync server, auth disabled). Driven through the
Chrome DevTools MCP against `http://localhost:5199/` (the claude-in-chrome
extension was not connected).

Steps:

1. New project set → New project (Default) → replace `index.qmd` with the
   document from the bug report (`format: q2-preview`, one `##` heading, one
   paragraph).
2. Hover the right half of the paragraph in the preview iframe → `+` bubble.
   (The DevTools `hover` tool lands on the element centre, which is the
   left/right midpoint; a synthetic `mousemove` at `rect.right - 5` was used
   for this single step. Every subsequent interaction used real pointer
   events.)
3. Click `+` (real click) → bubble self-expands, textarea auto-focused. Type
   `Comment`, Enter → source becomes `This paragraph is very good.[>> Comment]`
   and the bubble shows `Comment ✓` (matches the report's second screenshot).
4. Click `✓` (real click).

Observed DOM immediately after step 4 (inspected via script in the iframe):

```
wrapper style:  position: relative; box-shadow: rgba(140,190,240,0.6) 0 0 8px 2px   ← block glow ON
chrome:         <div data-q2-owns-focus style="... z-index: 1000">                  ← selfExpanded still true
bubble:         <div class="q2-comment-bubble" title="0 comments" style="... box-shadow: GLOW"></div>
bubble rect:    14 × 6 px, zero children                                            ← the empty pill
:hover chain:   MAIN > SECTION > DIV(wrapper) > P                                   ← pointer is over the paragraph, not the bubble
```

This matches the report's third and fourth screenshots exactly.

Follow-up probes:

- **Real mouse move to the heading** (leaving the block): the *bubble's* glow
  cleared (`isHovered` → false via the wrapper's `onMouseLeave`), the pill
  stayed (z-index still 1000), and the *wrapper* glow stayed ON.
- **`mousedown` outside the bubble**: the pill disappeared (the click-outside
  handler reset `selfExpanded`), but the wrapper glow still stayed ON.
- **Real hover onto the `+` bubble, then real move to the heading**: wrapper
  glow finally cleared. So the glow is stuck only because one `mouseleave`
  was lost; a subsequent genuine enter/leave pair repairs it.
- **Variant — global "Expand comments" mode, `✓` clicked without ever
  clicking the bubble** (`selfExpanded` false): after resolve the bubble
  renders `+` while the block is still hovered (acceptable), and the wrapper
  glow sticks the same way. So defect 1 needs `selfExpanded`; defect 2 does not.

## Diagnosis

### Defect 1 — empty pill: `selfExpanded` outlives the last comment

Relevant lines (`CommentBlock.tsx`, `CommentWrapper`):

```ts
const [selfExpanded, setSelfExpanded] = React.useState(false);
const expanded = (mode === 'expand' && comments.length > 0) || selfExpanded;
...
const chromeVisible = comments.length > 0 || isHovered || selfExpanded;
```

and the bubble body:

```tsx
{comments.length === 0 && !expanded ? (
    <div>+</div>
) : expanded ? (
    <>{comments.map(...)}{showInlineInput && <textarea .../>}</>
) : ( ...compact... )}
```

Timeline in 'show' mode:

1. Click `+` → `setSelfExpanded(true)`, `setShowInlineInput(true)`.
2. Enter → `addComment()` commits; `setCloseAtCount(0)`. When the new AST
   lands with one comment, the `closeAtCount` effect sets
   `showInlineInput(false)`. **`selfExpanded` stays true** (by design — the
   bubble should stay open to show the comment you just added).
3. Click `✓` → `resolveCommentAtIndex(0)` commits the block without the span.
   Nothing touches `selfExpanded`.
4. New AST lands: `comments = []`, `selfExpanded = true`, `showInlineInput =
   false`. Therefore `expanded = true`, `chromeVisible = true`, and the bubble
   body is the `expanded` branch with zero rows and no input → a bordered,
   padded `div` with no children. That is the pill. The `z-index: 1000`
   confirms the state.

The pill persists until a `mousedown` outside the bubble runs the
click-outside handler (`setSelfExpanded(false)`), or Escape inside the
(non-existent) input. In 'expand' mode without a prior bubble click,
`selfExpanded` is false so the same render falls through to `+` (fine).

### Defect 2 — stuck glow: `bubbleHovered` relies on a `mouseleave` that never fires

```tsx
const [bubbleHovered, setBubbleHovered] = React.useState(false);
...
<div style={{ boxShadow: bubbleHovered ? GLOW : 'none' }}   // block wrapper
     onMouseMove={... setIsHovered(rightHalf)}
     onMouseLeave={() => setIsHovered(false)}>
  ...
  <div className="q2-comment-bubble"
       onMouseEnter={() => setBubbleHovered(true)}
       onMouseLeave={() => setBubbleHovered(false)}>
```

When the user clicks `✓`, the pointer is over the `<button>` inside the
bubble. The resolve commit re-renders the bubble: the button (and the whole
row) unmounts and the bubble shrinks to the pill, so the pointer is now over
the paragraph. Two browser facts combine here:

- Chrome updates the `:hover` chain after layout **without** dispatching
  `mouseout`/`mouseover` (we observed `:hover` ending at `P` while React still
  believed the bubble was hovered).
- On the next real movement, Chrome dispatches `mouseout` from its *current*
  hovered element (`P`), not from the removed button. React synthesises
  `onMouseLeave` for the ancestors of the `mouseout` target up to the common
  ancestor with the `mouseover` target. The bubble is not an ancestor of `P`,
  so the bubble's `onMouseLeave` never runs. The wrapper *is* an ancestor of
  `P`, which is why `isHovered` cleared correctly in the same move.

Result: `bubbleHovered` is stuck at `true` until the pointer genuinely enters
and leaves the bubble again. Because the wrapper's glow is `bubbleHovered ?
GLOW : 'none'`, the block glows indefinitely — including after the pill is
dismissed, and across the 'expand'-mode variant.

This is the general React hazard "`onMouseLeave` is lost when the hovered
node is removed from the DOM"; it will bite any resolve (not only the last
one) whenever the bubble's new size no longer contains the pointer. It is
just most visible after the last resolve because the bubble collapses to
almost nothing.

## Fix plan

### Design

**Defect 1.** Collapse the self-expanded state when the block's comment list
becomes empty and no input is open. This is a derived-state correction, so it
belongs in an effect keyed on the same signal `closeAtCount` already uses
(`comments.length`), not in the `✓` handler: resetting in the handler would
flash the compact chip with the still-present comment until the commit
round-trip lands, and an effect also covers a *remote* collaborator resolving
the last comment.

```ts
// A bubble self-expanded to show its comments has nothing left to show
// once the last one is resolved (locally or remotely): collapse it so the
// chrome falls back to the hover-only `+` affordance. `showInlineInput`
// guards the legitimate empty-and-expanded state — `+` just clicked, input
// open, no comment yet.
React.useEffect(() => {
    if (comments.length === 0 && selfExpanded && !showInlineInput) {
        setSelfExpanded(false);
    }
}, [comments.length, selfExpanded, showInlineInput]);
```

Reachability check of the guard: every path that produces
`selfExpanded && comments.length === 0` with the input closed is a removal
(local resolve, remote resolve, or the source being edited under the bubble).
`+` → input open; Escape / click-outside → both false; Enter → input closes
only once `comments.length > closeAtCount`, i.e. never at zero.

**Defect 2.** Stop deriving `bubbleHovered` from the bubble's own
`onMouseEnter`/`onMouseLeave`. The wrapper already owns a `mousemove` handler
(for the right-half `isHovered` test) that also receives moves over the
bubble — the chrome stops propagation of pointer-down/up, mousedown, click and
keydown, but **not** mousemove — and a wrapper `mouseleave` that provably fires
(DOM-tree based, so the bubble poking outside the wrapper's box still counts
as inside). Make the wrapper the single source of truth:

```tsx
onMouseMove={(e) => {
    const rect = e.currentTarget.getBoundingClientRect();
    setIsHovered(e.clientX >= rect.left + rect.width / 2);
    // Derived from containment on every move rather than from the
    // bubble's own enter/leave: a resolve re-renders the bubble under a
    // stationary pointer, Chrome re-evaluates :hover after layout without
    // boundary events, and the next move's mouseout comes from the NEW
    // hovered node — so the bubble's onMouseLeave never fires and the glow
    // sticks (bd-bpt089zw).
    setBubbleHovered(!!bubbleRef.current?.contains(e.target as Node));
}}
onMouseLeave={() => {
    setIsHovered(false);
    setBubbleHovered(false);
}}
```

and delete the bubble's `onMouseEnter`/`onMouseLeave` pair. This fixes the
"stuck after any movement" case with no new browser features and is fully
exercisable in jsdom.

**Optional refinement (recommended, small):** clear the glow *before* the
first movement too — the exact state in the report's third screenshot. Track
the last pointer position in a ref from the wrapper's `mousemove`, and in the
existing layout effect that re-registers the bubble on size changes (deps
`[chromeVisible, comments.length, expanded, showInlineInput]`), re-test
containment geometrically:

```ts
// Bubble just changed size/content under a possibly stationary pointer:
// re-derive the hover from geometry so the glow can't outlive the button
// that was under the cursor.
const pt = lastPointerRef.current;
const r = bubbleRef.current?.getBoundingClientRect();
if (bubbleHoveredRef.current && (!pt || !r || !inside(pt, r))) setBubbleHovered(false);
```

I'd include it: it is ~10 lines, keyed on deps that already exist for the same
"bubble changed shape" reason, and it makes the result deterministic instead of
"fixed on the next pixel of movement". Happy to drop it if you'd rather keep
the change minimal.

**Not proposed:** a CSS-only glow via `.wrapper:has(.q2-comment-bubble:hover)`.
It would be the most robust (the browser's `:hover` is exactly the thing that
*is* kept correct), but it can't be tested in jsdom, it's the first `:has()`
in the preview renderer, and the module's inline-style convention would need a
class + injected rule. Worth reconsidering if the pointer-tracking approach
grows warts.

### Files

- `ts-packages/preview-renderer/src/q2-preview/custom/CommentBlock.tsx` —
  `CommentWrapper` only (both fixes).
- New test file
  `ts-packages/preview-renderer/src/q2-preview/custom/CommentBlock.resolveLast.integration.test.tsx`
  (same harness as `CommentBlock.bubbleText.integration.test.tsx`: jsdom,
  `<Ast>` + `previewRegistry` under a `PreviewContext.Provider` whose
  `resolveSource` returns `{ sourceNode, sourceEntry, reachabilityClass:
  'TopLevel' }` and whose `commitSubtreeEdit` is a `vi.fn()`).
- `hub-client/changelog.md` is **not** touched: the change is in
  `ts-packages/preview-renderer`, not `hub-client/`. (Confirm with the user if
  they want a changelog line anyway, since the visible effect is in hub-client.)

## Checklist (TDD)

### Phase 1 — tests first (must fail on `main`)

- [ ] T1 **empty pill**: mount a Para with one comment; click the bubble
  (`[title="1 comment"]`) → `✓` button (`[title="Resolve comment"]`)
  appears; click it → `commitSubtreeEdit` called with a Para whose inlines
  contain no `quarto-edit-comment` span; `rerender` with the comment-less AST
  → assert **no** `[data-q2-owns-focus]` chrome in the container (pointer is
  not hovering; nothing should be visible). Expected today: chrome present,
  bubble `title="0 comments"`, zero children.
- [ ] T2 **stuck glow after movement**: same setup; `fireEvent.mouseEnter`/
  `mouseMove` with `target` inside the bubble → wrapper `style.boxShadow`
  equals the glow; click `✓`, rerender comment-less; `fireEvent.mouseMove` on
  the wrapper with `target` = the `<p>` → wrapper `boxShadow === 'none'`.
  Expected today: still the glow.
- [ ] T3 **stuck glow after leaving**: as T2 but `fireEvent.mouseLeave` on
  the wrapper after the rerender → `boxShadow === 'none'`. (Today: glow.)
- [ ] T4 **no over-collapse**: two comments; click bubble; resolve index 0;
  rerender with one comment → bubble still expanded (`✓` still present, the
  remaining comment text shown). Guards the `comments.length === 0` condition.
- [ ] T5 **`+` still works**: comment-less block, `mouseMove` at the right half
  → `+`; click it → textarea present and the chrome stays (the
  `showInlineInput` guard of the collapse effect). Guards against the effect
  eating the add flow.
- [ ] T6 (only if the refinement ships) **glow clears without movement**: stub
  `getBoundingClientRect` on the bubble to a rect that excludes the recorded
  pointer; after the comment-less rerender, wrapper `boxShadow === 'none'`
  with no further events.
- [ ] Run `npm test -w ts-packages/preview-renderer -- CommentBlock.resolveLast`
  (integration config: `npm run test:integration -w ts-packages/preview-renderer`)
  and record the failing assertions.

### Phase 2 — fix

- [ ] Collapse effect for `selfExpanded` (defect 1).
- [ ] Wrapper-owned `bubbleHovered` derivation; remove the bubble's
  enter/leave handlers (defect 2).
- [ ] Optional refinement: pointer-position ref + geometric re-check in the
  size-change layout effect.
- [ ] Update the `CommentWrapper` header comment where it describes the
  bubble-hover → block-glow mirror.
- [ ] All T1–T6 green; full `npm run test:integration -w ts-packages/preview-renderer`
  and `npm run test` green.

### Phase 3 — verification

- [ ] `cargo xtask verify` (full: `preview-renderer` is bundled into
  hub-client, so the hub-build leg applies; `npm run build:all` in
  `hub-client/` at minimum).
- [ ] End-to-end in Chrome against `npx vite --port 5199` (dev server picks
  up the TS change via HMR): repeat the reproduction steps; after `✓`, the
  wrapper `style.boxShadow` must be `none` and no `[data-q2-owns-focus]`
  element may exist under the paragraph wrapper (or a `+` only while the
  right half is hovered). Also repeat the 'Expand comments' variant. Record
  the DOM probe output here.
- [ ] Commit; `braid close bd-bpt089zw`; ask before pushing.

## Open questions for review

1. Include the optional geometric re-check (clears the glow with the pointer
   stationary), or keep the minimal event-based fix?
2. After the last resolve, should the chrome show `+` while the right half is
   still hovered (current behaviour of the 'expand'-mode variant, and what the
   minimal fix yields), or disappear entirely until the pointer moves? The
   plan assumes `+`-while-hovered is fine.
3. Changelog: `hub-client/changelog.md` entry or not, given the code lives in
   `ts-packages/preview-renderer`?
