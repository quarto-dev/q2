# Block-editing UI glitches — round 3 (fixes & tests)

## Overview

Third round of the "spike-and-stabilize" block-editing glitch pass, same method
as round 2 (`2026-06-18-block-editing-glitches-2.md`). Each glitch was
reproduced live on the running dev server, root-caused by printing real runtime
values in the browser (temporary console probes), fixed and tuned against the
real DOM, and is recorded here verbatim so the work can be **rebuilt from a
clean slate**.

**TREAT THE WORKING TREE AS EMPTY OF THESE CHANGES.** This plan — not the tree
— is the source of truth. Phase C reverts the branch to clean; Phase D
re-implements from this document, TDD-first, one glitch per sub-agent.

Glitches this round:

- **G20** — the in-place editor for a nested list/definition item opens **taller
  than the rendered line it replaces** (a measurement artifact, not CSS padding).
- **G21** — committing an edit (Cmd-Enter or margin-click blur) **navigates focus
  away** to the next block (locked-cursor-era focus-restore residue).
- **G22** — *new feature*, not a bug fix: a **commit-status indicator** (a
  translucent glass "bulb") that shows pending / real-change / spurious-no-op /
  error for every commit, plus routing commit **errors** into the existing
  `PreviewErrorOverlay`. Restores visibility into spurious edits now that the
  underlying causes are fixed.

### Probes used during the spike (NOT part of the clean slate)

Two temporary console probes were added to diagnose G20 and MUST NOT be
re-applied — they are reverted at Phase C and never return:

- `outerBlocks.ts` `measureLeadingBlockBox` — a `[G20-probe]` `console.debug`
  block logging the RANGE-path measurement.
- `dispatchers.tsx` `EditTextarea` autosize effect — a `[G20-editor]`
  `console.debug` logging the live editor box.

`dispatchers.tsx` is touched by **nothing else** this round, so after Phase C it
should show **no diff** at all.

---

## Glitch index (checklist)

- [x] **G20 — nested list-item editor opens too tall.** Range path measured the
  leading line with `getBoundingClientRect()` (the union of all client rects,
  incl. the inter-block gap → ~32.6px) instead of the first client rect
  (~25.5px). Fix: `getClientRects()[0]`. File: `outerBlocks.ts`.
- [x] **G21 — commit navigates focus to the next block.** Post-commit focus
  restore called `outerBlockForAnchorR0` (outer-blocks-only, with a next-block
  fallback) for a nested anchor → jumped past the list. Fix: mode-aware
  `refocusTargetForAnchorR0`. Files: `outerBlocks.ts`, `PreviewRoot.tsx`.
- [x] **G22 — commit-status bulb + commit-error routing.** One classification
  funnel in `handleSetAst`; a glass bulb overlay for pending/change/spurious;
  errors routed to `PreviewErrorOverlay`; bulb hidden while the error pill
  shows. File: `ReactPreview.tsx` (hub-client).

---

## G20 — nested list-item editor opens too tall

### Symptom

Clicking a **list item that has a nested block under it** (an `<li>`/`<dd>` whose
leading text is followed by a sublist) opens an editing textarea that is
**taller than the one rendered line it replaces** — not too tall for its text,
just taller than the rendered HTML. Plain leaf items and headings are fine.

### Root cause (confirmed by live probe)

`measureLeadingBlockBox` (in `outerBlocks.ts`) has two measurement paths:

- **Element path** — non-list elements and list items *without* a nested
  pool-id child: measures the element's own rect minus padding/border. **Fine.**
- **Range path** — `<li>`/`<dd>` *with* a nested `[data-block-pool-id]` child:
  builds a DOM Range from the element start to *before* the sublist to isolate
  the leading-text run, then measured its height with
  `range.getBoundingClientRect()`.

`getBoundingClientRect()` returns the **union of every client rect the Range
touches**. The probe showed the Range spans **7 client rects** — the leading
line *plus* whitespace/gap fragments down to the sublist boundary:

```
[G20-probe] RANGE <li> pool=285  rangeBoundingH= 32.57  rangeFirstRectH= 25.5
            rangeRectCount= 7   lineHeight= 25.5px   firstChildRectH= 25.5
[G20-editor] contentHeight= 32.57  draftLineCount= 1  draft="an item"
             taOffsetHeight= 33  wrapperOffsetHeight= 33
```

So the editor's `contentHeight` came out **32.57px** while the rendered leading
line is **25.5px** (`= lineHeight = getClientRects()[0].height`). The extra ~7px
is the inter-block gap the union swallowed. List items carry **0** vertical
padding (the user's initial "inside padding" hypothesis was disproven by the
probe) — it is purely a measurement artifact unique to the Range path.

