---
title: 'q2 preview: hierarchical block navigator + breadcrumb/toolbar overlap'
date: 2026-06-25
description: 'Turns the hierarchical block breadcrumb navigator on by default for `q2 preview --allow-edit`, and moves it beside the rich-text toolbar for rich-text blocks so the two no longer overlap.'
---

**Strand:** bd-9x3zbuj8
**Date:** 2026-06-25
**Status:** DRAFT — awaiting user review before implementation

## Overview

Two independent changes to the `q2 preview` live block editor (the tiptap
rich-text editor, bd-sjb4pzx8 lineage):

1. **Task 1 — Bring the hierarchical block navigator to `q2 preview --allow-edit`.**
   The `BreadcrumbChip` (`◀ Dv ¶ ▶` ancestor-path navigator) ships in
   hub-client / quarto-hub.com but is invisible in `q2 preview`. It is gated
   solely on `PreviewContext.unlockNestingCursor`, which the SPA defaults OFF.
2. **Task 2 — Fix the breadcrumb / rich-text-toolbar overlap.** When a nested
   rich-text block is open, `BreadcrumbChip` and `RichTextToolbar` collide at
   the block's top-left. Move the breadcrumb to the **right** of the
   fixed-position rich-text controls so they sit side by side.

Both features live in the **shared** `@quarto/preview-renderer` package
(`ts-packages/preview-renderer/src/q2-preview/`), consumed by both hub-client
and the q2-preview SPA. The mount points are shared; only the gating flag and
the layout geometry differ.

## Background / architecture (verified 2026-06-25)

### How the breadcrumb is gated

- `BreadcrumbChip.tsx:189` — `const active = !!ctx?.unlockNestingCursor && !!et;`
  (`et = ctx.editTarget`). Returns `null` when `!active` (line 259). So the chip
  appears **only** when `unlockNestingCursor` is on **and** a block is being
  edited.
- Mounted once, for both environments, in `PreviewDocument.tsx:298`
  (`<BreadcrumbChip />`, a child of `#quarto-content`).
- `unlockNestingCursor` flows: host → `PreviewRoot` prop → `PreviewContext`.
  - **hub-client:** `ReactPreview.tsx:446` reads `usePreference('unlockNestingCursor')`;
    `schema.ts:25,49` defaults the preference **`true`**. → chip visible.
  - **q2-preview SPA:** `PreviewApp.tsx:370-372` sets
    `nestingCursor: parseNestingCursorParam(window.location.search)`;
    `parseNestingCursorParam` (PreviewApp.tsx:282-289) returns `true` **only**
    for `?nestingCursor=1`, else `false`. → chip hidden by default.

Because the chip self-gates on `editTarget`, it can never appear in read-only
`q2 preview` (no `--allow-edit` ⇒ `editingDisabled` ⇒ no edit target). So
defaulting the flag ON is safe: it only takes visible effect once the user is
actually editing under `--allow-edit`.

### The overlap (Task 2)

Both elements float above the top-left of the open edit box:

- **`RichTextToolbar`** (`styles.ts:63-80`, `.q2-rt-toolbar`): abs-positioned
  inside `.q2-richtext-editor` at `left:-2px; bottom:100%; margin-bottom:4px;
  z-index:20`. Fixed at the block's left edge; rendered by
  `RichTextEditor.tsx:232`. Rich-text blocks only (paragraphs/headings).
- **`BreadcrumbChip`** (`BreadcrumbChip.tsx`): abs-positioned inside
  `#quarto-content`, `z-index:50`. Geometry (`computeChipGeometry`) pins the
  crumb **band's right edge at `surfaceLeft`** (the edit surface's left edge =
  the block's left edge) and spills the band LEFT into the indent gutter/margin.
  Crucially, the `▶` in-arrow **and a future-crumb placeholder are rendered
  AFTER the band** (BreadcrumbChip.tsx:450-459), i.e. they extend to the RIGHT
  of `surfaceLeft`, over the content — directly on top of the toolbar's left
  buttons. The chip's higher `z-index` (50 > 20) paints it over the toolbar.

So the collision is structural: the breadcrumb's right tail and the toolbar's
left buttons both occupy the strip immediately right of the block's left edge.

## Task 1 — Bring the navigator to `q2 preview --allow-edit`

