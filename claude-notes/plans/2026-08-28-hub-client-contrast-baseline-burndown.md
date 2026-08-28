# hub-client: burn down axe color-contrast baseline

Strand: bd-7byucvr6
Date: 2026-08-28

## Overview

`hub-client/e2e/helpers/axe-baseline.json` characterizes 28 page+theme
entries of accepted serious `color-contrast` violations (111 nodes
total) on dev-harness surfaces. This is the only rule in the baseline —
every entry is CSS contrast debt. The strand notes fixes were gated
behind the Phase 5 design review; the user has explicitly directed
this burn-down now (2026-08-28), ahead of that review.

Goal: fix the underlying CSS (tokens/component values, minimal visual
delta, WCAG 1.4.3 AA: 4.5:1 normal text, 3:1 large text/UI) so the
baseline regenerates to empty.

## Work Items

- [x] Dump exact violation inventory (selector, fg/bg, ratio) per
  page+theme via a temporary axe dump spec against `vite dev`
  (111 nodes / 28 entries; matched the baseline counts exactly)
- [x] Fix CSS contrast violations (prefer token-level fixes in
  theme.css/ui.css; component CSS only where tokens can't reach)
- [x] Re-run dump spec: zero serious/critical color-contrast violations
  (42/42 scans clean; dump = `{}`)
- [x] Regenerate baseline via official path:
  `AXE_BASELINE_WRITE=1 npx playwright test --config playwright.harness.config.ts baseline-a11y --workers=1`
  against a fresh VITE_E2E=1 build — baseline is now `{}`. Read-mode
  run first failed with "FIXED: color-contrast no longer fires" on
  every entry, as the characterization model requires.
- [x] Full harness suite green — 160 passed (against VITE_E2E=1
  `vite preview` build)
- [x] Unit (1066) + integration (114) green; lint:css clean; eslint
  204 problems identical to clean main (all pre-existing, none in
  touched files); `npm run build:all` succeeds
- [ ] Two-commit workflow: code, then hub-client/changelog.md entry
- [ ] Report; do NOT push without explicit approval

## Fixes applied (all WCAG 1.4.3, 4.5:1 normal text)

| Cluster | Fix |
| --- | --- |
| White on teal primary buttons (18 nodes, 3.5:1) | New `--accent-action-bg` = `--posit-teal-dark-1` #2E6E71 (5.86:1); consumed by `.qh-btn.primary`, `--header-preview-bg`, `--dialog-primary-bg`; hover/active color-mixes rebased (4.90/6.46:1) |
| Teal ghost-accent text (3.5:1 light / 4.42:1 dark) | New per-theme `--accent-action-text`: light #2E6E71, dark #5fb3b7 (6.37:1 on #242424) |
| White on red danger button (3.81:1) | New `--btn-danger-bg` #c0392b (5.44:1); danger hover/active rebased (4.63/6.08:1) |
| Muted grays #777/#888 (58 nodes, 3.7-4.5:1) | `--editor-text-muted`: light #777→#696969 (4.58:1 on worst bg), dark #888→#999999 (4.68:1) |
| Red error text on tinted bg (3.04/3.33:1) | New per-theme `--editor-error-text` (#b0392e / #ff9480); applied in StatusTab |
| Amber loading text (2.63:1) + share warning-detail (4.42:1) | New `--editor-warning-text` (#92400e / #fbbf24); `--warning-detail` light → #92400e |
| Green replay restore/attribution text (2.69/3.68:1) | New per-theme `--editor-success-text` (#4a6a32 / #9dc183); applied in ReplayDrawer + StatusTab ready |
| about-tab commit-hash (3.72:1) | Deleted the `:root.light .version-info .commit-hash` override in ProjectSelector.css (falls back to `--text-secondary`, 5.45:1); AboutTab base rule → `--text-secondary` |
| footer-note (3.49/3.92:1) | Removed `opacity: 0.8` — full-strength `--text-muted` passes (5.22/5.38:1) |
| search-kbd dark (4.26:1) | `--text-muted` → `--text-secondary` (6.43:1) |
| gallery dark outline/link (2.97:1) | Gallery root gained `dev-gallery-page` class, added to the dark slate-ramp token scope (previews editor chrome under editor tokens) |

## Follow-up filed

Latent <4.5:1 pairs on surfaces outside the axe scan set (Editor.css
status text, navy-modal `--accent-secondary`, assorted teal text,
replay attribution hover) — see the strand linked
`discovered-from: bd-7byucvr6`.

## Details

- Baseline inventory (28 entries, all `color-contrast`): about-tab
  3/4, dialog-new-asset 1/4, dialog-share 1/2, gallery 7/3,
  minimal-header 1/1, notifications 1/1, projects-home-empty 4/3,
  projects-home-error 3/3, projects-home 3/3, replay-drawer 1/3,
  sidebar-empty 10/10, sidebar-sections 10/10, status-tab-error 6/6,
  status-tab-loading 3/4 (dark/light node counts).
- Known offenders from the strand: StatusTab error text/retry button
  (#db593b on #ece4e2, 3.04:1), editor section labels/muted text
  (#777 on #eef0f1, 3.91:1), teal primary buttons (white on #419599,
  3.5:1, app-wide), projects-home footer note (3.49:1).
- The baseline spec is self-enforcing: a baselined rule that shrinks
  or disappears FAILS the scan until the baseline is regenerated, so
  the regen step is mandatory, not optional.
- Phase 5 branch (`hub-client-uiux-phase5`) does NOT contain these
  fixes (it only adds setup-* entries); no conflict expected, but the
  token values touched here will need re-review at the Phase 5 gate.
