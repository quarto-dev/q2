# Hub-Client UI/UX Modernization Plan

**Date:** 2026-08-25
**Status:** Approved 2026-08-25 — Phases 0–4 complete (Phase 3 = PR #611); Phase 5 implemented 2026-08-27 (branch `hub-client-uiux-phase5`, deck at `.worktrees/phase5-review-deck/`) — awaiting design review
**Tracking:** braid epic `bd-2q55e6rc` (per-phase strands hang off it)
**Scope:** `hub-client/` React/TypeScript app only — no Rust, no WASM, no sync-protocol changes.

## Overview

The hub-client has a solid foundation: ~110 CSS custom-property tokens with
light/dark themes, a recent WCAG 2.2 pass (e71b1ac5), and a coherent visual
language — white surfaces + hairlines in light, desaturated slate in dark,
teal primary actions — introduced by the July 2026 projects-home rework
(f21e851e, the "QH-ProjectManagement-July26" language) and extended across the
editor chrome in August (8d5d4297, 99a5db89, 54bd1e3e). It also has a
WCAG-compliant `ModalDialog`, a skip link, and Playwright visual-regression
infrastructure (`playwright.visual.config.ts` + `DevHarness` `#/dev/` routes).

What it lacks is **systematization** — the layer a discerning enthusiast
notices: values live in ad-hoc numbers instead of scales, interaction patterns
are re-invented per component, keyboard support is uneven, motion is
unsystematic, and terminology drifts. This plan hardens the existing
zero-dependency, custom-CSS architecture into a coherent design system and
polishes the interaction details, building on the QH-ProjectManagement-July26
visual language rather than replacing it.

The work is staged so that **Phases 0–4 are objective** — WCAG compliance,
consistency, and engineering hygiene, value-preserving against the visual
baselines — and can be implemented without design feedback. Every opinionated
visual change is gathered into **Phase 5, the single feedback gate**, reviewed
with before/after diffs.

## Decisions locked (from user, 2026-08-25)

| Question | Decision |
|----------|----------|
| Component primitives | **Harden hand-rolled** — zero new runtime deps; build keyboard nav, focus management, ARIA into existing components |
| Icons | **Curate shared icon module** — consolidate per-component SVG functions into one `icons.tsx`; keep the existing Lucide/Feather visual style |
| Responsive scope | **Desktop polish + graceful narrow** — editor shell gains narrow-window handling; no tablet/touch workstream |
| Dual project selectors | **Leave as-is** — ProjectSelector/ProjectsHome consolidation is out of scope |
| WCAG scanning | **Approved (2026-08-25):** `@axe-core/playwright` devDependency for automated axe-core scans — matches quarto-cli's established checker (bundled axe-core 4.10.3). Scans run locally from Phase 0; CI wiring is deferred to Phase 7 |
| Staging | **Objective phases first (2026-08-25):** Phases 0–4 are WCAG/consistency/engineering only; all opinionated visual changes are gated in Phase 5 for a single review pass |

## Current-state findings (evidence for the work items)

**Token/scales gaps**
- No spacing, radius, shadow, z-index, or type scales — magic numbers throughout:
  radii of 4/5/6/7/8/9/10/12px all in use; z-index values scattered across
  0/1/10/30/40/60/70/100/1000/2000/10000/10001 with no layering system;
  font sizes from 10px to 35px ad-hoc, including half-pixel steps
  (10.5/11.5/12.5/13.5px).
- Hardcoded colors bypassing tokens: `FileSidebar.css:198` drop-overlay
  `rgba(65,149,153,0.08)`; `App.css` toggle shadow `rgba(0,0,0,0.2)`;
  `ProjectsHome.css` card shadow.
- Off-palette token values: the outline-panel icon tokens
  (`--outline-header-icon`/`--outline-code-icon`/`--outline-function-icon`,
  `theme.css:93-95`) hold Tailwind-palette hexes `#9333ea`/`#2563eb`/`#b45309`
  — off-brand next to the Posit ramps; `#2563eb` is also reused for
  `--replay-actor-me-text` (`theme.css:134`).
- Two different monospace stacks in use — `'SF Mono', Monaco…` and
  `'JetBrains Mono', ui-monospace…` — including within `Editor.css` itself
  (lines 25/534/587 vs 120); `ProjectsHome.css` uses the JetBrains Mono stack.
- `.ph-*` class prefix with a "rename to `.qh-*` pending" comment (`ui.css:4`).
- Duplicated patterns: truncation (`nowrap/hidden/ellipsis`) repeated 5+ times;
  `brightness(1.08)` hover duplicated across buttons; active-accent row
  treatment duplicated between file items and search results.

**Interaction gaps**
- FileSidebar context menu is right-click-only: no keyboard invocation
  (Shift+F10 / Menu key), no arrow-key navigation, no visible kebab affordance.
- `:focus-visible` styles exist in only two places (`SkipLink.css`,
  `ui.css` close button) — most interactive elements rely on browser defaults.
- Three notification patterns with inconsistent dismissal: `Toast` (2s
  auto-dismiss), `UpdateAvailableToast` (dismissible, re-appears),
  `EphemeralSessionBanner` (permanent, no close).
- Tooltips are native `title` attributes — unstyled, no delay control, not
  touch/keyboard friendly.
- Status conveyed by color alone in places (online dot, diagnostic severity).
- `prefers-reduced-motion` respected only in `ReplayDrawer.css`.
- Loading states are bare "Loading..." text; no skeletons.
- Terminology drift risk: e.g. header says "Switch project" while the nav
  refactor decision recorded "Choose New Project" — needs a copy audit.

**Assets to build on**
- `theme.css` token architecture is sound — extend it, don't replace it.
- `ModalDialog` focus management is exemplary — make it the enforced pattern.
- `DevHarness` + visual specs = a Storybook-free component gallery waiting to
  be grown.
- Recent sidebar cleanup (b62746e7) shows the intended direction.

## Design principles for the work

1. **Token-first**: no hex colors or bare z-index outside `theme.css` scale
   tokens after Phase 0 (enforced by `lint:css`); spacing, radii, shadows,
   and type migrate onto the scales through the Phase 1 component
   consolidation and the Phase 5 alignment pass; new or touched CSS always
   uses scale tokens.
2. **One pattern per problem**: one button system, one menu, one tooltip, one
   notification model, one focus ring.
3. **Keyboard parity**: every pointer affordance has a keyboard path; focus is
   always visible; ARIA patterns follow WAI-ARIA APG (treeview, menu, dialog).
4. **Respect the QH-ProjectManagement-July26 visual language**: Posit palette,
   teal primary, hairline borders, slate dark ramp. Refine, don't rebrand.
5. **Motion is functional**: 100–200ms, ease-out entrances, no gratuitous
   animation, `prefers-reduced-motion` honored globally. Animate
   `transform`/`opacity` only — no layout-property transitions.
6. **Zero new runtime dependencies.** Dev-only tooling requires explicit
   approval; `@axe-core/playwright` is approved (see decisions table).
7. **Direction-agnostic CSS**: logical properties (`margin-inline-start`,
   `inset-inline-end`) over physical ones in new/refactored CSS, enforced by
   `lint:css`, so a future RTL pass is not blocked.
8. **Objective before opinionated**: Phases 0–4 need no design feedback —
   they are value-preserving or standards-driven. Taste-level visual changes
   wait for the Phase 5 feedback gate.

## Process notes (repo conventions that apply)

- Per CLAUDE.md, test specifications come first in each phase. For visual work
  the honest adaptation is **characterization testing**: capture visual
  baselines of current behavior before changing it, then every visual change
  is a deliberate, reviewed diff in the Playwright report.
- Each phase ends at a clean commit boundary: `npm run build:all` +
  `npm run test:ci` green from `hub-client/`, then the two-commit changelog
  workflow (`hub-client/changelog.md` entry with the commit hash).
- On approval, create a braid epic + child strands per phase and a
  `claude-notes/plans/` pointer to this file, per repo workflow.
  (Done — and this file has since moved into `claude-notes/plans/`
  itself, replacing the pointer.)
- End-to-end verification per CLAUDE.md: visual claims are verified in a real
  browser session (dev server or `local-prod`), not inferred from tests.

---

## Phase 0 — Token foundation & CSS hygiene

**Goal:** every visual value flows from a scale; no off-token values remain.
Token migrations are value-preserving: the characterization baselines captured
in this phase must diff clean after migration — intentional value changes are
deferred to Phase 5.

### Test specifications (write first)
- [x] Add a `npm run lint:css` script (dependency-free Node script, mirroring
      the `cargo xtask lint` philosophy) that fails on: hex/`rgb()`/`rgba()`
      colors outside `theme.css`; bare `z-index` integers outside token
      definitions; `outline: none` without a `:focus-visible` counterpart in
      the same file; physical box properties (`margin-left`, `padding-right`,
      `left:`/`right:` insets…) where a logical equivalent exists. Initially
      runs with a grandfathered-exceptions list, burned down per rule:
      color/z-index to empty in Phase 0, `outline: none` in Phase 2 alongside
      the focus work, and physical properties opportunistically (any file
      touched in Phases 1–5 leaves clean) with repo-wide enforcement flipped
      on in Phase 7.
      **Done (16cb20b1):** `hub-client/scripts/lint-css.mjs` + exceptions
      JSON. One refinement: token *definitions* (custom-property
      declarations) may hold literals in any file, so the standalone
      src/debug/ page (which doesn't load theme.css) keeps its local token
      block; use sites must still reference var(--token).
- [x] Extend `DevHarness` with a `tokens` page rendering every scale token
      (spacing swatches, radii, shadows, type specimens) in both themes.
      **Done (305fec7c):** `#/dev/tokens` + visual/axe coverage.
- [x] Capture Playwright visual baselines of key screens (editor, projects
      home, dialogs, sidebar sections) in light + dark **before** any visual
      change — characterization baseline for all later phases.
      **Done (16cb20b1):** `e2e/baseline-screens.visual.spec.ts` — 8 harness
      routes × 2 themes. Scope note: the full editor shell can't render
      without sync+Monaco+WASM (the no-server visual config avoids those by
      design), so its chrome is baselined surface-by-surface (header,
      sidebar sections, dialogs, notifications).
- [x] Add `@axe-core/playwright` (**approved 2026-08-25**) and capture
      axe-core scans of the existing key screens (editor shell, projects
      home, each dialog) in both themes **before** any visual change — the
      a11y counterpart to the visual baselines, so token migrations that
      break contrast fail immediately rather than two phases later. Scans run
      locally (via `npm run test:ci`) from here on; wiring them into CI as a
      blocking check is deferred to Phase 7.
      **Done (16cb20b1):** `e2e/baseline-a11y.visual.spec.ts` +
      `helpers/axe-baseline.json`. Characterization model: current
      serious/critical violations (all color-contrast) are baselined per
      page+theme; new/worsened violations fail, fixed ones force a
      regeneration (`AXE_BASELINE_WRITE=1 ... --workers=1`). Run via
      `npm run test:visual` (the no-server Playwright config) rather than
      `test:ci` — test:ci is the vitest suites; wiring anything into
      blocking CI is Phase 7.

### Work items
- [x] Introduce scale tokens in `theme.css`:
      spacing (`--space-1`…`--space-8`, 4px base), radii
      (`--radius-sm/md/lg`), elevation (`--shadow-1/2/3`, per-theme values),
      z-layers (`--z-sticky/dropdown/overlay/modal/toast`), type scale
      (`--text-xs`…`--text-xl` + `--font-weight-*` + `--leading-*` +
      `--font-mono`),
      motion (`--duration-fast/base`, `--ease-out`, `--ease-standard`),
      `--focus-ring` (consistent 2px ring with offset, per-theme color).
      **Done (305fec7c).** The z-scale grew two extra layers (--z-base/raised
      for the double-buffered preview, --z-header, --z-skip, --z-max,
      --z-revealjs-menu) to cover actual usage value-preservingly.
- [x] Document the token layering convention (primitive ramp → semantic
      alias → component usage) in a header comment in `theme.css`, so new
      tokens land on the right layer. **Done (305fec7c).**
- [x] Migrate known offenders to tokens with identical computed values:
      `FileSidebar.css:198` drop overlay, `App.css` shadow, `ProjectsHome.css`
      card shadow. (Re-mapping the off-palette `--outline-*-icon` /
      `--replay-actor-me-text` values is a visual change — deferred to
      Phase 5.) **Done (7ea60da9)** — subsumed by the full color burn-down.
- [x] Complete the `.ph-*` → `.qh-*` rename (mechanical; do early before more
      CSS accrues). **Done (df1cd10b):** 611 replacements across 22 files;
      e2e selectors updated; verified pixel-clean.
- [x] Extract shared utilities in `ui.css`: `.qh-truncate`, standard hover-bg
      and active-accent-row mixins (as documented shared classes).
      **Done (7ea60da9):** `.qh-truncate` (5 exact-trio sites),
      `.qh-row-hover`, `.qh-active-accent-row` (logical
      border-inline-start). `.file-item`'s margin-offset variant of the
      active row is left for Phase 1 — unifying it is an alignment change,
      not a value-preserving one.
- [x] Burn down the `lint:css` exceptions list to empty for color/z-index rules.
      The `outline: none` rule's exceptions are owned by Phase 2 (focus work);
      the physical-properties rule burns down opportunistically — any file
      touched in Phases 1–5 leaves the file clean — with repo-wide enforcement
      flipped on in Phase 7.
      **Done (7ea60da9):** 187 → 85 exceptions; color and z-index sections
      are empty (enforced repo-wide from here).
- [x] Changelog entry (user-visible polish only if any; otherwise note as
      internal — changelog policy says no refactors, so likely no entry).
      **Decision: no entry** — Phase 0 is entirely internal (no user-visible
      change; baselines prove pixel-identity).

## Phase 1 — Component consistency — **DONE (2026-08-26, branch `hub-client-uiux-phase1`)**

**Goal:** one canonical implementation per primitive, styled by the scales.
Consolidation adopts the existing dominant pattern's appearance — this phase
aligns outliers, it does not restyle; visual diffs against baselines should
only ever show a genuine inconsistency being aligned.

### Test specifications (write first)
- [x] Grow `DevHarness` into a component gallery: every primitive (button
      variants/sizes/states, icon buttons, menus, dialogs, form controls,
      toasts, banners, tooltip) in default/hover/focus/disabled/error states ×
      light/dark.
      **Done (7e5de6fe):** `#/dev/gallery` (DevGalleryPage) — buttons, icon
      buttons, menu, form controls, icons; tooltip + disabled-input sections
      added in later commits. Covered by visual + axe baselines.
- [x] Playwright visual spec per gallery page; keyboard-interaction specs for
      the new menu (arrow keys, Home/End, Escape, type-ahead, focus return).
      **Done (7e5de6fe):** `e2e/menu-keyboard.visual.spec.ts` (9 specs);
      `e2e/projects-home-dialogs.visual.spec.ts` (dialog contract via the
      real UI) added in c740728a.

### Work items
- [x] **Button system** (e6a9a0f0): `.qh-btn` (variants + `.small`) and
      `.qh-icon-btn` (+ `.boxed` 28×28) are the system; disabled and
      token focus-visible states live on the base classes. MinimalHeader's
      `.icon-btn` folded into `.qh-icon-btn.boxed`. Documented boundaries:
      view-toggle = segmented control, `.qh-pager` = nav strip,
      `.preview-btn` = header primary pill. Also fixed a real dark-theme
      bug the gallery axe scan exposed: bare `.qh-btn` declared no color
      (black-on-dark, 1.35:1).
- [x] **Menu component** (7e5de6fe): `components/Menu.tsx` — APG
      menu-button pattern (arrows/Home/End, type-ahead, submenus with
      ArrowRight/ArrowLeft, Escape + focus return, scroll-into-view,
      viewport-edge flip for fixed placement). Adopted by the FileSidebar
      context menu (plus a visible hover/focus kebab per file row — new
      chrome, for Phase 5 ratification) and all four ProjectsHome action
      menus. The avatar popover stays a styled popover (it contains a
      form). Destructive-action rule (confirm-guard or undoable)
      documented in the module header. Copy-feedback items keep the menu
      open via `keepOpen`.
- [x] **Tooltip component** (73d9cbe5): `components/Tooltip.tsx` — 400ms
      hover delay, immediate on focus, `aria-describedby`, Escape dismiss,
      viewport-edge flip + horizontal clamp. Replaced every `title=` in
      the app chrome; iframe `title` (accessible name), AST content links,
      the classic ProjectSelector, and src/debug were deliberately
      skipped; redundant titles duplicating visible text were dropped and
      icon-only buttons gained `aria-label`s.
- [x] **Notification model** (c8c9883f): one system in
      `components/notifications.css` with the three documented tiers
      (transient / dismissible-persistent / session banner) and placement
      rules. Class names unchanged; Toast.css/UpdateAvailableToast.css
      deleted; banner rule moved out of Editor.css.
- [x] **Dialogs** (c740728a): all eight ad-hoc ProjectsHome dialogs routed
      through `ModalDialog` with `.qh-form-dialog` + `.dialog-content` /
      `.dialog-actions`. Two focus bugs fixed: ModalDialog captured its
      restore target post-autoFocus (now at render), and Menu's
      synchronous focus return stole focus from opening dialogs (now
      deferred past the commit and skipped when a dialog owns focus).
- [x] **Form controls** (b6ecc616): `.qh-input` gained disabled +
      `aria-invalid` states; the validation pattern is documented
      (field-level `aria-invalid` + `aria-describedby`; form-level
      `.qh-error.inline`); the SettingsTab custom checkbox gained a
      visible focus ring. Sidebar search/rename inputs remain documented
      contextual variants (compact sidebar chrome).
- [x] Status indicators gain icon/text alongside color (b6ecc616): audit
      found the plan's concern already satisfied — connection (dot +
      Online/Offline text), renderer status (dot + label), collaborators
      (dot + name), diagnostics (Monaco markers). No color-alone status
      remains.
- [x] **Icon module** (3b0729c2): `components/icons.tsx` with the
      documented contract (decorative `aria-hidden`, 24×24 stroke style,
      `currentColor`, size prop); 14 icons consolidated from 5 components;
      `MoreIcon` added for the kebab. Brand logo and the replay waveform
      (data viz) stay local.
- [x] **Monospace unification** (c920ad82): all three ad-hoc stacks across
      10 CSS files migrated to `var(--font-mono)` (JetBrains Mono stack,
      the dominant one). Baselines diffed clean.
- [x] **Design-system note** (b6ecc616): `hub-client/design-system.md` —
      token layers, primitives table, how-to-add-a-component rules.
- [x] Changelog entry (0200bc0b): three entries for the user-visible
      changes (menus/kebab, tooltips, dialogs).

## Phase 2 — Keyboard & assistive-tech completeness — **DONE (2026-08-26, branch `hub-client-uiux-phase2`)**

**Goal:** full keyboard parity; screen-reader coherence; documented shortcuts.

### Test specifications (write first)
- [x] Playwright keyboard-only walkthroughs: full tab order of the editor
      shell, file-tree arrow navigation, outline navigation, sidebar section
      switching, menu operation, dialog open/close with focus return.
      **Done (616f3df8):** `e2e/sidebar-keyboard.visual.spec.ts` — 15 specs
      over the (now stateful) `#/dev/sidebar` harness route. Scope note: the
      full editor shell can't boot in the no-server harness (Phase 0's known
      limit), so the tab-order walkthrough covers the sidebar chrome
      surface-by-surface.
