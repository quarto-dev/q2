# Breadcrumb chip — visual design + positioning rework

**Date:** 2026-06-15 (rewritten 2026-06-15 after a design review)
**Branch:** feature/block-editing-improvements (worktree `.worktrees/block-editing`)
**Builds on:** `2026-06-14-nesting-cursor-ui-enhancements.md` (geometry snapshot, caret-aware
nest-in, mode-aware highlight) — *successor* plan. The nesting cursor and its breadcrumb chip
(`BreadcrumbChip.tsx`, shipped commit `eccd89c2`) are the substrate.
**Status:** Design settled across the original brainstorm (2026-06-15) **and a follow-up review**
that changed the positioning model substantially (see *What changed in the rewrite*). TDD-first;
checklist below is ready to execute.

> **All cited files live under `ts-packages/preview-renderer/src/q2-preview/`** unless a path is
> given. The Playwright acceptance spec lives under `hub-client/e2e/` (that SPA bundles
> `preview-renderer` from source and runs the real WASM, so the iframe-side TS is exercised there).

## Overview

The breadcrumb chip works correctly but (a) **looks heavy** (default button chrome, full Pandoc
type names with `#id`/`.class`) and (b) **detaches on scroll** — it is positioned once by a
one-shot `useLayoutEffect` (`BreadcrumbChip.tsx:31-40`) with no scroll dependency, so it floats in
place while the editing surface scrolls away beneath it. This plan fixes both: a visual restyle
**and** a positioning rework so the chip lives *in the document plane* and tracks the surface with
zero scroll lag.

### What changed in the rewrite (read this if you saw the first draft)

1. **Positioning is no longer a "float above the surface."** The chip is anchored **in the content
   plane** so it scrolls natively with the surface (no JS on scroll, no lag). See §Positioning model.
2. **Horizontal layout = gutter-fill + margin-spill.** The crumbs occupy the *indent gutter* the
   nesting created: ancestors fill leftward from the surface's left edge; non-indenting containers
   (fenced `Div`) spill into the outer page margin; the page edge is the hard stop. See §Layout model.
3. **`§`/Section is dropped** (it was near-dead code). See §Why no Section crumb.
4. **Accessibility:** each crumb gains `aria-label` (a bare glyph would otherwise *be* the
   accessible name).
5. **Forward-crumb scaffolding:** the layout **pivots at surface-left** and reserves an (empty)
   right-side group + a `.q2-crumb-future` faded style, so the future-crumb feature
   (`2026-06-15-breadcrumb-forward-crumbs.md`) drops in without a re-layout. The *functionality* is
   out of scope here.

### What does NOT change (behavior contract)

Navigation is untouched: keyboard chord (`dispatchers.tsx` → `classifyNestingKey`), ◀/▶ buttons
(`requestNestingMove`), crumb-jump (`requestNestingSelect`), the `unlockNestingCursor && editTarget`
self-gate, pointer-isolation (`stopPropagation`/`preventDefault` on the chip), and the
ancestor-path *ordering/membership* (`buildAncestorPath` still returns the same surfaces in the same
order, with the same `isCurrent`). Only the rendered **text**, **CSS**, **DOM structure**, and
**position** change.

> **Forward reference (successor plan).** A successor —
> `2026-06-15-nesting-cursor-navigation-and-list-items.md` — changes **arrow** navigation, caret
> placement on dirty moves, and adds **list-item surfaces**. None alter the chord / ◀▶ / crumb-jump
> wiring or `buildAncestorPath` membership, so this plan's contract stands — **but this plan's tests
> must not depend on those** (see the test-robustness notes).

---

## Positioning model (the scroll fix)

**Mount: keep the chip at `#quarto-content`** (its current location, `PreviewDocument.tsx:298`) —
the full-width `page-columns` grid container (`:267-268`, spans screen-start → screen-end). This is
the **only** mount point from which the chip can paint into *both* the content gutter and the outer
page margin (the margin-spill requirement, §Layout model). It also keeps the chip **outside** the
edit region, so the pointer-isolation `contains()` guards (`useBlockEditHover.tsx:168, :225`) are
untouched, and the existing "must be a child of `#quarto-content`" constraint (`:293-296`) holds.

**Anchor in the content plane (no scroll listener).** Make `#quarto-content` `position: relative`
and position the chip `position: absolute` with coordinates computed **once per activation** (deps:
`editTarget` change), expressed **relative to `#quarto-content`** — which scrolls together with the
surface. Because both the chip's containing block and the surface share the same scroll, a
once-computed offset stays correct under scroll with **no recompute and no lag**.

> **Root cause (researched 2026-06-15, corrected from the first draft).** The iframe's scroll
> container is `#root { overflow: auto; height: 100vh }` (`q2-preview.html:9`), **not** the
> document/body. `#quarto-content` is a direct child of `#root` and currently has **no `position`**
> (just `display: grid`, `_bootstrap-rules.scss:309`). A non-positioned grid container is **not** the
> offset-parent for an abspos child, so today the chip's containing block resolves to the iframe's
> **initial containing block (the viewport)** — which does **not** move when `#root` scrolls
> internally, while the surface (inside `#root`) does. That is the detach. The concrete code defect is
> `BreadcrumbChip.tsx:34` using `surface.offsetParent` as `host` (a *different* element from the chip's
> real containing block). The fix: give `#quarto-content` `position: relative` (becomes the chip's
> offset-parent) and look it up **directly** (`document.getElementById('quarto-content')`), not via
> `surface.offsetParent`. Both then share `#root`'s scroll → once-computed offset is scroll-stable.
>
> **Stacking-context caveat.** Making `#quarto-content` positioned creates a **stacking context**.
> Audit its z-indexed children (sidebar, TOC, page-nav, any overlay rendered inside `#quarto-content`)
> for a regression before relying on this. The attribution overlay is rendered *outside*
> `#quarto-content` (`PreviewDocument.tsx:306`), so it is unaffected.

