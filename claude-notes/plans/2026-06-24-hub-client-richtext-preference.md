# Enable rich-text editor in hub-client q2-preview (default ON)

**Date:** 2026-06-24
**Strand:** bd-j1nto6eq (discovered-from bd-sjb4pzx8)
**Branch:** `braid/bd-sjb4pzx8-tiptap-rich-text-editor` (the rich-text feature branch; PR #335)
**Status:** ✅ Implemented + verified (2026-06-24). Commits `a6f16a1e` (feature)
and `b010dafe` (changelog). End-to-end on quarto-hub left to the user (see
Phase 4). Not yet pushed.
**Builds on / required reading:** the rich-text editor work (bd-sjb4pzx8),
the caret-at-click + reset.css fixes (bd-q9lyghv2, bd-dg8x84bu), and the q2
preview SPA default-on flip (commit 512907f7).

---

## Overview

The tiptap rich-text block editor is enabled in the standalone `q2 preview` SPA
(default ON via the `?richText` boot param). The **hub-client** web app
(quarto-hub) renders the same q2-preview iframe but never passes the `richText`
flag, so it always falls back to the monospaced textarea editor.

Goal: plumb `richText` from a new hub-client user preference (default **ON**)
through to `PreviewContext.richText`, mirroring the existing `unlockNestingCursor`
preference exactly, and expose an opt-out toggle in the Settings tab.

### Why this is small and low-risk

`Q2PreviewIframe` (`ts-packages/preview-renderer/src/iframe/Q2PreviewIframe.tsx`)
**already** accepts a `richText?: boolean` prop and forwards it unchanged into the
`UPDATE_AST` postMessage payload → `PreviewContext.richText` (declared line 106,
destructured 177, in payload 375, in effect deps 394). The iframe→context path is
identical to what the SPA already uses and has been browser-verified. So this is
purely a **hub-client wiring job** with an established template.

The closest analog is `unlockNestingCursor` (also a default-ON boolean
preference). We copy its flow end-to-end:

```
preferences schema (default true)
  → usePreference('richText') in ReactPreview
  → <ReactRenderer richText=...>
  → <Q2PreviewIframe richText=...>   (already wired beyond here)
  → UPDATE_AST payload → PreviewContext.richText
```

Plus a Settings toggle, mirroring the "Nesting cursor" checkbox.

### Scope notes

- **Default ON** (user-confirmed): matches the SPA and the `unlockNestingCursor`
  precedent; the Settings toggle is the opt-out.
- **v1 editor scope:** the rich editor handles **paragraphs and headings** only;
  every other block type silently falls back to the textarea. So "on by default"
  changes the edit surface only for ¶/headings — low blast radius.
- `richText` is a **Q2PreviewIframe-only** prop (like `unlockNestingCursor`);
  `ReactRenderer` only mounts that iframe for `q2-preview`/`revealjs`, so it never
  reaches q2-debug/slides.
- **No preference migration / version bump:** `z.boolean().default(true)` means
  old stored prefs (no `richText` key) parse cleanly and gain the default — the
  exact lesson encoded by the existing `unlockNestingCursor` regression test.

---

## Persistence / infra (confirmed)

- Preferences: `hub-client/src/services/preferences/schema.ts` (zod schema +
  `DEFAULT_PREFERENCES`), persisted in `localStorage` under
  `'quarto-hub:preferences'` (`services/preferences/index.ts`).
- Hook: `hub-client/src/hooks/usePreference.ts` — `usePreference(key)` returns a
  `[value, setValue]` tuple; writes broadcast `PREFERENCE_CHANGE_EVENT` so the
  Settings toggle updates the preview live (no reload).

---

## Work items

### Phase 0 — Tests first (TDD)

- [x] `schema.test.ts` (RED-first confirmed): assert `richText` defaults to `true`, AND (the critical
      migration guard, mirroring the `unlockNestingCursor` regression test) that
      an old stored prefs object **without** a `richText` key still parses,
      preserves its other fields, and fills `richText: true`. Run → confirm the
      "default true" assertions fail before the schema change.
- [x] `ReactRenderer.integration.test.tsx` (RED-first confirmed): a test asserting `richText` is
      forwarded to the mocked `Q2PreviewIframe` (via `capturedPreviewIframeProps`,
      mirroring the existing slide-sync forwarding test). Run → confirm it fails
      (prop not yet forwarded).

### Phase 1 — Preference schema

- [x] `services/preferences/schema.ts`: added `richText: z.boolean().default(true)`
      to `UserPreferencesSchema` (next to `unlockNestingCursor`) and
      `richText: true` to `DEFAULT_PREFERENCES`, with a short doc comment.
- [x] `schema.test.ts` → green (5/5).

### Phase 2 — Plumb the flag

- [x] `components/render/ReactRenderer.tsx`: added `richText?: boolean` to
      `ReactRendererProps` (next to `unlockNestingCursor`), destructure it, and
      forward `richText={richText}` to `<Q2PreviewIframe>`.
- [x] `components/render/ReactPreview.tsx`: `const [richText] =
      usePreference('richText')`; pass `richText={richText}` to `<ReactRenderer>`.
- [x] `ReactRenderer.integration.test.tsx` → green (13/13).

### Phase 3 — Settings toggle

- [x] `components/tabs/SettingsTab.tsx`: `const [richText, setRichText] =
      usePreference('richText')`; add a "Rich-text editor" checkbox in the Preview
      section (mirror the Nesting-cursor toggle), with a clear description.

### Phase 4 — Verify & ship

- [x] Full hub-client suites green: 660 unit + 76 integration (a stale env first failed 3 unrelated suites on a missing `minisearch` dep from the merged search feature; fixed via `npm install` from repo root). Was: `npm run test:integration` (+ unit) for the touched areas,
      then the full hub-client suites.
- [x] `cd hub-client && npm run build:all` — passed (strict project-references gate).
- [\~] End-to-end: NOT driven as a full authenticated hub session — that needs a
      running sync backend + Google auth + an open project, not feasible to stand
      up here (`debug.html` is a renderer harness, not the editor+preview path).
      Verified by layers instead, each leg covered: (1) preference defaults ON
      (schema.test); (2) `richText` reaches `Q2PreviewIframe` (forwarding test);
      (3) `ReactPreview → ReactRenderer` is a trivial prop pass identical to
      `unlockNestingCursor`; (4) `Q2PreviewIframe → PreviewContext.richText →`
      tiptap editor is the SAME component verified end-to-end in the q2 preview
      SPA earlier today (caret-at-click + italics). Stated honestly per the
      end-to-end policy; the user verifies on quarto-hub.
- [ ] `hub-client/changelog.md`: add a user-facing entry (two-commit workflow —
      this touches `hub-client/`).

---

## Risks / things to watch

- **`usePreference` typing:** the hook is generic over `PreferenceKey`; adding
  `richText` to the schema makes `usePreference('richText')` type-check. Confirm
  no exhaustiveness switch over preference keys needs updating.
- **Settings UI placement:** keep the toggle in the same "Preview" section as the
  nesting-cursor toggle for discoverability.
- **Do not** pass `richText` to q2-debug/slides paths — Q2PreviewIframe-only, as
  with `unlockNestingCursor`.
- **Changelog required** (hub-client change, user-facing).