- [x] Extend the axe-core scans (added in Phase 0) to the full e2e suite: one
      scan per DevHarness gallery page in addition to the key screens, in
      both themes, failing the local test run on serious/critical violations.
      **Done (61b122de):** `e2e/gallery-states-a11y.visual.spec.ts` — the
      interactive states the static scans never render (open menu, visible
      tooltip), both themes, strict zero-tolerance scoped via `.include()`.
      Caught a real one: menu shortcut hints failed contrast in dark
      (3.6:1) — fixed via a new `--menu-hint-text` semantic token.
- [x] Playwright spec emulating `forced-colors: active` over the DevHarness
      gallery and key screens, asserting controls keep visible boundaries
      (system-color keywords in effect); a manual Windows High Contrast pass
      is documented alongside the screen-reader smoke script.
      **Done (61b122de):** `e2e/forced-colors.visual.spec.ts` — 4 specs
      (gallery controls + menu, dialog, selected tree row, tooltip).
- [x] Screen-reader smoke script (manual, documented): VoiceOver pass over
      header, file tree, dialogs, notifications.
      **Done (66306759):** `hub-client/screen-reader-smoke.md`, including
      the manual WHCM pass.

### Work items
- [x] Global `:focus-visible` ring via `--focus-ring` on every interactive
      element; remove any bare `outline: none`.
      **Done (61b122de):** the lint:css `outline: none` exceptions list is
      empty — the rule is enforced repo-wide. `.qh-input` gained the ring
      (border-color-only focus was too subtle); two box-shadow halo rings
      standardized onto the token.
