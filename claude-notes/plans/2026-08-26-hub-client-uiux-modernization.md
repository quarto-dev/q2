# Hub-Client UI/UX Modernization (pointer)

**Date:** 2026-08-26
**Authoritative plan:** [`.posit/assistant/plans/2026-08-25-hub-client-uiux-modernization-plan.md`](../../../.posit/assistant/plans/2026-08-25-hub-client-uiux-modernization-plan.md)
**Braid epic:** bd-2q55e6rc

This file is the repo-side pointer required by the work-tracking workflow.
The full plan (phases, test specs, decisions) lives in the authoritative
file above; progress is tracked in braid.

## Strands

| Phase | Strand | Scope |
|-------|--------|-------|
| 0 — Token foundation & CSS hygiene | bd-5nm6v8bl | scale tokens, `lint:css`, DevHarness tokens page, visual + axe-core baselines, `.ph-*`→`.qh-*` rename |
| 1 — Component consistency | bd-iguk0hpd | button system, menu, tooltip, notifications, dialogs, forms, icons, mono unification |
| 2 — Keyboard & assistive-tech | bd-lavl5jv8 | focus rings, treeview, aria audits, forced-colors, shortcut map, copy audit |
| 3 — Functional states & motion safety | bd-6oxpa77k | reduced-motion global, loading/error/empty states |
| 4 — Graceful narrow viewports | bd-nubnj8ue | viewport matrix 1280–320px, reflow fixes |
| 5 — Visual refinement (feedback gate) | bd-tfsdmytf | single review gate for opinionated visual changes |
| 6 — Enthusiast details (stretch) | bd-j8s3ahjo | command palette, DnD affordances, quick-switcher |
| 7 — CI enforcement | bd-i1wlah5w | axe-core + lint:css as blocking CI checks |

## Phase 0 progress log

**Complete (2026-08-26)** on branch `hub-client-uiux-phase0`, four commits:

- `16cb20b1` test infrastructure: `lint:css` (scripts/lint-css.mjs +
  grandfathered exceptions), DevHarness baseline routes (projects-home,
  3 dialogs, sidebar, header, notifications), visual baseline spec,
  axe-core baseline spec + manifest.
- `305fec7c` scale tokens in theme.css + layering doc + `#/dev/tokens`
  gallery page; deterministic baselines (fixed IDB identity, frozen
  clock, transitions off, fonts awaited); visual config retries: 1.
- `df1cd10b` `.ph-*` → `.qh-*` rename (611 replacements, 22 files).
- `7ea60da9` color/z-index burn-down to zero exceptions (187 → 85; the
  rest are owned by Phases 2/7) + shared utilities (`.qh-truncate`,
  `.qh-row-hover`, `.qh-active-accent-row`).

Verification: `npm run build:all`, `npm run test:ci` (1005 + 112 + 133),
visual suite 42/42 pixel-clean vs pre-change baselines, axe baselines
hold, eslint identical to main (192 pre-existing problems, none added),
rename-touched e2e specs (projects-home, accessibility, files-header)
7/7 green on a real e2e build. No changelog entry — Phase 0 is internal
(changelog policy: user-facing changes only).

Deviations from the plan, recorded:

- Editor-shell baselines are covered surface-by-surface (header, sidebar
  sections, dialogs, notifications) — the full Editor needs live sync +
  Monaco + WASM, which the no-server visual config avoids by design.
- axe/lint:css run locally via `npm run test:visual` / `npm run lint:css`,
  not `test:ci` (vitest-only); blocking CI wiring is Phase 7 either way.
- lint:css color rule allows literals in token *definitions*
  (custom properties) in any file, so the standalone src/debug/ page
  keeps its local token block.
- ProjectsHome menu z-index 70 → --z-dropdown (60): order-preserving
  renumber, pixel-invisible.
- Known flake: bootHarness's identity-pinning occasionally hits an
  execution-context teardown under full parallelism; absorbed by
  `retries: 1` in the visual config (assertions themselves are
  deterministic).
