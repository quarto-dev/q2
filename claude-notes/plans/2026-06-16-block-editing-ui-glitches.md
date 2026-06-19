# Block-editing UI glitches — fixes & tests

**Date:** 2026-06-16
**Branch:** `feature/block-editing-improvements` (worktree `.worktrees/block-editing`)
**Status:** READY TO IMPLEMENT — clean working tree; build every glitch from this plan under TDD.

> **⚠️ CLEAN-SLATE STARTING STATE.** Every fix below was diagnosed and
> **live-validated** in a prior session (temporary edits → confirmed in the dev
> browser → **reverted**), then this plan was committed and **the working tree was
> reset**. So **no fix code exists in the tree** — `git diff` is empty. This plan
> is the *complete, self-contained source of truth*: each glitch carries its
> root cause, the validated fix (verbatim code where it's subtle), and a bound
> **Test Seam Spec** row. A fresh agent implements the whole set from here.
>
> **Method going forward (TDD).** For each glitch: write the bound test (RED per
> its named revert hunk), apply the fix, GREEN. The designs are settled — do **not**
> re-litigate them; the live validation already happened. The only live work left is
> the small amount flagged as such (G1's `CRUMB_W` tuning; the G9 blur was already
> tuned to the values recorded here).
>
> All cited files live under
> `ts-packages/preview-renderer/src/q2-preview/` unless a path is given.
> Playwright acceptance specs live under `hub-client/e2e/`.

## Context

The breadcrumb chip + nesting-cursor work landed across:
- `2026-06-15-breadcrumb-visual-design.md` (the chip's pivot-at-surface-left
  layout, gutter-fill / margin-spill scaffolding)
- `2026-06-15-nesting-cursor-navigation-and-list-items.md` (line-anchored nav,
  list-item surfaces — CURRENT.md)

Both are implemented and green at the jsdom/Rust tier. This plan collects the
**visual/interaction glitches** found while exercising the feature live, so they
can be fixed under TDD without re-litigating the design.

---

## Glitch index (checklist)

- [x] **G1 — Breadcrumb collapses to a single crumb when the indent gutter is
      narrow** (code-block-in-blockquote shows only `❝` or only `Cd`, never
      both). Fix: pivot-pinned **left-spill** into the margin + comfortable
      per-crumb width (`CRUMB_W=22`); at the left page edge, **spill right** past
      the pivot instead of ellipsizing. **DONE: Geometry + T17 + T18 (jsdom) + T19 (Playwright) all green.**
- [x] **G2 — `Plain` crumb wears the paragraph pilcrow `¶`** (a tight list item's
      leading block is a `Plain`, not a `Para`). Fix: `Plain → "Pl"`, `Para → ¶`.
      *Deterministic; no live test needed.*
- [x] **G3 — single-line ArrowDown/Up never steps off the surface** (the down
      arrow is eaten on any single-line block — surfaced via tight list items).
      Fix: `isOnLastVisualLine` must exclude `paddingBottom` from `fullHeight`.
      **DONE: fix applied + T21a/T21b Playwright green.**
- [x] **G4 — bare modifier keydowns trigger expand-on-edit** (pressing ⌘/⌃/⌥ to
      start a nest/commit chord wrongly expands). Fix: exclude bare-modifier keys
      from the §7 expand guard (`!isBareModifier`). *Implemented; T12 RED→GREEN proven.*
- [x] **G5 — nesting cursor doesn't carry the surface's expansion state.** Fix:
      `keepExpanded` through `openEditTarget`; clean path via `applyNestingRetarget`
      (gated per-caller), dirty path via `executeLanding`→`openFromResolved`. **Carry
      on nest-in/nest-out moves ONLY** (`spec.kind === 'nest'`); crumb jumps and arrow
      step-off do **not** carry — only nest moves keep you "looking at the same thing".
      *Design validated live 2026-06-16 (incl. the dirty path, once G6+G7 was fixed);
      nest-only scope decided 2026-06-16 by user; to implement.*
- [x] **G6 + G7 — dirty reland drops the editor / shows stale text. RESOLVED via
      the settle-gate.** Single root cause: the reland landed *early* (250ms
      backstop timer beat the slower async re-render), capturing a stale
      `anchorSlice`/seed — which both (G7) showed stale text and (G6) made
      self-heal byte-verify fail → DROP. Fix: gate the landing on the render
      reflecting the commit (`renderedContent` is the render identity); the
      `relandSettlingRef` guard is **removed**. *Implemented commit c6ccf715;
      448 integration + 445 unit tests green; fail-on-revert verified.*
- [ ] **G8 — list marker/number hover+click highlights the displaced item, and
      the parent is hard to click.** ~~Fix: in unlock mode, a hover/click whose
      `e.target` is the tight `<li>` resolves to the parent list surface.~~
      **REVERTED 2026-06-17 — the fix had a serious regression.** The `leaf ===
      target` discriminator was false: a tight item's text is a bare text node
      directly in the `<li>` (`Plain` renders as a fragment, `Str` as bare text),
      so a TEXT click *also* reports `e.target === <li>`. The branch therefore
      hijacked **every** tight-item click/hover to the parent list, making
      per-item editing impossible. The jsdom T4 test masked this by hand-building
      `<li><div>text</div></li>` — an inner `<div>` the renderer never produces;
      the e2e then asserted the broken behavior as correct. Reverted
      `findEditTarget`, deleted the g8 jsdom + marker-hit-test e2e, restored
      `s2-mode-aware-roving`. **Re-do (separate task):** the marker→parent-list
      affordance needs the marker in the `<ul>` gutter (CSS, so a bullet click is
      `e.target === <ul>` → list) or a coordinate check — not a DOM-identity test.
      Pre-G8 behavior (tight item → item editor) is the current, correct state.
- [x] **G9 — flash of stale content during the (now deterministic) reland gap.**
      Fix: blur the cell we left (`q2-reland-fade`, ~0.1s ease-out) from the
      editor-close render until the destination editor opens. *Implemented;
      T7 RED→GREEN; fadeSourceR0Ref + useLayoutEffect + clearRelandFade.*
- [x] **G10 — editing a tight list item's text turns the list loose.** Regression
      from Plan 3 (`58470cc1`). The text channel re-parses the item's inline text
      standalone → `Para`; `apply_node_edit` spliced it in with no coercion →
      loose. Fix: `preserve_leaf_variant` coerces a single-`Para` replacement back
      to `Plain` when the original was `Plain`. *T8/T9/T10 implemented RED→GREEN
      on lane/g10-tightness (2026-06-16). T11 DONE 2026-06-18: single-item edit
      bound through the real WASM build — fail-on-revert proven (revert the call
      site + rebuild WASM → both T11 tests RED with `['Para','Para']`; restore +
      rebuild → GREEN, 26 passed).*
- [x] **G11 — "expand on interest" via a second click.** A click *inside* an
      already-open editor (to place the caret where you want to read/edit) should
      expand the surface, like an in-surface keystroke (§7). Fix: textarea
      `onClick` → `setExpanded(true)`. *Implemented 2026-06-17; T14 RED→GREEN proven.*
- [x] **G12 — expanded / already-fitting editors scrolljack.** A textarea should
      capture the wheel ONLY when its content is genuinely clipped. Fix: set
      `overflow-y: auto` iff `scrollHeight > clientHeight + 6px`, else `hidden`
      (expanded + 1-liners + any fitting surface stop jacking; collapsed-spilling
      still scrolls its clipped window). *Implemented 2026-06-17; T15 Playwright GREEN proven (fail-on-revert also confirmed RED).*
- [x] **G13 (polish) — breadcrumb pill looks murky.** The translucent
      `rgba(255,255,255,0.85)` scrim read as murky over occluded content. Fix:
      opaque `rgb(243, 247, 250)` (very faint cool blue-grey, tiniest green);
      dropped the now-redundant `backdrop-filter`. *Design validated live
      2026-06-16; to implement.*

---

## G1 — Breadcrumb collapses to a single crumb in a narrow gutter

### Symptom

With a code block nested in a blockquote, the breadcrumb shows **only one crumb
at a time** — the blockquote `❝` when the cursor is on the blockquote level, the
code-block `Cd` when on the code-block level — never the expected `❝ Cd` pair.
Generalises to any shallow nesting and to crumbs whose container contributes
**no indentation of its own** (a code block, a non-indenting fenced `Div`).

### Root cause (confirmed live 2026-06-16)

The ancestor **path is correct** — `buildAncestorPath` (`nestingNav.ts:659`)
returns both crumbs (`BlockQuote` is `Descendable`, so it is indexed as an
ancestor; the code block is the current/leaf crumb). The bug is **display
selection**, in `BreadcrumbChip.tsx`:

```ts
const gutter   = surfaceLeft - colLeft;                 // blockquote indent ≈ 16–30px
const bandWidth = Math.max(gutter, MIN_GLYPH_W);        // ≈ the gutter
const slots    = surfaceLeft > 0 ? Math.floor(bandWidth / MIN_GLYPH_W) : crumbs.length;
const displayItems = selectDisplayItems(crumbs, slots); // slots == 1 → current only
```

`selectDisplayItems(crumbs, 1)` hits the `slots <= 1` branch and returns
**current-only** (`BreadcrumbChip.tsx:101`), dropping the `❝` ancestor. A
2-crumb path needs ~32px of gutter to survive; one blockquote provides ~16–30px,
so it never does. On the blockquote level `surfaceLeft == colLeft` → `gutter 0` →
again `slots 1` → only `❝`. Hence "one or the other, depending on the level."

**Diagnostic proof:** temporarily setting `bandWidth = max(gutter,
crumbs.length * MIN_GLYPH_W)` (grow the band rightward to fit every crumb) made
both `❝ Cd` appear — confirming path is fine, display selection is the bug.

### Chosen fix — pivot-pinned **left-spill** + comfortable per-crumb width

The crumb band keeps its **right edge pinned at the pivot** (`surfaceLeft`) and
grows **leftward** past `colLeft` into the page margin, because crumbs whose
container contributes no indentation (code block, non-indenting `Div`) have **no
gutter to live in** and belong in the margin — the "where you came from"
direction. Two coupled changes:

1. **Grow the band to fit the whole path at a *comfortable* width, not the
   legibility floor.** Today `MIN_GLYPH_W` (16px) doubles as both the "minimum
   legible width" (for counting slots / deciding when to ellipsize) *and* the
   actual rendered crumb width. Split them: introduce a comfortable target width
   `CRUMB_W` (~24–28px — **tune live**, see Open tuning below) used to size the
   band; keep `MIN_GLYPH_W` only as the floor for the page-edge ellipsize
   decision.

2. **Spill left; at the left page edge, spill RIGHT (no ellipsize).** Pin the
   band's right edge at the pivot and grow the chip's left edge into the margin.
   When the left spill would cross the `#quarto-content` left edge (x < 0), pin ◀
   at x=0 and — per the user's 2026-06-16 live decision — keep the band's
   *comfortable* width and let it extend **right past the pivot** (over the
   content) rather than crunching/ellipsizing. The content column has room to the
   right, so right-spill adds no horizontal scrollbar (unlike left-overflow). The
   right-edge-at-pivot invariant is intentionally relaxed in this case.

#### Geometry (IMPLEMENTED + live-validated 2026-06-16)

```ts
const OUT_W = MIN_GLYPH_W;
const gutter = surfaceLeft - colLeft;
const naturalWidth = crumbCount * CRUMB_W;              // comfortable, not the floor
const bandWidth = Math.max(gutter, naturalWidth, MIN_GLYPH_W);
// Pin band right edge at the pivot; excess width pushes chipLeft into the margin.
let chipLeft = surfaceLeft - OUT_W - bandWidth;
if (surfaceLeft > 0 && chipLeft < 0) {
    // Out of room on the LEFT: pin ◀ at x=0, keep the comfortable band width, and
    // let it spill RIGHT past the pivot (over the content). No compress/ellipsize.
    chipLeft = 0;
} else if (surfaceLeft <= 0) {
    // jsdom / unmeasured layout: keep the old anchor so the full path still renders.
    chipLeft = Math.max(0, colLeft - OUT_W);
}
const slots = surfaceLeft > 0 ? Math.floor(bandWidth / MIN_GLYPH_W) : crumbCount;
```

**Invariants this preserves:**
- *Deep indent unchanged.* When `gutter ≥ naturalWidth`, `bandWidth == gutter`
  and `chipLeft == colLeft − OUT_W` — identical to today (no regression for
  list/blockquote nesting that already has room).
- *Pivot stable EXCEPT under the left-edge clamp.* Normally the band's right edge
  is `surfaceLeft`; only when ◀ is pinned at x=0 does the band spill right past the
  pivot. `▶` + the future-crumb placeholder still sit just right of the pivot.
- *Full path by default AND at the edge.* `bandWidth ≥ naturalWidth` ⇒ `slots ≥
  crumbCount`, so `selectDisplayItems` returns the whole path. With right-spill at
  the edge the band keeps `naturalWidth`, so the path stays full there too — the
  current-only / root…current collapse now fires **only** when `colLeft`/the gutter
  themselves are pathologically narrow, not on a normal left-edge block.

#### Implemented status

- **DONE (live spike, 2026-06-16).** `computeChipGeometry` extracted + wired;
  left-spill + right-spill-at-edge shipped; `CRUMB_W` **live-tuned to 22** (started
  26, user chose "a little tighter"). T17 unit (4 cases incl. the right-spill edge)
  RED→GREEN with a proven fail-on-revert; full preview-renderer unit suite (445) +
  breadcrumb integration (7) + typecheck green.

### Design clean-up to land with the fix

- **Extract a pure `computeChipGeometry(...)`** from the `useLayoutEffect`
  body. Inputs: `{ surfaceLeft, colLeft, crumbCount }` (+ constants); output:
  `{ chipLeft, bandWidth, slots }`. This is the seam that makes the left-spill
  math **unit-testable without jsdom layout**, and shrinks the effect to
  measure → compute → `setGeom`. **Note (verified):** this output is a *new* shape,
  **not** the existing `ChipGeometry` interface — `ChipGeometry` currently has
  `{ top, chipLeft, bandWidth, displayItems }` and **no `slots`** (`slots` is a
  transient local consumed immediately by `selectDisplayItems`). Return the
  `{ chipLeft, bandWidth, slots }` triple from `computeChipGeometry` (matching T17),
  and keep the `displayItems`/`top` assembly in the effect; do **not** try to reuse
  `ChipGeometry` as the pure fn's return type.
- **Update the now-stale doc comments** in `BreadcrumbChip.tsx`: the header
  "Layout model" block (lines ~31–48), the `ChipGeometry.chipLeft` /
  `.bandWidth` field docs (lines ~81–90), and the "Nothing but ◀ is ever in the
  margin" comment (lines ~172–177) all describe the *old* gutter-only model and
  must describe left-spill (crumbs **do** enter the margin for shallow/zero
  indent).

### Test plan (TDD-first)

Write/extend these **red-first**, then implement:

1. **Unit — `computeChipGeometry` (new `BreadcrumbChip.geometry.test.ts`).**
   Pure-function table; no DOM. Cases:
   - *Deep indent, fits gutter*: `gutter ≥ naturalWidth` → `bandWidth == gutter`,
     `chipLeft == colLeft − OUT_W` (regression pin: old behaviour preserved).
   - *Shallow indent (blockquote), 2 crumbs*: `gutter ≈ 24`, `n = 2` →
     `bandWidth == naturalWidth`, `chipLeft < colLeft − OUT_W` (spilled left),
     `slots ≥ 2` (both crumbs survive). **This is the G1 regression test.**
   - *Zero indent (Div / top-level para)*: `surfaceLeft == colLeft` → band spills
     fully into the margin, right edge still at `surfaceLeft`.
   - *Left-edge right-spill*: `surfaceLeft` small, long path → `chipLeft == 0`,
     `bandWidth == naturalWidth` (kept comfortable, spills right past the pivot),
     `slots ≥ n` (full path, **no** ellipsize). *(Supersedes the original
     "compress + ellipsize" clamp — see the user's 2026-06-16 right-spill decision.)*
   - *jsdom/unmeasured*: `surfaceLeft <= 0` → `slots == crumbCount` (full path).
2. **Unit — `selectDisplayItems` (extend existing coverage in
   `nestingNav.test.ts` or a colocated test).** Pin that `slots >= n` returns the
   full path and that the collapse branches fire **only** for `slots < n`
   (documents that collapse is now page-edge-only).
3. **Integration — `p3-4-breadcrumb.integration.test.tsx`.** Add a
   **code-block-in-blockquote** fixture; assert **both** crumb buttons render
   (`title="BlockQuote"` and `title="CodeBlock"`) when editing the code block.
   (jsdom returns zero rects, so this exercises the `surfaceLeft <= 0` full-path
   branch — it guards the *path + selection*, not the px geometry.)
4. **e2e — `hub-client/e2e/q2-preview-breadcrumb-geometry.spec.ts`.** Real-layout
   geometry: code-in-blockquote fixture; assert two crumbs visible, the crumb
   row's right edge ≈ the textarea's left, and the leftmost crumb sits at
   `x ≥ 0` (no horizontal scrollbar added to `#root`). Mirror the existing
   margin-spill spec's structure. *fail-on-revert: prove it RED on current HEAD.*

### Implementation sketch (order)

1. ✅ Add `CRUMB_W` constant; extract `computeChipGeometry`; wire the effect to it.
   (T17 RED→GREEN; refactor kept breadcrumb integration green.)
2. ✅ Swap in the left-spill geometry + right-spill-at-edge; refresh the stale
   header / `ChipGeometry` / inline comments. (Done 2026-06-16.)
3. ✅ **Live-tuned `CRUMB_W` → 22** (started 26; user: "a little tighter").
4. ⏳ **Remaining:** add the e2e spec (T19, below); run `npm run build:all` +
   `npm run test:ci`. *(Not done in the spike — see "Remaining G1 work".)*

### Remaining G1 work (after the live spike)

- **T18 integration** — add the code-block-in-blockquote case to
  `p3-4-breadcrumb.integration.test.tsx`. **Caveat found during the spike:** in
  jsdom `surfaceLeft <= 0`, so BOTH the old gutter-only and new left-spill bodies
  take the `surfaceLeft <= 0 → slots = crumbCount` full-path branch — so a
  geometry revert does **NOT** redden T18 (it only guards `buildAncestorPath` +
  `selectDisplayItems` path/selection, not the px geometry). The real geometry
  binding is **T17** (pure, done) and **T19** (Playwright, below). Update the T18
  row's revert-hunk claim accordingly when writing it.
- **T19 e2e** (`hub-client/e2e/q2-preview-breadcrumb-geometry.spec.ts`) — real-layout
  px: code-in-blockquote → both crumbs visible; leftmost crumb `x ≥ 0` (no
  horizontal scrollbar on `#root`); and the **right-spill** case (a near-left block)
  → ◀ at x≈0 with the band extending right past the textarea's left, full path
  still visible. *fail-on-revert: prove RED on current HEAD.*

### Tuning outcome (recorded)

- **`CRUMB_W` = 22** (chosen 2026-06-16; `MIN_GLYPH_W` 16 remains the legibility
  floor). The value is not test-pinned — T17 derives expectations from the
  constant — so it stays freely tunable.
- **Margin availability** — left-spill assumes empty left margin. In the
  hub-client preview pane that holds; on a layout with a left sidebar / margin
  content the left-edge right-spill is the backstop. The visual-design plan's
  deferred "4g sidebar margin-occupancy" fixture is the place to harden if needed.

---

## G2 — `Plain` crumb wears the paragraph pilcrow `¶`

### Symptom

Editing a tight list item's text shows a `¶` crumb. The leading block of a tight
list item is a **`Plain`**, not a `Para` — `¶` (the pilcrow / paragraph mark) is
a paragraph glyph, so it misrepresents `Plain`.

### Root cause

`abbrevForSourceNode` (`nestingNav.ts:602–604`) maps **both** `Para` and `Plain`
→ `¶`. The breadcrumb-visual-design plan's abbreviation table does the same (one
"Para / Plain → ¶" row). `categoryForSourceNode` (633–636) maps both → `leaf-text`.

### Taxonomy determination

Our crumb taxonomy (`CrumbCategory = container | list | quote | leaf-text |
embed`) is a **color/role** axis, not a glyph axis. `Plain` holds `[Inline]`
without paragraph semantics; `Para` holds `[Inline]` *with* them. Both are
text-bearing leaves → **`leaf-text` is correct for `Plain` and stays unchanged**.
Only the **glyph** is wrong.

### Chosen fix (decided 2026-06-16)

`Plain → "Pl"` (2-char, matching the `Dv`/`Cd`/`Fg`/`Tb`/`DL` convention; derives
from the label "Plain"; tooltip already says "Plain"); `Para → ¶` unchanged.
Bonus: a tight list item now reads `• › Pl` and a loose one `• › ¶`, surfacing
the otherwise-invisible tight/loose distinction.

- **Touches:** `nestingNav.ts` `abbrevForSourceNode` — split
  `case 'Para': case 'Plain': return '¶'` into `case 'Para': return '¶';` and
  `case 'Plain': return 'Pl';`. Update the visual-design plan's abbreviation
  table (split the "Para / Plain" row).

### Test plan (TDD-first)

- **Unit — `nestingNav.test.ts:528`** currently asserts `Plain → "¶"`; flip to
  `Plain → "Pl"`. Keep `Para → "¶"` (`:524`) and `categoryForSourceNode` Plain →
  `leaf-text` (`:564`) unchanged. (RED first against current code.)

---

## G3 — Single-line ArrowDown/Up never steps off the surface

### Symptom

Click into a single-line block (surfaced via a tight list-item `Plain`, but
**not** Plain-specific). Pressing ArrowDown is "eaten" — it does not step off to
the next surface. Only happens in the in-textarea edit mode (after click-in).

### Root cause (confirmed live 2026-06-16)

`isOnLastVisualLine` (`caretGeometry.ts:162`) returns a **false negative** for a
genuine single line. The check:

```
markerOffsetTop + lineHeightPx + LAST_LINE_TOLERANCE  >=  fullHeight (= mirror.scrollHeight)
```

`scrollHeight` includes `paddingTop` + `paddingBottom` (and rounds up), but
`markerOffsetTop` includes only `paddingTop`. The comparison is short by
`paddingBottom + sub-pixel round-up`. Proven with live values (a tight list item):

```
markerOffsetTop=2  lineHeightPx=22.95  tol=2  →  26.95
scrollHeight = padTop(2) + line(22.95) + padBottom(2) = 26.95 → rounds to 27
26.95 >= 27  →  FALSE     (off by the padBottom term + round-up)
```

`value="one"`, `endsWithNewline:false`, `logicalLines:1`, `clientWidth:609` (no
wrap) — so it is purely the padding arithmetic, not a newline/wrap. Affects **any**
single-line edit surface with bottom padding (single-line paragraphs too); tight
list items just make it reliably reproducible.

Downstream: in `dispatchers.tsx` `onKeyDown`, `arrowOnEdge = isOnLastVisualLine(ta)`
false → not a leave key → treated as in-surface keystroke (expands + native caret
move) instead of cross-surface step-off.

### Chosen fix (confirmed live)

In `isOnLastVisualLine`, exclude bottom padding from the content height:
`const fullHeight = mirror.scrollHeight - parseFloat(cs.paddingBottom)`. Keep
`LAST_LINE_TOLERANCE` (still needed to absorb the sub-pixel `scrollHeight`
round-up). Audit `isOnFirstVisualLine` for the same mirror asymmetry (it compares
`marker.offsetTop < lineHeightPx`, which tolerates `paddingTop` — likely fine, but
verify).

### Test plan (TDD-first)

- **jsdom cannot see this** — `offsetTop`/`scrollHeight` are always 0 in jsdom, so
  `isOnLastVisualLine` always returns `true` there (per the file's own note).
- **e2e (Playwright) is the regression test:** a single-line block (list item or
  single-line paragraph) → ArrowDown **activates the next surface**; a genuinely
  multi-line block still navigates within its lines before stepping off (so we
  didn't over-correct). *fail-on-revert: prove RED on current HEAD.*

---

## G4 — Bare modifier keydowns trigger expand-on-edit

### Symptom

Pressing a modifier key (⌘/⌃/⌥) to *start* a nest/commit chord expands the editor
surface, even though the chord itself is a stepping-off command that should not
expand.

### Root cause

In `dispatchers.tsx` the §7 expand guard `if (!isLeaveKey && !expanded)
setExpanded(true)` fires on the **bare modifier keydowns** (`e.key` ∈
{Meta,Control,Alt,Shift}) that precede the full chord. The full chords are already
excluded (nest chords `return` early; commit/Esc/edge-arrow are leave keys) — only
the leading modifier keydowns leak through.

### Chosen fix (validated live 2026-06-16; to implement)

Add (note: each disjunct repeats `e.key ===`; the `||` shorthand
`e.key === 'Shift' || 'Control'` is a bug — always truthy):

```ts
const isBareModifier =
    e.key === 'Shift' || e.key === 'Control' || e.key === 'Alt' || e.key === 'Meta';
```

and extend the guard: `if (!isLeaveKey && !isBareModifier && !expanded)`. **Do not**
exclude all modifier-*held* keys — ⌘V (paste) must still expand to fit pasted
multi-line content; only the **bare** modifier keydowns are the problem.

### Test plan (TDD-first)

- **Integration (`s7-expand-on-edit.integration.test.tsx`)** — dispatch a keydown
  with `{key:'Meta'}` (and `Control`/`Alt`/`Shift`) on a collapsed editor; assert
  `data-expanded` stays absent (`expanded` unchanged). Assert a printable key and
  a non-edge arrow still expand. (RED first.)

---

## G5 — Nesting cursor should carry the surface's expansion state

### Concept

"The editing surface travels up/down the tree" — when stepping in/out (or jumping
via a crumb), the destination should inherit the source surface's expansion: if
the source is expanded, the destination opens expanded; if collapsed, collapsed.

### Scope decision (RESOLVED 2026-06-16 by user) — carry on NEST moves ONLY

Expansion carries **only on nest-in / nest-out** (`spec.kind === 'nest'`, distinguished
internally by `direction: 'in' | 'out'`). It does **NOT** carry on **crumb** jumps
(`spec.kind === 'crumb'`) or **arrow step-off** (`spec.kind === 'outerByLine'`).
Rationale: nest in/out keeps you visually *looking at the same thing* (the parent
contains the child you just left, or vice-versa); a crumb jump to a distant ancestor
does not. **The single predicate is `spec.kind === 'nest'`**, applied identically on
the clean and dirty paths so they cannot disagree.

> **⚠️ This supersedes earlier plan wording.** Prior drafts said carry on
> "`nest || crumb`" and said the clean path passes `keepExpanded: true`
> unconditionally. Both are now wrong (see the verified recipe below) — follow
> this section, not any `|| 'crumb'` text elsewhere.

### Chosen fix (line-verified 2026-06-16)

Verified against `PreviewRoot.tsx`: `PendingLanding` has two variants — `intent:'open'`
(carries `spec`) and `intent:'focus'` (**no `spec`**); `ResolverSpec.kind ∈
{'outerByLine','nest','crumb'}`; `applyNestingRetarget` (clean) serves **both** nest
**and** crumb callers; `openFromResolved` (dirty) is fed by `executeLanding`.

- **The seam — `openEditTarget` (opts ~436–440, reset line ~481):** add
  `keepExpanded?: boolean` to opts; change the reset to
  `if (!opts.keepExpanded) editExpandedRef.current = false`. **Also update the stale
  §7 comment (~477–480)** which currently says reland/nest-retarget "always open
  collapsed" — no longer true for nest moves.
- **Dirty path — `openFromResolved` + `executeLanding`:** give `openFromResolved` a
  `keepExpanded` param forwarded into the `openEditTarget` opts. In `executeLanding`,
  compute `const carryExpanded = pl.spec.kind === 'nest';` **after** the existing
  `if (pl.intent === 'focus') { … return; }` early-return (~line 724) — that return
  narrows `pl` to `intent:'open'`, so `pl.spec.kind` is type-safe with **no extra
  guard**. *Do not* hoist the computation above the focus guard (it would be a type
  error / undefined access on focus landings). Pass `carryExpanded` into
  `openFromResolved`; the clean **arrow** hop in `requestMove` that also calls
  `openFromResolved` passes nothing → collapsed (correct).
- **Clean path — `applyNestingRetarget` (THE BUG THE PLAN ORIGINALLY MISSED):**
  `applyNestingRetarget` takes no kind argument and is called by BOTH the clean nest
  caller (`requestNestingMove`) **and** the clean crumb caller (`requestNestingSelect`).
  Passing `keepExpanded: true` unconditionally would make **clean crumb jumps wrongly
  carry expansion**, and clean/dirty would then disagree on crumb. **Fix:** add a
  `kind: 'nest' | 'crumb'` (or bare `keepExpanded`) param to `applyNestingRetarget`,
  forward `keepExpanded: kind === 'nest'` into `openEditTarget`, and have the **nest**
  caller pass the carry while the **crumb** caller does not.

Deep-indent / fresh-click behavior is unchanged (default `keepExpanded:false`
resets to collapsed; keyboard-activate still writes `true` before `setEditTarget`).
Works because `editExpandedRef` survives the commit→reland window untouched — its only
write sites are the §7 reset, keyboard-activate (`useBlockEditHover.tsx`, fires only on
fresh activate), and the first-keystroke handler (`dispatchers.tsx`, on a textarea that
is unmounted during the commit→reland window), none of which fire in the window.

### Test plan (TDD-first)

- **Integration** — open A expanded (keyboard-activate or simulate first
  keystroke), nest in/out (clean) → destination opens with `editExpandedRef`/
  `data-expanded` true. Open A collapsed (click), nest → destination collapsed.
  Dirty variant: dirty A, nest → reland destination expanded (carried). Pin that a
  fresh **click-hop** to an unrelated block still opens collapsed.

> **Validated design (2026-06-16) — to implement, per the nest-only scope above.**
> G5 was applied and validated live, then reverted with the rest. The exact wiring,
> as line-verified against the current tree:
> `openEditTarget` opts gain `keepExpanded?: boolean`; the §7 reset is now
> `if (!opts.keepExpanded) editExpandedRef.current = false` (and the stale §7
> "always collapsed" comment is updated). The **single predicate is
> `spec.kind === 'nest'`** on both paths:
> - **Dirty** (`executeLanding` → `openFromResolved`): `const carryExpanded =
>   pl.spec.kind === 'nest';` computed **after** the `intent:'focus'` early-return
>   (which narrows `pl` to `intent:'open'` — no extra guard, do not hoist above it).
> - **Clean** (`applyNestingRetarget`, which serves both nest and crumb callers):
>   thread a `kind`/`keepExpanded` param so the **nest** caller carries and the
>   **crumb** caller does not.
>
> **Scope:** crumb jumps (`'crumb'`) and arrow step-off (`'outerByLine'`) do **not**
> carry. The clean arrow hop in `requestMove` is left without `keepExpanded`. Now that
> G6+G7 is fixed, the dirty path is confirmed (the relanded editor inherits expansion
> because `editExpandedRef` survives the commit window and the settle-gate lands on
> fresh content).

---

## G6 — Dirty nest / arrow step-off drops the editor onto the outer block

### Symptom

Dirty-edit a block, then nest-out (⌘⌃←) or arrow-step-off at the edge. The
destination (parent blockquote / heading below) **briefly activates**, then "a
moment later" the editor is dropped and the active-block cursor lands **one
further down**, on the next outer block. Same for nested list items. The clean
(non-dirty) path is unaffected.

### Root cause (confirmed live 2026-06-16 — full trace)

```
commitAndArmReland (nest/out)
executeLanding open (nest) → openEditTarget(parent)        ← parent editor opens
self-heal fired et.anchorR0=parent  DROP (close + outerBlock.focus)   ← TRIGGER
onBlur(parent)                                             ← outerBlock.focus() blurs it
requestFocusRestore → executeLanding FOCUS                ← focus lands on outer block
```

The **self-heal layout effect** (`PreviewRoot.tsx:337`, keyed on
`[astJson, renderedContent, untransformedAstJson]`) fires on the **post-commit
settling re-render**. By then the reland has opened the parent editor, so self-heal
is no longer a no-op: it runs `findReanchorCandidate` (`outerBlocks.ts:572`) for
the parent and **fails the strict byte-verify** (`sliced === anchorSlice`, line
590). The parent's `anchorSlice` carries `> ` blockquote markers **and** the
just-committed nested edit, which the settling content doesn't reproduce verbatim
→ no candidate → **DROP** path → `outerBlock.focus()` (378–380) synchronously
blurs the parent textarea → `onBlur` → `commitIfDirty` (clean draft → cancel →
`setEditTarget(null)`) → and the armed focus-restore lands on the outer block.

It is **not** a remount-blur (React does not fire `onBlur` on a re-render unmount —
see `dispatchers.tsx:208–219`); the blur is the self-heal DROP's `.focus()` steal.
Not StrictMode either (the preview root at `entry.tsx:340` is **not** wrapped).

### Approach taken (and why the obvious one failed)

- **Rejected — Approach A (defer `onBlur`, ignore if focus returned to the editor
  region):** treats the symptom (the blur), and is **timing-fragile** — for
  nest-out the settling render hadn't re-focused by the rAF, so it took the "focus
  left" branch and `commitIfDirty` *itself* closed the clean editor. "Works most of
  the time" for arrow-off, fails for nest-out. Reverted.
- **Chosen — self-heal trusts a just-relanded editor.** A reland authoritatively
  anchors the destination; self-heal's byte-verify cannot validate a relanded
  nested/marker-carrying `anchorSlice`, so it must **not DROP** on that settling
  fire.

### Chosen fix — SUPERSEDED (do NOT implement the guard; here for context only)

> **SUPERSEDED (2026-06-16).** The `relandSettlingRef` guard described below was a
> *symptom* fix — it taught self-heal to ignore the reland. The real cure (see
> **"G6 + G7 — unified settle-gate fix"** below) is to stop *lying* to self-heal:
> land only on a render that reflects the commit, so the editor's `anchorSlice`
> is genuinely valid current content and self-heal's normal byte-verify KEEPs it.
> **Do not add this guard** — implement the settle-gate instead. The text below is
> retained only as the record of why the guard approach was abandoned.

- New ref `relandSettlingRef = useRef<number | null>(null)` (declared after
  `editExpandedRef`).
- Set it to the opened `anchorR0` in **both** reland paths: `applyNestingRetarget`
  (clean) and `openFromResolved` (dirty relands).
- In the self-heal effect, **before** `findReanchorCandidate`: if
  `relandSettlingRef.current === et.anchorR0`, log + **return** (skip the fire) and
  clear the ref (consume-once).

With self-heal no longer nulling `editTarget`, the spurious `executeLanding FOCUS`
that still fires is caught by its **existing** guard (`if (editTargetRef.current
!== null) return`, `PreviewRoot.tsx:730`-ish) — so the editor stays. (The spurious
`onBlur → requestFocusRestore → FOCUS` cycle is now benign but still fires; see
G7 / system note.)

### Caveats / risks to resolve at implementation

- **Consume-once window.** Live testing showed a single settling self-heal fire,
  so consuming once sufficed. If a second settling fire DROPs, widen the window
  (e.g. clear on the next user keystroke or a short timestamp window instead of
  first-fire). Verify with multi-settle fixtures.
- **Stale-guard leak.** The clean-nest path sets the guard even though clean moves
  don't commit (so self-heal usually never fires to consume it). A later genuine
  collaborator edit on that same `anchorR0` would be skipped once. Low harm;
  consider clearing the guard on any fresh (non-reland) open / on commit.

### Test plan (TDD-first)

- **Integration (`p2-3b-real` / a new `s-reland-selfheal` test)** — simulate a
  dirty nest-out: assert that after the reland the self-heal effect does **not**
  DROP (editor stays open, `editTargetRef` non-null, no `outerBlock.focus`).
  Drive it through the real reland path, not a mocked self-heal.
- **e2e (Playwright)** — dirty nest-out from code-in-blockquote and dirty
  arrow-step-off: assert the destination editor stays active (textarea present /
  focused) and the active block does **not** jump to the next outer block.
  *fail-on-revert: RED on current HEAD.*

---

## G6 + G7 — RESOLVED: the unified settle-gate fix

### One root cause behind both symptoms (confirmed live + via `[Q2-DIAG G7]` logs)

G6 (editor DROPs to the outer block) and G7 (relanded editor shows stale text)
are **the same bug seen twice**. A dirty commit round-trips through an
intrinsically **async, multi-render pipeline** that crosses the iframe↔host
boundary:

```
iframe SET_AST → host applyNodeEdit (WASM) → new source → Automerge
  → host doRender (WASM, async) → new astJson + renderedContent → iframe re-render
```

The reland armed a **250 ms backstop `setTimeout`** *and* a props-change layout
effect. For any non-trivial doc the re-render takes **> 250 ms**, so the timer
fires **first** and calls `executeLanding` against the **pre-commit**
`renderedContent`/pool. `seedForRange` then slices stale bytes:

- the relanded editor's **seed is the pre-edit text** → **G7**;
- its **`anchorSlice` is a pre-edit slice**, so when the settled render finally
  arrives, self-heal's `findReanchorCandidate` byte-verify mismatches → **DROP** →
  **G6**.

The `relandSettlingRef` guard (see superseded section above) silenced G6's DROP
but could not fix G7's stale seed, because the landing still happened early.

Confirmed by the `[Q2-DIAG G7]` logs: `executeLanding` fired with **no preceding
`reland-effect fire`** (so it came from the timer, not the layout effect), and the
seam logged `contentReRendered: false` + `seedReflectsCommit: false`
(`preCommitContentLen === contentLen`, seed missing the committed edit).

### The fix — gate the landing on render identity

The preview render is a **pure function of the source**; the host even keeps
`{astJson, untransformedAstJson, renderedContent}` as one "render generation"
because the AST's byte offsets are only valid against that exact `renderedContent`.
So **`renderedContent` *is* the render's identity** — there is no separate
render-id (confirmed: the pipeline produces none; `DocumentProfile` is versioned
but not per-run).

Implementation (`PreviewRoot.tsx`):

1. **Snapshot at commit.** Every dirty-commit/reland site stashes the pre-commit
   `renderedContent` in `preCommitContentRef`: `commitAndArmReland` (nest/crumb),
   `requestMove` dirty branch (arrow step-off), and `handleClickSwitchBlur` (dirty
   click-switch). Cleared on land and in `cancelPendingLand`.
2. **Settle gate in `executeLanding`** (the single landing chokepoint): if
   `preCommitContentRef.current !== null && renderedContentRef.current ===
   preCommitContentRef.current`, **`return` without consuming** the pending
   landing. The props-change layout effect re-fires `executeLanding` on the next
   render; only once `renderedContent` differs (a newer generation that, via the
   host's latest-content-wins guard, includes the commit) does it land — seeding
   from fresh content.
3. **Remove the `relandSettlingRef` guard entirely.** Because the editor now opens
   only on a render whose content it was sliced from, its `anchorSlice` is a
   verbatim slice of current content, so **self-heal's existing byte-verify KEEPs
   it with no special case**. (During the in-flight window the editor is `null` —
   `commitAndArmReland` closed it — so self-heal never sees a half-landed editor.)
4. **De-duplicate the backstop timer (land this with the gate).** The 250 ms reland
   backstop is currently a **bare `250` literal at four sites** (`requestMove`,
   `requestFocusRestore`, `handleClickSwitchBlur`, `commitAndArmReland`), each preceded
   by a **byte-identical** guard block (`fallbackTimerRef` clear, `pl =
   pendingLandingRef.current` null-check, `pl.fromFile !== currentFilePathRef.current`
   bail) before calling `executeLanding(pl, poolRef.current, renderedContentRef.current)`.
   Extract a single `const RELAND_BACKSTOP_MS = 250;` and a shared
   `armRelandBackstop(...)` helper, so all four sites share one timer value and one
   guard. This also gives the no-op-dirty-commit residual (below) a single place to grow
   a watchdog / longer timeout if needed.

This is the "single authoritative transition that self-heal does not second-guess"
the holistic review called for — achieved not by *telling* self-heal to trust the
reland, but by giving it self-consistent state so the normal check passes.

### Wrap the gate behind a seam (the render-identity concept, named not built)

The gate is content-addressed (`renderedContent` value). If a second consumer ever
wants an explicit render-generation id (a monotonic counter or input-hash stamped
on each render — useful for scroll-sync / incremental rebuild / freeze), only the
gate's predicate changes. We name the concept and isolate it now; we do **not** pay
the cross-iframe-boundary plumbing until something else needs it (YAGNI).

### Known residual (accepted, logged)

- **No-op dirty commit.** If a "dirty" edit normalizes to a **byte-identical**
  source, `renderedContent` never changes, so the gate would defer the reland
  **indefinitely** (no watchdog force-lands it). Narrow: a same-source result
  usually fails the upstream `isDirty` check before commit. **Accepted-untested;**
  future hardening = a watchdog that force-lands after N renders / a longer
  timeout. See Test Seam Spec.
- The spurious `onBlur → requestFocusRestore → FOCUS` cycle still fires after a
  reland (benign — caught by the `editTarget !== null` guard). Unchanged.

---

## G8 — list marker/number resolves to the parent list (hover + click)

### Symptom & root cause (confirmed live + via `[Q2-DIAG G8]` logs)

Hovering a tight list item's **bullet/number** highlighted the *item* with the
box-shadow drawn beside the cursor, and there was no easy spot to target the parent
list. A tight `<li>` **borrows the leading `Plain`'s `data-block-pool-id`** — emitted
in `blocks/BulletList.tsx`, `blocks/OrderedList.tsx`, and `blocks/DefinitionList.tsx`
(the `<dd>` case), gated by the tight/loose predicate in `blocks/listBorrow.ts`
(`block.t !== 'Plain'` ⇒ no borrow). (Earlier drafts cited `outerBlocks.ts:333`, which
is only a *doc comment* describing the borrow, not its emission site.) So a tight item's
`<li>` *is* the leaf surface; `list-style: outside` puts the
marker in the `<li>`'s margin gutter, *outside* the border-box the box-shadow draws
on. The `[Q2-DIAG G8]` logs showed the structural signal: hovering the marker gives
`e.target === <li>` (`eTargetIsLi: true`), hovering the text gives an inner `<div>`.

### The fix (`useBlockEditHover.tsx`)

`findEditTarget` becomes marker-aware (unlock mode only). After `leaf =
target.closest('[data-block-pool-id]')`:

```ts
if (leaf && leaf === target && (target.tagName === 'LI' || target.tagName === 'DD')
    && ctx?.unlockNestingCursorRef?.current) {
    return leaf.parentElement?.closest('[data-block-pool-id]') ?? leaf; // parent list
}
return leaf;
```

`leaf === target` is the precise discriminator: it is true only for a **tight**
`<li>` (which *is* the pool-id leaf) hit on its own chrome; it is false for content
hovers (target is an inner element) and for **loose** items (the `<li>` has no
pool-id, so `closest` already returned the parent — climbing again would over-climb
to the grandparent). The loose-item safety rests on the renderer invariant that a
loose `<li>` carries no pool-id — enforced by `blocks/listBorrow.ts` and covered by
`s0-list-item-surfaces.integration.test.tsx`. One change propagates to hover, mouse-click `activate`,
click-switch, and touch (all route through `findEditTarget`); keyboard uses the
already-resolved `hoveredRef`.

---

## G9 — blur the outgoing cell during the reland gap

### Concept

With the settle-gate, the reland deliberately **waits** for the settled render, so
there is a brief, now-**deterministic** interval where the editor is closed and the
cell we left re-renders as static stale content. We know exactly which cell that is
(the commit source), so we blur it until the destination editor opens.

### The fix (`PreviewRoot.tsx` + `useBlockEditHover.tsx` stylesheet)

- `fadeSourceR0Ref` records the left cell's `anchorR0` at each dirty-commit site.
- A `useLayoutEffect` keyed on `editTarget` fires on the editor-close render
  (`editTarget === null && pendingLandingRef.current !== null`): it finds the
  source cell by **scanning all `[data-block-pool-id]` elements** for the editable
  entry at that byte offset (`outerBlockForAnchorR0` only scans **outer** blocks
  and misses a **nested** source — that was the first cut's bug) and adds the
  `q2-reland-fade` class.
- `clearRelandFade()` (called at the top of `openEditTarget` and in
  `cancelPendingLand`) removes the class when the destination editor opens.
- CSS keyframes in the preview stylesheet: `filter: blur(0)→blur(1px)`,
  `opacity 1→0.85`, `0.1s ease-out forwards`. The settle-gate guarantees content is
  **stable** during the gap (no intermediate renders), so the imperatively-added
  class survives the whole interval and animates cleanly.

Final intensity tuned live (1 px / 0.85). The blur *visual* is not unit-asserted
(see Test Seam Spec); the class-application *logic* is.

---

## G10 — preserve list tightness across a text-channel item edit

### Symptom, root cause, fix

See the checklist entry. Empirically proven (RED): editing item 1 of `- foo\n-
bar\n` to "foo edited" produced `"* foo edited\n\n* bar\n"` (loose; item 0 a
`Paragraph`). Root cause: the text channel re-parses the bare inline text
standalone → a `Para`; `apply_node_edit` spliced it in unchanged; the writer
correctly renders a `Para`-leading list as loose. Regression from Plan 3
(`58470cc1`), which first made list items individually editable.

Fix (`crates/pampa/src/apply_node_edit.rs`): `preserve_leaf_variant`, applied at
the leaf splice — if the original block was `Block::Plain` and the replacement is a
single `Block::Paragraph`, coerce it back to `Plain`. Principled: an inline-text
edit preserves the block's `Plain`/`Para` variant; genuine structural edits
(multi-block, non-`Para`, already-`Para` original) pass through and loosen
correctly. **Proven RED→GREEN in the prior session (full pampa suite green, 3949
passed), then reverted — re-implement under TDD (T8/T9/T10).**

Exact implementation (re-create verbatim — note the load-bearing `len() == 1`
guard, tested by T10):

```rust
// add `Plain` to the existing import:
use crate::pandoc::block::{Block, Blocks, Plain};

// at the leaf-splice site in `splice_in_blocks` (the `steps.split_first()` None arm):
let Some((head, tail)) = steps.split_first() else {
    let replacement = preserve_leaf_variant(current.get(leaf_idx), replacement);
    current.splice(leaf_idx..=leaf_idx, replacement);
    return;
};

// new helper:
fn preserve_leaf_variant(original: Option<&Block>, mut replacement: Vec<Block>) -> Vec<Block> {
    if matches!(original, Some(Block::Plain(_)))
        && replacement.len() == 1
        && matches!(replacement[0], Block::Paragraph(_))
    {
        if let Block::Paragraph(para) = replacement.remove(0) {
            return vec![Block::Plain(Plain { content: para.content, source_info: para.source_info })];
        }
    }
    replacement
}
```

---

## G11 — "expand on interest" via a second click

### Concept

This epic expands the edit surface on a gesture of **interest**. Keystrokes already
do this (§7 expand-on-edit); a **second click** is the same signal: you click a
block to edit it, see its code spilling out of the collapsed box, then click where
you actually want the caret — at which point you clearly want to read all of it, so
the surface should expand.

### The fix (`dispatchers.tsx`)

Add an `onClick` to the `EditTextarea` textarea: `if (!expanded) { setExpanded(true);
ctx.editExpandedRef.current = true; }`. No-op when already expanded.

**Why the activation click does not self-trigger (mechanism corrected & line-verified
2026-06-16):** the load-bearing fact is **mount timing**, not click-target arithmetic.
Mouse activation runs on **`onPointerUp`** (`useBlockEditHover.tsx`, the mouse branch
that calls `activate` → `ctx.setEditTarget`), *not* pointerdown. `setEditTarget` routes
to a `useState` setter (`PreviewRoot.tsx`, `setEditTargetRaw`), so the `<textarea>`
mounts only on a **later** React render — it does **not exist in the DOM** at the
moment the activating mousedown/mouseup are hit-tested. The browser therefore computes
that activating `click`'s target from the original block subtree (down and up share the
*same* block target on a normal click), so the click lands on the **block**, never the
textarea. (The earlier "pointerdown vs pointerup have *different* targets → common
ancestor is the block wrapper" story was a misdiagnosis: same target, and the decisive
fact is that the textarea isn't mounted yet.) A genuine *second* click inside the open
editor hit-tests both mousedown and mouseup against the now-mounted textarea → `click`
targets it → `onClick` fires. The handler is self-guarding (`if (!expanded)`,
idempotent), so no extra guard is required. No `preventDefault`/`stopPropagation` on the
activation path and no blocking `pointer-events` CSS interfere. Composes with G5 (carried
expansion) and §7 (keystroke): three "interest" triggers, while bare modifiers (G4) and
edge/leave keys correctly don't expand.

---

## G12 — expanded / already-fitting editors must not scrolljack

### Symptom & rule

A collapsed editor whose source spills should still scroll its **clipped window**
of code (internal scroll = wanted). But an **expanded** editor (sized to fit its
content) and an **activated editor that's already big enough** (a 1-liner, or any
block whose box already covers its source) have nothing clipped — yet the textarea
default (`overflow-y: auto`) still captured the wheel ("scrolljack"), so the page
wouldn't scroll over them. Unified rule: **scrolljack iff the content is genuinely
clipped.**

### The fix (`dispatchers.tsx`)

The §7 layout effect now runs for **both** states (it still only sets `height` when
`expanded`) and sets overflow from a clip test:

```ts
const SCROLLJACK_TOLERANCE_PX = 6;          // absorbs sub-pixel + small leading/padding deltas
const clips = ta.scrollHeight > ta.clientHeight + SCROLLJACK_TOLERANCE_PX;
ta.style.overflowY = clips ? 'auto' : 'hidden';
```

`hidden` lets the wheel pass to the page (no jack); `auto` keeps internal scrolling
for a collapsed, spilling surface. Recomputed on every `draft`/`expanded`/
`contentHeight` change. The 6px tolerance was tuned live (1px left the
least-overflowing surfaces still jacking).

---

## G13 (polish) — opaque breadcrumb pill

The breadcrumb pill was `rgba(255,255,255,0.85)` + `backdrop-filter: blur(2px)` —
the translucency read as **murky** over occluded content rather than helping. Fix
(`BreadcrumbChip.tsx`): opaque `background: rgb(243, 247, 250)` — a very faint cool
blue-grey with the tiniest hint of green (B highest, G a touch over R) — and the
now-redundant `backdrop-filter` removed (an opaque pill fully covers what's behind).
Tuned live.

---

## Test Seam Spec (FROZEN)

> Per the pre-validating-test-seams discipline: each row binds a test to the real
> module at the **lowest faithful tier**, names the concrete **seam** (mount +
> events + assertion surface), the **mock boundary** (only genuine env deps), and
> the **single production hunk whose revert reddens it**. Once a test is green its
> harness/assertions are **frozen** — never edited to go green. All TS jsdom seams
> reuse the **existing** harnesses cited; do not invent new ones.

| # | Tier | Real unit mounted | Seam: mount · events · assertion surface | Mock boundary | Named revert hunk → assertion RED |
|---|------|-------------------|------------------------------------------|---------------|-----------------------------------|
| T1 | jsdom integration | `PreviewRoot` reland landing (`executeLanding` settle gate) | Reuse `nest-caret`/`p3-3-nesting` `mountFixture` (unlock + `nestedEditBuffers`, `setAst` spy). Activate nested item (pointerdown/up on `[data-block-pool-id]`), dirty via `fireEvent.change`, nest-out via `fireEvent.keyDown(ta, nestingChord('ArrowLeft'))` → `setAst` called once. `vi.useFakeTimers`; `advanceTimersByTime(250)` **without** a prop swap (content still === pre-commit). **Assert: no `textarea` open** (gate deferred). Then `rerender` with the **settled** props (new `renderedContent`/astJson reflecting the edit). **Assert: `textarea.value` === the FRESH committed slice** (contains the edit). | `getBoundingClientRect`/`measureBlockBox` (jsdom zero-rect); fake timers. WASM not involved — settled props supplied by the test. | Remove the settle-gate `if (preCommitContentRef.current !== null && renderedContentRef.current === preCommitContentRef.current) return;` → the 250 ms timer lands on stale content → editor opens early seeded with **stale** text → `textarea.value` is the pre-edit slice → **RED**. |
| T2 | jsdom integration | `PreviewRoot` self-heal KEEP of a freshly-landed nested editor | Same harness; drive the dirty nest-out **fully to land** (commit → settled `rerender` → editor open on parent). Then fire **one more** settling `rerender` with **stable** content. **Assert: `textarea` still present**, `setAst` **not** called again (no DROP-commit), no `outerBlock.focus`. | as T1 | Same gate hunk (T1): without it the editor lands early with a stale `anchorSlice`; the post-land settle makes `findReanchorCandidate` mismatch → DROP → `textarea` null → **RED**. (Also implicitly guards that removing `relandSettlingRef` is safe.) |
| T3 | jsdom integration | snapshot-at-commit for the click-switch + arrow sites | **Expand existing** `p2-4d` (dirty click-switch) and `p2-4-real` (dirty arrow-move): before the existing settled `rerender`, add `vi.useFakeTimers` + `advanceTimersByTime(250)` with **stale** content. **Assert: no premature land** (textarea still closed); after the settled `rerender`, existing assertions (B's editor opens, value correct) still hold. | as T1 | Remove `preCommitContentRef.current = renderedContentRef.current` from `handleClickSwitchBlur` / `requestMove` dirty branch → gate sees `null` → lands early on the stale timer → premature/stale editor → **RED**. |
| T4 | jsdom integration | `useBlockEditHover` `findEditTarget` marker-aware branch | Reuse `useBlockEditHover.integration` `BlockHost`/`Inner` pattern but mount a **list DOM**: `<ul data-block-pool-id="U"><li data-block-pool-id="L"><div>text</div></li></ul>`, `ctx.pool[U].r=[10,40]`, `ctx.pool[L].r=[12,20]` (**distinct** r0), `unlockNestingCursorRef.current=true`. (a) pointerdown/up with `e.target` = the `<li>` → **assert `setEditTarget` arg `anchorR0 === 10`** (parent list). (b) `e.target` = inner `<div>` → **assert `anchorR0 === 12`** (item leaf). (c) pointermove with `e.target`=`<li>` → **assert `ul.style.boxShadow` truthy and `li.style.boxShadow === ''`**. | `ctx` (PreviewContext), `measureBlockBox` geometry. | Revert the `findEditTarget` marker branch → marker resolves to `<li>` leaf → (a) `anchorR0 === 12` not `10`; (c) `li` gets the outline → **RED**. Discriminator is the *distinct* r0s — keep them unequal. |
| T5 | jsdom integration | `findEditTarget` `leaf === target` precision (no over-climb on loose items) | Same harness; **loose** item DOM: `<li>` with **no** pool-id wrapping an inner `<p data-block-pool-id="L">`. pointerdown/up with `e.target` = the `<li>` → **assert `setEditTarget` `anchorR0` === the parent `<ul>`'s r0** (not the grandparent). | as T4 | Weaken the condition to `target.matches('li,dd')` (drop `leaf === target`) → a loose `<li>` climbs from `leaf` (already the `<ul>`) to the grandparent → wrong `anchorR0` → **RED**. |
| T6 | jsdom integration | `findEditTarget` unlock-gating | Same list DOM as T4 but `unlockNestingCursorRef.current=false`; marker hover/click resolves via the locked path (`resolveOuterBlock`), **not** the parent-climb. **Assert** the resolved surface matches locked-mode behavior (the outer block), unchanged from pre-fix. | as T4 | Remove `&& ctx?.unlockNestingCursorRef?.current` → the marker branch fires in locked mode too → resolved surface changes → **RED**. |
| T7 | jsdom integration | `PreviewRoot` G9 fade apply/clear + nested scan finder | Same reland harness as T1 with a **nested** source. On the editor-close render (post nest-out, pre settled `rerender`): **assert the source cell** (`querySelector('[data-block-pool-id="<source idx>"]')`) **has class `q2-reland-fade`**. After the settled `rerender` (land): **assert no element has `q2-reland-fade`**. | as T1 | (a) Remove `el.classList.add('q2-reland-fade')` in the apply effect → source cell never gets the class → first assert **RED**. (b) Replace the querySelectorAll scan with `outerBlockForAnchorR0` → **nested** source not found → no class → **RED** (binds the scan-vs-outer-only fix). (c) Remove `clearRelandFade()` from `openEditTarget` → class lingers after land → second assert **RED**. |
| T8 | Rust integration (pampa) | `apply_node_edit` (real) — bullet tightness | **TO IMPLEMENT** (proven RED→GREEN in prior session, then reverted). `node_edit_tests::text_edit_preserves_bullet_list_tightness`: `edit_nested_block("- foo\n- bar\n", list_item_block(ast,0,0,0).source_info(), "foo edited\n")` → re-parse → **assert `bl.content[0][0]` and `[1][0]` are `Block::Plain`**. | none (pure Rust) | Revert `preserve_leaf_variant` → `Para` spliced → loose → re-parsed `item[0][0]` is `Block::Paragraph` → **RED** (proven). |
| T9 | Rust integration (pampa) | `apply_node_edit` (real) — ordered tightness | **TO IMPLEMENT** (proven, then reverted). `text_edit_preserves_ordered_list_tightness`: same for `1. foo\n2. bar\n` / `Block::OrderedList`. | none | same hunk as T8 → **RED** (proven). |
| T10 | Rust integration (pampa) | `preserve_leaf_variant` over-fire guard | **TO ADD.** Edit a tight item with a **multi-paragraph** replacement (`"para one\n\npara two\n"`) → re-parse → **assert the item legitimately loosens** (leading `Block::Paragraph`, 2 blocks in the item). | none | Drop the `replacement.len() == 1` check in `preserve_leaf_variant` → it would coerce the first `Para` of a multi-block edit → **RED**. |
| T11 ✅ DONE | TS integration over real WASM | host `applyNodeEdit` tightness round-trip | **DONE 2026-06-18.** Expanded `hub-client/src/services/applyNodeEdit.wasm.test.ts` with two new describes that edit a **single item's leading `Plain` leaf** (BulletList `c[0][0]`; OrderedList `c[1][0][0]` — `c` is `[attrs, items]`) and **assert tightness**: re-parse the output QMD and assert per-item leading types `=== ['Plain','Plain']`, plus a byte-level `not.toMatch(/foo edited\n\n/)`. The existing whole-list round-trips don't exercise tightness. | WASM module (the unit under test runs in it; not mocked) | **Proven fail-on-revert:** removed the `preserve_leaf_variant` call at the leaf-splice site + rebuilt WASM → both T11 RED (`['Para','Para']`, loose); the other 24 stayed green; restore + rebuild → GREEN (26). |
| T12 | jsdom integration | `dispatchers.tsx` §7 expand guard — G4 bare-modifier exclusion | **Expand `s7-expand-on-edit.integration.test.tsx`.** Click-activate a block **collapsed** (`clickActivateTile`, assert `data-expanded` absent). (a) `fireEvent.keyDown(ta, {key:'Meta'})` (and `Control`/`Alt`/`Shift`) → **assert `data-expanded` still absent**. (b) a printable `fireEvent.keyDown(ta,{key:'x'})` + change → **assert `data-expanded` present** (the guard didn't over-exclude). | none (jsdom; `data-expanded` is the existing seam) | Remove the `!isBareModifier` term from `if (!isLeaveKey && !isBareModifier && !expanded)` → a bare `Meta` keydown expands → (a) `data-expanded` present → **RED**. Discriminator differs across (a) bare-mod vs (b) printable — both states asserted so it can't go vacuous. |
| T13 | jsdom integration | `PreviewRoot` G5 carried expansion (NEST-ONLY) | **Reuse `nest-caret`/`p3-3-nesting` `mountFixture`.** **(a) Nest carries:** open the source **expanded** (keyboard-activate, or type to expand — assert source `data-expanded` present), then **nest-in/out** move; drive the dirty path fully to land (commit → settled `rerender`). **Assert the relanded textarea has `data-expanded` present.** **(b) Collapsed companion:** open source **collapsed** (click) → nest → **assert relanded textarea `data-expanded` absent**. **(c) Crumb does NOT carry (the gating discriminator):** open source **expanded**, then **crumb** jump → **assert relanded textarea `data-expanded` ABSENT** (crumb drops expansion even from an expanded source). Pin a fresh **click-hop** to an unrelated block still opens collapsed. | as T1 (geometry, fake timers) | Two revert hunks: (i) revert the `if (!opts.keepExpanded)` guard in `openEditTarget` → expanded-source **nest** assertion (a) `data-expanded` present → **RED**. (ii) change the predicate from `spec.kind === 'nest'` to `=== 'nest' || === 'crumb'` (or make `applyNestingRetarget`'s crumb caller pass the carry) → **crumb** case (c) wrongly shows `data-expanded` present → **RED**. (Vacuity: (a) carries vs (b)/(c) don't — the nest-vs-crumb pair keeps the *kind* discriminator live, not just expanded-vs-collapsed.) |
| T14 | jsdom integration | `EditTextarea` G11 second-click expand | **Reuse `useBlockEditHover.integration`/reland harness.** Click-activate a block **collapsed** (assert `data-expanded` absent). `fireEvent.click(textarea)` → **assert `data-expanded` present**. Companion: starting **expanded**, `fireEvent.click(textarea)` → **assert still present** (no-op, and assert `setExpanded`-equivalent didn't toggle off). | none | Remove the textarea `onClick` `setExpanded(true)` block → the click leaves it collapsed → `data-expanded` absent → **RED**. |
| T15 | **Playwright (real layout)** | `EditTextarea` G12 scrolljack rule | **Expand an existing q2-preview e2e spec** (e.g. `q2-preview-item-edit-size.spec.ts`). jsdom reports `scrollHeight===clientHeight===0`, so this is browser-tier. (a) Activate a **collapsed, spilling** code block → **assert `getComputedStyle(textarea).overflowY === 'auto'`** (and a wheel over it scrolls the textarea, not the page). (b) Expand it (second click / keystroke) → **assert `overflowY === 'hidden'`** (wheel scrolls the page). (c) Activate a **1-liner / fitting** block → **assert `overflowY === 'hidden'`**. | real browser layout engine | Force `ta.style.overflowY = 'auto'` unconditionally (revert the clip test) → (b)/(c) read `'auto'` → **RED**. *Do not assert this in jsdom — 0/0 always reads `hidden`, vacuous pass.* |
| T16 | — (accepted-untested) | G13 breadcrumb pill color | **Accepted-untested at unit tier** — a color constant is theater to assert. Visually validated live (opaque `rgb(243,247,250)`, no `backdrop-filter`). *Optional* belt-and-suspenders: a Playwright assertion that the pill's computed `background-color` has alpha `1` (no `rgba(...,0.85)`) and `backdrop-filter` is `none`. | — | n/a (no behavior branch; revert cannot meaningfully redden a constant). |

> **ALL rows are RED-first against a clean tree.** The working tree was reset
> after this plan was committed, so **no fix exists in the tree** for *any*
> glitch — every row below (T1–T21) is written test-first: write the test (RED per
> its named revert hunk), apply the fix from that glitch's section, GREEN. Rows
> formerly marked "DONE" (T8/T9/T10) were proven RED→GREEN in the prior session
> and then reverted; treat them as TO IMPLEMENT like the rest.

| T17 ✅ DONE | jsdom **unit** (pure fn) | `computeChipGeometry` (G1 — **extracted** from the `BreadcrumbChip` `useLayoutEffect`) | `BreadcrumbChip.geometry.test.ts`, **no DOM** — call the pure fn over a table of `{surfaceLeft, colLeft, crumbCount}`: (a) deep indent, `gutter ≥ naturalWidth` → `bandWidth === gutter` && `chipLeft === colLeft − OUT_W` (regression pin: old behavior); (b) shallow blockquote, 2 crumbs, `gutter ≈ 24` → `bandWidth === 2·CRUMB_W` && `chipLeft < colLeft − OUT_W` (spilled left) && `slots ≥ 2`; (c) **left-edge right-spill** (small `surfaceLeft`, long path) → `chipLeft === 0` && `bandWidth === crumbCount·CRUMB_W` (kept comfortable, spills right) && `slots ≥ n` (full path, **no** ellipsize); (d) `surfaceLeft <= 0` (jsdom/unmeasured) → `slots === crumbCount`. Expectations derive `naturalWidth` from the exported `CRUMB_W`, so live-tuning the value cannot break them. | none (pure) | Proven fail-on-revert: against the gutter-only body, case (b) reddened (`expected 24 to be 44`) and (with the original clamp) case (c) reddened (`expected 24 to be …`); the left-spill + right-spill body greens all four. |
| T18 ⏳ TODO | jsdom integration | `BreadcrumbChip` path + display selection (G1) | **Expand `p3-4-breadcrumb.integration.test.tsx`** with a **code-block-in-blockquote** fixture; when editing the code block, **assert BOTH crumb buttons render** — `getByTitle('BlockQuote')` **and** `getByTitle('CodeBlock')` present. | `getBoundingClientRect` (jsdom 0) | **⚠️ REVISED (spike finding):** in jsdom `surfaceLeft <= 0`, so BOTH the gutter-only and left-spill bodies take the `surfaceLeft <= 0 → slots = crumbCount` full-path branch — a geometry revert does **NOT** redden T18. T18 therefore guards only `buildAncestorPath` + `selectDisplayItems` **path/selection** (both titles present), not the px geometry. Its real revert hunk is a `buildAncestorPath` break (drop the `BlockQuote` ancestor) → only `CodeBlock` present → **RED**. The geometry binding lives in T17 (pure) + T19 (Playwright). |
| T19 ⏳ TODO | **Playwright (real layout)** | `BreadcrumbChip` real px geometry (G1) | **Expand `q2-preview-breadcrumb-geometry.spec.ts`**: (i) code-in-blockquote → **two crumbs visible**, crumb row's right edge ≈ the textarea's left (pivot), leftmost crumb `x ≥ 0` (no horizontal scrollbar on `#root`); (ii) **right-spill case** — a block near the left edge → ◀ at `x ≈ 0` and the band extends **right past** the textarea's left, full path still visible. | real browser layout | Revert geometry to gutter-only → only one crumb visible / pivot wrong → **RED**. *Do not assert px in jsdom (rects 0, vacuous).* |
| T20 | jsdom **unit** (pure fn) | `abbrevForSourceNode` (G2) | **Expand `nestingNav.test.ts`** (~`:528`): assert `abbrevForSourceNode(Plain) === 'Pl'` **and** (kept distinct) `abbrevForSourceNode(Para) === '¶'`; assert `categoryForSourceNode(Plain) === 'leaf-text'` unchanged (glyph axis ≠ category axis). | none (pure) | Revert the split (`case 'Para': case 'Plain': return '¶'`) → `abbrevForSourceNode(Plain) === '¶'` → the `=== 'Pl'` assertion → **RED**. |
| T21 | **Playwright (real layout)** | `isOnLastVisualLine` (G3) | **Expand an existing nav e2e** (e.g. `q2-preview-block-nav-p2-5b.spec.ts`): (a) a **single-line** block (tight list item or 1-line para) → ArrowDown **activates the next surface** (steps off); (b) **exercised-the-right-thing guard:** a genuinely **multi-line** block → ArrowDown first moves the caret **within** its lines (does NOT step off until the last visual line), so we didn't over-correct. | real browser (jsdom `offsetTop`/`scrollHeight` are 0 → `isOnLastVisualLine` always true → **vacuous in jsdom**) | Revert `fullHeight = mirror.scrollHeight - parseFloat(cs.paddingBottom)` back to `= mirror.scrollHeight` → single-line false-negative returns → ArrowDown eaten on (a) → **RED**. |

### Refactor-induced vacuity checks (check 3)

- **T1/T2 discriminator is the seed *content*, not editor presence.** Asserting
  "a textarea opens after the settled render" would pass with or without the gate
  (the editor opens either way; the gate changes *when* and *with what seed*). The
  frozen assertion is `textarea.value === fresh slice` — which differs (fresh vs
  stale) across the gate's presence. Do not relax it to a presence check.
- **T4 keeps parent r0 ≠ leaf r0.** The whole discriminator is parent-list r0 vs
  item-leaf r0; if a future fixture refactor makes them equal the test goes vacuous.
  Pool ranges are pinned distinct in the seam.
- **T7 must target the specific source cell by index**, not "any element has the
  class" — and must assert both the **add** (during gap) and the **absent-after-land**
  (binds `clearRelandFade`). A nested source is required so revert-to-`outerBlockForAnchorR0`
  reddens it.
- **T20 (G2) is the skill's canonical vacuity trap — handled.** Pre-fix, both
  `Para` and `Plain` abbreviate to `¶`; the fix makes `Plain → 'Pl'`. The test
  therefore asserts BOTH `Plain === 'Pl'` AND `Para === '¶'` so the two **stay
  distinct** — a future "abbreviate everything to a glyph" refactor that re-collapsed
  them to a shared value would make the assertion identical across the states it
  guards (vacuous). The category axis (`Plain → 'leaf-text'`) is a separate
  assertion, never folded into the glyph assertion.
- **T13 (G5) keeps two discriminators live.** Expanded-source ⇒ `data-expanded`
  present, collapsed-source ⇒ absent keeps the *expansion-source* discriminator;
  and expanded-**nest** ⇒ present vs expanded-**crumb** ⇒ absent keeps the *move-kind*
  discriminator (binds the nest-only rule). A single expanded-nest-only assertion
  could pass both a stuck-on bug AND a crumb-also-carries bug.
- **T18 (G1) discriminates on `title` (full label), not the glyph.** The
  abbreviated glyph (`Cd`, `❝`) could collide or be refactored; both crumbs'
  presence is asserted via `getByTitle('BlockQuote')`/`getByTitle('CodeBlock')`.

### Missing-test pass (check 4) — accepted-untested, with rationale

- **G9 blur *visual*** (filter px / opacity / easing curve): not unit-assertable in
  jsdom (no layout/animation). Visually validated live (1 px / 0.85 / 0.1 s
  ease-out). *Accepted-untested at unit tier;* optional belt-and-suspenders =
  expand `q2-preview-self-heal-on-write.spec.ts` to assert a non-`none`
  `filter` computed style on the source cell mid-gap (browser tier; flaky timing).
- **G8 real marker hit-testing** ("hovering the *rendered* bullet yields
  `e.target === <li>`"): a browser-engine fact T4 cannot verify (T4 sets `e.target`
  directly). *Expand* `q2-preview-item-edit-size.spec.ts`: click the actual
  marker/number → assert the **parent list** is activated (real DOM). Pairs with T4
  (logic) for full faithful coverage.
- **Settle-gate no-op-commit** (byte-identical source defers the reland forever):
  documented residual above. *Accepted-untested;* needs the watchdog that doesn't
  exist yet — spec a test when that lands.
- **G9 fade on click-switch / arrow paths**: the apply effect is shared across all
  reland kinds; T7 (nest-out) binds the effect. Other kinds *accepted as
  shared-effect-covered*.
- **G8 keyboard-activate on a marker**: uses the already-resolved `hoveredRef`;
  transitively covered by T4's hover resolution. *Accepted as transitively covered.*

### e2e (expand existing only — do not add new specs)

The core logic of every fix is bound at the jsdom/Rust tier above (no layout engine
needed for the settle-gate, the structural marker resolution, the fade-class, or
tightness). e2e is **optional real-engine confirmation**, folded into existing
specs:

- `q2-preview-item-edit-size.spec.ts` → tight list **stays tight** after editing an
  item (real render); marker/number **click selects the parent list** (the G8
  browser-tier fact above).
- `q2-preview-self-heal-on-write.spec.ts` → dirty **nest-out relands with fresh
  (non-stale) content** and the editor **stays** (no drop to the outer block).

---

## Dirty-landing system — holistic review (RESOLVED, 2026-06-16)

The first time self-heal and dirty stepping were exercised together in anger
surfaced that the reland/self-heal/landing machinery raced across several renders.
The holistic pass landed not as a big rewrite but as **one principle**: a reland is
a single transition that **lands only on the render reflecting its commit**
(`renderedContent` as render identity). That made the editor's state
self-consistent, which **dissolved G6, G7 in one stroke and let the
`relandSettlingRef` guard be deleted** rather than extended. The spurious
`onBlur → requestFocusRestore → FOCUS` cycle remains (benign); a future explicit
render-generation id (named, not built — see the G6+G7 seam note) is the natural
next abstraction if scroll-sync / incremental-rebuild / freeze need to correlate
renders.

---

## Starting state (2026-06-16): CLEAN working tree

`git diff` is **empty** — this plan was committed and the tree reset, so **no fix
exists in the tree for any glitch**. `CURRENT.md` points here. Nothing accumulated;
implement everything below from scratch under TDD. The list below is a **map of
which file each glitch touches** (for the implementer), not a record of present
changes:

- **`PreviewRoot.tsx`** — G6+G7 settle-gate (`preCommitContentRef` + the
  `executeLanding` gate + snapshot at the three dirty-commit sites; the
  `relandSettlingRef` guard is **not** introduced); G5 (`keepExpanded` opt +
  conditional §7 reset + clean/dirty wiring); G9 (`fadeSourceR0Ref`, apply
  `useLayoutEffect`, `clearRelandFade`).
- **`dispatchers.tsx`** — G4 (`!isBareModifier`), G11 (textarea `onClick` expand),
  G12 (overflow-y clip rule, `SCROLLJACK_TOLERANCE_PX = 6`).
- **`useBlockEditHover.tsx`** — G8 (marker-aware `findEditTarget`), G9
  (`q2-reland-fade` keyframes/class).
- **`BreadcrumbChip.tsx`** — G13 (opaque pill) + G1 (extract `computeChipGeometry`,
  `CRUMB_W`, left-spill).
- **`nestingNav.ts`** — G2 (`abbrevForSourceNode` `Plain → 'Pl'`).
- **`caretGeometry.ts`** — G3 (`isOnLastVisualLine` `− paddingBottom`).
- **`crates/pampa/src/apply_node_edit.rs`** — G10 (`preserve_leaf_variant`).
- **`crates/pampa/tests/integration/node_edit_tests.rs`** — T8/T9/T10.

### Diagnostics

There are **no diagnostics in the tree** (all `[Q2-DIAG …]` scaffolding was
reverted). If you add temporary `console.log`s while implementing, use a greppable
`[Q2-DIAG …]` prefix and **strip them all** before declaring a glitch done — the
final code carries none.

### Implementation order (TDD from a clean tree, per the Test Seam Spec)

Suggested sequence (cheapest/most-isolated first); each step is write-test-RED →
apply-fix → GREEN:

1. **G10** (Rust, self-contained): **T8/T9/T10** RED → add `preserve_leaf_variant`
   → GREEN; later **T11** (host WASM round-trip, needs a WASM rebuild).
2. **G2** (pure unit): **T20** RED → split `abbrevForSourceNode` `Para`/`Plain` →
   GREEN.
3. **G3** (Playwright): **T21** RED → `isOnLastVisualLine` `− paddingBottom` → GREEN.
4. **G6+G7** (the keystone — others build on it): **T1/T2** RED → settle-gate
   (gate + snapshots + remove-the-guard-is-moot since it never existed) → GREEN;
   **T3** expands `p2-4d`/`p2-4-real`.
5. **G8**: **T4/T5/T6** against `useBlockEditHover`.
6. **G9** (depends on G6+G7's deterministic gap): **T7**; visual accepted-untested.
7. **G4/G5/G11**: **T12** (expand `s7-expand-on-edit`), **T13** (reland fixture),
   **T14** (textarea click).
8. **G12**: **T15** (browser tier). **G13/T16**: accepted-untested (optional e2e).
9. **G1**: extract `computeChipGeometry` + `CRUMB_W` + left-spill; **T17** (unit) +
   **T18** (integration) RED→GREEN; **T19** (e2e px) + live-tune `CRUMB_W`, record
   the chosen value in the G1 "Open tuning".
10. Ensure no `[Q2-DIAG …]` diagnostics remain; run `cargo nextest run --workspace`,
    `npm run build:all` + `npm run test:ci`, and `cargo xtask verify`.