- [x] File tree: APG treeview pattern — roving tabindex, arrow keys, Home/End,
      type-ahead, Enter to open, Shift+F10/Menu key for context menu.
      **Done (616f3df8)** — plus search results as a listbox with the same
      keys; row kebabs left the tab order (Shift+F10 is the keyboard path).
- [x] Outline panel: keyboard activation (Enter/Space) for all rows; collapse
      toggles keyboard-operable. **Done (616f3df8):** rows were already
      buttons; chevrons gained `aria-expanded` + per-symbol names.
- [x] Sidebar sections: APG accordion/tab semantics verified
      (`aria-expanded`, keyboard switching). **Done (616f3df8):** added
      `aria-controls` → `role="region"` panels with `aria-labelledby`.
- [x] Audit and complete `aria-label`s on all icon-only buttons; add
      `aria-describedby` where behavior isn't obvious (scroll-sync toggle,
      replay controls). **Done:** audit found Phase 1 had covered icon-only
      buttons; added the replay waveform slider's `aria-label`
      (61b122de). Scroll-sync toggle's visible description is inside its
      `<label>` (part of the accessible name); replay controls have labels
      + tooltips — no `aria-describedby` gaps remain.
- [x] Forced-colors (Windows High Contrast) audit: under
      `@media (forced-colors: active)`, hairline-only boundaries and
      shadow-only elevation get system-color treatments (`ButtonBorder`,
      `CanvasText`). **Done (61b122de):** `.qh-icon-btn`, `.qh-dialog`,
      `.qh-tooltip`, toasts. Buttons/inputs/menus already keep boundaries
      (UA-forced borders / existing hairlines); the selected tree row stays
      distinguishable via its accent border (forced to CanvasText).
      `forced-color-adjust: none` was not needed anywhere.
