# Vendored JavaScript resources

This directory holds JS payloads that Quarto's render pipeline embeds
into the binary via `include_bytes!` and ships as Project-scoped
artifacts when the relevant feature is active.

Adding a new resource here means a `BootstrapJsStage`-style stage that
detects a triggering condition and registers a `js:<feature>` artifact —
see `crates/quarto-core/src/stage/stages/bootstrap_js.rs` for the
prototype.

## `bootstrap/`

Bootstrap 5 JS runtime, used when a Bootstrap-backed theme is active.

- **`bootstrap.bundle.min.js`** — Bootstrap 5.3.1, the *bundled* build
  with Popper inlined. Fetched from
  <https://cdn.jsdelivr.net/npm/bootstrap@5.3.1/dist/js/bootstrap.bundle.min.js>.
  Size: 80,668 bytes.

  We deliberately ship the **bundle** (not `bootstrap.min.js`) so that
  popovers, tooltips, and auto-positioned dropdowns work without an
  extra Popper script. Quarto 1 ships the same bundled bytes but
  mislabels the file as `bootstrap.min.js`; we use the correct name.

### Version contract

The Bootstrap JS version here **must match** the Bootstrap SCSS version
under `resources/scss/bootstrap/` (see that directory's README). When
bumping Bootstrap, update both in the same commit. Mismatched JS/CSS
versions can produce subtle component bugs (e.g. JS expects a class
that the CSS no longer ships).

## `headroom/`

headroom.js — pins/unpins the fixed website header by scroll direction
(bd-ersobfbt).

- **`headroom.min.js`** — headroom.js v0.12.0, MIT, © Nick Williams.
  Byte-identical to the copy Quarto 1 vendors at
  `src/resources/projects/website/navigation/headroom.min.js` (upstream:
  <https://github.com/WickyNilliams/headroom.js>). Size: 4,570 bytes.

### Version contract

Quarto 1 has shipped v0.12.0 unchanged since 2021; q2 pins the same
version for behavioral parity. `resources/js/quarto-nav/quarto-nav.js`
consumes its `window.Headroom` global and the `headroom--pinned` /
`headroom--unpinned` class names, which the headroom SCSS block in
`resources/scss/bootstrap/_bootstrap-rules.scss` styles — bump all
three together.

## `quarto-nav/`

Fixed-header offset management + headroom wiring for website renders
(bd-ersobfbt).

- **`quarto-nav.js`** — q2 port of the header-machinery subset of
  Quarto 1's `quarto-nav.js` (deviations documented in the file
  header). Not minified; it is small and diff-reviewability wins.

Shipped by `QuartoNavJsTransform`
(`crates/quarto-core/src/transforms/quarto_nav_js.rs`) for website
projects with a navbar or sidebar; `headroom.min.js` ships alongside it
unless `pinned: true`. The hub-client preview injects both via `?raw`
imports in `ts-packages/preview-renderer/src/q2-preview/entry.tsx`
(the Phase F.1 pattern) — native and preview must load the same files.

### Version contract

Slated for replacement by the `position: sticky` +
`--quarto-header-height` redesign (bd-pt1wxeq2); keep changes here
minimal and self-contained.