### Chosen fix (`outerBlocks.ts` `measureLeadingBlockBox`) — VERBATIM

Use the Range's **first client rect** (the leading line's own fragment) instead
of the bounding union. Replace the `contentHeight` assignment in the range
branch (the block beginning `// jsdom does not implement
Range.getBoundingClientRect`):

```ts
        // G20: measure the leading-text line via the range's FIRST client rect,
        // NOT its bounding rect. `getBoundingClientRect()` unions every client
        // rect the range touches — the leading line PLUS the inter-block gap
        // down to the sublist boundary — inflating the height (~25.5px line →
        // ~32.6px union) so the editor sat taller than the rendered line it
        // replaces. `getClientRects()[0]` is the leading line's own fragment,
        // which matches the rendered line exactly. Falls back to the bounding
        // rect if client rects are unavailable.
        // jsdom implements neither — guard gracefully (contentHeight 0).
        const px = (v: string) => parseFloat(v) || 0;
        const rangeRects = typeof range.getClientRects === 'function'
            ? range.getClientRects()
            : null;
        let contentHeight: number;
        if (rangeRects !== null && rangeRects.length > 0) {
            contentHeight = px(String(rangeRects[0].height));
        } else {
            const rangeRect: DOMRect | null = typeof range.getBoundingClientRect === 'function'
                ? range.getBoundingClientRect()
                : null;
            contentHeight = rangeRect !== null ? px(String(rangeRect.height)) : 0;
        }
```

Everything else in the range branch (the `emptyBoxStyle`, `rangeUsed: true`
return) is unchanged. The Element path is **untouched** — leaf items and
headings keep their behavior.

### Why the first rect (not bounding/quantized/summed)