**Vertical:** `top = surfaceTop − chipH` (chip **bottom edge flush at the surface top**), out of
flow (no reflow), with a **high `z-index`** so it **occludes the content/gap above** the surface
rather than being occluded by it. (This is the user's "fixed, higher in z-order, occludes previous
content" — *not* CSS `position: fixed`.)

> **No top clamp.** `top` is left un-clamped even at the document top. The page's top margin (page
> padding + title block above the first editable block) keeps `surfaceTop > chipH`, so `top` stays
> positive — the chip never paints above `#quarto-content`'s top edge. (Assumption, per the user:
> a document whose very first block is the active surface *and* has near-zero top margin is the only
> edge case that could push `top` negative; accepted as out of scope.) The old e2e assertion
> `chipBox.y ≥ 0` becomes a natural consequence, not an explicit CSS clamp.

> **Rejected alternative — Option A (chip inside `#q2-active-edit-region`).** Anchoring the chip to
> the edit-region wrapper would scroll for free with *pure CSS* (no JS geometry) and was the leading
> option until the margin-spill requirement. It is **rejected** because `#q2-active-edit-region`
> lives in the `main.content` body-content column; the chip could not paint **left of** the text
> column into the outer page margin (it would be contained/clipped by the column). Keep it noted: if
> the margin-spill is ever cut, Option A becomes the simpler choice again.

---

## Layout model (gutter-fill + margin-spill)

The crumbs are **not** a fixed barrel floating above the surface. They occupy the **indent gutter**
that nesting created — the horizontal band from the active surface's left edge back toward (and past)
the page text-column margin. **Surface-left is the pivot.**

```
 page          text-column                         surface
 edge            margin (colLeft)                  left (pivot)
  |  ◀  Dv  Dv  | ❝  •  ¶ |                     [ editing surface text … ]
  |<- outer margin ->|<--- content gutter --->|<- (right: future-crumbs, empty for now) ->
```

- **Ancestors grow LEFT** from the pivot, innermost nearest the surface, outermost furthest left.
- **Indenting containers** (BlockQuote, BulletList, OrderedList, DefinitionList) consume the
  **content gutter** (between `colLeft` and the surface left): each crumb ≈ the indent its container
  contributes, so for pure list/quote nesting the crumbs **tile the gutter end-to-end naturally**.
- **Non-indenting containers** (fenced `Div`, **`Figure`** — the only non-indenting types that can
  be *ancestor* crumbs, see §Reachability note under the abbreviation table — and any zero-indent
  case incl. top-level) have no gutter to fill, so their crumbs **spill LEFT past `colLeft` into the
  outer page margin**. Two fenced Divs → `◀ Dv Dv` sitting in the page margin, the surface itself at
  the text column. (The leaf/current crumb participates too — in the all-zero-indent case it also
  spills, abutting the surface.)
- **◀ (out)** sits furthest left of all crumbs (protruding into the margin / toward the page edge).
- **Hard stop at the page edge**, then **compress**: shrink per-crumb width to a min legible glyph,
  then collapse middle crumbs to an ellipsis `…`. (Needs more than ~2 zero-indent crumbs to trigger.)
- **Right of the pivot** is reserved (empty) for future-crumbs — scaffolding only (see §Forward-crumb
  scaffolding).

**Per-crumb width — how to size to the indent.** Walk the active surface element's **DOM ancestors**
that carry `[data-block-pool-id]` (the rendered `blockquote`/`ul`/`ol`/`dl`/`div`/`figure` wrappers) and read
each `getBoundingClientRect().left`; crumb *i*'s band is `[ancestor_i.left, ancestor_{i+1}.left]`
(innermost band ends at the surface left). A **zero/near-zero band ⇒ min-width ⇒ spill left** falls
out automatically. **Fallback** (a source ancestor with no matching DOM element): approximate by
container type (list/ordered ≈ list indent, blockquote ≈ quote indent, div ≈ glyph width). All of
this is **measured once at activation** (content-plane, scroll-stable).

> **Imperfections (accepted).** Clean tiling holds for *pure* list/blockquote nesting. Mixed nesting
> over-provisions: BlockQuote→Div→Para is three crumbs but one indent step, so they compress to share
> it. `Div` nesting without visual indent is the expected imperfect case — "close in the common
> cases" is the bar, not pixel-perfect.

---

## Why no Section crumb (design note)

The source index is built from the **untransformed** AST (`sourceIndex.ts`, `buildSourceIndex`),
because the breadcrumb crumbs are **navigation targets into editable source ranges** —
`requestNestingSelect(r0, r1)` (`BreadcrumbChip.tsx:92`) jumps to real source bytes, and the nesting
cursor only roves over surfaces that *have* source bytes. `resolveSource` (`PreviewRoot.tsx:1273`)
bridges a rendered (transformed) block to its untransformed counterpart by **SourceInfo value**.

**sectionize** wraps `Header + content` into **Generated** section `Div`s that exist only in the
*transformed* AST and have **no source bytes**; they are therefore **absent from the untransformed
index by construction** (`sourceIndex.ts:11-12`). So `§`/Section would only ever fire for a
hand-authored `::: {.section}` Div (rare, and merely a `Div` carrying a `section` class — not a
distinct type). Rather than ship a near-dead mapping + a class-sniff, **drop `§` entirely**.

**Consequence:** the breadcrumb shows the **editable containment path** (Div / BlockQuote / list
ancestors), **not** the visual section outline. A paragraph under an `## H2` shows no "Section"
crumb, because in source-truth terms a header and the following paragraphs are *siblings*, not
parent/child. Synthesizing a section outline (display-only crumbs derived from the transformed AST
or header levels) is **possible in theory but out of scope** — note it as a possible future feature,
not a bug.

---

## Decision tables (the spec)

### Abbreviations (hybrid glyphs)

| Pandoc type        | Glyph        | Category   |
| ------------------ | ------------ | ---------- |
| Header (level n)   | `H1`…`H6`    | leaf-text  |
| Div                | `Dv`         | container  |
| BlockQuote         | `❝`          | quote      |
| BulletList         | `•`          | list       |
| OrderedList        | `1.`         | list       |
| DefinitionList     | `DL`         | list       |
| CodeBlock          | `Cd`         | embed      |
| Figure             | `Fg`         | embed      |
| Table              | `Tb`         | embed      |
| Para / Plain       | `¶`          | leaf-text  |
| *fallback (other)* | first 2 chars of type | leaf-text |

