# hub-client WCAG 2.2 A/AA compliance

Strand: bd-trkzm9rq
Branch: `feature/bd-trkzm9rq-hub-client-wcag-22` (stacked on `feature/editor-projects-home-visual-alignment`)

## Overview

Audit of hub-client against WCAG 2.2 A/AA found a set of concrete
violations. This plan fixes them in a stacked PR. No architectural
changes; one small shared component (`ModalDialog`) is introduced so
the three modal dialogs cannot drift apart on dialog semantics again.

Audit notes (verified against source 2026-08-19):

- **4.1.2 Name Role Value**: `NewFileDialog`, `NewAssetDialog`,
  `ShareDialog` render `.ph-dialog` divs with no `role="dialog"`,
  `aria-modal`, or `aria-labelledby`. `ShareDialog`'s close button has
  no accessible name (only `&times;` text). The 8 inline dialogs in
  `ProjectsHome` have the same gap, and its action menus use
  `role="menu"` with plain `<button>` children (required-owned-elements
  violation).
- **2.4.3 Focus Order**: dialogs move focus in on open but never
  return it on close; no Tab containment, so focus can fall behind the
  modal backdrop.
- **2.4.1 Bypass Blocks**: no skip-to-main link; `<main>` landmarks
  exist (Editor `.editor-main`, ProjectsHome `.ph-main`) but have no
  anchor target.
- **2.5.8 Target Size Minimum (new in 2.2)**: `ViewToggleControl`
  buttons are fixed 20x20px; `.close-btn` is a bare 24px glyph with
  `padding: 0` (clickable width ~13px).
- **2.4.7 Focus Visible**: `.close-btn` and `.rename-input`
  (`outline: none`, no replacement) have no visible focus indicator.
- **1.1.1 Non-text Content**: decorative SVGs in `ViewToggleControl`
  lack `aria-hidden`.
- **1.3.1 Info and Relationships**: `ProjectSelector` (classic home)
  has no `<main>` landmark.

Already compliant (checked, no action): Toast `role="status"`,
EphemeralSessionBanner `role="status"`, connection/status indicators
have text alongside color, form labels associated in all dialogs,
`html lang="en"`, icon buttons in MinimalHeader have aria-labels,
Escape/Enter handled in dialogs, `.ph-input:focus` border-color
indicator.

## Work Items

### Phase 1 — tests first

- [x] `ModalDialog.test.tsx`: role/aria-modal/aria-labelledby, Escape
  closes, Tab stays trapped, focus returns to trigger on close
- [x] Extend `NewFileDialog.integration.test.tsx`: dialog role +
  labelledby assertions
- [x] New `ShareDialog.test.tsx`: close button has accessible name,
  dialog role
- [x] `SkipLink.test.tsx`: renders first-focusable link targeting
  `#main-content`
- [x] `ViewToggleControl.test.tsx`: SVGs aria-hidden, aria-pressed
- [x] Verify all new tests FAIL before implementing (verified:
  module-not-found for new components; missing role/attributes for
  existing ones)

### Phase 2 — implementation

- [x] `ModalDialog.tsx`: shared backdrop + `role="dialog"` +
  `aria-modal="true"` + `aria-labelledby`, Escape handling, Tab trap,
  focus restore; refactor NewFileDialog/NewAssetDialog/ShareDialog
- [x] `ShareDialog` close button accessible name (via ModalDialog
  header)
- [x] `SkipLink.tsx` + render first in `App.tsx` main return;
  `id="main-content"` + `tabIndex={-1}` on Editor `.editor-main` and
  ProjectsHome `.ph-main`; visually-hidden-until-focus CSS
- [x] `ProjectSelector`: `<main>` landmark
- [x] `ViewToggleControl.css`: 20px -> 24px targets; SVGs aria-hidden;
  aria-pressed on toggles
- [x] `ui.css` `.close-btn`: 24x24 minimum box + `:focus-visible`
  outline
- [x] `FileSidebar.css` `.rename-input:focus`: visible indicator
- [x] ProjectsHome: role/aria-modal/aria-labelledby on all 8 inline
  dialogs; drop invalid `role="menu"` from action menus (plain button
  groups; full ARIA menu keyboard pattern not implemented)

### Phase 3 — verification

- [x] `npm run test` (unit) green — 923 passed (83 files)
- [x] `npm run test:integration` green — 111 passed (15 files)
- [x] `npm run lint` — no new problems on changed files (28
  pre-existing on both sides of the diff; new files clean)
- [x] `npm run build:all` succeeds (required for hub-client)
- [x] Playwright e2e spec (`e2e/accessibility.spec.ts`): skip link
  focuses main; New File dialog exposes dialog role, contains Tab,
  Escape closes, focus returns to trigger; ProjectsHome dialog
  semantics. 3 passed. Updated stale `getByRole('menu')` in
  projects-home.spec.ts (2 passed).
- [x] Pre-commit checklist (claude-notes/instructions/review.md)
- [x] Commit 1: code (`e71b1ac5`); Commit 2: `hub-client/changelog.md`
  entry with hash (`0a100657`) — two-commit workflow
- [x] `cargo xtask verify --skip-hub-build` green (needed a local
  `npm install @esbuild/darwin-arm64@0.28.0 --no-save` — the optional
  platform binary was missing from node_modules, breaking the
  quarto-hub-mcp bundle test; environmental, unrelated to this diff)
- [ ] Report; do NOT push without explicit approval

## Details

- ModalDialog keeps each dialog's existing Enter-key behavior via an
  `onKeyDown` passthrough; it owns Escape, trap, and restore only.
- Skip link lives in `App.tsx`'s main return so it serves both the
  projects-home and editor views; early-return screens (loading,
  login, setup) are single-purpose and don't need it.
- Font-size findings from the audit (11.5px/10.5px in ui.css) are NOT
  WCAG violations (no minimum size in 2.2; 1.4.4 is about resize
  capability) — deliberately out of scope.
- Contrast findings were all >= 4.5:1 or marginal-but-passing — out of
  scope.
- Adding eslint-plugin-jsx-a11y / axe-core is a follow-up, not this
  PR (new dependency, needs discussion).
- ProjectsHome dialogs got ARIA semantics only; Tab containment there
  would require extracting each inline dialog into a component (they
  are conditionally rendered blocks, so no shared hook). Follow-up
  candidate.

## E2E verification record

- `npx playwright test e2e/accessibility.spec.ts` — 3 passed:
  skip link Tab -> focus -> Enter -> `main#main-content` focused;
  New File dialog `role="dialog"`/`aria-modal`/accessible name,
  autofocus into filename, Tab stays inside, Escape closes, focus
  returns to the "+ New" trigger; ProjectsHome "New collection"
  dialog semantics.
- Output inspected via Playwright assertions (focused element,
  attributes, visibility) — not just absence of errors.