- [x] Keyboard shortcut map documented in one module; add a shortcuts
      reference (About tab section or `?` overlay).
      **Done (66306759):** `src/utils/keyboardShortcuts.ts` + About tab
      section rendering it, pinned by `AboutTab.test.tsx`.
- [x] Copy/terminology audit: one name per concept across header, menus,
      dialogs, settings, and docs (e.g. switch-project wording); consistent
      capitalization and punctuation in user-facing strings. Centralize the
      strings into one module (like the shortcut map) — not full i18n, but a
      single source that enforces terminology by structure and unblocks a
      future i18n pass.
      **Done (9b91c96d):** `src/strings.ts` with the copy conventions in
      the header; chrome components migrated. ProjectsHome/ProjectSelector/
      ProjectSetSetup keep local strings (noted in the module). Fixes:
      "Upload asset" → "Add asset", Title Case menu items → sentence case,
      `...` → `…`. "Delete" (files) vs "Remove from this device" (synced
      projects) turned out to be a real semantic distinction, not drift.
- [x] Changelog entry. **Done (23890411):** four entries.

**Fixes found by the new specs (worth knowing):**
- Menu→dialog focus chain was broken end-to-end (pre-existing
  `projects-home-dialogs` failure): Menu's deferred focus return let the
  dialog capture the doomed menu item as its restore target. Menu now
  returns focus synchronously *before* the commit and handles Enter/Space
  explicitly (the default activation click otherwise lands on the
  re-focused trigger); ModalDialog's restore is deferred past the commit
  so StrictMode's mount-time cleanup can't steal the autoFocus (9b91c96d).
