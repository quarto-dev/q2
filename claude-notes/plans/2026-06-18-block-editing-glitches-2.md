# Block-editing UI glitches — round 2 (fixes & tests)

**Date:** 2026-06-18
**Branch:** `feature/block-editing-improvements` (worktree `.worktrees/block-editing`)
**Status:** READY TO IMPLEMENT — **clean-slate**. Every fix below was diagnosed and
**live-validated** on the running dev server during the 2026-06-18 session, then
captured here as the **independent source of truth** so it can be rebuilt from
scratch on a clean worktree under TDD. Continues the glitch namespace of
`2026-06-16-block-editing-ui-glitches.md` (which ended at G13); this round is
**G14–G18** (all validated in the exploratory pass). G18 also opens a deferred **Layer 2**
(an audit of spurious "dirty" writes), tracked separately — see its section.

> **⚠️ CLEAN-SLATE — treat the working tree as empty of these changes.** Each item
> carries its root cause, the **verbatim** fix (exact code where it is subtle), and
> a bound **fail-on-revert** test. A fresh agent implements the whole set from this
> plan: for each glitch write the bound test (RED per its named revert hunk), apply
> the fix, GREEN. The designs are settled — do **not** re-litigate them; the live
> validation already happened. The only live-tuned values (G15's `EDITOR_FONT_SIZE`
> = `0.825em`) are recorded inline.
>
> **Method.** Lowest-faithful-tier test, fail-on-revert (RED on the named hunk,
> GREEN with the fix). All cited production files live under
> `ts-packages/preview-renderer/src/q2-preview/` unless a path is given — **G17 is
> under `hub-client/`.** See *Clean-slate map & implementation order* at the foot of
> this plan for the full file list and the suggested sequence.

## Glitch index (checklist)

- [x] **G14 — reland-fade (G9 blur) sometimes doesn't go away.** The "blur the
      cell you left" effect on a dirty nest-in/out could stick on an unrelated
      cell. Fixed by resetting the sticky source ref (A), unifying the fade with
      the settle-gate lifecycle (B), and a 1 s watchdog fallback (D). **VALIDATED:
      reset bound fail-on-revert; B behavior-neutral; full preview-renderer
      suite green (445 unit / 458 integration) + typecheck clean.**
- [x] **G15 — a one-line editing surface inflates to two lines on expand.**
      Typing / second-click / horizontal-arrow on a single-line block (commonly a
      list item) grew the editor to two lines. Root cause: the textarea's default
      `rows=2` polluted the §7 autosize. Fixed at the source with `rows={1}`,
      paired with a small editor font reduction (`0.9em → 0.825em`) to kill the
      residual first-keystroke grow. **VALIDATED (live-validated 2026-06-18); browser
      tier — jsdom-invisible. Unit/integration unaffected (445 / 458) + typecheck
      clean. e2e binding: see Test plan.**
- [x] **G16 — down-arrow "caught" in a blockquote-wrapped loose list.** ArrowDown
      from a block re-landed on the block you were editing instead of stepping to
      the next one; up-arrow worked, and the same list *outside* a blockquote
      worked. Root cause: a block's source range absorbs the trailing `> `
      blockquote continuation line, which `surfaceLineSpan` could not trim
      (whitespace-only), so the trimmed span bled past its content. Fixed by
      making `surfaceLineSpan` span only lines that carry visible content
      (treating `>` as a non-content marker). **VALIDATED: bound fail-on-revert at the
      pure-unit tier (6 G16 tests); nestingNav 127 / preview-renderer 452 unit /
      458 integration green; typecheck clean. Downstream workaround for the deeper
      pampa over-absorption — see `2026-06-18-block-range-gutter-tightness.md`.**
- [x] **G17 — nesting cursor ON by default (settings).** The `unlockNestingCursor`
      preference shipped default-off (P3.2); the feature is now mature, so flip the
      default to **on**. Touches `hub-client/` (the preference schema + one
      consumer fallback + the backward-compat schema test). **VALIDATED
      (2026-06-18): schema + gating tests green (9), hub-client typecheck clean.**
- [x] **G18 — robust activation & nesting (the "two-click → dead nesting" bug).**
      Clicking between items could need two clicks to activate AND leave nest
      chords + breadcrumbs dead (a stale `pendingLandingRef` from a no-op commit's
      deferred reland tripped the re-entrancy guard). **Layer 1 VALIDATED:** activate B
      directly instead of deferring to a reland, + a clear-on-open invariant — so
      both symptoms are impossible by construction, regardless of upstream cause.
      Live-validated; p2-4d rewritten (fail-on-revert proven). **Layer 2
      (deferred):** the spurious-dirty *trigger* (false-dirty on nested blocks →
      null commit) merits its own audit pass — **now G19.**
- [x] **G19 — spurious "dirty" detection on a clean nested block (the G18
      Layer-2 audit).** `handleClickSwitchBlur` compared the draft against the
      **raw** `et.anchorSlice` while the four other dirty checks use the
      canonical clean-buffer baseline (`seededDraft ?? anchorSlice`). For a
      nested block the seeded clean buffer (`"oh"`) ≠ the raw slice
      (`"> oh\n>"`), so an **untouched** editor read as dirty and a click-switch
      committed **byte-identical** content (a "null commit"). Fixed by extracting
      one `editBaseline(et)` / `isDirty(draft, et)` pair (in `outerBlocks.ts`)
      and routing **all five** comparison sites through it so no site can drift
      to the raw slice again. **VALIDATED (spike, 2026-06-18):** headline
      false-dirty null-commit bound at the jsdom-integration tier (fail-on-revert
      proven — reverting only the `:1087` baseline source reddens it);
      preview-renderer **452 unit / 456 integration** (455 + the new spike)
      green; typecheck clean; full suite unchanged by the four behaviour-neutral
      re-routings. Pampa source-range work is **not** required (verdict below).

---

## G14 — reland-fade (G9) sometimes doesn't clear

### Symptom

The G9 reland-fade — a ~0.1 s blur applied to the outgoing cell during the
deterministic settle-gate gap on a **dirty nest-in / nest-out** — sometimes does
**not** go away: a stale blur lingers, occasionally on a cell unrelated to the
move.

### How G9 works (the three moving parts)

The fade is **imperative DOM-class mutation outside React's control**:

- **Arm** (`commitAndArmReland`, the dirty nest/crumb chokepoint): snapshots
  `preCommitContentRef` (settle-gate) **and** records `fadeSourceR0Ref =
  et.anchorR0` (the cell being left).
- **Apply** (a `useLayoutEffect` keyed on `[editTarget]`): on the editor-close
  render (`editTarget === null` + a pending landing), scans **all**
  `[data-block-pool-id]` and adds `q2-reland-fade` to the element whose pool
  entry has `r[0] === fadeSourceR0Ref`. CSS is `0.1s ease-out **forwards**`, so
  once applied it *holds* until the class is physically removed.
- **Clear** (`clearRelandFade`): removes the class. Originally called from only
  two sites — the top of `openEditTarget` (land) and `cancelPendingLand` (abort).

### Root cause — two leak vectors (confirmed by code trace)

The fade was cleared **only** by those two sites, and `forwards` means it never
self-reverts, so any reland-conclusion that bypasses them leaks the blur:

1. **`fadeSourceR0Ref` was never reset to `null`** — assigned once in
   `commitAndArmReland`, read in the apply effect, never written back. It stayed
   *sticky* across moves.
2. **The apply effect fired for *any* pending landing, and only `intent:'open'`
   landings clear.** `executeLanding`'s `intent:'focus'` branch (a plain close)
   focuses the outer block and returns **without** `openEditTarget` → no
   `clearRelandFade`.

Combined: a dirty nest-out arms `fadeSourceR0Ref` and (pre-fix) left it set; a
*later* normal close stashes a `'focus'` landing, the apply effect re-fires
against the **stale** `r0`, fades whatever cell now sits there, and the focus
landing never clears it → **persistent blur on an unrelated cell.** No exotic
timing needed: "dirty nest move, then close an editor normally."

3. **(Deeper, out of scope.)** A no-op dirty commit whose render never advances
   makes the settle-gate (`executeLanding`, `:793`) defer **forever** →
   `openEditTarget` never runs → fade (and the editor) hang. Tracked as the
   prior plan's documented settle-gate residual; G14 only adds a cosmetic
   fallback for the *blur* (D), not a fix for the land-hang.

### Alternatives explored (recorded)

- **A — minimal patch.** Reset `fadeSourceR0Ref` in `clearRelandFade`; (initially
  also an `intent==='open'` guard in the apply effect — **dropped as redundant**,
  see below).
