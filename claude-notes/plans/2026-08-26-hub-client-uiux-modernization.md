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

(Updated as work lands; see braid comments for the running trail.)
