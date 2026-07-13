# q2-preview: consolidate per-block edit chrome into one pop-up toolbar

**Date:** 2026-07-13
**Area:** `ts-packages/preview-renderer/src/q2-preview` (the q2-preview iframe UI,
bundled by both hub-client's preview pane and `q2-preview-spa`).
**Tracking:** bd-igpm0xur

## Overview

### Problem

When a block is being edited in the q2-preview, a left-margin affordance shows
`Editing…` plus a two-button `rich text` / `plain text` toggle
(`richtext/EditAffordance.tsx`). It is absolutely positioned into the **left
margin** of the edit box (`right: calc(100% + 0.7rem)`, `styles.ts:17-23`), so in
the default two-pane preview it is **clipped/cut off** — there isn't margin room
to the left.

Separately, a floating **pop-up toolbar** (`richtext/RichTextToolbar.tsx`) with
formatting buttons (B/I/S/sub/sup/link) appears **only** for the rich-text
surface. Code chunks and other plain-textarea blocks get **no** pop-up toolbar —
only the `Editing…` left-margin label.

### Goal

One **pop-up toolbar** is the single, consistent edit-chrome host for **every**
editable block:

1. Move the rich/plain choice from the left-margin text buttons to a single
   **icon toggle** on the pop-up toolbar.
2. The pop-up toolbar appears for **all** editable blocks — including code
   chunks — even when there is no rich/plain choice.
3. For a block with no rich/plain choice (code chunk, etc.) the toolbar shows the
   **existing nested type indicator** (the breadcrumb’s current-type crumb).
4. Remove the now-redundant `Editing…` label (delete `EditAffordance` entirely).

### Key design decision (confirmed with user — “A”)

The type/nesting breadcrumb is currently gated on `unlockNestingCursor`
(**off by default**, `PreviewRoot.tsx:347`, `PreviewContext.tsx:184`). Under the
chosen design:

- The toolbar **always shows the current block’s type crumb** while editing any
  block (so a code chunk’s toolbar is never empty).
- The **full interactive ancestor path + ◀/▶ nesting navigation** stays gated on
  `unlockNestingCursor`, exactly as today.
- The **standalone floating `BreadcrumbChip` is folded into the toolbar** (one
  host); the separate floating chip is retired.

**Flag gate (fixes a regression the naïve `ctx.richText`-only gate would
introduce).** `richText` and `unlockNestingCursor` are **independent** boot
params (`?richText` defaults on, `?richText=0` opts out, `PreviewContext.tsx:189`;
`?nestingCursor=1` defaults off). Today the standalone `BreadcrumbChip` renders on
`unlockNestingCursor` **alone** — it never checks `richText` (`BreadcrumbChip.tsx:199`).
So the combination `?richText=0&nestingCursor=1` ("plain textareas + nesting
navigation", a supported mode the whole nesting feature targets) currently shows
the chip. Because we retire the chip and fold the breadcrumb into the toolbar, the
toolbar must render whenever **either** feature is on:

> **The toolbar renders when `ctx.richText || ctx.unlockNestingCursor`** — not on
> `ctx.richText` alone. This reproduces today's behavior exactly: the mode toggle
> when rich is on, the full breadcrumb when nesting is on, both when both, and no
> chrome when neither (matching the pre-change "no affordance, no chip" state).

## Target design

### One toolbar *component*, rendered at one of two mount points

The single edit-chrome host is **one component**, `EditToolbar`, parameterized by
an optional tiptap `editor`. (This is a rename+generalization of today's
`RichTextToolbar`, not a second near-duplicate shell — see "Why one component".)
Its contents, left → right:

| Section | When shown | Source of truth |
| --- | --- | --- |
| **Mode-toggle icon** (Markdown mark) | block is rich-supported (`Para`/`Header`/`Plain`) | new `ModeToggle` |
| **Formatting marks** (B I S x₂ x² 🔗) | `editor != null` (the rich editor is mounted) | existing marks/link block in `EditToolbar` |
| **Type / nesting indicator** | always (min = current-type crumb); full ancestor path when `unlockNestingCursor` | new `EditTypeIndicator` wrapping existing `BreadcrumbCrumbs` |

Per surface (same component, different props / mount point):

- **Rich surface** (Para/Header/Plain in rich mode): `[toggle] B I S x₂ x² 🔗 │ [indicator]`.
  `RichTextEditor` renders `<EditToolbar editor={editor} />` (it owns the tiptap
  `editor`, so this is the only place marks can render).
- **Plain surface** (code chunks, CustomBlocks, and Para/Header/Plain switched to
  plain): `[toggle?] │ [indicator]` (no marks). `renderMeasuredEdit` renders
  `<EditToolbar />` (no `editor`).

**Exactly one toolbar renders** per edit session, mutually exclusive:
`RichTextEditor` mounts **iff** the rich editor is active (`richActive`), and
`renderMeasuredEdit` renders `EditToolbar` **iff** `!richActive`. The gate:

```
richActive = richEditorActiveForType(ctx, sourceNodeType)   // richSupported && editorMode !== 'plain'
renderMeasuredEdit renders EditToolbar  ⇔  (ctx.richText || ctx.unlockNestingCursor) && !richActive
```

`richActive` is computed **once at the `Block` callsite** (`dispatchers.tsx:572`,
where `sourceNodeType` is in scope) via `richEditorActiveForType` and threaded into
`renderMeasuredEdit`. For this to make the "toolbar doubles or vanishes" drift
*structurally* impossible (not just a runtime invariant to police), the surface
choice and the toolbar gate must read the **same function**. Today they do **not**:
`renderBlockEditSurface` (`dispatchers.tsx:522`) hand-inlines
`richTextAvailable(...) && (ctx.editorMode ?? 'rich') !== 'plain'` — a textual
duplicate of `richEditorActiveForType`, safe only by coincidence. **This plan
refactors `renderBlockEditSurface` to call `richEditorActiveForType(ctx, sourceNodeType)`**
so the mount decision and the `!richActive` gate share one predicate; then the
exactly-one invariant is genuinely structural. **Do not skip this and rely on the
two copies staying textually identical — that runtime coincidence is precisely what
this design eliminates.**

### Why one component (not a shell + two hosts)

The goal is literally "one toolbar as the single host", so it is expressed as one
component. Folding the plain-surface toolbar into `RichTextToolbar` (renamed
`EditToolbar`, gaining an optional `editor`) means:

- The marks/link cluster renders only when `editor != null`; its `editor.on(…)`
  subscription guards `if (!editor) return`.
- The above/below placement effect lives **once, inline** in `EditToolbar` — so
  **no `useChromePlacement` hook is needed** (its only reason to exist was sharing
  placement between two shells that no longer both exist). It still calls the
  shared, tested pure fn `shouldPlaceChromeBelow` (`editChromeGeometry.ts`).
- One `ensureRichTextStyles()` call site covers both mount points.

The one detail this introduces: the placement effect's `closest()` currently
targets `.q2-richtext-editor` (`RichTextToolbar.tsx:66`), but the plain surface's
offset parent is `#q2-active-edit-region` (the `position: relative` set in
`renderMeasuredEdit`, `dispatchers.tsx:75`). `EditToolbar` must measure against
**both** — use `closest('.q2-richtext-editor, #q2-active-edit-region')` (or add a
shared `data-` attribute to both boxes).

### Component map (new / changed / deleted)

New:
- `richtext/ModeToggle.tsx` — single-icon rich/plain toggle. Reads `ctx.editorMode`,
  calls `setEditorMode` with the `editorModeSwitchRef` guard + mousedown-preventDefault
  (ported verbatim from `EditAffordance.choose`, `EditAffordance.tsx:29-40`) so a mode
  switch never blurs → commits/closes the session — the guard is consumed **unchanged**
  by `RichTextEditor`'s commit + focusout handlers (`RichTextEditor.tsx:156,251`), which
  this plan does not touch. **Final glyph (confirmed with user, superseding the initial
  `</>` proposal):** the **Markdown mark** as an inline SVG (`dcurtis/markdown-mark`),
  drawn in `currentColor` so it recolors with the button's hover/active state —
  **highlight-only** (the icon never flips; state shows via `aria-pressed` + the
  `.q2-rt-tb-active` tint, like the B/I/S mark buttons). Chosen over `</>` because it
  names the actual plain surface (Markdown source), not generic "code". A single inline
  SVG, not an icon library.
  **A11y:** unlike the mark buttons (whose visible text `B`/`I` is their accessible
  name), an icon carries no text, so give the button an explicit **stable
  `aria-label`** (e.g. `"Toggle plain-text editing"`) plus `aria-pressed={editorMode ===
  'plain'}` for state — the WAI-ARIA toggle-button pattern (stable name, state via
  `aria-pressed`), not a label that flips text. Keep the hover `title` as a
  sighted-user hint; it may still flip ("Edit as plain text" / "Edit as rich text").
- `richtext/EditTypeIndicator.tsx` — renders the full `<BreadcrumbCrumbs layout="inline">`
  when `unlockNestingCursor` is on (current behavior — the ◀/▶ nesting nav keeps its
  `requestNestingMove`/`requestNestingSelect` wiring via `BreadcrumbCrumbs`
  (`BreadcrumbCrumbs.tsx:191,222,235`); folding in changes only *where* the breadcrumb
  renders, not the handlers), else a **minimal single
  current-type crumb** (non-interactive, no ◀/▶) built from
  `buildAncestorPath(...).at(-1)` (the doc contract guarantees the exact-match
  "current" crumb is always the last entry, `nestingNav.ts:704` — same data the
  full breadcrumb uses, so category color / abbrev / label stay identical). **Reads
  `editTarget`/`sourceIndex`/`unlockNestingCursor` from `useContext(PreviewContext)`**
  (like `BreadcrumbCrumbs` already does) — no `ctx` prop drilling. Reused by the
  toolbar in both mount points. **Must call `ensureBreadcrumbStyles()` itself** (top of
  the component, both branches): the `.q2-crumb` / `.q2-crumb-cat-*` rules live in the
  breadcrumb stylesheet, which today is injected **only** from inside `BreadcrumbCrumbs`
  (`BreadcrumbCrumbs.tsx:174`). The minimal branch (nesting off — the **default**) does
  not render `BreadcrumbCrumbs`, so without an explicit `ensureBreadcrumbStyles()` the
  minimal crumb renders **unstyled/uncolored**. This bug is invisible to the jsdom unit
  tests (they don't apply CSS) — it would only surface in the browser E2E, so design it
  in rather than catch it later. **Accepted positioning tradeoff:** with nesting on,
  folding the full ancestor path in moves it from the left-margin gutter (pivot-pinned
  spill geometry) into the toolbar row (natural width) — a deep path now widens the
  toolbar rather than spilling into the margin; the toolbar's left edge stays fixed.
  Simpler, and accepted.

Changed:
- `richtext/RichTextToolbar.tsx` → **rename to `richtext/EditToolbar.tsx`** and
  generalize: `editor` becomes optional (`editor?: Editor | null`); the marks +
  link-editor block renders only when `editor != null` (its `editor.on(…)`
  subscription guards `if (!editor) return`); prepend `ModeToggle` (shown when the
  block is rich-supported); replace the `trailing` inline-breadcrumb prop with
  `<EditTypeIndicator />`; keep the above/below placement effect inline but broaden
  its `closest()` to `.q2-richtext-editor, #q2-active-edit-region`. **Always calls
  `ensureRichTextStyles()`** unconditionally at the top: today it is `EditAffordance`
  (`EditAffordance.tsx:24`) that injects the stylesheet at the plain mount point where
  `RichTextEditor` isn't mounted — after `EditAffordance` is deleted the toolbar must,
  or `.q2-rt-toolbar` is unstyled in plain mode.
- `richtext/RichTextEditor.tsx` — drop the `inlineBreadcrumb` construction
  (`:265-284`, now inside `EditTypeIndicator`); render `<EditToolbar editor={editor} />`.
  **Keep the existing `{editor && …}` guard** (`:291`) so the toolbar mounts only once
  tiptap's `editor` exists — there is a transient window during editor init where the
  rich surface renders zero toolbars (matching today), which is fine; do **not** render
  `EditToolbar` with a null `editor` on this path.
- `dispatchers.tsx` — refactor `renderBlockEditSurface` (`:517-526`) to pick the rich
  surface via `richEditorActiveForType(ctx, sourceNodeType)` instead of its inlined
  duplicate of that predicate (`:522`), so the mount decision and the toolbar gate
  read the *same function value* — making the exactly-one-toolbar invariant structural,
  not a policed textual coincidence. At the `Block` callsite
  (`:572`), compute `richActive = richEditorActiveForType(ctx, sourceNodeType)` and
  pass it into `renderMeasuredEdit` alongside `richSupported`. `renderMeasuredEdit`
  (`:63-95`) replaces `{ctx.richText && <EditAffordance…/>}` with
  `{(ctx.richText || ctx.unlockNestingCursor) && !richActive && <EditToolbar />}`.
  (The `CustomBlock` callsite `:636` passes `richActive = false`, so CustomBlocks
  get the toolbar with no toggle + a type crumb — consistent with the goal.)
  Also update the now-stale `// Positioning context for the left-margin edit
  affordance` comment (`:74`): the wrapper's `position: relative` is now the
  plain-surface toolbar's offset parent.
- `PreviewDocument.tsx` — remove `<BreadcrumbChip />` (`:298`) and its import (`:22`).
- `richtext/styles.ts` — delete `.q2-edit-affordance*` / `.q2-edit-mode-toggle*`
  rules (`:13-56`); add `.q2-rt-tb-mode` styling for the toggle glyph.

Deleted:
- `richtext/EditAffordance.tsx`.
- `richtext/RichTextToolbar.tsx` — as a *name* (renamed to `EditToolbar.tsx` above;
  update the import in `RichTextEditor.tsx:26`).
- `BreadcrumbChip.tsx` (and its now-dead geometry: `computeChipGeometry`,
  `selectDisplayItems`, `MIN_GLYPH_W`, `CRUMB_W`, `CHIP_FLIP_GAP`).
- **No `useChromePlacement.ts`** — not created (placement stays inline in the single
  `EditToolbar`; see "Why one component").
- `BreadcrumbCrumbs.tsx` is **kept**, but with the standalone chip gone, `inline` is
  the *only* remaining layout. This is a definite collapse, not an "audit" (see
  Phase 5): delete the `layout` and `bandWidth` props, `MIN_GLYPH_W` and the
  band-width inline styling (`:189,196`), the `ellipsis` `CrumbDisplayItem` kind, and
  the standalone-only CSS (`.q2-breadcrumb-chip`, `#quarto-content { position: relative }`,
  the fixed-band rules). It reduces to an always-inline crumb row.

## Phases & checklist

> TDD: tests first (Phase 0), verify they fail, then implement. Targeted tests per
> phase; full verification at the end.

### Phase 0 — Tests first

- [x] **ModeToggle unit** (`richtext/ModeToggle.test.tsx`): renders one `</>` button
      with a stable, non-empty `aria-label`; `aria-pressed` reflects
      `editorMode === 'plain'`; click calls `setEditorMode` with the opposite mode and
      toggles `editorModeSwitchRef.current` true→false.
- [x] **EditTypeIndicator unit** (`richtext/EditTypeIndicator.test.tsx`): with
      `unlockNestingCursor` off → renders exactly one current-type crumb (e.g. `Cd`
      for a code block) and **no** ◀/▶ buttons; with it on → renders full
      `BreadcrumbCrumbs` (◀/▶ present).
- [x] **Placement**: covered by the existing `editChromeGeometry.test.ts` (pure
      `shouldPlaceChromeBelow`) — no separate hook test (the hook is not created).
      The default-above / flip-below behavior is exercised via the toolbar-presence
      integration test below (jsdom zero-rect keeps the default 'above').
- [x] **Toolbar-presence integration** (`edit-toolbar.integration.test.tsx`, new):
  - Para in rich mode (nesting off): exactly one `.q2-rt-toolbar`; contains
    `.q2-rt-tb-mode` (`</>`), mark buttons, and a `¶` current-type crumb; **no**
    `.q2-edit-affordance`, **no** text `Editing…`.
  - Para toggled to plain: exactly one `.q2-rt-toolbar` (from the wrapper);
    `.q2-rt-tb-mode` pressed; **no** mark buttons; type crumb present.
  - Code block: exactly one `.q2-rt-toolbar`; **no** `.q2-rt-tb-mode`; **no** marks;
    type crumb whose **visible text is the abbrev `Cd`** (the `title`/tooltip carries the
    fuller `CodeBlock.<class>` label — `labelForSourceNode` does not return the language
    per se); no `.q2-edit-affordance`.
  - `unlockNestingCursor` on: toolbar indicator is the full `BreadcrumbCrumbs`
    (◀/▶ present); still exactly one `.q2-rt-toolbar`; **no** standalone
    `[data-testid="q2-breadcrumb-chip"]`.
  - **Regression guard — `richText` off + `unlockNestingCursor` on**: editing any
    block still renders exactly one `.q2-rt-toolbar` carrying the full breadcrumb
    (◀/▶ present); **no** `.q2-rt-tb-mode` (rich off → no toggle). This is the case
    the naïve `ctx.richText`-only gate would have dropped.
- [x] **Toggle-behavior integration**: clicking `.q2-rt-tb-mode` in a Para’s rich
      toolbar swaps to the plain textarea (marks disappear) without committing/closing
      the edit (switch-ref guard); clicking again returns to rich.
- [x] Confirm all new tests **fail** against current code before implementing.
      (Unit tests fail to import missing modules; 4/5 integration cases fail — the
      1 that passes is the existing rich-inline-breadcrumb behavior we preserve.)

### Phase 1 — Rename + generalize the toolbar component

- [x] Rename `RichTextToolbar.tsx` → `EditToolbar.tsx`; make `editor` optional
      (`editor?: Editor | null`); gate the marks + link-editor block and the
      `editor.on(…)` subscription on `editor != null` (behavior-preserving when
      `editor` is present).
- [x] Broaden the inline placement effect's `closest()` to
      `.q2-richtext-editor, #q2-active-edit-region`; keep it calling
      `shouldPlaceChromeBelow`. Update the import in `RichTextEditor.tsx:26`.
- [x] Run: `EditToolbar`/placement tests + `editChromeGeometry.test.ts` (green;
      inline-breadcrumb + plain-list-item + caret integration all pass).

### Phase 2 — Mode toggle icon

- [x] Add `ModeToggle`; wire the switch-ref guard (port from `EditAffordance`).
- [x] `.q2-rt-tb-mode` CSS (reuse `.q2-rt-tb-btn` + `.q2-rt-tb-active`; glyph `</>`).
      Accessibility: stable `aria-label` (e.g. “Toggle plain-text editing”) +
      `aria-pressed` = (mode === 'plain'); hover `title` = “Edit as plain text” /
      “Edit as rich text” as a sighted-user hint. (ModeToggle unit test green.)

### Phase 3 — Type indicator

- [ ] Add `EditTypeIndicator` (min current-type crumb vs full `BreadcrumbCrumbs`),
      reading `editTarget`/`sourceIndex`/`unlockNestingCursor` from
      `useContext(PreviewContext)` (no `ctx` prop).
- [ ] `EditTypeIndicator` calls `ensureBreadcrumbStyles()` at the top (both branches),
      so the minimal-crumb (nesting-off) path is styled — the `.q2-crumb-cat-*` rules
      are otherwise injected only by `BreadcrumbCrumbs` (`BreadcrumbCrumbs.tsx:174`).
- [ ] Reuse crumb data via `buildAncestorPath(...).at(-1)` so the minimal indicator
      matches the full breadcrumb’s current crumb exactly (category color, abbrev, label).

### Phase 4 — Wire the toolbar into both mount points; delete `EditAffordance`

- [x] `EditToolbar`: prepend `ModeToggle` (when rich-supported), append
      `<EditTypeIndicator />`, ensure it calls `ensureRichTextStyles()`.
- [x] `RichTextEditor`: drop `inlineBreadcrumb`; render `<EditToolbar editor={editor} richSupported />`.
- [x] `dispatchers.tsx`: compute `richActive = richEditorActiveForType(ctx, sourceNodeType)`
      at the `Block` callsite and thread it into `renderMeasuredEdit`;
      `renderMeasuredEdit` renders `<EditToolbar />` when
      `(ctx.richText || ctx.unlockNestingCursor) && !richActive`. Refactored
      `renderBlockEditSurface` to read the same `richEditorActiveForType`. Updated
      stale `position: relative` comment.
- [x] Delete `EditAffordance.tsx`; remove its CSS rules from `styles.ts`.
      (Post-Phase-4: unit tests + 4/5 edit-toolbar cases green; the 1 failure is
      the plain+nesting `standaloneChip toBeNull()` — resolved by the Phase 5 chip
      deletion below.)

### Phase 5 — Fold in the standalone breadcrumb chip; collapse `BreadcrumbCrumbs`

- [x] Remove `<BreadcrumbChip />` + import from `PreviewDocument.tsx`.
- [x] Delete `BreadcrumbChip.tsx` + dead geometry; **delete** `BreadcrumbChip.geometry.test.ts`.
- [x] **Deleted** `p3-4-breadcrumb.integration.test.tsx` (standalone-chip
      geometry/positioning/spill/ellipsis — moot once the chip is gone).
- [x] **Updated `p3-3-unlocked-subclauses.integration.test.tsx`** — re-pointed the
      `[data-testid="q2-breadcrumb-chip"]` queries (Test 2, steps 2 & 5) to
      `.q2-rt-toolbar`; crumb/aria-current/re-derivation assertions unchanged.
- [x] **Audited `g5-carry-expansion.integration.test.tsx`** — passes UNCHANGED. Its
      `.q2-crumb[title="BlockQuote"]` query is by class, so it resolves against the
      folded toolbar; the graceful jsdom fallback is intact.
- [x] Confirm `p3-4-inline-breadcrumb.integration.test.tsx` and
      `plain-list-item-richtext.integration.test.tsx` still pass. **Deviation from the
      plan's "confirm still pass":** both had obsolete standalone-chip / no-toolbar
      assertions that are no longer true under the `(richText || nesting)` gate, so
      they were UPDATED (not just confirmed):
      - `p3-4-inline-breadcrumb` Test 2 rewritten: a CodeBlock now gets the *folded
        toolbar* (one `.q2-rt-toolbar`, `Cd` crumb, ◀/▶, no mode toggle) and **no**
        standalone chip — instead of the old "no toolbar + standalone chip".
      - `plain-list-item-richtext` second test (fixture has `unlockNestingCursor:true`):
        dropped the `toolbar toBeNull()` assertion; the toolbar now renders with the
        breadcrumb, so the "richText off" signal is now "textarea present + no
        `.q2-rt-tb-mode` + no marks".
- [x] Collapse `BreadcrumbCrumbs.tsx` to always-inline: dropped the `layout`/`bandWidth`
      props, `MIN_GLYPH_W` + band-width inline styles, the `ellipsis` `CrumbDisplayItem`
      kind (prop simplified to `crumbs: AncestorCrumb[]`), and the standalone-only CSS
      (`.q2-breadcrumb-chip`, `#quarto-content { position: relative }`, fixed-band /
      ellipsis rules). Updated `EditTypeIndicator`'s call to the simplified signature.

### Phase 6 — Verification & polish

- [x] `cd ts-packages/preview-renderer && vitest run` (whole suite) green — unit
      **519 passed** / 36 skipped; integration **530 passed** / 1 skipped. `tsc --noEmit` clean.
- [x] `cd hub-client && npm run build:all` (production build — stricter TS project refs).
      Ran `tsc -b` (project references, clean) + `vite build` (main bundle incl.
      `q2-preview-*.js`) + `build:sandboxed`, all green. **Skipped `build:wasm`** — the
      change is pure TypeScript (no Rust touched) and the WASM artifact is current.
      Also validated the third preview-renderer consumer: `cargo xtask build-q2-preview-spa`
      bundled cleanly (`q2-preview-*.js` 1.14 MB).
- [~] **E2E (primary): hub-client dev browser** — **NOT run in this environment** (no
      visual browser access). Strongest proxy done: the jsdom integration tests drive the
      REAL `PreviewRoot` with real pointer/mousedown gestures through the actual dispatcher
      + `EditToolbar` + `RichTextEditor` (tiptap) + `ModeToggle` + `EditTypeIndicator`,
      asserting the DOM/behavior contract (toggle swaps rich↔plain surface, marks
      appear/disappear, crumb present, **no `.q2-edit-affordance`**, **no `Editing…`**, **no**
      standalone chip, exactly one `.q2-rt-toolbar`). **Not verified without a browser:** CSS
      layout — the "no cut-off left-margin text" visual property, Markdown-mark icon appearance, and
      the above/below pixel flip. **Recommend the user do the hub-client dev browser check.**
- [~] **E2E (secondary): `q2 preview`** — SPA bundle **rebuilt** via
      `cargo xtask build-q2-preview-spa` (compiles/bundles cleanly), so a subsequent
      `q2 preview` is fresh. Not browser-inspected here. No Rust touched → `cargo xtask verify`
      not required.
- [x] **Snapshot/test bookkeeping** (per CLAUDE.md): no `.snap` snapshots changed. Test-file
      inventory:
      - **Added:** `richtext/ModeToggle.test.tsx`, `richtext/EditTypeIndicator.test.tsx`,
        `edit-toolbar.integration.test.tsx`.
      - **Deleted (jsdom):** `BreadcrumbChip.geometry.test.ts`, `p3-4-breadcrumb.integration.test.tsx`.
      - **Updated (jsdom):** `p3-3-unlocked-subclauses` (chip→toolbar re-point),
        `p3-4-inline-breadcrumb` (Test 2: CodeBlock now folded toolbar, no chip),
        `plain-list-item-richtext` (2nd test: no-toggle/no-marks instead of no-toolbar),
        `g5-carry-expansion` (comment only).
      - **PLAN GAP — Playwright browser specs (not in the plan):** `q2-preview-breadcrumb-geometry.spec.ts`
        (**deleted** — 1302 lines of standalone-chip spill geometry, the browser analog of the deleted
        `p3-4-breadcrumb`; the folded toolbar has no spill geometry); `q2-preview-crumb-no-carry-expansion.spec.ts`
        (**re-pointed** chip locator → `.q2-rt-toolbar`; crumb-jump behavior unchanged);
        `q2-preview-breadcrumb-isolation.spec.ts` (**re-pointed** Test A's chip `waitFor` →
        `.q2-breadcrumb-out`; ◀/▶ clicks + nesting-chord tests unchanged). These browser specs
        were **not run** (require WASM build + sync server + browser); edits are selector/comment
        only and are type-safe.
- [ ] **Changelog**: user-visible hub-client UI change → needs a `hub-client/changelog.md`
      entry via the two-commit workflow. **Deferred** — not committed yet (work is on `main`,
      no commit/branch requested; per prior guidance, batch the entry at commit/merge).

## Files (quick reference)

- Left-margin affordance (delete): `richtext/EditAffordance.tsx`; CSS `richtext/styles.ts:13-56`.
- Pop-up toolbar (rename `RichTextToolbar.tsx` → `EditToolbar.tsx`, generalize to
  optional `editor`): mounted from `richtext/RichTextEditor.tsx:289-294` (rich) and
  `dispatchers.tsx:63-95` `renderMeasuredEdit` (plain). Import at `RichTextEditor.tsx:26`.
- Shared wrapper (mount plain toolbar / compute `richActive`): `dispatchers.tsx:63-95`
  + `Block` callsite `:572`; surface dispatch `:517-526`.
- Mode state: `PreviewRoot.tsx:266,269,1540-1542`; context `PreviewContext.tsx:194-210`.
- Support predicate: `richTextSupport.ts:25-41` (`richTextAvailable`, `richEditorActiveForType`).
- Flags (independent): `richText` `PreviewContext.tsx:189` / `PreviewRoot.tsx:167,1539`;
  `unlockNestingCursor` `PreviewContext.tsx:184` / `PreviewRoot.tsx:347,1537`.
- Breadcrumb: `BreadcrumbCrumbs.tsx` (keep, collapse to inline-only), `BreadcrumbChip.tsx`
  (delete), `PreviewDocument.tsx:22,298`; type glyphs `nestingNav.ts:580-737`
  (`abbrevForSourceNode` → visible crumb text, `Cd` for code; `labelForSourceNode` →
  `title`/tooltip, `CodeBlock.<id|class>` — not the language string);
  current-crumb contract `nestingNav.ts:704`.
- Placement geometry (shared pure fn, keep): `editChromeGeometry.ts` +
  `editChromeGeometry.test.ts` (no `useChromePlacement` hook — placement stays inline
  in `EditToolbar`).

## Tracking

Create a braid strand referencing this plan, e.g.:

```
braid create "q2-preview: consolidate edit chrome into one pop-up toolbar" \
  -t feature -p 2 \
  -d "Move rich/plain toggle to a toolbar icon; toolbar pops up for all blocks \
(code chunks show the existing type indicator); remove the cut-off Editing… \
left-margin affordance and fold in the standalone breadcrumb chip. Plan: \
claude-notes/plans/2026-07-13-preview-edit-toolbar-consolidation.md"
```
