# Investigation notes — bd-navbar-logo-unstyled-gbzd8vcu

**Date:** 2026-08-19, main @ `f387bd68`.

## Repro confirmation at HEAD

Invocation:

```
cargo run --bin q2 -- render claude-notes/plans/navbar-logo-brand-markup-investigation/repro
```

Observed output (inspected by hand):

- **Defect 1 (no sizing rule):** `_site/site_libs/quarto/quarto-theme-8de4e546dab01d3a.css` contains **zero** occurrences of `navbar-logo` (`grep -c` → 0). The 512×512 `logo.svg` therefore renders at natural size.
- **Defect 2 (markup shape):** `_site/index.html` brand markup is the single inlined anchor:

  ```html
  <a class="navbar-brand" href="./"><img src="logo.svg" alt="Repro logo" class="navbar-logo"> Navbar Logo Repro</a>
  ```

  No `navbar-brand-container`, no `navbar-brand-logo` anchor, no `navbar-title` span — Q1's user-CSS hooks are absent.

## Q1 reference targets

- Markup: `external-sources/quarto-cli/src/resources/projects/website/templates/navbrand.ejs`
- CSS: `external-sources/quarto-cli/src/resources/projects/website/navigation/quarto-nav.scss` lines 116–170 (brand container/logo layout) and 196 (`.navbar-logo` sizing). Both captured verbatim in the plan.

## Pre-flight note (unrelated to the strand)

`cargo xtask verify --skip-hub-build` initially failed one test:
`quarto-preview::integration config_endpoint::config_reports_embedded_asset_manifest_hashes`
("a real embedded viewer dist must advertise assets.viewer"). Cause: this
checkout's `q2-preview-spa/dist/` predated the spa-manifest feature — real
dist, no `spa-manifest.json` — so the embedded dist advertised no hash.
`cargo xtask build-q2-preview-spa` regenerated the dist (manifest hash
`cf5ec75d…`), after which the test and the **full workspace suite passed:
12896/12896** (198 skipped). Stale local artifact, not a code regression.