### Approach (recommended)

Mirror the `richText` default-on / opt-out contract exactly. The richText
precedent: `parseRichTextParam` (PreviewApp.tsx:297-303) returns `true` unless
`?richText=0`.

Change `parseNestingCursorParam` to default **ON**, opt out via
`?nestingCursor=0`:

```ts
export function parseNestingCursorParam(search: string): boolean {
  try {
    return new URLSearchParams(search).get('nestingCursor') !== '0';
  } catch {
    return true;
  }
}
```

(Also export it, matching `parseRichTextParam`, so it can be unit-tested
directly.)

No other wiring is needed — the flag already flows SPA → `Q2PreviewIframe` →
`entry.tsx` → `PreviewRoot` → `PreviewContext`, and the chip mounts in the
shared `PreviewDocument`.

### Open question for Task 1

- **Default-on globally, or only under `--allow-edit`?** Recommendation:
  default-on globally (simplest, matches `richText`). The chip self-gates on
  `editTarget`, so read-only previews never show it — there is no user-visible
  downside, and it keeps the SPA's flag handling uniform. If we'd rather be
  explicit, we could AND it with the fetched `allowEdit` state, but that adds a
  dependency for no observable behavior change. **Proposed: default-on globally.**

### TDD for Task 1

1. **Unit (RED first):** add `parseNestingCursorParam.test.ts` (clone of
   `parseRichTextParam.test.ts`): defaults ON for `''`, `?page=…`,
   `?nestingCursor=1`, any other value; OFF only for `?nestingCursor=0`. Fails
   against the current `=== '1'` implementation.
2. **Integration:** `p3-2-nesting-cursor-spa.integration.test.tsx` already
   exercises SPA nesting-cursor plumbing — update/extend it to assert the chip
   is present by default (no `?nestingCursor=1`) and absent with
   `?nestingCursor=0`.
3. **e2e (Playwright):** the geometry specs currently force
   `unlockNestingCursor: true` explicitly. Add/adjust one spec asserting the
   default boot (no param) shows the chip when editing under allow-edit. Audit
   `q2-preview-block-nav-p2-5b.spec.ts` and `q2-preview-locked-hover.spec.ts`,
   which deliberately set `unlockNestingCursor: false` — they pass the flag
   explicitly so they are unaffected by the default flip, but confirm.

## Task 2 — Side-by-side layout (breadcrumb right of toolbar)