- `baseline-screens` full-page screenshots were too coarse to catch
  sidebar-local changes (0.86% of pixels, under the 1% tolerance) —
  component routes now screenshot the surface's container element
  (616f3df8). **CI follow-up:** the committed `chromium-linux` baselines
  need regeneration for this (element crop + sidebar search box + dialog
  title casing + input focus ring) — use the `recreate-all-snapshots`
  workflow dispatch on this branch; local darwin snapshots are gitignored.

## Phase 3 — Functional states & motion safety — **DONE (2026-08-26, branch `hub-client-uiux-phase3`)**

**Goal:** every async surface has working loading/error/empty behavior, and
motion is safe for all users. Presentation here is plain and token-styled;
designed visuals (skeletons, transitions, micro-states) are deferred to the
Phase 5 feedback gate.

### Test specifications (write first)
- [x] Visual specs run with animations disabled (deterministic screenshots).
      **Done (92fdd7ba):** `bootHarness` emulates `reducedMotion: 'reduce'`
      for every visual spec, so determinism comes from the app's own global
      rule rather than Phase 0's addStyleTag transition-killer (removed).
      **Trap recorded:** Playwright 1.60 silently ignores the config-level
      `reducedMotion` option on the default context (verified:
      `browser.newContext` honors it, project `use` does not) — emulation
      lives in the helper, with a comment in `playwright.visual.config.ts`.
- [x] Playwright spec emulating `prefers-reduced-motion: reduce` asserting no
      transitions/animations are applied (computed-style checks).
      **Done (92fdd7ba):** `e2e/reduced-motion.visual.spec.ts` — 3 reduce
      specs (sidebar/header transitions, toast entrance animation +
      iteration count) + 2 `no-preference` counter-checks proving the
      assertions aren't vacuous.
- [x] Playwright specs covering each async surface (project list, file tree,
      preview pane boot): a loading state, an error surface with working
      retry, and an empty state with correct copy.
      **Done (601d291c):** `e2e/async-states.visual.spec.ts` — 14 specs over
      six new dev-harness routes (`projects-home-loading/-error/-empty`,
      `sidebar-empty`, `status-tab-loading/-error`), with offscreen action
      recorders asserting the retry buttons actually fire. Preview pane boot
      is covered at its status surface (StatusTab WASM states) — the real
      boot needs WASM+sync, the no-server harness's known limit. axe
      coverage for the new routes joined the characterization baseline
      (they render pre-existing chrome with baselined contrast debt).

### Work items
- [x] Global `prefers-reduced-motion` handling (extend the ReplayDrawer
      pattern app-wide) — structured so any transition added in Phase 5 is
      automatically covered.
      **Done (92fdd7ba):** one global rule in `ui.css` collapses all
      durations to 0.01ms under reduce (0.01ms, not `none`, so
      transitionend/animationend listeners still fire); ReplayDrawer's
      local block removed as subsumed; per-component blocks banned by
      convention (documented at the rule).
