# ProjectsHome polish: dark-mode contrast, right-click menu, per-collection sort

**Strand:** bd-je3w8q39
**Branch:** `feature/85-projects-collections-ui` (PR #394, after merging main on 2026-07-22)
**Context:** Review feedback from Carlos while end-to-end testing PR #394's
collections-based projects home against a local dev server.

## Overview

Three UI improvements to `hub-client/src/components/ProjectsHome.tsx` / `.css`:

1. **Dark-mode surface: darker, less saturated.** The projects home inherits
   `--bg-modal` → `--posit-blue-dark-2` (`#213D4F`), a saturated navy. Some
   elements are hard to read on it — notably the "Connect / Import" outline
   button, whose text color `--accent-secondary` → `--posit-blue` (`#447099`)
   has poor contrast against the navy. Fix by scoping token overrides to
   `:root.dark .projects-home` in `ProjectsHome.css` (custom properties
   cascade, so descendants pick them up) rather than touching global tokens in
   `theme.css`, which would restyle every modal in the app.
2. **Right-click opens the project context menu.** Project cards inside
   collections (and rows in "Everything else", for consistency) get an
   `onContextMenu` handler that opens the same menu as the ⋯ button, replacing
   the browser's native context menu on those elements.
3. **Per-collection sort control.** Each collection header gets a sort button
   (mirroring the existing global sort for "Everything else": newest / oldest /
   name) that reorders that collection's cards. Today `renderCollection`
   hardcodes newest-first. Per-collection choice lives in component state
   (`Record<collectionId, SortOrder>`), defaulting to `newest`; it is a local
   view preference, not synced to the collection document.

## Design decisions

- **Scoped dark tokens, not global.** `ProjectsHome.css` already routes all
  colors through theme tokens; we redefine ~8 of them on
  `:root.dark .projects-home` with a darker, desaturated slate ramp
  (hue kept slightly blue, chroma way down). Light mode untouched.
- **Sort comparator extracted for testability.** The three-way comparator
  currently inlined in `everythingElse` moves to
  `src/utils/projectSort.ts` (`sortProjectItems(items, order)`), used by both
  the global list and per-collection sorting. Unit-tested (TDD: tests written
  first).
- **Right-click reuses existing menu state** (`openMenu`), so Escape /
  click-outside / one-menu-at-a-time behavior is identical to the ⋯ path.
- Per-collection sort state is **not persisted** (resets on reload). If we
  later want persistence it belongs in local user settings, not the shared
  collection doc — flag in review if desired.

## Work items

### Phase 1 — tests first

- [ ] `src/utils/projectSort.test.ts`: newest / oldest / name orderings,
      tie and empty-list behavior (comparator does not throw, stable input
      untouched). Verify tests fail against a stub before implementing.

### Phase 2 — implementation

- [ ] `src/utils/projectSort.ts`: extract comparator; rewire `everythingElse`.
- [ ] `ProjectsHome.tsx`: `onContextMenu` on `.ph-card` and `.ph-row`.
- [ ] `ProjectsHome.tsx`: per-collection sort state + header button + menu;
      `renderCollection` sorts via the helper.
- [ ] `ProjectsHome.css`: `:root.dark .projects-home` token overrides;
      readable outline-button color in dark mode.

### Phase 3 — verification

- [ ] `cd hub-client && npm run test` (vitest) — new tests green.
- [ ] `npm run build:all` — production build green.
- [ ] End-to-end against the running dev server (localhost:5173) via Chrome:
      dark-mode contrast inspected, right-click menu exercised on a card in a
      collection, per-collection sort toggled; screenshots/notes recorded here.
- [ ] Changelog entries (two-commit workflow) + `npm run test:wasm`.

## Verification record

_(to be filled in during Phase 3)_