Header level is read from `node.c[0]` (the level int); `labelForSourceNode` already branches on
`t === 'Header'` reading `c[1]` for the Attr, so the node shape is known. **`Section`/`§` is
intentionally absent** (see §Why no Section crumb). All other rows are reachable as non-Opaque crumbs
(verified against `sourceIndex.ts`: top-level + Div/BlockQuote/list/Figure-body descendants are
non-Opaque; only Table cells and Figure/Table captions are Opaque).

> **Reachability note — ancestor crumbs vs. leaf-only (researched 2026-06-15 against `sourceIndex.ts`
> `descendBlock`).** Only **Div, BlockQuote, BulletList, OrderedList, DefinitionList, and Figure**
> (via its *body* blocks) descend into indexed children, so **those are the only types that can ever
> appear as a *containing ancestor* crumb.** **CodeBlock (`Cd`), Table (`Tb`), Header, Para, Plain**
> are not descended into — they can only ever be the **innermost/current** crumb. The abbrev/category
> functions still need every row (the current crumb can be any leaf type), but the gutter/spill
> *layout* (§Phase 3b/3c) only handles the 6 ancestor-capable types, of which **`Div` and `Figure`
> are non-indenting** (margin-spill) and **BlockQuote / the three list types are indenting**
> (gutter-fill).

### Color palette (by category)

| Category   | Members                          | Hue     |
| ---------- | -------------------------------- | ------- |
| container  | Div                              | indigo `#4f46e5` |
| list       | BulletList, OrderedList, DefinitionList | green `#15803d` |
| quote      | BlockQuote                       | amber `#b45309` |
| leaf-text  | Para, Plain, Header              | blue `#0284c7`  |
| embed      | CodeBlock, Figure, Table         | teal `#0f766e`  |