- [x] Loading/error/empty states for every async surface, function first:
      error surfaces with retry for failed loads (project list, file open,
      sync errors); bare "Loading..." text replaced by a simple token-styled
      indicator; correct empty copy. Skeleton screens and illustrated empty
      states are Phase 5 visual work.
      **Done (601d291c):** `components/Loading.tsx` (spinner + label,
      `role="status"`) replaced every bare "Loading…" (App auth/identity
      gates, ProjectsHome, classic ProjectSelector). Retry: legacy
      project-list load failure no longer falls through to the wrong "No
      projects yet" copy (new error surface + Try again); failed project
      opens offer Try again via a new `onRetry` prop (App re-invokes the
      last attempt); WASM boot errors offer Reload (StatusTab `onRetry`,
      defaults to page reload). `.qh-error-action` now inherits banner
      colors (`currentColor`) so it works on editor-theme error surfaces.
      **Found by the new specs:** the empty file tree rendered
      `role="tree"` with zero items (aria-required-children) — the
      tree/listbox now drops its widget role when empty. **Scope-outs:**
      classic ProjectSelector's error banner keeps its form (dual-selector
      consolidation is out of scope); in-editor sync loss was already
      covered (header Online/Offline + auto-reconnect). **Debt filed:**
      the new baselines record pre-existing contrast failures on surfaces
      never scanned before (status-tab error text 3.04:1, section labels
      3.91:1, teal primary buttons 3.5:1) — burn-down strand filed.
- [x] Changelog entry (retry/error behavior is user-visible).
      **Done (418a3626):** two entries (retry/loading, reduced motion).