The user's constraint: the rich-text controls must keep a **fixed** position
(at the block's left edge); the **variable-width** breadcrumb goes to their
right. This rules out putting the breadcrumb on the left (which would make the
toolbar's x-position depend on breadcrumb width).

**Decision (user, 2026-06-25): Option B — render the breadcrumb inline in the
toolbar's flex row for rich-text blocks; keep the standalone chip (unchanged
left-spill geometry) for plain blocks.**

### Chosen design — inline in the toolbar row

For a rich-text block, the floating chrome above the block becomes a single flex
row:

```
[ B  I  S  x₂  x²  🔗  │  ◀ Dv ¶ ▶ ]
  └──── toolbar (fixed) ────┘ └ breadcrumb ┘
```

For a plain block (code, list, div, table, raw, or a Para/Header the user
toggled to "plain" mode), there is no toolbar, so the **standalone**
`BreadcrumbChip` renders exactly as today (current left-spill geometry — no
overlap, nothing to change).

This means the breadcrumb has **two render paths**, but each is simple and only
the rich path needed fixing. No measurement, no `ResizeObserver`, no timing race
— flexbox lays the two groups side by side, and the toolbar's left edge stays
pinned at the block's left edge regardless of breadcrumb width.

### Implementation outline

1. **Extract a presentational crumb component.** Factor the crumb UI out of
   `BreadcrumbChip` into a reusable, **position-agnostic** `BreadcrumbCrumbs`
   (the `◀` / band / crumb buttons / `▶`, with the click handlers wired to
   `ctx.requestNestingMove` / `ctx.requestNestingSelect`, and
   `buildAncestorPath(ctx.sourceIndex, et.anchorR0, et.anchorR1)`). It carries
   NO absolute positioning / geometry — that stays in `BreadcrumbChip` for the
   standalone path.
   - The standalone `BreadcrumbChip` keeps its `computeChipGeometry` /
     left-spill model and renders `<BreadcrumbCrumbs>` inside the positioned
     pill. **No geometry change** → the big geometry test suite is preserved.
   - Note: in the inline row, the crumb band should size to its **natural**
     content width (it is not pinned to a gutter), so `BreadcrumbCrumbs` must
     accept a "natural width / no fixed band" mode. Keep the flex-fill band
     mode for the standalone chip.

2. **Render the inline breadcrumb in `RichTextEditor`.** In
   `RichTextEditor.tsx:230-235`, wrap the toolbar + inline breadcrumb in a flex
   row (or append `<BreadcrumbCrumbs>` inside `.q2-rt-toolbar` after the
   buttons, separated by a `.q2-rt-tb-sep`). Gate on
   `ctx.unlockNestingCursor && ctx.editTarget` (same condition as the chip).

3. **Suppress the standalone chip when the rich editor is active for the active
   target.** The rich editor is chosen by
   `richTextAvailable(ctx, nodeType) && (ctx.editorMode ?? 'rich') !== 'plain'`
   (`dispatchers.tsx:515-529`; `RICHTEXT_SUPPORTED_TYPES = {Para, Header}`).
   `BreadcrumbChip` must return `null` in exactly that case to avoid a double
   breadcrumb. The wrinkle: `editTarget` does **not** carry the node type
   (`PreviewContext.tsx:62-83`), so the chip can't compute the predicate
   synchronously today. Two clean options to settle during build:
   - **(preferred) Stamp the hint on `editTarget` at activation.** The
     activation path resolves the node anyway, so add a
     `richSupported: boolean` field to the `editTarget` object; the chip
     suppresses when `editTarget.richSupported && editorMode !== 'plain'`.
     Synchronous, flash-free, single source of truth.
   - **(fallback) A context flag toggled by `RichTextEditor` mount/unmount**
     (e.g. `ctx.richEditorActive` state). Simpler to wire but risks a
     one-frame flash of the standalone chip before the flag lands.

4. **Styling.** The inline breadcrumb sits in `.q2-rt-toolbar`\'s solid pill, so
   the standalone chip's opaque-pill / shadow / `z-index` styling is not needed
   inline — just spacing (a separator + small gap). Keep the crumb category
   colors (`.q2-crumb-cat-*`).

### Test impact (Task 2)

Because the standalone geometry is **unchanged**, the large geometry suite is
largely preserved — a major advantage of Option B:

- **Preserved (verify still green):** `BreadcrumbChip.geometry.test.ts`,
  `q2-preview-breadcrumb-geometry.spec.ts` (standalone path still exercised via
  plain blocks / code blocks), `q2-preview-breadcrumb-isolation.spec.ts`
  (pointer isolation — re-verify for the inline path too).
- **New tests:** assert the inline breadcrumb renders inside the toolbar row for
  a rich-text block (Para/Header) and that the **standalone** chip is suppressed
  for that same block (no double breadcrumb). Assert the toolbar's left edge is
  unchanged regardless of breadcrumb width.
- **Re-check:** `p3-4-breadcrumb.integration.test.tsx`,
  `g5-carry-expansion.integration.test.tsx`,
  `p3-3-unlocked-subclauses.integration.test.tsx` — any that open a Para/Header
  edit target now find the breadcrumb inline rather than standalone; update
  selectors accordingly.

### TDD for Task 2

1. **RED:** test that opening a Para/Header edit target renders crumbs **inside**
   the toolbar row and that `data-testid="q2-breadcrumb-chip"` (standalone) is
   **absent** for that target. Fails today (standalone chip renders, overlapping).
2. Extract `BreadcrumbCrumbs`; render it inline in `RichTextEditor`; add the
   suppression hint to `editTarget`.
3. Re-check the integration specs above; confirm the standalone geometry suite
   still passes (plain/code blocks).
4. Verify no overlap and a stable toolbar position in a real browser
   (navigate in/out to vary breadcrumb width).

## End-to-end verification (required before declaring done)

Per CLAUDE.md, tests are necessary but not sufficient. For both tasks:

1. Rebuild the SPA chain so `q2 preview` picks up the change:
   `cd hub-client && npm run build:wasm` (only if Rust changed — it isn't here),
   then build the SPA bundle and re-embed. For TS-only SPA changes, the relevant
   chain is the q2-preview-spa build + `cargo xtask build-q2-preview-spa` +
   `cargo build --bin q2`. (Confirm exact commands; no Rust change here, so WASM
   rebuild is likely unnecessary.)
2. `cargo run --bin q2 -- preview <fixture-dir> --allow-edit`, open a doc with a
   **nested** rich-text block (e.g. a paragraph inside a div inside a list),
   click to edit, and confirm:
   - Task 1: the `◀ … ▶` navigator appears.
   - Task 2: navigator and toolbar are side by side with no overlap; toolbar
     position is stable as the breadcrumb width changes (navigate in/out).
3. Capture a screenshot and note it in this plan + the strand.

## hub-client changelog

Both changes are under `hub-client` scope's influence (shared package consumed
by hub-client). Per CLAUDE.md, add `hub-client/changelog.md` entries (two-commit
workflow: code commit first, then changelog entry with the hash). Note: Task 1
changes default behavior **only in the SPA**; hub-client already defaults the
preference on, so describe accordingly. Task 2 affects both environments
(shared geometry) — call that out.

## Work items

### Task 1 — navigator in q2 preview ✅ (pending real-binary e2e verification)
- [x] Add failing `parseNestingCursorParam.test.ts` (default-on / `=0` opt-out) — RED confirmed (4/6 failed), now GREEN
- [x] Flip `parseNestingCursorParam` to default-on; export it
- [x] Update `p3-2-nesting-cursor-spa.integration.test.tsx` for default-on (8/8 pass)
- [x] Add/adjust e2e spec for default-boot chip visibility under allow-edit
      (`q2-preview-spa/e2e/nesting-cursor.spec.ts`: default-boot → unlocked +
      chip visible; `?nestingCursor=0` → locked). NOTE: requires SPA/WASM
      rebuild + binary build to run; deferred to combined e2e verification.
- [x] Confirm `unlockNestingCursor:false` specs unaffected — the hub-client
      e2e specs (`q2-preview-block-nav-p2-5b`, `q2-preview-locked-hover`) set
      the preference via localStorage, a separate path from the SPA URL-param
      default; the only SPA e2e depending on the old default was
      `nesting-cursor.spec.ts` (updated above).

### Task 2 — side-by-side layout (Option B — inline in toolbar row) ✅ (pending real-binary e2e)
- [x] Settle the design fork → **Option B** (user, 2026-06-25)
- [x] Failing test: rich-block edit renders crumbs inline in the toolbar row;
      standalone `q2-breadcrumb-chip` absent for that target
      (`p3-4-inline-breadcrumb.integration.test.tsx`, 2 tests, GREEN)
- [x] Extract position-agnostic `BreadcrumbCrumbs` from `BreadcrumbChip`
      (`BreadcrumbCrumbs.tsx`; `layout: 'standalone' | 'inline'`; shared
      `ensureBreadcrumbStyles()`; inline = natural width, standalone = flex band)
- [x] Render `BreadcrumbCrumbs` inline in `RichTextEditor` toolbar row
      (new `trailing` slot on `RichTextToolbar`, after a separator)
- [x] Suppress standalone `BreadcrumbChip` when rich editor is active for the
      target. NOTE: chose a cleaner mechanism than stamping `editTarget` — a
      shared leaf module `richTextSupport.ts` (`RICHTEXT_SUPPORTED_TYPES`,
      `richTextAvailable`, `richEditorActiveForType`) + `currentSourceNodeType`
      (resolves the active node's type from `sourceIndex`). Fully synchronous &
      reactive (tracks `editorMode`/`richText`), no activation-path changes, no
      flash, no context flag. `dispatchers.tsx` now imports from the leaf module.
- [x] Confirm standalone geometry suite still green (plain/code blocks) —
      `BreadcrumbChip.geometry.test.ts` + the breadcrumb integration suite pass
      unchanged (they don't enable `richText`, so they exercise the standalone path)
- [x] Re-check breadcrumb integration specs — all green (unit 500, integration 515)

### Cross-cutting
- [x] preview-renderer suites green (unit 500, integration 515)
- [x] q2-preview-spa suites green (unit 37, integration 75) + production build
- [x] hub-client production build green; unit 662 + integration 76 green
- [x] e2e verification in a real `q2 preview --allow-edit` session + screenshot
      (see below). Full q2-preview-spa e2e suite green (37 tests).
- [x] `hub-client/changelog.md` entry (commit df3287bf, referencing 15b9287c)
- [ ] `cargo xtask verify --skip-hub-build` before push (Rust untouched; binary
      builds clean — `cargo build --bin q2` succeeded)

## Commits (branch `braid/bd-9x3zbuj8-preview-hierarchy-nav-overlap`)

- `15b9287c` feat(preview-renderer): inline nesting breadcrumb in the toolbar (Task 2)
- `d73079be` feat(q2-preview): default the hierarchical block navigator ON (Task 1)
- `ad18bf91` test(q2-preview e2e): pin richText=0 in edit-cell-sizing (bd-n4v4phe4)
- `553ded66` docs(plan)
- `df3287bf` docs(hub-client): changelog

## End-to-end verification record (2026-06-25)

Built the chain (TS-only change → no WASM rebuild needed): `cargo xtask
build-q2-preview-spa` → `cargo build --bin q2`. Ran
`q2 preview <fixture> --allow-edit` and drove it in Chrome.

Clicking a paragraph nested in a callout div (the user's exact scenario) opened
the rich editor and produced this toolbar DOM (`.q2-rt-toolbar` textContent):

```
BISx₂x²🔗◀Dv¶▶
```

i.e. `B I S x₂ x² 🔗` (formatting, fixed at the block's left edge) followed by
`◀ Dv ¶ ▶` (the nesting navigator, inline to the right). Inspected:
`toolbarPresent: true, prosemirrorPresent: true, standaloneChipPresent: false,
inlineCrumbs: ["Dv","¶"]`. Screenshot confirmed the side-by-side layout with no
overlap. Both tasks verified through the real binary.

Real-binary e2e (`q2-preview-spa/e2e/nesting-cursor.spec.ts`, 3 tests, all green):
default boot → rich editor + inline navigator + standalone chip suppressed;
`?richText=0` → clean textarea buffer (no `>`); `?nestingCursor=0` → locked
whole-quote with `>`.

## Discovered work (filed, out of scope)

- **bd-n4v4phe4** (fixed + closed): `edit-cell-sizing.spec.ts` (q2-preview-spa
  e2e) no-reflow tests waited for a `textarea` but rich-text default-on opens the
  rich editor. Pinned `?richText=0` (the bd-038tnyqy baseline pattern). Fixed
  here because it shares the e2e suite I ran to verify these tasks.
- **bd-fpys25b0** (filed, NOT fixed here): the entire hub-client block-editing
  e2e suite (12 `q2-preview-*.spec.ts` that wait for `textarea`) has been red
  since bd-j1nto6eq (rich-text default-on) — none pin `richText`. Pre-existing,
  systemic, not in default `cargo xtask verify` (`test:e2e` is opt-in). The 3
  breadcrumb specs there test the STANDALONE chip; with `richText=0` pinned they
  stay valid and are unaffected by Task 2's suppression. Left for a dedicated
  hub-client-e2e session.

## Key files

| Concern | File |
| --- | --- |
| Breadcrumb component + geometry | `ts-packages/preview-renderer/src/q2-preview/BreadcrumbChip.tsx` |
| Breadcrumb mount | `ts-packages/preview-renderer/src/q2-preview/PreviewDocument.tsx:298` |
| Rich-text toolbar | `ts-packages/preview-renderer/src/q2-preview/richtext/RichTextToolbar.tsx` |
| Toolbar + editor styles | `ts-packages/preview-renderer/src/q2-preview/richtext/styles.ts` |
| Editor wrapper | `ts-packages/preview-renderer/src/q2-preview/richtext/RichTextEditor.tsx:230-235` |
| SPA flag defaulting | `q2-preview-spa/src/PreviewApp.tsx:282-303, 370-375` |
| hub-client preference | `hub-client/src/services/preferences/schema.ts:25,49`; `hub-client/src/components/render/ReactPreview.tsx:446` |
| Flag plumbing | `entry.tsx`, `iframe/Q2PreviewIframe.tsx`, `PreviewRoot.tsx`, `PreviewContext.tsx` |