Tuned for a light background. **Dark-mode is out of scope** (the preview's theme story is separate);
flag as a follow-up if the chip looks wrong against the dark theme.

### Tooltip

Each crumb's `title` = the existing full label (`labelForSourceNode` output). **Note the exact
format:** `${t}#${id}` or `${t}.${classes[0]}` — **no space** (e.g. `Div.d`, `Header#h-id`, `Para`),
per `nestingNav.ts:481-482`. Arrow tooltips unchanged (`Out (⌘⌃←)` etc.).

### Accessibility

Each crumb `<button>` currently has **no `aria-label`**; its accessible name comes from `textContent`
(the full label). Replacing the text with a glyph would silently degrade the accessible name to e.g.
"bullet". **Add `aria-label={c.label}`** (the full type) to each crumb so the accessible name is
stable regardless of glyph. `title` is *not* an accessible name. Keep `aria-current` as-is. Color is
not the sole information channel (glyph + tooltip + aria-label carry it), so WCAG 1.4.1 is satisfied.

---

## Where the code lives (verified against the worktree 2026-06-15)

- **`nestingNav.ts:459` `labelForSourceNode`** — returns full names (no space: `Div.d`,
  `Header#h-id`). Other caller: `buildAncestorPath` (`:500`). **Keep as-is**; reuse for the tooltip.
- **`nestingNav.ts:500` `buildAncestorPath`** — produces `AncestorCrumb[]` `{label, r0, r1,
  isCurrent}`. Extend with `abbrev: string` + `category: CrumbCategory`.
- **`BreadcrumbChip.tsx:31-40`** — the one-shot `useLayoutEffect` positioning. **Replace** per
  §Positioning model (content-plane anchor; recompute on `editTarget` change, not scroll).
- **`BreadcrumbChip.tsx:56-105`** — markup. Render `c.abbrev`, add `title={c.label}` +
  `aria-label={c.label}` + category class; pivot the layout at surface-left; add the empty
  right-group. Move container styles into an injected stylesheet (pattern: the `<style>` block at
  `useBlockEditHover.tsx:315`); keep only computed geometry inline.
- **`PreviewDocument.tsx:267-298`** — `#quarto-content` host (mount point) + `<BreadcrumbChip />`.
  Ensure `#quarto-content` is the chip's positioned offset-parent.
- **`dispatchers.tsx:67`** — `#q2-active-edit-region` wrapper (the surface the chip anchors to, via
  `ctx.activeEditRegionRef`).

## Test inventory (these assert what we are changing)

1. **`nestingNav.test.ts`** — `labelForSourceNode` cases stay green. **Add** `abbrevForSourceNode` +
   `categoryForSourceNode` cases and extend `buildAncestorPath` to carry `abbrev`/`category`.
2. **`p3-4-breadcrumb.integration.test.tsx`** — **there are 7 tests, not 6.**
   - **Test 4** (`:216`) asserts crumb `textContent` `['Div.d','Para']` → update to `['Dv','¶']`; add
     `title`/`aria-label` = full label (`'Div.d'` — **no space**, matching the fixture's class `"d"`),
     and a category-class assertion.
   - **Tests 6** (`:296`, `:308`) **and 7** (`:345`, `:362`) select the crumb via
     `find(c => c.textContent === 'Div.d')` — which the abbreviation **breaks**. Switch the *selector*
     to `find(c => c.getAttribute('title') === 'Div.d')`; **leave the behavioral assertions
     unchanged**. (These are the nav/jump behavior-contract proofs.)
   - **Tests 1-3** (`:166`/`:184`/`:196`) and **test 5** (`:248`, selects via `.q2-breadcrumb-out/in`)
     stay **byte-for-byte unchanged**.
   - **Robustness (successor plan):** tests 6-7 assert landed **surface/range**, not a **caret
     column** — keep it that way (the successor's dirty-move caret changes would otherwise force an
     edit to a "must-stay-green" test). Audit when executing.
3. **`p3-3-unlocked-subclauses.integration.test.tsx`** — this is the **ancestor-only re-derive**
   fail-on-revert test (`:184-305`). It has **four** `textContent` assertions, not one:
   - `:262` `crumbs1 == ['Div.a','Para']` (step 2, first render)
   - `:265` `currentCrumb1.textContent == 'Para'`
   - `:300` `crumbs2 == ['Div.b','Para']` (step 5, **after** the Div class flips `a → b`)
   - `:303` `currentCrumb2.textContent == 'Para'`

   **Do NOT naively flatten all four to `['Dv','¶']`.** This test's entire fail-on-revert power is
   that the **ancestor label changes** `Div.a → Div.b` across the re-render (a memoized path would
   show stale `Div.a` → RED at step 5). Abbreviation collapses **both** `Div.a` and `Div.b` to `Dv`,
   so a flattened step-5 assertion (`['Dv','¶'] == ['Dv','¶']`) would be **identical before and after
   and the test would go vacuous** — it could no longer detect the stale-path bug.

   **Correct migration** (mirrors the `p3-4` 6 & 7 pattern): keep the `textContent` checks on the
   *abbreviated* glyphs (`['Dv','¶']` both renders) **for the gating/shape**, but **move the
   discriminating assertion onto the full label** via `title`/`aria-label`: step 2 asserts the
   ancestor crumb's `getAttribute('title') === 'Div.a'`; step 5 asserts `=== 'Div.b'`. The full label
   still differs, so the regression is still caught. (Add this as the explicit Phase-2 item below.)
   Grep for every occurrence before editing.
4. **`hub-client/e2e/q2-preview-breadcrumb-geometry.spec.ts`** — currently a *static, vertical-only*
   snapshot: it asserts (a) chip bottom ≤ surface top, (b) gap < 12px, (c) `chipBox.y ≥ 0`. **It does
   NOT assert any horizontal/left alignment** (the first draft of this plan said it did — it does
   not). **Rewrite** per §Phase 4 — adding the **scroll-tracking** assertion (the case the static
   snapshot misses) and the **margin-spill / overflow-clip** checks. Assertion (c) survives as a
   natural consequence of the page top margin (no explicit clamp — see §Positioning).

---

## Test Seam Spec (pre-validated — coordinator handoff, 2026-06-16)

> **Purpose.** The test-design pass was done **up front, in the coordinator context**, reading the real
> code — so an executor (subagent or solo) never has to *invent* how to bind a test. Each row below names
> the **tier**, the **real module under test** (no reimplementation), the **seam** (mount + events +
> assertion surface), the **mock boundary**, and the **named revert hunk** whose removal must turn the
> test RED. A test that can pass with its revert hunk neutralized is theater — fix the test, not prod.
> **Frozen rule:** once a test goes green, its assertions + harness are frozen; do not edit a test to
> make it green. Verified line numbers are against the worktree at 2026-06-16.

**Tiering decision (where the risk lives).** Pure label/category arithmetic and the path-derivation /
self-heal / move-reland state machine are deterministic in jsdom → **jsdom, driving the real
`PreviewRoot`** (mock only `getBoundingClientRect`). Anything that depends on a real layout engine —
chip↔surface geometry, scroll-tracking, gutter-fill bands, margin-spill, overflow/scrollbar — is **not
measurable in jsdom** (it returns zero rects) → **headless Playwright** against the real hub preview.
Do not push layout into jsdom (it will measure zeros and pass vacuously); do not push pure logic into
Playwright (slower, lower-faithful for what jsdom already proves).

### Tier A — pure unit (`nestingNav.test.ts`, jsdom, no DOM)

| Item | Real unit | Seam (call + assertion surface) | Mock | Named revert hunk → RED |
| ---- | --------- | ------------------------------- | ---- | ----------------------- |
| **0a** | `abbrevForSourceNode(node)` | Direct calls with hand-built `BlockNode` literals, **mirroring the existing `labelForSourceNode` cases at `:450–474`**; assert the returned string per the abbreviation table. | none (pure) | The per-type glyph map / `c[0]` Header-level read. Revert to returning `node.t` → `Header(2)` yields `'Header'` not `'H2'` → RED. |
| **0b** | `categoryForSourceNode(node)` | Same literal-driven call shape; assert the `CrumbCategory`. | none | Revert to a constant `'leaf-text'` → `Div` expecting `'container'` → RED. |
| **0c** | `buildAncestorPath(SI, r0, r1)` carries `abbrev`+`category` | Reuse the **real `SI = buildSourceIndex(JSON.stringify(ANCESTOR_AST))`** already at `:417`; extend the existing `.toEqual([...])` objects (`:421–438`) with the new fields. | none | The `.map(...)` popul­ating `abbrev`/`category` in `buildAncestorPath` (`nestingNav.ts:528–533`). Drop the fields → `toEqual` full-object match → RED. |

### Tier B — real-component integration (jsdom, drive the real `PreviewRoot`)

> Harness is **already correct** in these files: real `PreviewRoot`, real chip, real context callbacks
> driven through the real buttons; the *only* mock is `mockTileRects` (spies `getBoundingClientRect` —
> a genuine browser-geometry dependency jsdom lacks). Do not add new mocks; do not reimplement nav.

| Item | File / test | Seam (mount → events → assertion surface) | Named revert hunk(s) → RED |
| ---- | ----------- | ----------------------------------------- | -------------------------- |
| **0d / 2c-proof** | `p3-4` test 4 (`:216–233`) | `mountFixture({unlockNestingCursor:true})` → `openEditor(container,'2')` (real `pointerdown`/`up`) → `chip(container)`. Assert `.q2-crumb` `textContent === ['Dv','¶']`; **each crumb `getAttribute('title')`/`getAttribute('aria-label')` === full label** (`'Div.d'` / `'Para'` — **no space**); category class `q2-crumb-cat-container` on `crumbs[0]`; keep the `aria-current` checks byte-for-byte. | **Four independent reverts**, one per prod change: (i) `abbrevForSourceNode` → textContent RED; (ii) drop `title={c.label}` → title RED; (iii) drop `aria-label={c.label}` → aria RED; (iv) drop `q2-crumb-cat-${category}` → class RED. |
| **0e** | `p3-4` tests 6 (`:308`) & 7 (`:361`) | **Selector swap only:** `find(c => c.textContent === 'Div.d')` → `find(c => c.getAttribute('title') === 'Div.d')`. **Behavioral assertions unchanged** (caret-follow ▶; dirty-commit-reland). | Original behavioral hunks unchanged: 6 ← caret-aware `childSurfaceTowardLine` descent; 7 ← `requestNestingSelect` commit-if-dirty + `resolveLanding kind:'crumb'`. The selector swap is **test-only**; verify 6/7 still revert RED on those *behavioral* hunks, not on the selector. **Ordering: 2c (title attr) must ship before re-running 6/7**, else `divCrumb!` is `undefined` → setup throw (not a behavioral RED). |
| **2d** | `p3-3-unlocked-subclauses` test 2 (`:224–305`) | `render(makeAst2('a'))` → open child pool-id `1` → assert → `rerender(makeAst2('b'))` → assert re-derive. Shape/gating on abbreviated glyphs: `['Dv','¶']` (`:262`/`:300`), current `'¶'` (`:265`/`:303`). **Discriminator on the full label via `title`:** step 2 ancestor crumb `getAttribute('title') === 'Div.a'`; step 5 `=== 'Div.b'`. | The exact hunk this test exists to guard: memoizing `buildAncestorPath` on `[anchorR0,anchorR1]` (stable across the ancestor-only change). Revert (add the memo) → step-5 title stays `'Div.a'` → `=== 'Div.b'` RED. **⚠ Vacuity trap:** the discriminator may **not** live on `textContent` — abbreviation collapses both `Div.a` and `Div.b` to `Dv`, so a flattened `['Dv','¶']`==`['Dv','¶']` step-5 check is identical before/after and survives the memo revert (theater). It **must** be `title`. |

### Tier C — real layout engine (Playwright, `hub-client/e2e/q2-preview-breadcrumb-geometry.spec.ts` rewrite)

> All measure via real `boundingBox()` / `evaluate`. Mount = real hub preview iframe through
> `projectFactory`. No mocks. The current spec is vertical-only (a/b/c); the rewrite adds the
> horizontal + scroll cases. **Each geometry assertion needs a named revert** except the explicit
> go/no-go gate (4a), which is a *design-validation* gate, not a regression guard — say so in the spec.

| Item | Fixture | Seam + assertion surface | Named revert hunk → RED (or gate) |
| ---- | ------- | ------------------------ | --------------------------------- |
| **4a** | deep zero-indent (fenced Div ×N) | (i) `#quarto-content` `boundingBox().x ≈ 0`; (ii) **no `#root` horizontal scrollbar**: `evaluate(#root ⇒ scrollWidth <= clientWidth + 1)`. | **GATE, not a revert test.** If either fails → **stop & reassess** (Option A fallback). Mark `test.fixme`/comment as a gate so a future reader doesn't mistake it for coverage. |
| **4b** | **tall** doc, active surface **mid-document** | Open editor; `gap = taBox.y − (chipBox.y+chipBox.height)`; **scroll the iframe `#root`** via `evaluate(#root ⇒ scrollTop += 400)` (NOT `window`); re-measure; assert `gap` unchanged within TOL. | Revert **3a** (restore `surface.offsetParent` host / non-`relative` `#quarto-content`) → chip detaches, gap changes → RED. **⚠ Vacuity trap (plan 4b):** scrolling `window` instead of `#root` leaves content unmoved → the test passes with or without the fix. The scroll target is load-bearing. |
| **4c** | indented: **loose-list `Para`** or **blockquote `Para`** (NOT a tight-list `Plain` — successor turns those into directly-activated surfaces and shifts the rect) | Assert crumb row spans ≈ `colLeft → surfaceLeft`. | Revert **3b** (gutter band measurement) → crumbs render at natural width, not tiled → span wrong → RED. |
| **4d** | fenced-`Div`-wrapped (zero indent) | Assert crumbs sit **left of** the text-column margin (outer page margin); `◀` is leftmost. | Revert **3c** (margin-spill) → crumbs sit at `surfaceLeft` → RED. |
| **4d-fig** | editable block inside a `:::{#fig-…}` **Figure body** | Assert the `Fg` ancestor crumb renders and the row spills into the page margin like 4d. | Revert **3c** *and* the Figure ancestor-walk path (`Figure.tsx:31` pool-id) → RED. |

### Coverage gaps found by the prep pass (reasoning fills what revert cannot)

Fail-on-revert validates *existing* tests; it cannot find a *missing* one. Reading the plan against the
code surfaced four:

- **G1 — compression/ellipsize (3c "min glyph width → ellipsize middle crumbs") is untested.** No
  Phase-4 item exercises it, and its threshold is an open question. This is **load-bearing safety**: it
  is the rule that keeps the leftmost crumb `≥ x≈0` so deep nesting never adds an `#root` horizontal
  scrollbar (overlaps 4a). **Decision required:** once the min-glyph/ellipsize rule is defined, add
  **4d-compress** (deep zero-indent forcing overflow; assert middle crumbs ellipsized **and** leftmost
  `≥ 0`), revert hunk = the clamp/compress branch in 3c. If deferred, **`log()`/note it explicitly** —
  silent truncation reads as "covered."
- **G2 — sidebar margin-occupancy (open question) fixture under-specified.** Needs a concrete
  `_quarto.yml` (left sidebar or `toc-location: left`). Spec it into 4a/4d or accept-and-note.
- **G3 — forward-crumb scaffolding (2b: pivot + empty right group + `.q2-crumb-future`) is a
  structural contract the successor plan drops into, with no assertion.** Add a **cheap jsdom assertion**
  (in `p3-4` test 3 or a new test) that the `.q2-breadcrumb-future` placeholder element exists when the
  chip renders — so the successor's drop-in contract is guarded. Revert hunk = removing the placeholder
  span in 2b.
- **G4 — the faint scrim band + glyph color (2a) are cosmetic** → no assertion (correctly untested);
  category *class* is asserted in 0d, which is the testable surface. No action.

---

## Phase 0 — Test specs (TDD; write first, watch them fail)

- [x] **0a.** `nestingNav.test.ts`: `abbrevForSourceNode` cases for every abbreviation-table row
  (Header level → `H2`; Div → `Dv`; BlockQuote → `❝`; BulletList → `•`; OrderedList → `1.`;
  DefinitionList → `DL`; CodeBlock → `Cd`; Figure → `Fg`; Table → `Tb`; Para/Plain → `¶`; unknown →
  first-2-chars). **No `§` case** (Section is dropped).
- [x] **0b.** `nestingNav.test.ts`: `categoryForSourceNode` cases (container/list/quote/leaf-text/
  embed). `Div → container`.
- [x] **0c.** `nestingNav.test.ts`: extend `buildAncestorPath` to assert each crumb carries `abbrev`
  + `category` alongside the unchanged `label`/`isCurrent`/range.
- [x] **0d.** `p3-4` test 4: expected `textContent` `['Dv','¶']`; assert
  `crumbs[0].getAttribute('title') === 'Div.d'` and `crumbs[0].getAttribute('aria-label') === 'Div.d'`
  (full label, **no space**); current crumb's title/aria-label `'Para'`; category class on each crumb.
- [x] **0e.** `p3-4` tests 6 & 7: change the crumb selector from `textContent === 'Div.d'` to
  `getAttribute('title') === 'Div.d'`; **do not touch the behavioral assertions**.
- [x] **0g.** (gap G3) `p3-4` test 3 (or a sibling): assert the **forward-crumb placeholder**
  `.q2-breadcrumb-future` element exists when the chip renders — guards the successor plan's drop-in
  contract. Revert hunk = the placeholder span added in 2b.
- [x] **0f.** Run unit + integration suites; confirm 0a-0e/0g **fail** for the right reasons
  (functions/fields/attributes not yet present) and tests 1-3/5 still pass.

## Phase 1 — Label/category model (`nestingNav.ts`)

- [x] **1a.** Add `type CrumbCategory = 'container' | 'list' | 'quote' | 'leaf-text' | 'embed'`.
- [x] **1b.** Add `abbrevForSourceNode(node): string` — pure, mirrors `labelForSourceNode`'s
  defensive style; Header level from `c[0]`; **no `§`/section special-case**; first-2-chars fallback.
- [x] **1c.** Add `categoryForSourceNode(node): CrumbCategory` per the palette table. **Specify the
  default branch:** types not in the palette (e.g. `RawBlock` → `Ra`, `LineBlock` → `Li`, or any future
  type — these are reachable as the *current* crumb and emit `data-block-pool-id`) fall back to
  **`leaf-text`** (blue). The abbreviation table's "first 2 chars" fallback and this category fallback
  must agree on the same default. (Indent behavior is keyed on **node type** — §Layout — not on
  `category`, which is color only; do not conflate them.)
- [x] **1d.** Extend `AncestorCrumb` with `abbrev` + `category`; populate both in `buildAncestorPath`
  (reuse `labelForSourceNode` for the unchanged `label`). Verify 0a-0c go green.

## Phase 2 — Chip markup + styling (`BreadcrumbChip.tsx`)

- [x] **2a.** Add an injected `<style>` node (pattern: `useBlockEditHover.tsx:315`) defining
  `.q2-breadcrumb-chip`, `.q2-crumb`, `.q2-crumb-current`, `.q2-breadcrumb-out/in`, the five category
  color classes (`.q2-crumb-cat-container` etc.), and **`.q2-crumb-future`** (reduced opacity — the
  forward-crumb scaffolding). Direction-B styling: colored glyph text, ghost arrows (no
  border/background, faint hover bg), ~12px text, current = bold + underline, non-current `:hover`
  underline. **Separators (`›`) are dropped** — spatial position conveys order in the gutter layout.
  **Background treatment (decided):** drop the barrel box. Paint a **faint scrim band behind the
  breadcrumb area** — a subtle translucent backing under the crumb row's occupied region; it does
  **not** have to span the full row. It gives the glyphs legibility over whatever content the chip
  occludes, without the heavy pill chrome. Tune opacity/extent during 2e/4f.
- [x] **2b.** Restructure markup as **three zones pivoting at surface-left**: a left group
  (`◀` + ancestor crumbs, including the current) that grows leftward, the surface-left **seam**, and
  an **empty right group** (`<span className="q2-breadcrumb-future" />` placeholder) for future-crumbs.
- [x] **2c.** Render `c.abbrev` as button text; add `title={c.label}`, `aria-label={c.label}`, and
  `q2-crumb-cat-${c.category}`. Keep `aria-current` and all `onClick`/`onPointerDown` handlers
  byte-for-byte (the behavior contract).
- [x] **2d.** `p3-3` (`:184-305`): render-shape assertions move to abbreviated glyphs
  (`['Dv','¶']` at `:262`/`:300`; current crumb `'¶'` at `:265`/`:303`), **but the discriminating
  ancestor-label assertion moves onto the full label** — step 2 asserts the ancestor crumb's
  `getAttribute('title') === 'Div.a'`, step 5 asserts `=== 'Div.b'` (so the stale-path regression is
  still caught; see Test inventory §3 — do **not** flatten both to `Dv`). Grep first; keep all
  chip-class/gating assertions.
- [x] **2e.** Move container/flex styles into the stylesheet; leave only computed geometry inline.

## Phase 3 — Positioning (content-plane anchor; the scroll fix)

> Phase 3a fixes the user's actual bug (float-doesn't-scroll). 3b/3c layer on the gutter aesthetics.

- [x] **3a.** **Kill the float.** Mount stays at `#quarto-content`; make it `position: relative`
  (the chip's offset-parent — it currently has none, which is the root cause; see §Positioning).
  Replace the `useLayoutEffect`: take the host as `document.getElementById('quarto-content')`
  **directly** (NOT `surface.offsetParent` — that is the current defect at `BreadcrumbChip.tsx:34`);
  compute, **relative to that host**, `surfaceLeft`/`surfaceTop` (and `colLeft =
  main#quarto-document-content left`), set `top = surfaceTop − chipH`, high `z-index`; **recompute on
  `editTarget` change only, never on scroll**. Keep null-safety on zero rects (jsdom). Right-anchor
  the crumb row at `surfaceLeft` with natural widths for now (proves the scroll fix independent of
  gutter measurement). **Then audit the new stacking context** `position: relative` creates on
  `#quarto-content` (sidebar/TOC/page-nav z-order — see §Positioning caveat).
- [x] **3b.** **Gutter-fill.** Walk the surface's `[data-block-pool-id]` DOM ancestors; size each
  ancestor crumb to its measured indent band `[ancestor_i.left, ancestor_{i+1}.left]` (innermost ends
  at `surfaceLeft`); fallback to approximate-by-type when a DOM ancestor is missing (list/ordered ≈
  list indent, blockquote ≈ quote indent, **`Div`/`Figure` ≈ glyph width** — non-indenting). *Verified
  2026-06-15:* all six ancestor-capable wrappers emit `data-block-pool-id` when editable — `Div.tsx:84`,
  `BlockQuote.tsx:12`, `BulletList.tsx:41`, `OrderedList.tsx:48`, `DefinitionList.tsx:26`, `Figure.tsx:31`
  (the `<figure>` itself; body blocks nest with their own pool-ids) — so the ancestor walk finds them.
  (A `<figure>` may carry a small default inline margin; band-measurement absorbs it automatically,
  which is why we measure rather than assume.)
- [x] **3c.** **Margin-spill + edge clamp.** Zero/near-zero bands get a min-width and spill left of
  `colLeft` into the outer page margin; `◀` furthest left; **clamp the leftmost crumb at
  `#quarto-content`'s left edge (x ≈ 0)** — the "page edge" hard stop. This is the rule for "use only
  the left margin we have available": confine the divs + `◀` to `[0, colLeft]`. **Overlapping a left
  sidebar that occupies that band is acceptable** (the chip is `position:absolute` + high z-index, so
  it paints over the sidebar — it does NOT scroll). **Going past x ≈ 0 is NOT acceptable** (it leaves
  `#root`'s content box → horizontal scrollbar). When the divs + `◀` don't fit `[0, colLeft]`,
  compress: min glyph width → ellipsize middle crumbs.
- [x] **3d.** Confirm it never reflows (already `position:absolute`) and that the chip can paint into
  the outer margin (no clipping — see Phase 4 overflow check).

## Phase 4 — e2e geometry rewrite + verification

- [x] **4a.** **Overflow go/no-go (refined by research 2026-06-15).** The iframe scroller is
  `#root { overflow: auto }` (`q2-preview.html:9`); `.page-columns`/`#quarto-content` have **no
  `overflow-x: hidden/clip`** (default `visible`). The safe path: `#quarto-content` spans the **full
  screen width** (`grid-column: screen-start/screen-end`), so the outer page margin is a grid column
  *inside* `#quarto-content` — the chip can paint there at a **positive `left`** within
  `#quarto-content`'s box, **not** a negative one. The edge clamp (3c) is therefore load-bearing:
  it must keep the leftmost crumb **≥ `#quarto-content`'s left edge (≈ screen x 0)** so nothing
  spills past `#root`'s content box and triggers a horizontal scrollbar / clip. **Verify in-browser:**
  (i) `#quarto-content` left ≈ 0; (ii) deep margin-spill does not add an `#root` horizontal
  scrollbar. If either fails — **stop and reassess** (Option A is the fallback if margin-spill is cut).
- [x] **4b.** **Scroll-tracking assertion** (the bug): a **tall** fixture with the active surface
  **mid-document**; open the editor, record chip↔surface gap, **scroll the iframe**, assert the gap is
  unchanged (chip stays glued, no lag). **NOTE: scroll the `#root` element inside the iframe** (the
  actual scroller — `q2-preview.html:9`), **not** the page/window — scrolling the window will not move
  the content and the test would pass vacuously.
- [x] **4c.** **Gutter-fill geometry:** an **indented** fixture (a *loose*-list `Para` or a
  blockquote `Para` — **not** a tight-list `Plain`, which the successor turns into a directly-activated
  surface and would shift this fixture's rect). Assert the crumb row spans ≈ from the content-column
  margin to the surface left.
- [x] **4d.** **Margin-spill geometry:** a **fenced-`Div`-wrapped** fixture (zero indent). Assert the
  crumbs sit **left of** the text-column margin (in the outer page margin) and `◀` is leftmost.
- [ ] **4d-fig.** **Figure margin-spill geometry:** a fixture with an editable block inside a
  `![cap](img){#fig-x}`/`:::{#fig-…}`-style **Figure body** (figures are common, not rare, and are
  non-indenting → margin-spill). Assert the `Fg` ancestor crumb renders and the crumb row spills into
  the page margin like the Div case. (Confirms the Figure-as-ancestor path end-to-end: `Figure.tsx:31`
  emits the wrapper pool-id, body block nests with its own.)
- [x] **4d-compress.** (gap G1) Once the min-glyph/ellipsize rule is defined (Open question), add a
  Playwright test: deep zero-indent nesting that forces overflow; assert middle crumbs ellipsized **and**
  leftmost crumb `≥ x≈0` (no `#root` horizontal scrollbar). Revert hunk = the clamp/compress branch in
  3c. **If deferred, note it explicitly here** — do not let it read as covered.
- [ ] **4g.** (gap G2) Define the sidebar margin-occupancy fixture concretely (`_quarto.yml` left
  sidebar or `toc-location: left`); fold into 4a/4d or explicitly accept-and-note.
- [x] **4e.** `npm run build:all` from `hub-client/` (production build is stricter than vitest);
  then `cd hub-client && npm run test:ci` (vitest) green; then the rewritten Playwright spec:
  `cd hub-client && npx playwright test e2e/q2-preview-breadcrumb-geometry.spec.ts --project=chromium`.
- [x] **4f.** **End-to-end visual check** (per CLAUDE.md): launch the hub/preview against a nested
  fixture with `unlockNestingCursor` on; open deeply-nested blocks (list, blockquote, fenced Div) and
  confirm by eye: scrolls glued to the surface; gutter-fill for list/quote; margin-spill for Divs;
  abbreviated colored glyphs; occludes content above (z-index); tooltip + aria on hover. **Record the
  invocation + an observation note here.**

## Design evolution (2026-06-16, after in-browser iteration)

The §Layout model above ("gutter-fill + margin-spill" — Divs/Figures spill into the
outer margin) was **revised during Phase 4 browser verification** to a cleaner rule the
user settled on. Recording the final design + the two root causes here; the prose above
is kept for history but **this section is authoritative for the shipped behavior.**

**Final layout rule (per active block):**
- **◀ (out-arrow) sits in the outer margin with its RIGHT edge flush at the text-column
  left (`colLeft` = `main#quarto-document-content` left).** It is the *only* thing in the
  margin.
- **The crumb row fills the indent gutter `[colLeft, surfaceLeft]`** and flexes so its
  right edge **meets the surface left** (the pivot). Crumbs never enter the outer margin.
- **▶ (in-arrow) + the future placeholder sit just right of `surfaceLeft`** (over content).
- **`surfaceLeft` is the `<textarea>`'s left, not the `#q2-active-edit-region` wrapper's.**
  The wrapper spans the full text column (left = `colLeft`) for every block, so anchoring
  to it lost the indent; the textarea sits at the block's real (indented) content left.
- **Per-ancestor band measurement (old 3b) was dropped.** The crumbs share the gutter
  equally via flexbox; when the path is too long for the gutter at `MIN_GLYPH_W` (16px) the
  middle ellipsizes (root … current). A **zero-indent** block has an empty gutter, so the
  band clamps to `MIN_GLYPH_W` and the current crumb overshoots **right into the content**
  (never into the margin) — the one accepted compromise.

**Root cause #1 — abspos child of a CSS grid is contained by its grid *area*, not the
grid box.** `#quarto-content` is `display:grid` (`.page-columns`). The chip auto-placed
into the body content column, so a computed `left` resolved against the column edge
(`colLeft`), not the page box — margin painting was impossible. **Fix:** give the chip
`grid-column: screen-start / screen-end; grid-row: 1 / -1` so its grid area is the full
page width; computed `left`/`top` (measured vs `#quarto-content`) then resolve correctly.

**Root cause #2 — the chip anchored to the full-width edit-region wrapper**, so the pivot
never tracked indentation (a list item and a fenced Div rendered identically). **Fix:**
anchor to the `<textarea>` inside the wrapper.

**Scroll fix (3a) — the original bug — is unchanged and proven** (4b: gap delta = 0 after
scrolling `#root` 400px). The grid/anchor fixes are layered on top of it.

Phase 4 e2e assertions were rewritten to this contract (◀ right ≈ `colLeft`; crumbs start
at `colLeft`, meet `surfaceLeft` for indented blocks; nothing but ◀ in the margin), all
measured **iframe-relative via `.evaluate()`** (a coordinate-system bug — mixing
`.boundingBox()` page-coords with `.evaluate()` iframe-coords — was fixed).

## Phase 5 — Wrap-up

- [ ] **5a.** `cargo xtask verify` is **not** required (no Rust touched); the hub-client build + tests
  in Phase 4 are the gate. (Double-check no Rust files changed.)
- [ ] **5b.** Update `hub-client/changelog.md` only if hub-client source changed (the chip lives in
  `ts-packages/preview-renderer`, bundled by hub-client — confirm whether the changelog convention
  applies; follow the two-commit workflow if so).
- [ ] **5c.** Stop the brainstorm server; `.superpowers/` is gitignored.
- [ ] **5d.** Commit; prepare PR description; **wait for explicit push approval** (GIT PUSH POLICY).

## Open questions / risks

- **Overflow-clip go/no-go (Phase 4a).** *Partly resolved by research:* `#root` is the scroller
  (`overflow:auto`), `#quarto-content`/`.page-columns` are not clipped — margin-spill works **iff** the
  chip stays at positive `left` within full-width `#quarto-content` (edge clamp is load-bearing).
  Still verify in-browser per 4a.
- **Stacking-context regression (researched 2026-06-15 → LOW risk, no extra code).** Making
  `#quarto-content` `position: relative` creates a stacking context but does **not** establish a
  containing block for `position: fixed` (only `transform` does), and there is **no `position: fixed`
  inside** `#quarto-content`. The sidebar (`.sidebar { position: sticky; will-change: top }`,
  `_bootstrap-rules.scss:1431-1433`) already has its own stacking context, so its z-index (1 desktop /
  `$zindex-modal` 1055 mobile-collapse) is unaffected. The attribution overlay (`position: fixed`) is a
  sibling *outside* `#quarto-content`; navbar/footer use `display: contents`. The chip's `z-index: 50`
  paints above sidebar (1) and `main` (0) within the new local context — intended. **No code change
  needed beyond `position: relative`.** Residual browser checks during 4f: (i) desktop — chip above
  sidebar + content; (ii) **narrow/mobile** — the collapsing sidebar at z-index 1055 is now local to
  `#quarto-content`; confirm it still overlays correctly and isn't trapped below the navbar (editing on
  mobile is unlikely, so chip-vs-mobile-sidebar order is academic); (iii) sticky sidebar still sticks.
- **Margin occupancy (resolved).** Rule: confine the spill to `[0, colLeft]` (the available left
  margin); **overlapping a left sidebar / `toc-left` there is acceptable** (chip paints over it); the
  hard stop is x ≈ 0 (`#quarto-content` left edge) so it never scrolls. **Test with a sidebar:** seed
  a project whose `_quarto.yml` produces a left sidebar (the e2e factory builds projects — add a
  fixture with `website:`/`sidebar:` config or a doc with `toc-location: left`) and confirm a
  deep-nested Div breadcrumb (i) overlaps the sidebar and (ii) does **not** add an `#root` horizontal
  scrollbar.
- **Page-edge squish threshold.** Define the min legible glyph width and the ellipsize rule for deep
  nesting.
- **Per-ancestor measurement robustness.** A source ancestor with no matching DOM `[data-block-pool-id]`
  → fall back to approximate-by-type widths.
- **Background treatment (resolved).** Faint scrim band behind the breadcrumb area (not a barrel box,
  need not span the full row). Tune opacity/extent during 2e/4f.
- **Glyph rendering.** `❝`, `¶`, `•` across target platforms via the system sans stack; if any glyph
  is missing on Windows, fall back to a letter form (`BQ`, `P`). Verify in the e2e pass.
- **Dark mode.** Deferred; flag if the chip looks wrong against the dark theme.
- **Div / Figure non-indent.** Accepted imperfection — clean tiling only for pure list/blockquote
  nesting; `Div` and `Figure` ancestors margin-spill instead (the only non-indenting ancestor types).

## Related plans

- `2026-06-15-breadcrumb-forward-crumbs.md` — **forward-crumbs** (faded preview of the nest-in
  descent target, to the right of the pivot). *Functionality* deferred there; this plan ships only the
  display scaffolding (pivot-at-surface-left, empty right group, `.q2-crumb-future`).
- `2026-06-15-nesting-cursor-navigation-and-list-items.md` — successor (list-item surfaces,
  line-anchored nav). Run **this** plan first.