**Verification:** typecheck, lint:css, test:ci (1251 tests), test:visual
(115 specs incl. the 19 new ones), and build:all all green; new snapshots
inspected visually (error banner + Try again render correctly in both
themes). **CI follow-up:** the new routes need `chromium-linux` baselines
via the `recreate-all-snapshots` workflow dispatch on this branch (or the
retry step's auto-commit), same as Phase 2.

## Phase 4 — Graceful narrow viewports — **DONE (2026-08-26, branch `hub-client-uiux-phase4`)**

**Goal:** small windows and split-screen use degrade gracefully; no horizontal
scroll, no clipped controls.

### Test specifications (write first)
- [x] Playwright viewport matrix specs at 1280 / 900 / 700 / 480 / 320px
      widths for editor shell, projects home, and each dialog — 320px covers
      the WCAG 1.4.10 reflow requirement (400% zoom at 1280px).
      **Done:** `e2e/viewport-matrix.visual.spec.ts` — 44 layout assertions
      (no horizontal scroll on document + surface; controls inside viewport)
      + 34 screenshots (both themes at the widths that depart from the 1280
      baselines). New composed `#/dev/editor-shell(-markup/-preview)` harness
      routes render the real MinimalHeader/SidebarTabs/`.editor-main
      view-mode-*` flex rules with placeholder panes (Monaco/iframe need
      services the no-server harness lacks). Shared assertions live in
      `e2e/helpers/visual.ts` (`expectNoHorizontalScroll`,
      `expectInsideViewport`). **Watched failing first:** 4 assertion
      failures at 320px (projects-home header overflow 94px, avatar menu
      clipped left, new-asset dialog clipped top+bottom in a short viewport,
      preview-mode panes overflow 42px); the row-identity spec was written
      alongside its fix and verified failing by reverting the rule (name
      squeezed to 14px).

### Work items
- [x] Dialogs/menus verified against small viewports (max-width rules already
      partially exist — audit and complete). **Done:** dialogs already cap
      width (`max-width: calc(100vw - 48px)`); added the missing height cap —
      `.qh-dialog` gets `max-height: calc(100vh - 48px)` + flex column, and
      `.dialog-content` scrolls internally (header/actions pinned), fixing
      top+bottom clipping of tall dialogs in short windows. Menus gained
      `max-width: calc(100vw - 48px)` (content-box, so the 320px-wide avatar
      menu no longer clips past the left edge at 320px; the 250px min-width
      floor still wins where they conflict). Menu/Tooltip already had
      fixed-placement viewport flip/clamp (Phase 1) — anchor-relative menus
      rely on right-anchoring, verified by matrix specs opening the New,
      avatar, and file-row context menus at 320px. **Review follow-up
      (same branch):** the peek popover (`.qh-peek`, 320px content-box =
      360px total, anchored to the row's left edge) clipped 57px past a
      320px viewport — at ≤480px it now spans its anchor's full width
      (the row/card is always inside the viewport). The row-menu submenu
      needed no fix: `.qh-row .qh-menu` (specificity 0,2,0) overrides
      `.qh-submenu`'s rightward `left: calc(100% + 4px)`, anchoring
      submenus to each item's right edge — inside the viewport at 320
      and 1280 alike (measured, then pinned by a matrix spec).
- [x] ProjectsHome grid: extend the existing 980/760 breakpoints for
      intermediate widths. **Done:** new ≤480px breakpoint — card grid to one
      column; header wraps to two rows (actions row, full-width search row)
      with the brand mark giving way (the wordmark already drops at 760px);
      `.qh-main` padding 30px → 16px. Project rows: the name keeps an 8ch
      floor and the metadata ellipsizes around it — previously the nowrap
      meta plus the row's three action buttons could squeeze the name to
      zero width (found in screenshot review, not by the matrix assertions).
- [x] Editor shell at narrow widths: fix clipping/overflow in place —
      truncation, wrapping, min-widths — so nothing breaks. **Done:** ≤700px:
      markup/preview summary-strip min/max-widths 120–180px → 80–120px;
      header `.project-name` truncates with ellipsis (media-query scoped —
      `overflow: hidden` changes the span's baseline alignment, and the
      1280px baselines must stay pixel-identical). ≤480px: sidebar width
      floor 180px → 120px (rows/labels already truncate); MinimalHeader
      wraps to two rows — previously `.header-left` collapsed to ~9px flex
      width while its icon buttons painted on, ending up *under*
      `.header-right` (later in paint order): an overlap no viewport-bounds
      assertion catches, pinned by asserting `.header-left`'s own
      scrollWidth. Layout redesigns (sidebar drawer, split-view collapse,
      header overflow menu) remain Phase 5 design work.
- [x] Fix any reflow failures the 320px matrix row surfaces (no horizontal
      scroll, no clipped controls — WCAG 1.4.10). **Done:** all four audit
      failures fixed; 44/44 assertions pass at every matrix width.
- [x] Changelog entry. **Done:** four entries (second commit, two-commit
      workflow).

**Verification:** lint:css clean; eslint clean on touched files (the
repo-wide `npm run lint` reports 204 pre-existing problems in untouched
files); test:ci green (1006 unit + 112 integration + 133 wasm = 1251);
test:visual green (194 specs — all 1280px characterization baselines
pixel-identical, confirming the changes are media-query scoped or
value-preserving); build:all green. New 320/480px snapshots inspected
visually (wrapped headers, single-column rows, internal dialog scroll all
render correctly in both themes). **Trap recorded:** Playwright's
`--update-snapshots` skips rewrites when the new capture is within the
spec's `maxDiffPixelRatio` tolerance — the row-name fix (a ~0.5% pixel
change) silently kept the stale capture; force-regenerate by deleting the
PNG and running `--update-snapshots=missing`. **CI follow-up:** the new
routes need `chromium-linux` baselines via the `recreate-all-snapshots`
workflow dispatch on this branch, same as Phases 2–3.

## Phase 5 — Visual refinement (feedback gate) — **IMPLEMENTED (2026-08-27, branch `hub-client-uiux-phase5`), awaiting design review**

**Goal:** every opinionated visual change lands here, in one reviewable stage.
Phases 0–4 are objective — WCAG compliance, consistency, engineering hygiene —
and value-preserving against the characterization baselines, so they need no
design feedback. This phase is the single feedback gate: each change is
proposed with before/after Playwright diffs (light + dark), lands as its own
commit, and can be individually approved, held, or reverted.

### Test specifications (write first)
- [x] Before/after visual-diff deck per proposed change, generated from the
      Playwright baselines in both themes, assembled for review.
      **Done:** the deck is `.worktrees/phase5-review-deck/` (gitignored,
      local-only) — one directory per change with before/after pairs, a
      README with the decision table, and motion videos. New permanent
      coverage added along the way: `#/dev/replay` harness route +
      `outline-replay.visual.spec.ts`, `hover-states.visual.spec.ts`
      (hover/press/kebab/tooltip captures), drawer specs in
      `viewport-matrix`.
- [x] The full Phase 0–4 spec suite (visual, axe-core, keyboard,
      reduced-motion, viewport matrix) re-run green after each change.
      **Done:** green after every change (221 specs at the end); axe
      baseline regenerated twice (replay route added; projects-home-loading
      debt cleared by the skeleton).

### Work items
- [x] **Ratify-or-adjust review of Phase 1's new visible elements** (670d7fe4f):
      kebab, tooltip, notification placement captured for review
      (deck `change-00-ratify-phase1/`); proposal is ratify as-is. The
      tooltip capture needed a page-region clip — Playwright element
      screenshots of the portaled position:fixed bubble capture it
      unstyled (tooling artifact, computed styles verified correct).
      **Review adjustments landed:** (1) fullscreen preview went blank at
      ≤700px — the split-collapse rule hid the fullscreen pane; fixed by
      excluding `.fullscreen` (60b156d43, regression spec watched red
      first). (2) The drawer toggle was drawer-only and undiscoverable —
      now permanent header chrome that hides/shows the static sidebar
      above 900px too, in muted grey with a sidebar-tinted active state
      (38922590d). (3) Round 2 (e3a86d053): view-mode switcher hidden at
      ≤700px (the Preview pill covers switching), Share + Preview back
      inline (kebab menu retired), the toggle is a grey chip in the
      sidebar's own tint, and switch-project is teal (the one
      exit-to-another-view action). **Note:** PR #622 merged before these
      adjustments; they ride the follow-up branch
      `hub-client-uiux-phase5-review`.
- [x] Off-palette token values (00e577087): outline icons + replay me-chip
      re-mapped onto Posit ramps — one hue family per meaning (header blue,
      code teal, function orange), each theme picking the ramp step that
      keeps ≥4.5:1. New primitives: teal/orange dark+light steps, two
      posit-blue alphas. axe: replay light contrast nodes 4→3.
- [x] Motion design (c552ff922): three shared `from`-only keyframes
      (qh-fade-in/qh-rise-in/qh-pop-in), transform/opacity only — dialogs
      fade+4px rise at 200ms, menus/tooltips fade+scale(0.97) at 100ms,
      sidebar section content rises on expand (collapse stays instant:
      height transitions are layout motion), toast timings migrated to
      tokens. View toggle keeps the Change-2 cross-fade (no sliding pill).
- [x] Hover/press micro-states (b7f321633): `filter: brightness()` hovers
      replaced by color-mix tokens (--btn-accent-*/--btn-danger-*), new
      :active press states on filled buttons, one 100ms ease-out hover
      transition across buttons/icon buttons/menu items/rows/view toggle.