Three options were weighed live:
- **A (chosen)** — `getClientRects()[0]`: exact match to the rendered single
  line; one-line change. For *wrapped* multi-line leading text it opens at one
  line and the existing §7 expand-on-edit grows it (and the textarea re-wraps at
  its own width anyway, so matching the rendered wrap height isn't meaningful).
- B — quantize the union: `round(boundingH / lineHeight) * lineHeight`. Handles
  wrapping but rounding is fragile near `.5`.
- C — sum only rects whose height ≈ `lineHeight`. Most faithful for wrapping,
  more code + a tolerance to tune.

A was confirmed correct live for **both 1-line and 2-line** problem items. B/C
are the fallback if a genuinely-wrapping leading item ever looks wrong on open.

### Consistency note (why one change suffices)

`measureLeadingBlockBox` is the single leading-height chokepoint: `measureBlockBox`
delegates to it 1:1, and all three height producers go through `measureBlockBox`
— click/keyboard activation (`useBlockEditHover` `activate`), arrow-key reland
(`PreviewRoot` `openEditTarget` box:`'snapshot'` fallback), and the geometry
snapshot cache (`snapshotOuterBlockGeometry`). No second site measures editor
height from a range.

### Test plan (TDD-first / fail-on-revert) — jsdom unit

- **Tier:** jsdom unit (no real layout).
- **Seam / file:** `src/q2-preview/s0-list-item-surfaces.integration.test.tsx`
  (the existing §0 list-item-surface suite, which already stubs `Range`
  prototype methods and exercises `measureLeadingBlockBox`'s `rangeUsed` flag).
- **Real unit mounted:** the actual `measureLeadingBlockBox` export (not a copy).
- **Mock boundary:** jsdom implements neither `Range.prototype.getClientRects`
  nor `getBoundingClientRect`; stub **both** on the prototype for the test:
  - `getClientRects` → a `DOMRectList`-like `[{ height: 25.5 }, { height: 7 }]`
    (first = leading line, second = the gap fragment),
  - `getBoundingClientRect` → `{ height: 32.5 }` (the union).
- **Construct:** an `<li>` containing a leading text node followed by a child
  with `data-block-pool-id` (forces the Range path).
- **Exact assertion:** `measureLeadingBlockBox(li).contentHeight === 25.5`
  (the first client rect), **and** `rangeUsed === true` (vacuity guard — proves
  the Range path ran, not the element path).
- **Named revert hunk:** replace the `getClientRects()[0]` branch with the old
  `const rangeRect = …getBoundingClientRect(); contentHeight = rangeRect.height`.
  **Predicted RED:** `expected 25.5, got 32.5`.

### Live-tuned constants

None. The fix is structural; 25.5/32.6 are observed values, not tuned.

### Accepted-untested (logged, not silently omitted)

- **The real-browser inequality** `getClientRects()[0].height <
  getBoundingClientRect().height` (i.e. that the inter-block gap actually
  exists) is an environment property, not our logic — confirmed live during the
  spike, not re-asserted in CI. Our logic (selecting `[0]`) IS bound at jsdom above.
- **The `rangeRects === null` / empty fallback** (no client rects available,
  e.g. jsdom with no stub) → bounding rect, else 0. Defensive graceful
  degradation, identical to pre-fix behaviour; not separately tested.

### Status

Implemented & confirmed live (1-line and 2-line nested items). **Clean-slate
rebuild DONE (2026-06-19)** — TDD test added to
`s0-list-item-surfaces.integration.test.tsx` (asserts `contentHeight === 25.5`
+ `rangeUsed === true`); RED `expected 25.5, got 32.5` before fix, GREEN after,
fail-on-revert binding proven. Integration suite 458 passed / 1 skipped, tsc clean.

---

## G21 — commit navigates focus to the next block

### Symptom

While the **nesting-cursor setting is ON**, editing a nested list item and
committing (Cmd-Enter, or blurring by clicking in the margin) moves focus/cursor
**away from the edited item to the next block or the one after** — the user
expects the edited node to stay selected, with no navigation.

### Root cause (confirmed by live probe)

Post-commit focus restore flows: blur / Cmd-Enter (`dispatchers.tsx`
`EditTextarea`) → `ctx.requestFocusRestore(anchorR0)` stashes a `focus`-intent
landing → after the commit re-render, `PreviewRoot` `executeLanding` calls
**`outerBlockForAnchorR0(host, pool, anchorR0)`** to focus a block.

`outerBlockForAnchorR0` scans **outer (top-level) blocks only**
(`enumerateOuterBlocks`) and, when no outer block has `r0 === anchorR0`, falls
back to the **next outer block** (`r0 > anchorR0`). A nested list item is *not*
an outer block — its anchor lives inside a list — so the exact match always
fails and focus lands on the top-level block after the whole list. Probe:

```
[G21-focus] unlock=true  anchorR0=1273
            outerBlockForAnchorR0 → pool=480, r0=1813   ← 540 bytes PAST the edit
            exactSurfacePool=340                          ← the actual edited node
```

The "next block" fallback is locked-cursor-era residue (outer-blocks-only
roving). The fix mirrors the **mode-aware roving partition** already used in
`useBlockEditHover` `onKeyDown` (`unlockNestingCursor ? enumerateNestingSurfaces
: enumerateOuterBlocks`).

**Decided (user):** focus-restore is *kept* on both blur and Cmd-Enter/Esc (the
edited node should stay selected after a commit, both modalities) — the bug was
*where* it landed, not *that* it happened. Locked-mode nested editing is not
reachable today, so that branch keeps the old behavior; revisit later.

### Chosen fix — part 1: new helper in `outerBlocks.ts` — VERBATIM

Add after `outerBlockForAnchorR0`:

```ts
/**
 * G21: mode-aware focus-restore target after a commit closes the editor.
 *
 * The old post-commit focus path called `outerBlockForAnchorR0` unconditionally.
 * That scans OUTER blocks only and, when no outer block has `r0 === anchorR0`
 * (always true for a nested list/def item, whose anchor lives inside a list),
 * falls back to the NEXT outer block (`r0 > anchorR0`) — so committing a nested
 * item navigated focus AWAY to the following top-level block. That fallback is
 * locked-cursor-era residue.
 *
 * Mode-aware behaviour (mirrors the roving partition in `onKeyDown`):
 *  - **Unlock nesting cursor (setting ON):** the roving partition is the full
 *    nesting-surface set, so restore focus to the EXACT edited surface
 *    (`r[0] === anchorR0`), scanning ALL `[data-block-pool-id]` elements. No
 *    next-block fallback — never navigate forward. Returns null if the edited
 *    surface did not survive (caller then leaves focus where it is).
 *  - **Locked (setting OFF):** the roving partition is outer blocks only, and
 *    nested-item editing is not reachable, so keep the existing
 *    `outerBlockForAnchorR0` behaviour for outer-block edits.
 *
 * Pure DOM read (no React); same shape as `outerBlockForAnchorR0`.
 */
export function refocusTargetForAnchorR0(
    host: Element,
    pool: unknown[],
    anchorR0: number,
    opts: { unlock: boolean },
): Element | null {
    if (!opts.unlock) {
        return outerBlockForAnchorR0(host, pool, anchorR0);
    }
    // Unlock mode: exact-r0 match across every surface (leaf, li/dd proxy, or
    // container) — the just-edited node. No forward fallback.
    const all = Array.from(host.querySelectorAll<Element>('[data-block-pool-id]'));
    for (const el of all) {
        const pidAttr = el.getAttribute('data-block-pool-id');
        if (pidAttr === null) continue;
        const entry = pool[Number(pidAttr)];
        if (!isOriginalEntry(entry)) continue;
        if (entry.r[0] === anchorR0) return el;
    }
    return null;
}
```

### Chosen fix — part 2: call site in `PreviewRoot.tsx` `executeLanding` — VERBATIM

Add `refocusTargetForAnchorR0` to the `./outerBlocks` import. In `executeLanding`'s
`intent === 'focus'` branch, replace the `outerBlockForAnchorR0` call:

```ts
            if (!previewHostRef.current) return;
            // G21: mode-aware focus restore. In unlock mode, return focus to the
            // EXACT edited surface (no next-block jump); in locked mode, keep the
            // outer-block behaviour.
            const refocus = refocusTargetForAnchorR0(
                previewHostRef.current, currentPool, pl.anchorR0,
                { unlock: unlockNestingCursorRef.current ?? false },
            );
            if (refocus) {
                (refocus as HTMLElement).focus?.();
            }
            pendingLandingRef.current = null; // consumed
            return;
```

`outerBlockForAnchorR0` stays imported — it is still used by the self-heal DROP
branch and is now the locked-mode delegate.

### Test plan (TDD-first / fail-on-revert) — jsdom unit (+ deferred integration)

- **Tier:** jsdom unit (pure DOM read; `querySelectorAll` works in jsdom).
- **Seam / file:** `src/q2-preview/outerBlocks.integration.test.ts` (where
  `outerBlockForAnchorR0` is already unit-tested).
- **Real unit mounted:** the actual `refocusTargetForAnchorR0` export.
- **Mock boundary:** none — build a real detached host with three
  `[data-block-pool-id]` elements and a plain `pool` array:
  - outer block A at `r=[100,…]`,
  - **nested** `<li>` B at `r=[150,…]` (inside a `<ul>`; B is NOT an outer block),
  - outer block C at `r=[200,…]` (the "next" block after B).
- **Exact assertion (unlock):** `refocusTargetForAnchorR0(host, pool, 150,
  { unlock: true })` returns the element whose pool entry `r[0] === 150` (B) —
  assert via its `data-block-pool-id` → `pool[id].r[0] === 150` (vacuity guard,
  not just non-null). It must **not** be C.
- **Companion (locked):** `{ unlock: false }` delegates to
  `outerBlockForAnchorR0` (documents the locked branch).
- **Named revert hunk:** replace the unlock branch body (`const all = …; for
  (…) return el; return null`) with `return outerBlockForAnchorR0(host, pool,
  anchorR0)`. **Predicted RED:** unlock case returns C (the next outer block),
  not B → `expected pool-id of B (r0=150), got pool-id of C (r0=200)`.
- **Deferred (integration, optional):** mount `PreviewRoot` in unlock mode,
  commit a nested item, assert `document.activeElement` is the edited surface.
  Heavier (jsdom focus + reland timing + the settle-gate); the unit binding
  above already pins the mechanism. Mark deferred, not skipped silently.

### Live-tuned constants

None.

### Accepted-untested (logged, not silently omitted)

- **Unlock-mode no-match → `null`** (the edited surface was deleted / didn't
  survive → the caller leaves focus put; the "never navigate forward" guarantee
  for the delete case) is transitively guarded by the no-forward-fallback named
  revert above, but not separately asserted. *Optional hardening:* add a third
  unit case — `unlock:true` + an `anchorR0` matching no surface → `null`.
- **The `executeLanding` call-site wiring** (reads `unlockNestingCursorRef`,
  calls `refocusTargetForAnchorR0`, focuses the result) is exercised only by the
  deferred integration test above — accepted-untested at the unit tier.
- **Locked-branch companion harness:** the `{ unlock: false }` case delegates to
  `outerBlockForAnchorR0` → `enumerateOuterBlocks` → `isVisibleBlock`, so it
  needs the existing suite's `getBoundingClientRect` visibility mocks. Follow the
  established `outerBlocks.integration.test.ts` pattern; this is a harness detail,
  not a binding gap.

### Status

Implemented & confirmed live (Cmd-Enter and margin-click blur, unlock mode; non-nested edits still refocus correctly; ArrowDown roving continues from the edited item). **Clean-slate rebuild DONE (2026-06-19)** — `refocusTargetForAnchorR0` added to `outerBlocks.ts`, call site rewired in `PreviewRoot.tsx` `executeLanding`. Three TDD tests added to `outerBlocks.integration.test.ts` (unlock exact-match → B; unlock no-match → null [the optional hardening from accepted-untested]; locked delegates → outer block). RED `refocusTargetForAnchorR0 is not a function`; revert-RED returns null (binding proven); GREEN after. Integration suite 461 passed / 1 skipped, tsc clean.

---

## G22 — commit-status bulb + commit-error routing (new feature)

### Goal

Now that the spurious-edit *causes* are fixed (round 2 + G21), we lost the
ability to *see* a spurious commit. Add a small commit-status indicator that,
for **every** commit, shows:

- **pending** (amber) — a commit is going out,
- **change** (green) — `applyNodeEdit` produced a real diff,
- **spurious** (blue) — `applyNodeEdit` round-tripped to **identical QMD** (the
  false-dirty bug class — what we want to catch),
- **error** (red) — parse/apply rejected; routed to the existing error overlay.

### Architecture — one funnel, one signal

Every commit channel (text / subtree / nesting — six payload-builder sites in
`PreviewRoot`) funnels through `setAst` → (iframe `postMessage` SET_AST) → the
**single** `handleSetAst` in `hub-client/.../ReactPreview.tsx`. That is the one
place all three outcomes are distinguishable:

```
applyNodeEdit OK → newQmd === renderedContent ? SPURIOUS : CHANGE
parse fail / applyNodeEdit throw           → ERROR
```

So the indicator **hooks the funnel, not the sources**: classify in
`handleSetAst`, store one status state, render one overlay. The bulb lives in
`ReactPreview`'s own `position:relative` container (it already hosts
`PreviewErrorOverlay`), so it is a parent-side overlay on the preview iframe —
**no new cross-boundary messaging**. The spurious case is the only real
behavior change (previously `onContentRewrite` was called unconditionally; the
no-op write is harmless but was invisible).

### Placement decision (user)

- The bulb sits in the **bottom-right corner**, the same corner as the
  collapsed `PreviewErrorOverlay` pill.
- On **error**, the bulb is **suppressed** and the **error pill takes over the
  corner** (the pill "replaces" the bulb). The pill keeps its **original look**
  (the shared `PreviewErrorOverlay` is *not* modified).
- The bulb is **transient** (no persistence): idle → opacity 0.

### Chosen implementation (`ReactPreview.tsx`) — VERBATIM

**1. Import** (top of file): add `import type { CSSProperties } from 'react';`.

**2. Module-level bulb component** (above `export default function ReactPreview`):

```tsx
/**
 * G22: commit-status indicator — a translucent "glass" status dot in the
 * preview's bottom-right corner that glows out of the page and fades to nothing.
 *
 *   pending  → amber  (commit going out)
 *   change   → green  (real diff written)
 *   spurious → blue   (no-op round-trip — the false-dirty bug class)
 *   error    → (not lit here; the error pill takes over the corner)
 *
 * Modern / non-skeuomorphic: no bezel, no fixture, no persistence. The dot is
 * defined by LIGHT not by a housing — a colored radial bloom, a thin colored
 * glass rim, and a `backdrop-filter` refraction — so it reads on a white page
 * now and on a dark background later. Idle → opacity 0 (gone). Lit states bloom
 * in (scale + fade) and dim back out; only the attention state (blue) pulses.
 */
const COMMIT_BULB_CSS = `
@keyframes q2-bulb-pulse {
  0%, 100% { opacity: 0.84; }
  50% { opacity: 1; }
}
.q2-commit-bulb {
  position: absolute;
  /* Sits in the bottom-right corner where the error pill appears, so on error
     the pill visually replaces the bulb. */
  bottom: 20px;
  right: 20px;
  width: 13px;
  height: 13px;
  border-radius: 50%;
  background: radial-gradient(circle at 50% 45%, var(--c-bright) 0%, var(--c-soft) 58%, transparent 100%);
  border: 1px solid var(--c-rim);
  backdrop-filter: blur(3px) saturate(1.4);
  -webkit-backdrop-filter: blur(3px) saturate(1.4);
  box-shadow: 0 0 var(--c-glow-blur) var(--c-glow-spread) var(--c-glow);
  opacity: var(--bulb-opacity);
  transform: scale(var(--bulb-scale));
  transition:
    opacity 300ms ease,
    box-shadow 220ms ease,
    background 220ms ease,
    border-color 220ms ease,
    transform 300ms cubic-bezier(0.2, 0.7, 0.3, 1.3);
  pointer-events: none;
  z-index: 50;
  will-change: opacity, transform;
}
/* Gentle, slow breath — only the attention state (blue) pulses. */
.q2-commit-bulb[data-pulse="1"] { animation: q2-bulb-pulse 1.5s ease-in-out infinite; }
`;

function CommitStatusBulb({
  status,
}: {
  status: 'idle' | 'pending' | 'change' | 'spurious' | 'error';
}) {
  // RGB triples + per-state "loudness" — green (a normal change) is quiet; blue
  // (spurious) glows harder, pulses, and lingers so it pulls the eye. Derived
  // from one hue so it carries to light and dark backgrounds.
  //
  // 'error' is intentionally NOT lit here: on error the bulb hands off to the
  // error pill (PreviewErrorOverlay), which appears in the same corner — see the
  // render below where the bulb is suppressed while a commit error is showing.
  const PALETTE: Record<
    'pending' | 'change' | 'spurious',
    { rgb: string; glowA: number; blur: string; spread: string; pulse: boolean; title: string }
  > = {
    pending: { rgb: '255,160,30', glowA: 0.42, blur: '9px', spread: '0px', pulse: false, title: 'Committing…' },
    change: { rgb: '40,200,100', glowA: 0.36, blur: '8px', spread: '0px', pulse: false, title: 'Change committed' },
    spurious: { rgb: '60,140,255', glowA: 0.6, blur: '13px', spread: '1px', pulse: true, title: 'No change (spurious edit)' },
  };
  const on = status === 'pending' || status === 'change' || status === 'spurious';
  const p = on ? PALETTE[status] : null;
  const rgb = p?.rgb ?? '128,128,128';
  return (
    <>
      <style>{COMMIT_BULB_CSS}</style>
      <div
        className="q2-commit-bulb"
        data-pulse={p?.pulse ? '1' : undefined}
        title={p?.title}
        style={
          {
            '--c-bright': `rgba(${rgb},0.92)`,
            '--c-soft': `rgba(${rgb},0.30)`,
            '--c-rim': `rgba(${rgb},0.55)`,
            '--c-glow': `rgba(${rgb},${p?.glowA ?? 0})`,
            '--c-glow-blur': p?.blur ?? '0px',
            '--c-glow-spread': p?.spread ?? '0px',
            // No persistence: idle fades to nothing and shrinks slightly so lit
            // states bloom back in.
            '--bulb-opacity': on ? 1 : 0,
            '--bulb-scale': on ? 1 : 0.55,
          } as CSSProperties
        }
      />
    </>
  );
}
```

**3. State + helpers** (inside `ReactPreview`, after the `renderTimeoutRef` /
`lastContentRef` refs):

```tsx
  // G22: commit-status indicator. Every commit channel (text / subtree /
  // nesting) funnels through handleSetAst, so we classify the outcome there and
  // drive ONE bulb overlay from a single status state — no per-site plumbing.
  const [commitStatus, setCommitStatus] = useState<
    'idle' | 'pending' | 'change' | 'spurious' | 'error'
  >('idle');
  const commitStatusTimerRef = useRef<number | null>(null);
  const commitPendingSinceRef = useRef<number>(0);
  // G22: the message for a rejected commit, surfaced in the existing
  // PreviewErrorOverlay. Persists until the next SUCCESSFUL commit.
  const [commitError, setCommitError] = useState<string | null>(null);

  // Light the bulb amber when a commit goes out.
  const beginCommitStatus = useCallback(() => {
    if (commitStatusTimerRef.current !== null) clearTimeout(commitStatusTimerRef.current);
    commitPendingSinceRef.current = Date.now();
    setCommitStatus('pending');
  }, []);

  // applyNodeEdit is synchronous, so hold 'pending' a minimum so it is visible,
  // then flip to the result colour, then auto-clear to idle.
  const settleCommitStatus = useCallback(
    (result: 'change' | 'spurious' | 'error') => {
      const MIN_PENDING_MS = 200;
      // Loudness via dwell time: a normal change blinks briefly; a spurious /
      // rejected commit lingers longer so it draws the eye.
      const HOLD_MS = { change: 450, spurious: 1100, error: 1700 }[result];
      const elapsed = Date.now() - commitPendingSinceRef.current;
      const delay = Math.max(0, MIN_PENDING_MS - elapsed);
      if (commitStatusTimerRef.current !== null) clearTimeout(commitStatusTimerRef.current);
      commitStatusTimerRef.current = window.setTimeout(() => {
        setCommitStatus(result);
        commitStatusTimerRef.current = window.setTimeout(() => {
          setCommitStatus('idle');
          commitStatusTimerRef.current = null;
        }, HOLD_MS);
      }, delay);
    },
    [],
  );

  useEffect(() => () => {
    if (commitStatusTimerRef.current !== null) clearTimeout(commitStatusTimerRef.current);
  }, []);
```

**4. Classification in `handleSetAst`** (the `pipelineKindForFormat(format) ===
'preview'` branch). After the `untransformedAstJson` guard, before the `try`:

```tsx
        // G22: a commit is going out — light the bulb amber.
        beginCommitStatus();
```

Parse-fail branch:

```tsx
            if (!parseResult.success || !parseResult.ast) {
              console.error('parse_qmd_content failed:', parseResult.error);
              settleCommitStatus('error'); // G22: parse rejected
              setCommitError(`Edit could not be parsed: ${parseResult.error ?? 'unknown error'}`);
              return;
            }
```

After `applyNodeEdit` succeeds, before `onContentRewrite`:

```tsx
          // G22: a commit that round-trips to identical QMD is a SPURIOUS edit
          // (the false-dirty bug class) — surface it instead of silently writing.
          settleCommitStatus(
            newQmd === rendered.renderedContent ? 'spurious' : 'change',
          );
          setCommitError(null); // G22: a successful commit clears the prior error
          onContentRewrite(newQmd);
```

`catch (err)` branch:

```tsx
        } catch (err) {
          console.error('apply_node_edit failed:', err);
          settleCommitStatus('error'); // G22: apply rejected
          setCommitError(`Edit could not be applied: ${err instanceof Error ? err.message : String(err)}`);
        }
```

Add `beginCommitStatus, settleCommitStatus` to the `handleSetAst` `useCallback`
dependency array.

**5. Render** — bulb (suppressed while the error pill shows) + overlay wiring.
First child of the outer `<div>`:

```tsx
      {/* Bulb lives in the same bottom-right corner as the error pill; while a
          commit error is showing, the pill replaces the bulb (bulb → idle). */}
      <CommitStatusBulb status={commitError ? 'idle' : commitStatus} />
```

And extend the existing `PreviewErrorOverlay` props (render error still takes
precedence; otherwise the commit error shows):

```tsx
      <PreviewErrorOverlay
        error={currentError ?? (commitError ? { message: commitError } : null)}
        visible={previewState === 'ERROR_FROM_GOOD' || commitError != null}
        collapsed={errorOverlayCollapsed}
        onToggleCollapsed={setErrorOverlayCollapsed}
      />
```

### Testability refactor (do this in the clean-slate rebuild)

The spurious-vs-change decision is the **load-bearing** behavior (it is the
bug-detector). Extract the inline ternary into a tiny exported pure helper so it
is bindable without mounting the component or WASM:

```tsx
// Exported for unit test. SPURIOUS = the edit round-tripped to identical QMD.
export function classifyCommitOutcome(
  newQmd: string,
  renderedContent: string,
): 'change' | 'spurious' {
  return newQmd === renderedContent ? 'spurious' : 'change';
}
```

…and call `settleCommitStatus(classifyCommitOutcome(newQmd, rendered.renderedContent))`.

### Test plan (TDD-first / fail-on-revert) — hub-client vitest

- **(a) Spurious classification — PRIMARY binding.**
  - Tier: pure unit (vitest). File: new
    `hub-client/src/components/render/commitStatus.test.ts`.
  - Real unit: `classifyCommitOutcome` (exported from `ReactPreview.tsx`).
  - Assertion: `classifyCommitOutcome('x','x') === 'spurious'` and
    `classifyCommitOutcome('a','b') === 'change'`.
  - **Named revert:** drop the `=== renderedContent` check (always return
    `'change'`). **Predicted RED:** spurious case `expected 'spurious', got
    'change'`.
- **(b) Bulb error-handoff — SECONDARY binding.**
  - Tier: jsdom component render (vitest + `@testing-library/react`). File:
    same `commitStatus.test.ts` or a sibling.
  - Real unit: `CommitStatusBulb` (export it for the test).
  - Assertions: `status="spurious"` → the `.q2-commit-bulb` has
    `data-pulse="1"` and `--bulb-opacity: 1`; `status="error"` and
    `status="idle"` → `--bulb-opacity: 0` (the bulb is OFF; the pill owns error).
  - **Named revert:** add `error` back into the lit set / `PALETTE` (make
    `on` include `'error'`). **Predicted RED:** error case `expected opacity 0,
    got 1`.
- **Accepted-untested (live-tuned — no correctness assertion possible):** all
  bulb **visual + timing constants** — colors (`rgb` triples), `glowA`, `blur`,
  `spread`, the pulse keyframe (`1.5s`, `0.84↔1`), `HOLD_MS` (`450/1100/1700`),
  `MIN_PENDING_MS` (`200`), size (`13px`), corner offset (`bottom/right: 20px`),
  the transition curve. These were tuned live against the browser; recorded
  verbatim above. Rationale: visual/temporal, not behavioral.
- **Accepted-untested (integration-deferred):** the overlay-visibility wiring
  (`previewState === 'ERROR_FROM_GOOD' || commitError != null`) and the
  bulb-suppressed-while-error render (`commitError ? 'idle' : commitStatus`) —
  both are JSX wiring whose unit test requires mounting `ReactPreview` with WASM
  mocks. The mechanism is pinned by (a)+(b); a full mount test is deferred.

### Live-tuned constants (cannot be revert-tested — recorded for rebuild)

| Constant | Value | Note |
|---|---|---|
| pending colour | `rgb 255,160,30` (amber) | |
| change colour | `rgb 40,200,100` (green) | quietest: `glowA 0.36`, blur `8px`, no pulse |
| spurious colour | `rgb 60,140,255` (blue) | louder: `glowA 0.6`, blur `13px`, **pulses**, lingers `1100ms` |
| error | red — **not the bulb**; the error pill | |
| glow alpha base | `0.92` bright / `0.30` soft / `0.55` rim/glow | |
| pulse | `1.5s` ease-in-out, opacity `0.84↔1`, **blue only** | softened from an earlier 1.1s/0.65↔1 |
| dwell (`HOLD_MS`) | change `450` / spurious `1100` / error `1700` | loudness via dwell time |
| min pending | `200ms` | so amber is visible before the result |
| size / position | `13px`, `bottom:20px right:20px` | bottom-right, same corner as the error pill |
| backdrop | `blur(3px) saturate(1.4)` | glassmorphism refraction |

### Two-commit changelog requirement

G22 touches `hub-client/`. Per repo policy, the implementation needs **two
commits**: (1) the code; (2) `hub-client/changelog.md` referencing commit (1)'s
short hash, one user-facing sentence (e.g. "Add a commit-status indicator and
surface block-edit errors in the preview").

### Status

Implemented & confirmed live (green/blue/amber bulb; error → pill replaces bulb;
errors surfaced in `PreviewErrorOverlay`). Visual design accepted by user as
"pretty good." **Clean-slate rebuild DONE (2026-06-19)** — `classifyCommitOutcome`
extracted + exported (testability refactor), `CommitStatusBulb` exported, funnel
classification wired into `handleSetAst`, bulb + overlay render wiring added in
`ReactPreview.tsx`. New test file `commitStatus.test.tsx`: (a) `classifyCommitOutcome`
spurious/change (binding proof: force `'change'` → RED), (b) `CommitStatusBulb`
error-handoff (binding proof: add `'error'` to lit set → RED `--bulb-opacity 0`
vs 1). hub-client `test:ci` 42/589 + 8/66 + 17/113 passing, typecheck clean,
`build:all` succeeded. **Two-commit changelog still pending** (needs commit hash —
done at finishing/commit step).

---

## Clean-slate map & implementation order

### Files touched by the FINAL fixes (the clean slate)

| Glitch | Production file(s) | Test file(s) |
|---|---|---|
| **G20** | `ts-packages/preview-renderer/src/q2-preview/outerBlocks.ts` (`measureLeadingBlockBox`) | `src/q2-preview/s0-list-item-surfaces.integration.test.tsx` |
| **G21** | `ts-packages/preview-renderer/src/q2-preview/outerBlocks.ts` (`refocusTargetForAnchorR0`), `…/PreviewRoot.tsx` (`executeLanding` + import) | `src/q2-preview/outerBlocks.integration.test.ts` |
| **G22** | `hub-client/src/components/render/ReactPreview.tsx` | `hub-client/src/components/render/commitStatus.test.ts` (new) |
| **G22 docs** | `hub-client/changelog.md` | — |

**`dispatchers.tsx` must show NO diff** after Phase C (its only change was the
G20 probe).

### Shared-file sequencing

- **G20 and G21 both edit `outerBlocks.ts`** (different functions:
  `measureLeadingBlockBox` vs the new `refocusTargetForAnchorR0`). Run them
  **sequentially** (G20 then G21), not in parallel, to avoid a merge conflict in
  that file. G21 also edits `PreviewRoot.tsx` (disjoint from G20).
- **G22 is in `hub-client/`** — fully disjoint from G20/G21 (preview-renderer).
  It may run **in parallel** with the G20→G21 chain.

### Suggested implementation order (most-isolated first)

1. **G22** (hub-client, disjoint) — in parallel with the chain below. Includes
   the `classifyCommitOutcome` extraction + tests (a)+(b) + the two-commit
   changelog.
2. **G20** (`outerBlocks.ts` `measureLeadingBlockBox`) — first half of the
   shared-file chain.
3. **G21** (`outerBlocks.ts` `refocusTargetForAnchorR0` + `PreviewRoot.tsx`) —
   after G20 lands in `outerBlocks.ts`.

### Phase D verification

- Capture **real baseline** counts on the clean tree first
  (`npx vitest run` and `npx vitest run --config vitest.integration.config.ts`
  in `ts-packages/preview-renderer`; `npm run test:ci` in `hub-client`), then
  verify by **deltas**, not absolute numbers.
- Whole-round gate: both preview-renderer suites + `npx tsc --noEmit`; and since
  hub-client changed, `cd hub-client && npm run build:all && npm run test:ci &&
  npm run typecheck` (the WASM leg in `build:all` is the stricter gate).
- Confirm the changed-file set equals this map exactly — no strays (especially:
  `dispatchers.tsx` clean).
