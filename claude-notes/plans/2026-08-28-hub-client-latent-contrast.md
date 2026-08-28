# hub-client: fix latent contrast pairs outside the axe scan set

Strand: bd-uue5voml (discovered-from bd-7byucvr6)
Branch: `fix/bd-uue5voml-latent-contrast` (stacked on
`fix/bd-7byucvr6-contrast-baseline-burndown`)
Date: 2026-08-28

## Overview

bd-7byucvr6 burned the axe baseline to zero, but the harness scan set
does not cover every surface. This plan fixes the known latent <4.5:1
pairs (WCAG 1.4.3) enumerated in the strand, plus two same-class pairs
found during the audit (`.diagnostic-info` / `.diagnostic-note` banner
text), and adds axe coverage for the editor-shell + status surfaces so
the class cannot regress silently.

All ratios verified by computation (WCAG rel-luminance); "L"/"D" =
light/dark theme.

## Work Items

### Phase 1 — token and CSS fixes

- [ ] Editor.css text usages → per-theme text tokens (all verified
  failing L, several D; fixed ≥4.95:1 everywhere):
  - `.diagnostic-item.diagnostic-error/-warning` (banner text, 3.20/2.46 L)
    → `--editor-error-text` / `--editor-warning-text`
  - `.sync-status.connected/.disconnected` (dead CSS — no consumers;
    fixed anyway) → `--editor-success-text` / `--editor-error-text`
  - `.disconnect-btn:hover` color (dead CSS) → `--editor-error-text`
    (border keeps `--editor-error`)
  - `.preview-status-error` (3.45 L / 3.49 D) → `--editor-error-text`
  - `.preview-status-clear-confirm-btn` color → `--editor-error-text`
    (border keeps `--editor-error`)
  - `.replay-mode-banner` (3.02 L) → `--editor-success-text`
  - `.preview-error-title`, `.preview-error-expand-btn` (~3.5 L / 3.6 D)
    → `--editor-error-text`
  - `.preview-error-diagnostics .diagnostic-line`,
    `.diagnostic-source-file` (2.94 L) → `--editor-warning-text`
  - `.preview-error-diagnostics .diagnostic-title` → `--editor-error-text`
- [ ] Audit finding beyond the strand's enumerated lines:
  `.diagnostic-info` (4.38 L on banner) and `.diagnostic-note` (2.66 L /
  4.48 D) fail too; `--editor-info`/`--editor-note` are consumed only by
  diagnostic text, so fix at the token level: light `--editor-info` →
  `--posit-blue-dark-1` (6.41 banner), `--editor-note` →
  `var(--editor-text-muted)` both themes (4.60 L / 4.99 D banner).
- [ ] `:root.dark --accent-secondary`: `--posit-blue` (#447099, 2.18:1 on
  navy modal #213D4F) → `#93b3d0` (5.20:1 navy, 7.10 page-bg; matches
  the slate-ramp value). Consumers are all text/border (qh-btn.outline,
  qh-link, spinner, link-browser hover border) — no fills.
- [ ] ErrorBoundary fallback: hardcoded `#fee/#fcc/#900` inline styles →
  theme tokens (`--error-bg-subtle`/`--diagnostics-border`/`--error-text`)
  so the dark `--accent-secondary` bump doesn't regress its outline
  buttons (and the fallback finally follows the theme).
- [ ] Teal text → `--accent-action-text` (3.5:1 → ≥5.3:1):
  ProjectTab `.export-zip-btn` color + `.screenshot-btn:hover` color
  (borders keep `--posit-teal`), MinimalHeader `.header-switch-btn`,
  ProjectsHome `.qh-fork-btn:hover`/`.qh-peek-btn:hover`/
  `.qh-join-kicker`/`.qh-join-collection-name`, ui.css
  `.qh-menu-item.accent`, ProjectSelector `.connecting` color.
- [ ] ReplayDrawer `.replay-drawer__attribution--on:hover`: text
  `--editor-bg` on `--editor-success` fill (3.02 L) →
  `var(--posit-blue-dark-3)` (4.95:1 on #72994E in both themes).

### Phase 2 — axe coverage for the editor shell

- [ ] New harness route `editor-status-states`: diagnostics banner (all
  four kinds, mirroring Editor.tsx markup), PreviewStatusBar in error
  state, PreviewErrorOverlay expanded + collapsed, replay-mode-banner —
  inside EditorChrome so the editor token scope applies.
- [ ] Add `editor-shell` and `editor-status-states` to SCAN_PAGES in
  baseline-a11y.harness.spec.ts; regenerate baseline (expect no new
  entries — pages scan clean after Phase 1).

### Phase 3 — verification

- [ ] `npm run lint:css`, unit tests, `npm run build:all`
- [ ] Harness suite green incl. baseline-a11y read-mode
- [ ] Two-commit workflow + hub-client/changelog.md entry
- [ ] Report; do NOT push without approval

## Details

- Contrast model: WCAG 2.x relative luminance; alpha backgrounds
  composited over their actual surface (banner = error-bg-subtle over
  editor-bg; status bar = input-bg-alpha over bg-modal; etc.).
- `.qh-menu-item.accent` on the dark navy ramp lands at 4.67:1 with
  `--accent-action-text` — passes, tightest of the set.
- Dark `--border-focus` (#447099, 2.18:1 on navy — WCAG 1.4.11 non-text)
  is the same hue family but a different criterion and strand; file
  follow-up, do not fix here.
