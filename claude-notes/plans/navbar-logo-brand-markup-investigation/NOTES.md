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

## End-to-end verification after the fix (2026-08-19, phase 5)

Fixture extended with a dark theme (`theme: {light: cosmo, dark: darkly}`)
and distinct variants (`logo: {light: logo.svg, dark: logo-dark.svg}` +
`logo-alt`). Invocation:

```
cargo run --bin q2 -- render claude-notes/plans/navbar-logo-brand-markup-investigation/repro
```

Observed output (inspected by hand):

- **Markup** (`_site/index.html`) — full Q1 shape:

  ```html
  <div class="navbar-brand-container mx-auto"><a href="./" class="navbar-brand navbar-brand-logo"><img src="logo.svg" alt="Repro logo" class="navbar-logo light-content"><img src="logo-dark.svg" alt="Repro logo" class="navbar-logo dark-content"></a><a class="navbar-brand" href="./"><span class="navbar-title">Navbar Logo Repro</span></a></div>
  ```

- **CSS**: both compiled variants (`quarto-theme-*.css` and
  `quarto-theme-dark-*.css`) contain
  `.navbar-logo{max-height:24px;width:auto;padding-right:4px}`,
  `.navbar-brand-container{min-width:0;display:flex;align-items:center}`
  (+ the lg `margin-right:1em` media rule), and the
  `body.quarto-light .dark-content{display:none !important}` toggling
  pair.
- **Assets**: both `logo.svg` and `logo-dark.svg` copied into `_site/`.
- **Browser** (chrome-devtools MCP against the served `_site`): light
  mode renders the light logo at a computed 24px beside the title with
  the dark img `display:none`; clicking the color-scheme toggle flips
  `body` to `quarto-dark` and swaps the imgs (dark at 24px, light
  hidden). Screenshots inspected in both modes — 24px logo beside the
  title, navbar intact.

## Pre-flight note (unrelated to the strand)

`cargo xtask verify --skip-hub-build` initially failed one test:
`quarto-preview::integration config_endpoint::config_reports_embedded_asset_manifest_hashes`
("a real embedded viewer dist must advertise assets.viewer"). Cause: this
checkout's `q2-preview-spa/dist/` predated the spa-manifest feature — real
dist, no `spa-manifest.json` — so the embedded dist advertised no hash.
`cargo xtask build-q2-preview-spa` regenerated the dist (manifest hash
`cf5ec75d…`), after which the test and the **full workspace suite passed:
12896/12896** (198 skipped). Stale local artifact, not a code regression.