- **B — unify fade with the settle-gate lifecycle.** One `closeSettleGate()`
  helper that tears down `preCommitContentRef` **and** the fade together, routed
  through *every* reland-conclusion (land, cancel, **and** the focus branch). The
  gate and fade then cannot diverge by construction.
- **C — drive the fade from React state** (`fadingR0`) instead of orphaned DOM
  classes. Most robust (orphaning becomes impossible; survives mid-gap
  re-renders), but threads state through `PreviewContext` + the block dispatcher
  and costs one extra render per gap. **Not taken** — too heavy for one cosmetic
  effect now that the leak is closed by guards.
- **D — self-expiring fallback.** A 1 s watchdog force-clears the blur if nothing
  else does (covers vector 3's land-hang at the *cosmetic* level only).

**Chosen: A (reset only) + B + D.** B was selected as "slightly more principled"
than A alone — see the latent bug it removes, next.

#### The latent (masked) bug B removes

`preCommitContentRef` (the gate's source ref) is the exact twin of
`fadeSourceR0Ref`: armed at the same three commit sites, and likewise left
**stale** by the focus branch (which nulled `pendingLandingRef` but not
`preCommitContentRef`). Unlike the fade, this is **masked today** by two guards:
(i) the gate's reader (`:793`) sits *after* the focus-branch early `return`, so it
never runs on a focus landing; and (ii) every `'open'` landing re-snapshots
`preCommitContentRef` at creation, overwriting any stale value before the reader
runs. So it is a *latent* stale write, not a live bug. B removes it — routing the
focus branch through `closeSettleGate()` — so the code no longer *depends* on
those two masking guards (e.g. a future `'open'` producer that forgets to
re-snapshot would otherwise read a stale gate and wrongly defer a reland, looking
exactly like the vector-3 hang). **B changes no observed behavior** (full suite
unchanged), which is the point: it is defense-in-depth + a single explicit
lifecycle.

#### Why the `intent==='open'` guard was dropped (redundant with the reset)

`fadeSourceR0Ref` is assigned non-null in exactly one place
(`commitAndArmReland`), which immediately stashes an `'open'` landing and closes
the editor; every land/cancel/focus nulls it again via `clearRelandFade`. So a
non-null `r0` in the apply effect **always** belongs to an in-flight nest/crumb
open-reland — an explicit intent-check is redundant, and keeping it would leave
two overlapping guards neither of which a test could redden alone. The reset is
the single load-bearing hunk; the invariant is documented inline in the apply
effect.

### Chosen fix (all in `PreviewRoot.tsx`)

- **A (reset):** `clearRelandFade` now sets `fadeSourceR0Ref.current = null`
  (and cancels the watchdog) before removing the class — done unconditionally so
  the ref never lingers.
- **B (`closeSettleGate`):** new helper
  `closeSettleGate = () => { preCommitContentRef.current = null; clearRelandFade(); }`,
  routed through all three reland-conclusion sites — `openEditTarget` (land),
  `cancelPendingLand` (abort), and **`executeLanding`'s `'focus'` branch** (the
  previously-untouched path, covering all its exits).
- **D (watchdog):** `FADE_WATCHDOG_MS = 1000` + a `fadeTimeoutRef`. The apply
  effect arms it when it fades a cell; `clearRelandFade` cancels it on a normal
  land/cancel. Cosmetic only — it clears the *blur* if the reland hangs (vector
  3); it does **not** un-stick the editor.

### Verbatim changes (clean-slate) — `PreviewRoot.tsx`

```ts
// 1. Module constant, beside RELAND_BACKSTOP_MS:
const FADE_WATCHDOG_MS = 1000;

// 2. Ref, beside fadeSourceR0Ref:
const fadeTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

// 3. clearRelandFade — cancel watchdog + reset the source ref (A) before removing the class:
const clearRelandFade = useCallback(() => {
    if (fadeTimeoutRef.current !== null) {
        clearTimeout(fadeTimeoutRef.current);
        fadeTimeoutRef.current = null;
    }
    fadeSourceR0Ref.current = null;                 // A: ref never lingers
    if (!previewHostRef.current) return;
    previewHostRef.current.querySelectorAll('.q2-reland-fade').forEach((el) => {
        (el as HTMLElement).classList.remove('q2-reland-fade');
    });
}, []);

// 4. NEW helper closeSettleGate (B) — declared after clearRelandFade:
const closeSettleGate = useCallback(() => {
    preCommitContentRef.current = null;
    clearRelandFade();
}, [clearRelandFade]);

// 5. openEditTarget: replace the two lines
//      preCommitContentRef.current = null; clearRelandFade();
//    with:
closeSettleGate();
//    and change its deps array `[clearRelandFade]` → `[closeSettleGate]`.

// 6. executeLanding, FIRST line inside `if (pl.intent === 'focus') {` (covers all its exits):
closeSettleGate();
//    and add closeSettleGate to executeLanding's deps.

// 7. cancelPendingLand: drop `preCommitContentRef.current = null;` and the trailing
//    `clearRelandFade();`, replace with `closeSettleGate();`; deps → `[closeSettleGate]`.

// 8. The G9 apply useLayoutEffect (keyed on [editTarget]): drop the redundant
//    intent guard, track whether a cell was faded, and arm the watchdog:
const r0 = fadeSourceR0Ref.current;
if (r0 === null) return;                            // A: the ref IS the whole guard
if (!previewHostRef.current) return;
const pool = poolRef.current;
let faded = false;
previewHostRef.current.querySelectorAll<HTMLElement>('[data-block-pool-id]').forEach((el) => {
    const pid = Number(el.getAttribute('data-block-pool-id'));
    const entry = pool[pid] as { r: [number, number] } | undefined;
    if (entry?.r[0] === r0) { el.classList.add('q2-reland-fade'); faded = true; }
});
if (faded) {                                        // D: watchdog
    if (fadeTimeoutRef.current !== null) clearTimeout(fadeTimeoutRef.current);
    fadeTimeoutRef.current = setTimeout(() => {
        fadeTimeoutRef.current = null;
        clearRelandFade();
    }, FADE_WATCHDOG_MS);
}
```

(`commitAndArmReland` is unchanged — it still sets `fadeSourceR0Ref.current =
et.anchorR0` and `preCommitContentRef.current = renderedContentRef.current`. That
remains the *only* site that arms the fade, which is what makes the A invariant —
"non-null `r0` ⇒ in-flight open-reland" — hold.)

### Test plan (TDD-first / fail-on-revert)

| # | Tier | Real unit mounted | Seam · assertion | Named revert hunk → RED |
|---|------|-------------------|------------------|--------------------------|
| **G14-1** | jsdom integration | `PreviewRoot` reland-fade reset (A) | **Added to `g9-reland-fade.integration.test.tsx`.** Dirty nest-out → settled rerender lands on the blockquote (fade cleared, ref reset). Then **plain blur** the blockquote editor (→ a `'focus'` landing). **Assert IMMEDIATELY (no timer advance): no `.q2-reland-fade` anywhere** — binds the reset, not B's focus-branch clear. | Restore the sticky ref (remove `fadeSourceR0Ref.current = null` from `clearRelandFade`) → the plain-close apply effect re-fades pool[1] (stale r0=2) → `expected 0, got 1` → **RED** (proven 2026-06-18; T7 stays green). |
| **G14-T7** (pre-existing) | jsdom integration | G9 apply/clear with a nested source | Unchanged — still green under A+B+D. | (its three original hunks) |

> **Mock boundary:** `getBoundingClientRect` on `[data-block-pool-id]` tiles
> (the `g9-reland-fade` harness mocks rects so blocks read as visible) +
> `PointerEvent.pointerType` forced via `defineProperty`. The unit under test
> (`PreviewRoot` fade lifecycle) is real.
>
> **Exercise precondition (else G14-1 is vacuous).** The dirty nest-out MUST arm
> `fadeSourceR0Ref` (= `et.anchorR0` of the cell left) before the plain blur. If
> it does not, the reverted code's apply effect reads `r0 === null` and
> early-returns → 0 fades → no RED. The 2026-06-18 proof (`expected 0, got 1`)
> confirms the nest-out armed it; the fixture must keep a pool entry whose
> `r[0] === 2` present at plain-blur time (the cell the stale `r0` would re-fade).

**Accepted-untested, with rationale:**
- **B (focus-branch gate teardown)** — masked today (see above); no observable
  behavioral delta, so a test would be vacuous. Bound only by the invariant
  argument + the full suite staying green.
- **D (1 s watchdog visual)** — the *blur clearing after 1 s* is animation/timing
  at the browser tier; the no-op-commit hang that triggers it is the prior plan's
  untested residual. Cosmetic; accepted-untested.

### Status

- A + B + D in `PreviewRoot.tsx`. G14-1 bound fail-on-revert.
- Live-validated on the dev server (vectors 1+2 fixed; D fallback confirmed).
- `npm run typecheck` clean; preview-renderer **445 unit / 458 integration**
  (1 pre-existing skip) green.
- **Still to do before commit:** full `cargo xtask verify` is unnecessary (no
  Rust / WASM change); but `hub-client` build + `npm run build:all` should be run
  since it bundles `preview-renderer` from source. Changelog: this lives under
  `ts-packages/`, not `hub-client/`, so the hub-client changelog rule is
  borderline — note the fix there if the eventual commit touches `hub-client/`.

---

## G15 — one-line editing surface inflates to two lines on expand

### Symptom

Activate a **single-line** block (commonly a list item) and then do anything that
expands it — **type** a character, **second-click** (G11 expand-on-interest), or
**arrow left/right** — and the editor grows to **two lines** despite there being
only one line of content. A separate, smaller residual: even before expanding, a
freshly-activated one-line editor was a touch too short for its monospace text, so
it grew slightly on the first keystroke ("the dreaded 1-line expanding textarea").

### Why the existing "special case" didn't cover it

There is **no** explicit "don't expand single-line list items" guard. What looked
like one is incidental: a bare ArrowUp/Down on a single visual line is
`arrowOnEdge` → a §7 leave-key → it *steps off the surface* (`requestMove`) instead
of expanding (this is the G3 `isOnLastVisualLine` behavior). So **only** arrow
up/down dodged the bug — by never reaching the expand path. Every other expand
gesture (typing `dispatchers.tsx:419`, second-click `:341`, horizontal arrows —
none are leave-keys) calls `setExpanded(true)` and hits the bug. Hence "not general
enough."

### Root cause (sizing model — confirmed by code trace)

The §7 autosize effect (`dispatchers.tsx` `useLayoutEffect`, the expanded branch):

```ts
ta.style.height = 'auto';
ta.style.height = `${Math.max(contentHeight, ta.scrollHeight)}px`;
```

The textarea had **no `rows` attribute**, so HTML's default `rows=2` applied. Setting
`height:'auto'` therefore resolved the textarea to its **two-row** intrinsic height;
and per spec, when content fits without scrolling `scrollHeight === clientHeight`, so
for one line of content `ta.scrollHeight` read the **two-row box**, not the single
text line. `Math.max(contentHeight ≈ 1 line, scrollHeight ≈ 2 rows)` = **2 rows**.
The `contentHeight` floor (the original rendered line) was correct; the polluted
`scrollHeight` term was the bug. Not list-specific — any single-line block was
affected; lists merely make single-line content the common case.

### Chosen fix (both in `dispatchers.tsx`, paired)

1. **`rows={1}` on the editor textarea** — the principled root-cause fix (in the
   spirit of G14's B: remove the cause, don't compensate). It makes the textarea's
   intrinsic / `height:'auto'` baseline **one** line, so the autosize grows *from*
   one line to fit content and never the reverse. Fixes every expand gesture and
   every block type at the source; the arrow-up/down step-off remains correct on
   its own merits but is no longer load-bearing for sizing. (Equivalent imperative
   alternative — reset `height='0px'` before reading `scrollHeight` — was rejected
   as compensating-not-curing.)
2. **Editor font `0.9em → 0.825em`** (extracted to `EDITOR_FONT_SIZE`) — kills the
   *residual* first-keystroke grow. The editor is sized to `contentHeight` (the
   rendered line box, proportional font) but renders source in `monospace`; at
   `0.9em` the monospace line box was a hair taller than the rendered line, so the
   `max(contentHeight, scrollHeight)` picked `scrollHeight` the instant it expanded.
   Tuning the monospace line box to sit just **below** the rendered line makes `max`
   stay at `contentHeight` → no grow. **Live-tuned 2026-06-18: 0.9 → 0.85 → 0.825**
   (user chose 0.825). `caretGeometry`'s measurement mirror copies the textarea's
   *computed* font size (`caretGeometry.ts:57`), so `isOnLastVisualLine` /
   `isOnFirstVisualLine` track this automatically — no second edit.

The two are **paired**: `rows={1}` removes the gross 2-line inflation; the font
reduction removes the fine 1-px-ish grow. Either alone leaves a visible artifact.

### Test plan (TWO tiers — jsdom binding + deferred browser confirmation)

The *visual* 2-line inflation is jsdom-invisible (jsdom reports
`scrollHeight === clientHeight === 0` and has no font metrics, so the autosize
effect is vacuous there — a **Playwright-only** symptom, like G3/G12/T15/T21).
But the **structural fix itself (`rows={1}`) IS mechanically bindable at the
jsdom tier** as a fail-on-revert, because `rows` is a reflected IDL attribute:
`textarea.rows` reads `1` with the fix and the HTML default `2` without it. We
take that as the **binding** (per the "a simpler methodology than a hard
fail-on-revert is fine when the symptom is browser-tier" decision, 2026-06-18),
and keep the Playwright height assertion as **optional real-engine confirmation**.

| # | Tier | Real unit | Seam · assertion | Named revert hunk → RED |
|---|------|-----------|------------------|--------------------------|
| **G15-0** (binding) | jsdom integration | `EditTextarea` mount | **In `useEditableBlock.integration.test.tsx`.** Mount the editor textarea; assert `ta.rows === 1`. | Remove `rows={1}` → `ta.rows === 2` (HTML default) → `expected 1, got 2` → **RED**. Mechanical, no layout engine needed. |
| **G15-font** (mandatory edit, NOT new coverage) | jsdom integration | same file | **Loosen** the existing strict assertion `expect(ta.style.fontSize).toBe('0.9em')` → `expect(ta.style.fontSize).toMatch(/^[0-9.]+em$/)`. | n/a — this is a *required* edit, not a binding: once `EDITOR_FONT_SIZE='0.825em'` ships, the strict `'0.9em'` assertion **breaks**. Loosening keeps it green without pinning the tuned value. |
| **G15-1** ⏳ deferred/optional | Playwright (real layout) | `EditTextarea` sizing on expand | **Expand `hub-client/e2e/q2-preview-item-edit-size.spec.ts`.** Activate a **single-line** list item; trigger expand (type a char / second-click); **first assert the editor actually entered the expanded state**, THEN assert its height stays ≈ one line, NOT ~2×. | Remove `rows={1}` → `height:'auto'` resolves to the 2-row default → editor expands to ~2 lines → height assertion **RED**. |

> **Why G15-0 is the binding, not G15-1.** G15-0 binds the *mechanism* the named
> revert removes (`rows={1}`) at the cheap jsdom tier and reddens deterministically
> on revert — a true fail-on-revert. G15-1 binds the *visual symptom* but needs the
> real layout engine, so it is real-engine confirmation, not the gate.
>
> **Mock boundary (G15-0):** none — `ta.rows` is a plain reflected attribute in
> jsdom; the `EditTextarea` mount is real.
>
> **Path-exercised guard (G15-1 only, mandatory if/when written).** The Playwright
> height assertion is **vacuous unless the expand path actually ran**: if the
> gesture failed to `setExpanded(true)`, the height stays at `contentHeight`
> **whether or not `rows={1}` is present**. The spec therefore requires BOTH:
> (i) an explicit "is expanded" assertion, and (ii) a companion case in the **same
> spec** — a genuinely multi-line item that **does** grow on the same gesture —
> proving the expand+autosize engine is live.

**Accepted-untested, with rationale:**
- **`EDITOR_FONT_SIZE` value (`0.825em`)** — a tuned constant, not a behavior
  branch (same posture as G1's `CRUMB_W` and G13's pill color in the prior plan).
  Reverting a constant can't meaningfully redden a test; the value was chosen by
  live visual validation. G15-0 binds the *structural* fix (`rows={1}`); the font
  size only sharpens the residual (hence G15-font *loosens* rather than pins it).

### Status

- `rows={1}` + `EDITOR_FONT_SIZE = '0.825em'` in `dispatchers.tsx`.
- Live-validated on the dev server 2026-06-18 (single-line items stay one line on
  type / second-click / horizontal arrow; multi-line still grows; arrow up/down
  step-off unchanged).
- `npm run typecheck` clean. The binding is **G15-0** (`ta.rows === 1`, jsdom) +
  the mandatory **G15-font** loosening, both in `useEditableBlock.integration.test.tsx`.
- **G15-1 (Playwright) is deferred/optional** real-engine confirmation, not the
  gate. Same pre-commit notes as G14 (no Rust/WASM; run `hub-client`
  `npm run build:all`; changelog only if the commit touches `hub-client/`).

---

## G16 — down-arrow "caught" in a blockquote-wrapped loose list

### Symptom

Inside a blockquote that wraps a **loose** list, ArrowDown is "caught" — it
re-lands on the block you are already editing instead of stepping to the next
one. Up-arrow works; the same list **outside** a blockquote works. Minimal repro:

```
> and can we actually
>
> 1.  oh
>
> 2.  dear
>
> 3.  > > god
```

Down fails from `and can we actually`, `oh`, `dear`; `god` (last block) is fine.

### Why the existing behaviour didn't cover it

There is no explicit "single-line list item" special case — what *looked* like one
is incidental: a bare ArrowUp/Down on a single visual line is `arrowOnEdge` → a
leave-key → it steps off via `requestMove`. **Up** scans from `L0 − 1` (above the
block) so it never re-hits self; **down** scans from `destLine = L0 +
draftLineCount` (one line *past* a 1-line block) — and that line still fell
*inside* the block's inflated span. Outside a blockquote the blank separators are
true whitespace (fully trimmed), so the span isn't inflated and down works.

### Root cause (byte-level)

Down-nav scans from `destLine` and calls `surfaceAtLine`, which ranks surfaces by
their **trimmed line span** (`surfaceLineSpan`). The old trim stripped only ASCII
whitespace. A loose list-item's node range **absorbs the trailing `> ` blockquote
continuation line**, and `>` is not whitespace, so `trimEnd` stopped at it and the
span bled one line past the content — and *overlapped* the next item's content
line. Parsed from the repro (`pampa -t json --json-source-location full`):

```
L0 > and can we actually   Para  [2,24]   span was [0,1]  → should be [0,0]
L2 > 1.  oh                 Plain [31,39]  span was [2,4]  → should be [2,2]
L4 > 2.  dear               Plain [43,53]  span was [4,6]  → should be [4,4]
```

`oh` (line 2, 1-line draft) → `destLine = 2+1 = 3`; with span `[2,4]`,
`surfaceAtLine(3)` re-resolved to `oh` itself → "caught". (Latent bonus bug:
`surfaceAtLine(4)` — `dear`'s own content line — resolved to `oh` because the
inflated spans overlap.)

### Chosen fix (`nestingNav.ts` `surfaceLineSpan`) — VERBATIM

Span only the lines that carry **visible content**, where a blockquote marker `>`
counts as non-content (like whitespace). One pass over the surface text:

```ts
export function surfaceLineSpan(
  surface: NestingSurface,
  content: string,
  map: ByteLineMap,
): [number, number] {
  const startLine = map.lineOf(surface.r0);
  const endLine = map.lineOf(Math.max(surface.r0, surface.r1 - 1));

  // Span the lines that carry the surface's *visible content*. A content
  // character is anything that is neither whitespace NOR a blockquote marker
  // (`>`). This generalises the old `trimStart`/`trimEnd` (which stripped only
  // whitespace): a node range absorbs the next line's leading indent / blockquote
  // prefix at `r1`, and inside a blockquote that prefix is `> ` — whose `>` a
  // whitespace trim cannot strip, so the span bled one line past its content and
  // overlapped the next item's content line (the loose-list-in-blockquote
  // down-nav "caught" bug). Outside a blockquote there are no `>` markers, so this
  // is byte-for-byte the prior behaviour. One pass over the surface text,
  // counting newlines to track the line.
  let line = startLine;
  let first = -1;
  let last = -1;
  for (const ch of sliceUtf8(content, surface.r0, surface.r1)) {
    if (ch === '\n') { line++; continue; }
    if (!/\s/.test(ch) && ch !== '>') {
      if (first === -1) first = line;
      last = line;
    }
  }

  // Degenerate all-blank surface (only whitespace / `>`): fall back to the raw
  // line span, matching the prior degenerate guard.
  return first === -1 ? [startLine, endLine] : [first, last];
}
```

This is the **downstream workaround**: the real defect is producer-side — pampa
block ranges over-absorb the `> ` gutter. The parser-level fix (extending Plan
7g's tight-range contract to block-leaf trailing ranges) is the research plan
`2026-06-18-block-range-gutter-tightness.md`; when it lands, this `>`-aware trim
can be simplified back out.

### Notes on the function

- **`>` as non-content is safe.** A lone `>` at line start is always a blockquote
  marker; a content `>` (e.g. `a > b`) always co-occurs with other content chars,
  so its line is still marked content. The only false case — a line whose *only*
  content is literally `>` — does not occur as block content in markdown.
- **No behaviour change outside blockquotes.** Without `>` markers, "not
  whitespace and not `>`" == "not whitespace" — byte-for-byte the old trim. The
  §3 and NEST3 `surfaceLineSpan` tests pass unchanged.
- **CRLF-clean.** `/\s/` treats `\r` as whitespace (a newline boundary is not
  content), matching the old `trimStart`/`trimEnd`.

### Test plan (TDD-first / fail-on-revert) — pure unit tier

`surfaceLineSpan` / `surfaceAtLine` are pure (no DOM), so this binds at the unit
tier — no browser needed. Added to `nestingNav.test.ts` as a `G16` block,
following the **established NEST3 / §1 idiom** already in that file: inline
`{ r0, r1 }` surface literals + `buildByteLineMap(BQLOOSE_CONTENT)` (from
`../utils/byteLineMap`, already imported). The offsets below were **derived once
from the real `pampa` binary** and are then hardcoded in the test (exactly like
the NEST3 fixture) — the test does **not** shell out to pampa or inline an AST.

#### Fixture — `BQLOOSE_CONTENT` (VERBATIM — note the trailing spaces)

> **⚠️ CRITICAL: the blank blockquote lines are `> ` (marker + trailing space),
> not `>`.** The printed repro at the top of this section renders the trailing
> space invisibly, but it is load-bearing: with a bare `>` the document is 62
> bytes and the offsets shift (`oh` → `[30,37]`, `dear` → `[41,50]`,
> `BlockQuote` → `[0,62]`), so the hardcoded assertions below would not match.
> Use the escaped string literal exactly:

```ts
// 65 bytes. Blank blockquote lines are "> " (trailing space) — see warning above.
const BQLOOSE_CONTENT =
  '> and can we actually\n> \n> 1.  oh\n> \n> 2.  dear\n> \n> 3.  > > god\n';
```

Derivation command (run once to confirm/regenerate the offsets; not part of the test):

```bash
printf '> and can we actually\n> \n> 1.  oh\n> \n> 2.  dear\n> \n> 3.  > > god\n' > /tmp/bqloose.qmd
pampa -t json --json-source-location full /tmp/bqloose.qmd \
  | jq -r '[.. | objects | select(.t and .l) | "\(.t)\t[\(.l.b.o),\(.l.e.o)]"] | .[]'
# → BlockQuote [0,65]  Para [2,24]  OrderedList [27,65]
#   Plain(oh) [31,39]  Plain(dear) [43,53]  (item-3 nested BlockQuote/Para [57,65]…[61,65])
```

For **G16-at**, the `surfaceAtLine` surface SET is the inline array
`[{r0:0,r1:65} /*BlockQuote*/, {r0:27,r1:65} /*OrderedList*/, {r0:31,r1:39} /*oh*/,
{r0:43,r1:53} /*dear*/]` (add `{r0:57,r1:65}` for the item-3 leaf if exercising
line 6). With the fixed `>`-aware span, line 3's deepest containing surface is the
OrderedList **container** whose only leaf children (`oh`→[2,2], `dear`→[4,4]) do
**not** cover line 3 → `surfaceAtLine`'s A2 container-gap check returns `null`
(verified against the real container structure 2026-06-18).

| # | Real unit | Assertion surface | Named revert → RED |
|---|-----------|-------------------|--------------------|
| **G16-span** | `surfaceLineSpan` | `oh [31,39]` → `[2,2]`; `dear [43,53]` → `[4,4]`; leading Para `[2,24]` → `[0,0]`; outer BlockQuote `[0,65]` → `[0,6]` | Old whitespace-only trim → `oh` → `[2,4]`, `dear` → `[4,6]` → **RED** (proven 2026-06-18). |
| **G16-at** | `surfaceAtLine` (down-nav consequence) | line 3 (between `oh`/`dear`) → `null` (container-gap); line 4 → `dear [43,53]`; line 2 → `oh [31,39]` | Old trim → `surfaceAtLine(4)` returns `oh [31,39]` (the overlap) and `surfaceAtLine(3)` returns `oh` → **RED** (proven: `expected [43,53], received [31,39]`). |

> **Mock boundary:** none — both are pure functions over the `BQLOOSE_CONTENT`
> string + inline `{r0,r1}` surface literals (offsets derived once from the real
> `pampa` binary; see the fixture block above); no DOM, no React, no rects. **Discriminator discipline (check 2):** the two assertions that pass
> either way (content-line `oh`, outer-BlockQuote span) are kept only as **shape
> guards**; the 5 that move the span/resolution one line (`oh`/`dear` spans,
> `surfaceAtLine(3)`/`(4)`) are the discriminators and all redden on the old
> whitespace-only trim.

**Fail-on-revert proven** 2026-06-18: against the old whitespace-only trim, 5 of
the 6 G16 assertions reddened (the two that pass either way — content-line `oh`
and the outer-blockquote span — are kept as guards). GREEN after the fix; §3 +
NEST3 span tests unchanged.

**Optional belt-and-suspenders (not required):** a Playwright spec that ArrowDown
from a blockquote loose-list item activates the next surface. The pure-unit
binding already pins the root cause; e2e would only re-confirm the real render
path. Accepted-untested unless the unit binding ever feels insufficient.

### Status

- `surfaceLineSpan` rewritten (simplified single-pass form) in `nestingNav.ts`;
  6 G16 unit tests in `nestingNav.test.ts`, fail-on-revert proven.
- nestingNav **127** / preview-renderer **452 unit / 458 integration** green;
  typecheck clean. Live-validated on the dev server (down now steps correctly;
  up + outside-blockquote unchanged).
- No Rust/WASM change; same pre-commit notes as G14.

---

## G17 — nesting cursor ON by default (settings)

### Change

The `unlockNestingCursor` preference shipped **default-off** (P3.2) because the
nesting-cursor feature was new and it gates the `regenerateNestedBuffers` WASM
pass. The feature is now mature; flip the default to **on**. New / unset
preferences get nesting-cursor on; users who explicitly saved `false` keep it
(standard default-change semantics). The `SettingsTab` toggle then renders
default-checked with no change to that component.

> **Implication (intended).** Default-on means the `regenerateNestedBuffers` WASM
> pass — previously unreachable when off — now runs for **every** render. This is
> the deliberate cost of shipping the feature on; if it ever profiles as a hotspot
> that's a separate optimisation, not a reason to default it off.

### Verbatim changes (clean-slate) — all under `hub-client/`

```ts
// hub-client/src/services/preferences/schema.ts
// (a) the zod schema default:
unlockNestingCursor: z.boolean().default(true),     // was .default(false)
// (b) the DEFAULT_PREFERENCES object:
unlockNestingCursor: true,                          // was false

// hub-client/src/components/render/ReactPreview.tsx
// the computeNestedEditBuffers fallback when the pref is undefined:
unlockNestingCursor ?? true,                        // was ?? false
```

(Update the two `default-off` doc comments — `schema.ts` field doc and
`ReactPreview.tsx:309` — to `default-ON`.)

### Test (update the existing backward-compat test)

`hub-client/src/services/preferences/schema.test.ts` has a regression test
("preserves other settings when `unlockNestingCursor` is absent") that fills a
missing key with the schema default. Flip its expectation:

```ts
// was: expect(result.unlockNestingCursor).toBe(false);
expect(result.unlockNestingCursor).toBe(true);
```

**Named revert hunk (it IS revert-checkable, despite being a default pin):**
revert `schema.ts` `unlockNestingCursor: z.boolean().default(true)` → `.default(false)`
⇒ the schema fills the absent key with `false` ⇒ `expect(result.unlockNestingCursor).toBe(true)`
**RED**. So this row binds the **schema default** specifically. The load-bearing
half of the test (other settings preserved when the key is absent) is unchanged.
`p3-2-gating.test.tsx` needs no change — it drives the flag explicitly, not via
the default.

**Missing-test pass (check 3): two parallel production flips this test does NOT cover.**
The schema-test only exercises the **zod schema** default. G17 also flips:
- `DEFAULT_PREFERENCES.unlockNestingCursor: false → true` (the literal used when
  no stored prefs object exists at all — a different code path from "key absent
  in an existing object").
- `ReactPreview.tsx` `unlockNestingCursor ?? false → ?? true` (the runtime
  fallback when the pref is `undefined` at the consumer).

**One further site — `SettingsTab.tsx:100` `checked={unlockNestingCursor}` —
needs NO code change (verified 2026-06-18).** The toggle reads the value straight
from `usePreference('unlockNestingCursor')` with **no** `?? false` fallback of its
own, so it inherits the new default transitively through the schema flip. It is
recorded here only so a future reader does not mistake its absence from the
3-site edit list for an oversight: the enumeration above is complete for sites
that need a *direct* edit.

These are **accepted-untested** as parallel constant/fallback flips: reverting
the schema default alone reddens the one test above; the other two are the same
default expressed in two more places (a defence-in-depth triple, intentionally
kept in lockstep). A dedicated test for each would assert a constant equals a
constant — vacuous. They are bound by the live default-on validation (clear the
stored pref / fresh profile) recorded in Status, and by hub-client typecheck +
`p3-2-gating` staying green. If they ever drift out of lockstep, that is a real
bug — flagged here so a future reader does not read silence as coverage.

### Status

- Schema default + `DEFAULT_PREFERENCES` + the `ReactPreview` fallback flipped;
  backward-compat test updated. **schema + p3-2-gating tests green (9); hub-client
  `npm run typecheck` clean** (2026-06-18). Live default-on confirmed (clear the
  stored pref / fresh profile to observe, since an explicitly-saved value wins).
- **Pre-commit:** this touches `hub-client/`, so it needs a `hub-client/changelog.md`
  entry (two-commit workflow) and a green `npm run build:all` before the commit.

---


## G18 — robust activation & nesting (the "two-click → dead nesting" bug)

> **A family of bugs across two layers.** **Layer 1** (this section, **VALIDATED**): a
> transient hiccup in the commit/landing machinery could leave you with a block
> that needs two clicks to activate *and* dead nesting keys/breadcrumbs — and that
> should be **structurally impossible no matter what the rest of the system does.**
> **Layer 2** (deferred, *audit*): the specific *triggers* — spurious "dirty"
> detection that commits byte-identical content. Layer 1 makes the UI robust to
> Layer 2; Layer 2 is fixed separately in a second pass (see end).

### Symptom (reported 2026-06-18, fully clean — no edits)

Just clicking between items (no typing): sometimes an item needs **two** clicks to
activate, and when it does, the nest-in/out chords **and** breadcrumbs are dead;
deactivating + reactivating fixes it, and the next activation is a clean one-click.
No clear per-node pattern.

### The two halves, patiently

**The dead-nesting half.** Both nesting entry points bail on one guard —
`if (pendingLandingRef.current !== null) return;` (`requestNestingMove` :1308,
`requestNestingSelect` :1371). A "landing" is the system's note-to-self to *open an
editor* after an async commit re-renders. If that note is ever left **stale** (a
commit that never produces a fresh render, or a reland that can't resolve), the
guard bricks *all* nesting until something clears it. A plain close clears it
(`requestFocusRestore` :1005) — which is why deactivating "fixes" it.

**The double-click half — one physical click is three events.** Clicking B while
A's editor is open fires, in order: `pointerdown(B)` → `blur(A)` → `pointerup(B)`.
The old activation logic split across all three:
- `pointerdown(B)` notices a *different* block is targeted and records "a switch to
  B is pending." It does **not** open B yet.
- `blur(A)` runs `handleClickSwitchBlur`. If A is "dirty," it **commits A**,
  **stashes a landing for B**, sets a flag, and **closes A** — deferring B's open
  to a reland.
- `pointerup(B)` sees that flag and **skips `activate(B)`**, trusting the reland.

So after one click: A is closed, B is *not yet open* — B is supposed to appear via
the reland once A's commit re-renders. **Why defer at all?** A real edit to A
changes the document length and **shifts B's byte offsets**, so B can only be
opened at its *new* position after the commit re-renders; the reland re-finds B by
line. **Why it fails:** when the commit is a no-op (byte-identical — see Layer 2),
the document never changes, so the reland — which *waits* for content that reflects
the commit — waits **forever** (even the 250 ms backstop re-defers under the
settle-gate). B never opens → you click again → the second click activates B
directly → **two clicks.** And the landing stashed for B is left orphaned →
**dead nesting.**

So both halves are the same fragility: **one piece of free-floating async state
(`pendingLandingRef`) gates both activation and nesting, and can be orphaned.**

### Layer-1 fix (architectural — robust by construction)

Two changes, so the failure is impossible regardless of *why* a commit/reland might
misbehave:

**(b) Activate B directly — never defer activation to a reland**
(`PreviewRoot.tsx` `handleClickSwitchBlur`, dirty branch). Commit A fire-and-forget
and close, but do **not** stash a landing or skip `activate`. `onPointerUp`
activates B at its clicked position; **self-heal** re-anchors B after A's commit
round-trips (B's content is unchanged by A's edit, so the re-anchor KEEPs it). No
landing is stashed → none can be orphaned. B opens on the **first** click for every
cause.

```ts
// handleClickSwitchBlur, the `isDirty` branch — replaces the stash-landing +
// arm-backstop + dirtySwitchHandledRef=true block:
clickSwitchRef.current = null;
const destA = buildNestingCommitDestination(et);
if (destA !== null) {
    setAstRef.current({
        __isPreviewNodeEdit: true,
        channel: 'text',
        destinationSourceInfoJson: destA,
        newText: normalizeLineEndings(draft),
    } as unknown as PandocAST);
}
editDraftRef.current = null;
setEditTargetRaw(null);
return true; // committed here; blur must not also focus-restore/commitIfDirty.
            // dirtySwitchHandledRef stays false → onPointerUp activates B.
```

**(clear-on-open invariant)** (`useBlockEditHover.tsx` `activate`). A fresh
activation supersedes any pending landing — the reland paths never reach
`activate`, so a landing present here is orphaned. Clearing it means **while an
editor is open, `pendingLandingRef` is always null → the nesting guard can never
brick nesting**, no matter what upstream left stale.

```ts
// in activate(), right after the same-block dedup, before setEditTarget:
ctx.cancelPendingLand?.();
```

Together these make **both** symptoms impossible by construction: B always opens on
the first click (b), and nesting always works while you're editing (clear-on-open).

### Test changes (clean-slate: these revert + re-apply with the code)

`p2-4d.integration.test.tsx` on a clean tree asserts the *old* reland mechanism, so
G18 rewrites it:
- **Rewrite** the dirty-click-switch test → *"commits A and opens B directly on
  pointerup (no deferred reland)"*: after `pointerup(B)`, assert B's textarea is
  present **immediately** (value `"para2"`), with `setAst` called once for A.
  **Fail-on-revert (proven 2026-06-18):** re-introduce `dirtySwitchHandledRef =
  true` (the old skip) → `pointerup` skips activate → B absent → RED.
- **Delete** the three obsolete cases that tested the removed click-switch
  machinery: the destLine **delta/projection** test and the two **settle-gate
  defers the click-switch** tests. (Click-switch no longer uses a landing or the
  settle-gate.)
- The unmodified-switch, active-region, and empty-area tests are unchanged.

> **Tier · unit · seam · mock boundary · revert (frozen spec).** jsdom
> integration · real `PreviewRoot` (+ real `handleClickSwitchBlur` /
> `onPointerDown`/`Up` / `commitIfDirty`) · mount the 4-tile fixture, fire
> `pointerdown(A)`/`up(A)` → type → `pointerdown(B)` → `blur(A)` → `pointerup(B)`,
> assert B's textarea present with `value==='para2'` and `setAst` called once ·
> mock `getBoundingClientRect` on tiles + `PointerEvent.pointerType` · **revert**
> re-introduce `dirtySwitchHandledRef = true` → `pointerup` skips `activate(B)` →
> B absent (`querySelector('textarea')===null`) → **RED** (proven 2026-06-18).
> **Exercise check:** the mid-sequence `setAst`-called-once assertion proves the
> dirty branch ran (A was committed), so B-absent on revert is the direct-activate
> fix, not a dead path. **Deletion safety (check 3):** the 3 removed cases tested
> the destLine projection + settle-gate-defer of the *old* reland machinery, which
> G18 deletes entirely — no live behaviour is left unguarded by their removal.

**clear-on-open** is **accepted-untested at the unit tier (with rationale):** the
primary fix (b) removes the click-switch stale-landing *at the source*, and a
faithful brick-then-recover test needs the full nesting + failed-reland harness to
manufacture a stale `'open'` landing. It is bound by the invariant argument +
live validation (2026-06-18). Revertable in one line if we later build that harness.

### Layer 2 (deferred — spurious dirty writes, an audit)

The *trigger* in the reported clean scenario: `handleClickSwitchBlur`'s dirty check
(`:1087`) compares the draft against the **raw** `et.anchorSlice`, while every other
dirty check uses the P3.3 clean-buffer-aware `normalizeLineEndings(seededDraft ??
anchorSlice).trimEnd()`. For a nested block the seeded clean buffer (`"oh"`) differs
from the raw slice (`"oh\n> \n> "`), so an **untouched** editor reads as dirty →
a **byte-identical (null) commit**. Layer 1 makes that harmless to the UI, but the
spurious write itself (and any siblings) should go. **This merits its own pass —
an audit of every draft-vs-baseline comparison + whether `anchorSlice` should be
trimmed at capture.** Not done here; open as its own plan/strand when we start
Layer 2. (We thought existing trimming covered this; it doesn't, everywhere.)

### Status

- Layer 1: `handleClickSwitchBlur` direct-activate (b) +
  `activate` clear-on-open. **Live-validated 2026-06-18** (clean clicks activate in
  one click + nesting works *even with the Layer-2 spurious-dirty still present*;
  genuine dirty switch commits A and self-heal re-anchors B).
- p2-4d rewritten (4 tests, was 7); **fail-on-revert proven** for the direct-open
  test. preview-renderer **452 unit / 455 integration** green; typecheck clean.
- Files: `PreviewRoot.tsx`, `useBlockEditHover.tsx`, `p2-4d.integration.test.tsx`.
- Layer 2 (audit) deferred.

---

## G19 — spurious "dirty" detection on a clean nested block (G18 Layer-2 audit)

> **Clean-slate, VALIDATED by spike 2026-06-18.** This is the deferred Layer-2
> audit promised by G18. Layer 1 already makes the UI robust to a null commit;
> G19 removes the spurious write itself. The audit's headline finding is a
> single drifted comparison site; the fix centralises the dirty baseline so the
> drift cannot recur. **Independent source of truth — implement under TDD from
> this section.**

### Symptom (reported 2026-06-18, fully clean — no edits)

Just clicking between items (no typing) on a document with **nested** blocks
(a blockquote- or list-wrapped paragraph) sometimes writes a **byte-identical
commit** — the document does not change, but `setAst` fires with a node-edit
payload whose `newText` equals the existing source. This "null commit" is the
*trigger* behind the G18 two-click/dead-nesting family (Layer 1 made it
harmless to the UI; here we stop it at the source).

### The invariant to enforce

**An untouched editor compares equal to its baseline.** Always compare the
draft against the value the editor was *seeded* with (`seededDraft`), never
against the raw source slice. For a top-level block the two coincide; for a
nested block they diverge and only `seededDraft` is correct.

### Root cause (confirmed by code trace + a live spike)

There are **five** draft-vs-baseline comparison sites in the q2-preview editing
layer. Four already baseline against the clean buffer
`normalizeLineEndings(et.seededDraft ?? et.anchorSlice).trimEnd()`. **One had
drifted** to the raw slice:

| # | Site | File · symbol | Baseline source (before) | Empty-draft policy |
|---|------|---------------|--------------------------|--------------------|
| 1 | commit-on-blur / Cmd-Enter | `dispatchers.tsx` · `EditTextarea` `commitIfDirty` (`:159`/`:277`) | `seededDraft ?? anchorSlice` ✓ | three-way (delete-aware) |
| 2 | arrow step-off | `dispatchers.tsx` · `onKeyDown` arrow branch (`:482`) | `baseline` (= seededDraft ?? anchorSlice) ✓ | empty-from-non-empty = dirty (delete) |
| 3 | **click-switch blur** | **`PreviewRoot.tsx` · `handleClickSwitchBlur` (`:1087`)** | **`et.anchorSlice` ✗ — RAW SLICE** | `!!normalized &&` (empty not dirty) |
| 4 | nest in/out | `PreviewRoot.tsx` · `requestNestingMove` (`:1281`/`:1284`) | `seededDraft ?? anchorSlice` ✓ | `!!draftNorm &&` (empty not dirty) |
| 5 | crumb jump | `PreviewRoot.tsx` · `requestNestingSelect` (`:1343`/`:1346`) | `seededDraft ?? anchorSlice` ✓ | `!!draftNorm &&` (empty not dirty) |

Site 3 is the bug. For a nested block A seeded with clean buffer `"oh"` while
its raw slice is `"> oh\n> \n"` → `anchorSlice = "> oh\n>"`, an untouched editor
has `draft === seededDraft === "oh"`, and `"oh" !== "> oh\n>"` ⇒ `isDirty` true.
`handleClickSwitchBlur` then commits A (`setAst` with `newText: "oh"`,
`destinationSourceInfoJson: {"t":0,"r":[6,14],"d":0}`) and returns `true` — a
**null commit** — instead of falling through to `commitIfDirty`, which (site 1,
canonical) would have correctly read it clean and just closed the editor.

Note the three PreviewRoot sites (3, 4, 5) already share *identical* dirty
semantics (`!!norm && norm !== baseline`); site 3 differed **only** in its
baseline source. Sites 1 and 2 have delete-by-emptying semantics (an emptied
non-empty block is a dirty *delete*), so they keep a three-way / no-`!!` form —
but they still source the baseline from the same expression.

**No other spurious-dirty trigger exists.** The IME path (`onBlur`,
`dispatchers.tsx:378`) gates on `isComposingRef` before any dirty check; the
touch/pointer paths only *activate* (no dirty comparison). The full enumeration
above is the complete set of comparison sites.

### Chosen fix — one baseline, one dirty predicate (all sites route through it)

Extract the canonical baseline and the shared boolean into `outerBlocks.ts`
(which already owns the `anchorSlice` / `seededDraft` contract via
`seedForRange`). This makes the baseline source a **single definition** — the
recurrence of a raw-slice drift becomes impossible by construction.

#### Verbatim helpers — `outerBlocks.ts` (append after `seedForRange`)

```ts
/**
 * The canonical dirty baseline for an open edit target: the value the draft was
 * seeded with at open (`seededDraft`), falling back to `anchorSlice` for
 * non-nested blocks (or pre-P3.3 activation paths that never set `seededDraft`).
 *
 * G19 (Layer 2): this is the SINGLE source of the baseline. For a nested block
 * the raw `anchorSlice` carries the ancestor `> `/indent prefix, while the clean
 * `seededDraft` does not — comparing a draft against the raw slice reads an
 * untouched clean-buffer editor as dirty. Every draft-vs-baseline comparison
 * MUST route through this helper so no site can drift to the raw slice again.
 */
export function editBaseline(et: { seededDraft?: string; anchorSlice: string }): string {
    return normalizeLineEndings(et.seededDraft ?? et.anchorSlice).trimEnd();
}

/**
 * Whether `draft` differs from the edit target's canonical baseline, treating an
 * empty draft as NOT dirty (the policy shared by the click-switch + nesting-move
 * sites). Sites with delete-by-emptying semantics (commitIfDirty, arrow step-off)
 * keep their own three-way empty handling but still source the baseline from
 * `editBaseline`.
 */
export function isDirty(draft: string, et: { seededDraft?: string; anchorSlice: string }): boolean {
    const draftNorm = normalizeLineEndings(draft).trimEnd();
    return !!draftNorm && draftNorm !== editBaseline(et);
}
```

#### Wiring (verbatim) — the five sites

**`PreviewRoot.tsx`** — add `isDirty` to the `./outerBlocks` import. Then:

```ts
// 1. handleClickSwitchBlur (THE BUG, was :1086–1087):
//      const normalized = normalizeLineEndings(draft).trimEnd();
//      const isDirty = !!normalized && normalized !== et.anchorSlice;
//      if (!isDirty) {
//    becomes (the local `normalized` is unused elsewhere — the dirty branch's
//    payload uses normalizeLineEndings(draft) un-trimmed at the existing line):
if (!isDirty(draft, et)) {

// 2. requestNestingMove (was :1280–1284): drop the inline baseline/draftNorm:
//      const baseline = normalizeLineEndings(et.seededDraft ?? et.anchorSlice).trimEnd();
//      const draftSrc = live ? live.draft : (editDraftRef.current ?? '');
//      const draftNorm = normalizeLineEndings(draftSrc).trimEnd();
//      const isDirty = !!draftNorm && draftNorm !== baseline;
//      if (!isDirty) {
//    becomes (draftSrc is still needed later for commitAndArmReland):
const draftSrc = live ? live.draft : (editDraftRef.current ?? '');
if (!isDirty(draftSrc, et)) {

// 3. requestNestingSelect (was :1343–1346): identical transform to #2.
const draftSrc = live ? live.draft : (editDraftRef.current ?? '');
if (!isDirty(draftSrc, et)) {
```

**`dispatchers.tsx`** — add `import { editBaseline } from './outerBlocks';`. Then:

```ts
// 4. EditTextarea baseline (was :159) — pure de-dup, NO behaviour change
//    (the inline expression was already editBaseline's body):
//      const baseline = normalizeLineEndings(ctx.editTarget!.seededDraft ?? anchorSlice).trimEnd();
const baseline = editBaseline(ctx.editTarget!);
// `baseline` still feeds BOTH commitIfDirty (site 1) and the arrow step-off
// (site 2), so this one line re-sources both dispatcher checks.

// 5. The destructure (was :153) drops the now-unused `anchorSlice`:
//      const { contentHeight, anchorSlice } = ctx.editTarget!;
const { contentHeight } = ctx.editTarget!;   // anchorSlice no longer referenced
```

> **Why a string helper + a boolean helper, not one boolean for all five.** The
> empty-draft policy genuinely differs: sites 1–2 treat *emptied-from-non-empty*
> as a dirty **delete**; sites 3–5 treat empty as clean. A single boolean cannot
> serve both without a policy flag. Splitting it as `editBaseline` (the one
> drift-prone primitive — used by all five) + `isDirty` (the boolean shared by
> the three identical sites 3–5) keeps each site's empty semantics intact while
> still removing every duplicate copy of the baseline expression. The bug site
> (3) converges with its two already-correct siblings (4, 5).

### Capture-seam question (audit item 2) — verdict: no change needed

`captureEditTarget` (`:454`) and `seedForRange` (`:618`) **already** produce
`anchorSlice = normalizeLineEndings(sliceBytes(...)).trimEnd()`. The divergence
that caused the bug is **not** un-normalised trailing whitespace — it is the
**interior** `> `/indent prefixes, which are *real source* and must not be
trimmed (trimming them would corrupt multi-line nested content). The correct
baseline is the clean buffer `seededDraft`, which `seedForRange` already
computes from `nestedEditBuffers[siKey]`. **So no extra capture-time
normalisation is warranted; the fix is entirely on the comparison side.**

### Test plan (TDD-first / fail-on-revert)

`handleClickSwitchBlur` reads only `editTargetRef`/`clickSwitchRef` and calls
`setAst`; the faithful tier is jsdom-integration against the real `PreviewRoot`
(no browser engine needed). The seam mirrors `p2-4d.integration.test.tsx`, with
two additions: `unlockNestingCursor: true` and a `nestedEditBuffers` prop that
injects a clean buffer for A whose key is `serializeSourceEntry({t:0,r:[r0,r1],d:0})`
(e.g. `'0:6-14:0'`). The clean buffer (`"oh"`) must differ from the block's raw
slice (`"> oh\n>"`) — that divergence IS the test.

| # | Tier | Real unit mounted | Seam · assertion | Named revert hunk → RED |
|---|------|-------------------|------------------|--------------------------|
| **G19-1** (headline) | jsdom integration | `PreviewRoot` → real `handleClickSwitchBlur` + `commitIfDirty` | **New `g19-spurious-dirty.integration.test.tsx`.** Mount with `unlockNestingCursor:true`, `nestedEditBuffers:{'0:6-14:0':'oh'}`, content `'intro\n> oh\n> \npara2\n'`, pool `[[0,6],[6,14],[14,20]]`. Activate A (pool 1) → assert `textarea.value === 'oh'` (seeded clean). **Do not type.** `pointerdown` B (pool 2), then `blur` A. **Assert `setAst` NOT called** (no null commit). | Revert site 3 to `normalized !== et.anchorSlice` → blur reads `"oh" !== "> oh\n>"` ⇒ dirty ⇒ `setAst` called once with `newText:"oh"`, `r:[6,14]` → `expected 0 calls, got 1` → **RED**. *(Proven in the 2026-06-18 spike: RED before fix / after single-hunk revert; GREEN with fix.)* |

> **Mock boundary:** `getBoundingClientRect` on tiles (`mockTileRects`) +
> `PointerEvent.pointerType` via `defineProperty` — same as the p2-4d harness;
> the unit under test (`PreviewRoot` dirty/commit path) and the
> `seedForRange` → `editBaseline`/`isDirty` chain are all real. **Exercise check
> (non-vacuous by construction):** the `value === 'oh'` assertion *before* the
> blur proves the clean-buffer seam is genuinely established (seededDraft "oh" ≠
> raw slice "> oh\n>"); the divergence the fix depends on is therefore present
> when the headline assertion runs.

**Accepted-untested, with rationale:**
- **Sites 4 & 5 re-routing (`requestNestingMove`/`Select`)** — already used the
  canonical baseline before G19; the re-route is behaviour-neutral and is bound
  by the existing `p3-3-nesting` / `nest-caret` suites staying green (they
  exercise clean vs dirty nested hops). No new test; a revert of the helper
  reddens those suites.
- **Sites 1 & 2 re-sourcing (`dispatchers.tsx`)** — pure de-dup of an identical
  expression; bound by `s6-delete-by-emptying`, `s7-expand-on-edit`,
  `p3-3-seeding` staying green.
- **`editBaseline` / `isDirty` as pure units** — exercised through G19-1 and the
  existing suites; an optional 2-assertion unit test in `outerBlocks.test.ts`
  (`isDirty('oh', {seededDraft:'oh', anchorSlice:'> oh\n>'}) === false`;
  `isDirty('oh!', …) === true`) is cheap belt-and-suspenders, not required.

### Pampa-dependency verdict (the open question — actively disproven)

**Not required.** "Compare against the stored seed" is complete on the TS side:
`seededDraft` is the ground truth for "what the user opened," so an untouched
editor compares equal **by construction**, regardless of how pampa block ranges
absorb the trailing `> ` gutter. The pampa tight-range work
(`2026-06-18-block-range-gutter-tightness.md`) would only tighten the
*single-line* nested `anchorSlice` (making it coincide with the clean buffer for
that one case); it would **not** strip the **interior** multi-line `> ` prefixes,
which are real source — so the clean-buffer (`seededDraft`) machinery stays
needed regardless. Tight ranges are therefore neither necessary nor sufficient
for G19.

The one place a canonical source representation *would* matter is a
**commit-round-trip divergence**: after a commit re-render the clean buffer is
regenerated by the host's `regenerateNestedBuffers` WASM pass, and if that
regeneration produced a `seededDraft` that differed (e.g. in interior
whitespace) from the pre-commit seed for byte-identical content, a *re-anchored*
editor could read dirty. That risk lives in the self-heal/re-anchor path (bound
today by the `self-heal-on-write` suite), **not** in the dirty-check audited
here, and the deeper "eliminate the clean-buffer indirection entirely" ambition
belongs to `2026-06-18-qmd-per-line-provenance.md`, not G19. No such divergence
was observed in the spike.

### Status

- `editBaseline` + `isDirty` in `outerBlocks.ts`; sites 3/4/5 routed in
  `PreviewRoot.tsx`, sites 1/2 re-sourced in `dispatchers.tsx`.
- **Spike-validated 2026-06-18:** G19-1 RED on current tree (null commit
  reproduced: `setAst` called once, `newText:"oh"`, `r:[6,14]`); GREEN after the
  fix; **fail-on-revert proven** (revert site 3's baseline source → RED, helper
  still in place). Full preview-renderer **452 unit / 456 integration**
  (455 + the spike) green; `npx tsc --noEmit` clean.
- **Tree reverted to clean-slate** after the spike — this section is the source
  of truth, not the working tree.
- **Files:** `outerBlocks.ts` (helpers), `PreviewRoot.tsx` (sites 3–5 + import),
  `dispatchers.tsx` (sites 1–2 + import + destructure), new
  `g19-spurious-dirty.integration.test.tsx`.
- No Rust/WASM change; same pre-commit notes as G14 (run `hub-client`
  `npm run build:all` since it bundles `preview-renderer` from source; changelog
  only if the eventual commit also touches `hub-client/` — G17 already does).

---

## Clean-slate map & implementation order

**Files touched (the entire round, vs a clean tree):**

| Glitch | Production | Test |
|--------|-----------|------|
| G14 | `q2-preview/PreviewRoot.tsx` | `q2-preview/g9-reland-fade.integration.test.tsx` |
| G15 | `q2-preview/dispatchers.tsx` | `q2-preview/useEditableBlock.integration.test.tsx` (assertion loosened) |
| G16 | `q2-preview/nestingNav.ts` | `q2-preview/nestingNav.test.ts` |
| G17 | `hub-client/.../preferences/schema.ts`, `hub-client/.../render/ReactPreview.tsx` | `hub-client/.../preferences/schema.test.ts` |
| G18 | `q2-preview/PreviewRoot.tsx` (`handleClickSwitchBlur`), `q2-preview/useBlockEditHover.tsx` (`activate`) | `q2-preview/p2-4d.integration.test.tsx` (rewrite 1 test, **delete** 3 obsolete) |
| G19 | `q2-preview/outerBlocks.ts` (`editBaseline`/`isDirty`), `q2-preview/PreviewRoot.tsx` (sites 3–5 + import), `q2-preview/dispatchers.tsx` (sites 1–2 + import + destructure) | `q2-preview/g19-spurious-dirty.integration.test.tsx` (new) |

(`q2-preview/` = `ts-packages/preview-renderer/src/q2-preview/`. `PreviewRoot.tsx`
is touched by G14, G18, and G19 — different functions. `dispatchers.tsx` by G15
and G19. **Sequence G19 after G18** — both touch `handleClickSwitchBlur`; G19's
single-line change there assumes G18's direct-activate body is already in place.)

**G18 Layer 1 and G19 (its Layer 2)** are both in this clean-slate set; G19 was
deferred from the G18 pass and is now fully specced above.

**Suggested order** (most isolated first; each is RED → fix → GREEN):

1. **G16** (pure unit, self-contained): `surfaceLineSpan` + the `nestingNav.test.ts`
   `G16` block. No DOM, no browser.
2. **G15** (jsdom binding + browser confirmation): `rows={1}` +
   `EDITOR_FONT_SIZE = '0.825em'`. Binding is **G15-0** (`ta.rows === 1`) in
   `useEditableBlock.integration.test.tsx`; also **loosen** that file's font
   assertion to `/^[0-9.]+em$/` (mandatory — else it breaks on `0.825em`).
   The Playwright height spec (G15-1) is optional/deferred real-engine confirmation.
3. **G14** (jsdom integration): the `PreviewRoot` settle-gate/fade rework + the
   `g9-reland-fade` A(2) test. Verbatim above.
4. **G18** (jsdom integration): `handleClickSwitchBlur` direct-activate +
   `activate` clear-on-open; rewrite the p2-4d dirty-switch test, delete the 3
   obsolete reland/settle-gate cases. (After G14, since both touch `PreviewRoot.tsx`.)
5. **G17** (hub-client): the default flip + schema-test expectation.
6. **G19** (jsdom integration): extract `editBaseline`/`isDirty` in
   `outerBlocks.ts`; route the five comparison sites; add
   `g19-spurious-dirty.integration.test.tsx`. **After G18** (shares
   `handleClickSwitchBlur`).

**Verification (final, whole round):**

> **On the test counts in this plan.** The per-section `Status` blocks cite
> absolute counts (445/458, 452/458, 452/455, 452/456) that are **intermediate
> snapshots from the 2026-06-18 exploratory session** — each glitch was validated
> on its own against a tree reverted between glitches, so the absolutes disagree
> and will drift from today's baseline. **Do not treat any single absolute as the
> target.** Capture the real baseline by running the suites on the clean tree
> *before* starting, then verify the round by the **deltas** below.

Expected deltas vs. the clean-tree baseline (preview-renderer):
- **G16:** +6 unit (`nestingNav.test.ts` G16 block).
- **G15:** +1 unit (`G15-0` `rows===1`); `G15-font` edits an existing assertion (±0).
- **G14:** +1 integration (`G14-1` added to `g9-reland-fade`).
- **G18:** −3 integration (`p2-4d` goes 7 → 4: rewrite 1, delete 3).
- **G19:** +1 integration (new `g19-spurious-dirty`).
- **Net: +7 unit, −1 integration.**

- `cd ts-packages/preview-renderer && npm run typecheck` — clean.
- preview-renderer green (`npx vitest run` and
  `npx vitest run --config vitest.integration.config.ts`) at `baseline + the
  deltas above`.
- `cd hub-client && npm run typecheck` — clean; schema + p3-2-gating tests green.
- No Rust/WASM change in this round, so `cargo xtask verify` is not required for
  correctness — **but** because G17 + the preview-renderer changes are bundled by
  `hub-client`, run `cd hub-client && npm run build:all` and `npm run test:ci`
  before committing, and add the `hub-client/changelog.md` entry (G17 makes the
  commit touch `hub-client/`).
- **e2e (deferred, optional):** G15-1 (single-line item stays one line on expand)
  and a G16 browser-tier confirmation (ArrowDown steps off a blockquote loose
  item). The core of every fix is bound at the unit/integration tier; e2e is
  real-engine confirmation only.