- [x] Skeleton loading + empty states (0c751c752): `.qh-skeleton`
      (opacity pulse); ProjectsHome loading is a skeleton card grid shaped
      like the page (role="status" preserved via aria-label); empty states
      gain muted icon treatments. StatusTab/boot gates keep the spinner
      (no known content shape).
- [x] Narrow-viewport layout design (b95d5e18a): sidebar overlay drawer
      with scrim ≤900px (useSidebarDrawer + SidebarDrawer shared by Editor
      and the harness; focus in/out, Escape, scrim click, Tab trap, inert
      when off-canvas; `display: contents` above the breakpoint); split
      view collapses to the editor pane ≤700px (toggle's split button
      disabled with explanatory tooltip); header share/preview collapse
      into a kebab overflow menu ≤700px (Phase 1 Menu).
- [x] Alignment pass: type scale (23d57bd78 — half-pixel sizes onto the
      scale, new `--text-2xs: 10px`; 7.5px facepile glyph documented
      exception), radius scale (c5c5f23d0 — new xs/xl steps; ~70
      declarations migrated; buttons 7→8, menus 9→8, cards 10→12),
      truncation + icons (ed09d1a3e — eight ad-hoc trios onto
      `.qh-truncate`; `.qh-btn` gains inline-flex for future icon+text).
      **Scope-out filed as bd-aqhrmebz:** wholesale spacing-grid migration
      (2px/6px/10px one-offs) needs per-component design review, not a
      mechanical sweep.
- [x] Changelog entry (two-commit workflow): six user-visible entries under
      2026-08-27.

**Traps recorded for future phases:**
- The projects-home footer renders the live git commit hash — captures of
  it now mask `.qh-footer` (the 1% tolerance had silently absorbed the
  per-commit churn; every baseline was perpetually stale).
- Playwright element screenshots of portaled `position: fixed` content
  (the tooltip) capture it unstyled — use page-region clips.
- eslint's react-hooks v6 rules: no `Date.now()` in render (module-level
  fixtures), no refs nested in props objects, no sync setState in effects
  (`useSyncExternalStore` for media queries).
- **CI follow-up (as with Phases 2–4):** `chromium-linux` baselines for
  all new/changed captures need the `recreate-all-snapshots` workflow
  dispatch on this branch — the full set this time (radius sweep).

## Phase 6 — Enthusiast details (stretch; only after Phases 0–5)

**Goal:** the details that make a tool feel crafted. Each item is independently
shippable and explicitly optional.

- [ ] **Command palette** (Cmd/Ctrl+K): file switching, actions (new file,
      share, theme cycle, view modes), fuzzy matching, full keyboard operability.
      This is the hallmark of modern tools (VS Code, Linear, Obsidian) and the
      single highest-delight item for Quarto's enthusiast base — but it is a
      feature, not polish, so it is gated behind Phases 0–5.
- [ ] Drag-and-drop affordances: visible drop indicators in the file tree
      (beyond the current overlay), drag handles where sortable.
- [ ] Recent-files / quick-switcher (Cmd/Ctrl+P) if the command palette lands.

## Phase 7 — CI enforcement (deferred final step)

**Goal:** turn the locally-enforced checks from Phases 0–5 into blocking CI
checks — deliberately last, so the early phases don't add CI machinery before
the checks have proven stable and false-positive-free in local runs.

### Work items
- [ ] Wire the axe-core scans into CI as a blocking check (fail on
      serious/critical violations) — the scans have run locally since
      Phase 0/Phase 2, so this flips enforcement, not coverage.
- [ ] Wire `npm run lint:css` into CI and flip repo-wide enforcement on
      (exceptions list already burned down per-rule in Phases 0–5).
- [ ] Confirm the Playwright visual specs run in CI (or document why they
      remain local-only, e.g. screenshot-platform drift).

## Out of scope (explicit)

- ProjectSelector/ProjectsHome consolidation and the `qh-ui-variant` toggle
  (user decision: leave as-is).
- New runtime dependencies (Radix, Tailwind, icon libraries — all declined).
- Tablet/touch-first layouts, mobile support.
- Rebranding or replacing the QH-ProjectManagement-July26 visual language.
- Rust/WASM/sync changes; preview-renderer internals.

## Risks & mitigations

| Risk | Mitigation |
|------|------------|
| Visual churn breaks existing Playwright e2e selectors | Phase 0 baselines first; class rename (.ph→.qh) done mechanically in one commit; grep for test selectors using old classes |
| Token migration produces subtle visual diffs | Characterization baselines + per-screen visual diffs reviewed in the Playwright report before commit |
| Hand-rolled menu/tooltip a11y is genuinely hard | Follow WAI-ARIA APG reference implementations exactly; keyboard specs written before implementation |
| Scope creep across 7 phases | Each phase is independently shippable; Phase 6 items are stretch and individually optional |
| Phase 5 feedback gate stalls on too many bundled changes | Each visual change lands as its own commit with its own before/after diff — individually approvable, holdable, or revertible |
| `lint:css` false positives | Grandfathered-exceptions list; rules only enforced repo-wide once exceptions hit zero |

## Suggested sequencing

Phases are ordered so foundations precede dependents and each lands at a clean
commit boundary: **0 → 1 → 2 → 3 → 4** — all objective work that can be
implemented without design feedback — then the **Phase 5** visual-refinement
gate, the single point requiring review. Phase 6 items are pulled in
individually as desired. **Phase 7** (CI enforcement) is the deferred final
step: all checks run locally from the phase that introduces them, and only
become blocking CI checks once they've proven stable. Estimated as 7–8
focused sessions of work (one per phase, Phase 1 possibly two; Phase 7 is a
short follow-up).
