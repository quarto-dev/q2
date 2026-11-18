# Invisible code-copy button (and other SVG icons): `colorToRGB()` undefined on the HTML path

**Strand:** bd-65ja3het
**Status:** implemented; PR open

## Symptom

On the Connect docs rendered with q2
(<https://docs.posit.co/connect/__unreleased__/admin/database/index.html>),
code blocks render `<button class="code-copy-button"><i class="bi"></i></button>`
inside `div.code-copy-outer-scaffold`, but no icon appears, even on hover.

## Root cause

The icon is a CSS `background-image` holding an SVG data URI, whose fill is
interpolated from a Sass helper:

```scss
// resources/scss/html/templates/copy-code.scss:82
background-image: url('data:image/svg+xml,<svg ... fill="#{colorToRGB($code-copy-color)}" ...');
```

`colorToRGB` is **not defined on the HTML/Bootstrap compile path**. In q2 it
exists only in `resources/scss/revealjs/quarto-revealjs.scss:216`. Quarto 1
defines it in `src/resources/formats/html/_quarto-functions.scss`, but when
q2 ported that file into `_bootstrap-functions.scss`, it brought over only
`colorToRGBA` (bd-0pic6 A4).

Dart Sass treats an unknown function call as a plain CSS function, so it
reports no error. The compiled CSS contains the literal text:

```
fill="colorToRGB(#6a737d)"
```

Inside a `data:` URL, the unescaped `#` starts a URL **fragment**. The SVG is
cut off at `fill="colorToRGB(`, the image fails to parse, and the icon is
blank.

### Reproduction (done)

- **Live site:** the theme CSS has 16 literal `colorToRGB(#…)` calls in light
  mode and 16 in dark mode.
- **Fresh build of q2 `main` @ 59be796b2:** a minimal `format: html` doc
  (with a filename code block, a plain code block and a collapsible callout)
  produces `index_files/styles.css` with 16 literal `colorToRGB(` calls. The
  bug therefore does not depend on the Connect theme, the filename, or the
  website structure.
- **Headless Chrome check:** the same SVG data URI renders blank with
  `fill="colorToRGB(#6a737d)"` and draws correctly with
  `fill="rgb(106, 115, 125)"`.

Repro scratch files are in `/tmp/q2-copybtn/` (not committed).

## Blast radius (wider than the copy button)

Every `colorToRGB()` call site on the HTML path is broken:

| File | Lines | What breaks |
|---|---|---|
| `resources/scss/html/templates/copy-code.scss` | 82, 88, 92, 96 | code-copy icon (normal, checked, hover states) |
| `resources/scss/bootstrap/_bootstrap-rules.scss` | 1888, 1894 | `.code-tools-button` icon |
| | 1898, 1902 | copy icon in `#quarto-embedded-source-code-modal` |
| | 2050 | `.quarto-notebook-link::before` icon |
| | 2600 | **collapsible callout chevron** (`.callout-toggle::before`, all 5 types) |
| `resources/scss/html/templates/title-block.scss` | 66, 70 | code-tools icon inside the title banner |
| `resources/scss/dashboard/quarto-dashboard.scss` | 448 | dashboard icon (compiled on top of the Bootstrap layers, so also affected) |

The reveal.js path is **not** affected, because `quarto-revealjs.scss` defines
its own `colorToRGB`. `link-external.scss` already works around the problem
with inline `red()/green()/blue()` (bd-t3n1nghb). That is a third instance of
the same bug class.

## Proposed fix

1. **Define `colorToRGB` in `resources/scss/bootstrap/_bootstrap-functions.scss`**,
   next to `colorToRGBA`, copied verbatim from Q1's `_quarto-functions.scss`:
   ```scss
   @function colorToRGB($color) {
     @return "rgb(" + red($color) + ", " + green($color) + ", " + blue($color) + ")";
   }
   ```
   This fixes every call site at once and keeps the SCSS identical to Q1, so
   future syncs with Q1 stay easy. The reveal.js bundle does not load
   `_bootstrap-functions.scss` (`assemble_reveal_scss` in `bundle.rs`), so
   the two definitions never collide.

2. **Optional cleanup:** restore `link-external.scss` to the Q1 form
   (`colorToRGB($link-color)`) and drop its workaround comment. The comment
   would otherwise become stale and claim the function "only the reveal
   layer defines". Low priority; I'd fold it in only if you want the
   divergence from Q1 removed.

3. **Regression tests** (`crates/quarto-sass/src/compile.rs`, next to
   `test_compile_theme_css_evaluates_color_to_rgba_in_toggle_icons`):
   - Default CSS and a named theme (cosmo): `!css.contains("colorToRGB(")`,
     plus a positive check that the copy-code icon rule contains
     `fill="rgb(`.
   - Optionally a **generic guard**: assert that no compiled HTML theme CSS
     contains `fill="` followed by an identifier and `(` other than
     `rgb(`/`rgba(`. That would catch the next undefined helper inside a
     data URI (this bug class has now appeared three times). Implemented as a
     small scan over the compiled CSS, not a regex dependency, if regex isn't
     already available in the crate.
   - Dashboard: extend `crates/quarto-core/tests/integration/dashboard_format.rs`
     (or the sass test) with the same `!contains("colorToRGB(")` check.

4. **Verification:**
   - `cargo nextest run -p quarto-sass` and the dashboard integration test.
   - Re-render the `/tmp/q2-copybtn/repro` doc and confirm there are zero
     `colorToRGB(` occurrences. Then check in a browser that the copy icon
     appears on hover and the collapsible-callout chevron renders.
   - Optionally render the Connect docs snapshot (`cscheid/q2-docs`, exported
     to `/tmp/q2-copybtn/docs`) and spot-check `admin/database/`.
   - `cargo xtask verify` before PR.

## Decisions (2026-11-17 review)

- Step 2 (`link-external.scss` revert to Q1's `colorToRGB($link-color)`): **yes**.
- Generic `fill=` guard: **no**; targeted asserts are enough for now.
- Q1 functions audit: **yes, as a separate strand** → bd-5e7xtb5k.

## Progress

- [x] `colorToRGB` added to `_bootstrap-functions.scss`
- [x] `link-external.scss` restored to the Q1 form (comment updated)
- [x] Tests: `test_compile_html_css_evaluates_color_to_rgb_in_svg_icons`
      (default/cosmo/darkly; copy-code, callout-toggle and code-tools rules
      have `fill="rgb(`), plus a reveal assert and a dashboard integration
      assert. Confirmed that both HTML tests fail with the function removed.
- [x] Re-rendered repro: 0 literal `colorToRGB(` calls. A headless-Chrome
      screenshot shows the copy icons and the callout chevron.
- [x] `cargo xtask verify` (local R-only failures: cairo/XQuartz libXrender missing, bsicons not installed; styles.css hash baseline re-captured)
- [ ] PR + CI
